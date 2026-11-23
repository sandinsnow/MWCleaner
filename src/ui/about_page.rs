use gpui::*;
use gpui::prelude::*;

use crate::core::update::{self, ReleaseInfo};
use crate::ui::{badge, brand_icon, button, dot, page_header, pump, theme, transparent};

enum Msg {
    Done(Result<ReleaseInfo, String>),
}

enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(ReleaseInfo),
    Failed(String),
}

pub struct AboutPage {
    state: UpdateState,
    link_error: Option<String>,
}

impl AboutPage {
    pub fn new() -> Self {
        Self {
            state: UpdateState::Idle,
            link_error: None,
        }
    }

    fn checking(&self) -> bool {
        matches!(self.state, UpdateState::Checking)
    }

    fn start_check(&mut self, cx: &mut Context<Self>) {
        self.state = UpdateState::Checking;
        cx.notify();
        pump(
            cx,
            |tx| {
                let _ = tx.try_send(Msg::Done(update::check_latest()));
            },
            |this, msg, cx| {
                let Msg::Done(r) = msg;
                this.state = match r {
                    Ok(info) => {
                        if update::is_newer(&info.tag, update::current_version()) {
                            UpdateState::Available(info)
                        } else {
                            UpdateState::UpToDate
                        }
                    }
                    Err(e) => UpdateState::Failed(e),
                };
                cx.notify();
            },
        );
    }
}

fn open_url(url: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let w: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let op: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
    let r = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(op.as_ptr()),
            PCWSTR(w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    r.0 as isize > 32
}

impl Render for AboutPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ver = update::current_version();
        let (status_text, status_color) = match &self.state {
            UpdateState::Idle => (
                format!("当前版本 v{ver} · 检查 GitHub 上的最新 Release"),
                theme::muted(),
            ),
            UpdateState::Checking => ("正在连接 GitHub…".to_string(), theme::muted()),
            UpdateState::UpToDate => (format!("已是最新版本（v{ver}）"), theme::success()),
            UpdateState::Available(i) => (format!("发现新版本 {}", i.tag), theme::warning()),
            UpdateState::Failed(e) => (format!("检查失败：{e}"), theme::danger()),
        };

        let links = [
            ("源码仓库", update::REPO_URL),
            ("问题反馈", update::ISSUES_URL),
            ("发布历史", update::RELEASES_URL),
        ];

        div()
            .flex()
            .flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("关于", "版本信息、更新检查与开源仓库"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .px_5()
                    .py_5()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(theme::border())
                    .bg(theme::panel())
                    .children(brand_icon().map(|im| img(im).size(px(64.)).flex_none()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("MWCleaner"),
                                    )
                                    .child(badge(format!("v{ver}"), theme::accent(), theme::accent_soft()))
                                    .child(badge("单文件版", theme::muted(), theme::track())),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(theme::muted())
                                    .truncate()
                                    .child("Windows 清理与优化 · 垃圾 / 注册表 / 大文件 / 启动项 / 服务 / 卸载"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .truncate()
                                    .child("Rust + GPUI · Windows 10 1903+ / Windows 11 · x64 · MIT License · © 2026 M.Ming"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(button(
                                "about-check",
                                if self.checking() { "检查中…" } else { "检查更新" },
                                true,
                                !self.checking(),
                                cx.listener(|this, _, _, cx| this.start_check(cx)),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_right()
                                    .text_size(px(12.))
                                    .text_color(status_color)
                                    .truncate()
                                    .child(status_text),
                            ),
                    )
                    .when(matches!(self.state, UpdateState::Available(_)), |d| {
                        let UpdateState::Available(info) = &self.state else {
                            return d;
                        };
                        let dl = info.download_url.clone().unwrap_or_else(|| info.url.clone());
                        let page_url = info.url.clone();
                        let head = if info.published.is_empty() {
                            format!("最新版本 {}", info.tag)
                        } else {
                            format!("最新版本 {} · 发布于 {}", info.tag, info.published)
                        };
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .px_4()
                                .py_3()
                                .rounded(px(8.))
                                .bg(theme::panel_hover())
                                .child(div().text_size(px(13.)).child(head))
                                .when(!info.notes.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(theme::muted())
                                            .line_clamp(5)
                                            .child(info.notes.clone()),
                                    )
                                })
                                .child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(button("about-dl", "前往下载", true, true, cx.listener(move |this, _, _, cx| {
                                            if !open_url(&dl) {
                                                this.link_error = Some(format!("无法打开默认浏览器：{dl}"));
                                            }
                                            cx.notify();
                                        })))
                                        .child(button("about-page", "查看发布页", false, true, cx.listener(move |this, _, _, cx| {
                                            if !open_url(&page_url) {
                                                this.link_error = Some(format!("无法打开默认浏览器：{page_url}"));
                                            }
                                            cx.notify();
                                        }))),
                                ),
                        )
                    })
                    .when(matches!(self.state, UpdateState::Failed(_)), |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(button("about-fallback", "打开发布页", false, true, cx.listener(|this, _, _, cx| {
                                    if !open_url(update::RELEASES_URL) {
                                        this.link_error = Some(format!("无法打开默认浏览器：{}", update::RELEASES_URL));
                                    }
                                    cx.notify();
                                })))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(12.))
                                        .text_color(theme::muted())
                                        .truncate()
                                        .child("直连与系统代理重试均失败；检查网络或代理设置后重试"),
                                ),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child("更新检查仅读取 GitHub 公开 API，不收集任何本机信息"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .bg(theme::panel())
                    .children(links.into_iter().map(|(label, url)| {
                        div()
                            .id(SharedString::from(format!("about-link-{label}")))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .py_2()
                            .rounded(px(8.))
                            .border_1()
                            .border_color(transparent())
                            .cursor_pointer()
                            .focusable()
                            .focus(|s| s.border_color(theme::accent()))
                            .hover(|s| s.bg(theme::panel_hover()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if open_url(url) {
                                    this.link_error = None;
                                } else {
                                    this.link_error = Some(format!("无法打开默认浏览器：{url}"));
                                }
                                cx.notify();
                            }))
                            .child(dot(theme::accent()))
                            .child(div().w(px(88.)).text_size(px(13.)).child(label))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_right()
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .truncate()
                                    .child(url.trim_start_matches("https://")),
                            )
                    }))
                    .when(self.link_error.is_some(), |d| {
                        d.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_size(px(12.))
                                .text_color(theme::danger())
                                .truncate()
                                .child(self.link_error.clone().unwrap_or_default()),
                        )
                    }),
            )
    }
}