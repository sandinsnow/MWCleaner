use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;

use crate::core::{report, settings, startup};
use crate::ui::{button, checkbox, open_in_explorer, page_header, pump, start_status_ticker, theme, transparent, Status};

enum ExportMsg {
    Stage(&'static str),
    Done(PathBuf),
    Failed(String),
}

pub struct SettingsPage {
    /// B5：开机自启写注册表失败时的提示（仅设置页内可见）
    self_run_error: Option<String>,
    // D3：HTML 报告导出状态
    exporting: bool,
    exported: Option<PathBuf>,
    stage: &'static str,
    status: Status,
}

impl SettingsPage {
    const DEFAULT_STATUS: &'static str = "导出单文件 HTML 清理报告（垃圾、大文件、注册表、启动项、服务建议快照），保存到导出目录";

    pub fn new(cx: &mut Context<Self>) -> Self {
        start_status_ticker(cx);
        Self {
            self_run_error: None,
            exporting: false,
            exported: None,
            stage: "",
            status: Status::new(),
        }
    }

    /// D3：pump 后台串行五步采集（export_report 内部），UI 只收结果消息，不冻结。
    fn start_export(&mut self, cx: &mut Context<Self>) {
        if self.exporting {
            return;
        }
        self.exporting = true;
        self.stage = "准备导出…";
        self.exported = None;
        cx.notify();
        pump(
            cx,
            move |tx| {
                let stage = |s: &'static str| {
                    let _ = tx.try_send(ExportMsg::Stage(s));
                };
                let cancel = std::sync::atomic::AtomicBool::new(false);
                match report::export_report(&stage, &cancel) {
                    Ok(path) => {
                        let _ = tx.try_send(ExportMsg::Done(path));
                    }
                    Err(e) => {
                        let _ = tx.try_send(ExportMsg::Failed(e.to_string()));
                    }
                }
            },
            |this, msg, cx| {
                match msg {
                    ExportMsg::Stage(s) => {
                        this.stage = s;
                    }
                    ExportMsg::Done(path) => {
                        this.exporting = false;
                        this.exported = Some(path.clone());
                        // spec 2.4：完成态常驻，不随 8s TTL 回落
                        this.status
                            .set_persistent(format!("已导出报告 {}", path.display()));
                    }
                    ExportMsg::Failed(e) => {
                        this.exporting = false;
                        this.status.set_persistent(format!("导出失败：{e}"));
                    }
                }
                cx.notify();
            },
        );
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = theme::mode();
        let options = [
            (theme::MODE_DARK, "深色"),
            (theme::MODE_LIGHT, "浅色"),
            (theme::MODE_SYSTEM, "跟随系统"),
        ];
        let buttons = options.into_iter().map(|(m, label)| {
            let active = current == m;
            div()
                .id(SharedString::from(format!("theme-{m}")))
                .px_4()
                .py_1p5()
                .rounded_md()
                .border_1()
                .border_color(transparent())
                .text_size(px(13.))
                .cursor_pointer()
                .focusable()
                .focus(|s| s.border_color(theme::accent()))
                .bg(if active { theme::accent() } else { theme::panel_hover() })
                .text_color(if active { rgb(0xffffff) } else { theme::text() })
                .hover(|d| d.bg(if active { theme::accent_hover() } else { theme::border() }))
                .on_click(cx.listener(move |_, _, _, cx| {
                    theme::set_mode(m);
                    settings::save_theme(m);
                    cx.notify();
                }))
                .child(label)
        });

        div().flex().flex_col()
            .id("settings-page").overflow_y_scroll()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("设置", "应用外观与偏好"))
            .child(
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("主题"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("选择应用配色，切换立即生效；「跟随系统」实时跟随 Windows 应用主题变化（约 2 秒内生效）"),
                    )
                    .child(div().flex().gap_2().children(buttons)),
            )
            .child(
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("清理"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("垃圾清理的删除方式；移入回收站后可从回收站还原"),
                    )
                    .child(
                        div().flex().items_center().gap_2()
                            .child(checkbox("set-recycle", settings::load_flag("RecycleBin", true), cx.listener(|_, _, _, cx| {
                                settings::save_flag("RecycleBin", !settings::load_flag("RecycleBin", true));
                                cx.notify();
                            })))
                            .child(div().text_size(px(13.)).child("清理时移入回收站")),
                    ),
            )
            .child(
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("启动"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("登录 Windows 后自动启动 MWCleaner（写入当前用户注册表 Run 键，不做常驻后台）"),
                    )
                    .child(
                        div().flex().items_center().gap_2()
                            .child(checkbox("set-self-run", startup::self_run_enabled(), cx.listener(|this, _, _, cx| {
                                let target = !startup::self_run_enabled();
                                match startup::set_self_run(target) {
                                    Ok(()) => this.self_run_error = None,
                                    Err(e) => this.self_run_error = Some(format!("设置开机自启失败：{e}")),
                                }
                                cx.notify();
                            })))
                            .child(div().text_size(px(13.)).child("开机自启动")),
                    )
                    .when_some(self.self_run_error.clone(), |d, msg| {
                        d.child(div().text_size(px(12.)).text_color(theme::danger()).child(msg))
                    }),
            )
            .child(
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("大文件分析"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("扫描时忽略小于阈值的文件"),
                    )
                    .child(div().flex().gap_2().children({
                        let current = settings::load_u32("BigFileMinMB", 100);
                        // 重借出短命引用：闭包 move 的是临时借用，后续链式面板仍可用 cx
                        let cx = &mut *cx;
                        [50u32, 100, 500].into_iter().map(move |v| {
                            let active = current == v;
                            div()
                                .id(SharedString::from(format!("set-minmb-{v}")))
                                .px_4()
                                .py_1p5()
                                .rounded_md()
                                .border_1()
                                .border_color(transparent())
                                .text_size(px(13.))
                                .cursor_pointer()
                                .focusable()
                                .focus(|s| s.border_color(theme::accent()))
                                .bg(if active { theme::accent() } else { theme::panel_hover() })
                                .text_color(if active { rgb(0xffffff) } else { theme::text() })
                                .hover(move |d| d.bg(if active { theme::accent_hover() } else { theme::border() }))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    settings::save_u32("BigFileMinMB", v);
                                    cx.notify();
                                }))
                                .child(format!("{v} MB"))
                        })
                    })),
            )
            .child(
                // D3：报告面板——即时采集五节快照并落盘导出目录
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("报告"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("导出单文件 HTML 清理报告（垃圾、大文件、注册表、启动项、服务建议快照），保存到导出目录；大文件全盘扫描耗时较长，请耐心等待"),
                    )
                    .child(div().flex().items_center().gap_2()
                        .child(button("report-export",
                            if self.exporting { "导出中…" } else { "导出 HTML 报告" },
                            true,
                            !self.exporting,
                            cx.listener(|this, _, _, cx| this.start_export(cx)),
                        ))
                        .when(self.exported.is_some(), |d| {
                            d.child(button("report-open-dir", "打开目录", false, true, cx.listener(|this, _, _, cx| {
                                if let Some(path) = this.exported.clone() {
                                    open_in_explorer(&crate::core::exports_dir(), Some(&path));
                                }
                                cx.notify();
                            })))
                        }))
                    .child(
                        div().text_size(px(13.)).truncate().text_color(
                            if self.exporting { theme::accent() } else { theme::muted() },
                        )
                        .child(if self.exporting {
                            SharedString::from(self.stage)
                        } else {
                            self.status.view(Self::DEFAULT_STATUS)
                        }),
                    ),
            )
    }
}
