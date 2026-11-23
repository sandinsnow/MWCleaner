use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::core::registry::{self, CleanOutcome, RegIssue};
use crate::ui::{button, checkbox, danger_button, empty_state, open_in_explorer, page_header, pump, start_status_ticker, theme, ArmedConfirm, Status};

enum Msg {
    Scanned(u64, Vec<RegIssue>),
    Cleaned(Result<CleanOutcome, String>),
}

#[derive(PartialEq)]
enum Phase {
    Idle,
    Scanning,
    Scanned,
    Cleaning,
    Done,
}

pub struct RegistryPage {
    phase: Phase,
    issues: Vec<RegIssue>,
    status: Status,
    armed: ArmedConfirm<()>,
    cancel: Option<Arc<AtomicBool>>,
    cancelled: bool,
    scan_gen: u64,
    last_backup: Option<PathBuf>,
}

fn backups_dir() -> PathBuf {
    crate::core::app_data_dir().join("backups")
}

fn latest_backup() -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(backups_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("registry-"))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v.pop()
}

impl RegistryPage {
    const DEFAULT_STATUS: &'static str = "扫描无效卸载项、缺失 DLL 引用与无效 App Paths";

    pub fn new(cx: &mut Context<Self>) -> Self {
        let this = Self {
            phase: Phase::Idle,
            issues: Vec::new(),
            status: Status::new(),
            armed: ArmedConfirm::new(),
            cancel: None,
            cancelled: false,
            scan_gen: 0,
            last_backup: None,
        };
        start_status_ticker(cx);
        this
    }

    fn busy(&self) -> bool {
        self.phase == Phase::Scanning || self.phase == Phase::Cleaning
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Scanning;
        self.armed.reset();
        self.cancelled = false;
        self.scan_gen += 1;
        let gen = self.scan_gen;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        cx.notify();
        pump(
            cx,
            move |tx| {
                let _ = tx.try_send(Msg::Scanned(gen, registry::scan(&cancel)));
            },
            |this, msg, cx| {
                if let Msg::Scanned(g, issues) = msg {
                    if g != this.scan_gen {
                        return;
                    }
                    this.cancel = None;
                    if this.cancelled {
                        this.phase = Phase::Idle;
                        this.status.set("扫描已取消");
                    } else {
                        this.status.set(format!("发现 {} 个可疑项", issues.len()));
                        this.issues = issues;
                        this.phase = Phase::Scanned;
                    }
                }
                cx.notify();
            },
        );
    }

    fn start_clean(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Cleaning;
        self.armed.reset();
        cx.notify();
        let issues = self.issues.clone();
        pump(
            cx,
            move |tx| {
                let _ = tx.try_send(Msg::Cleaned(registry::backup_and_clean(&issues)));
            },
            |this, msg, cx| {
                if let Msg::Cleaned(r) = msg {
                    match r {
                        Ok(o) => {
                            this.status.set(format!(
                                "已删除 {} 项，失败 {} 项；备份：{}",
                                o.deleted,
                                o.failed,
                                o.backup.display()
                            ));
                            this.last_backup = Some(o.backup);
                        }
                        Err(e) => this.status.set(e),
                    }
                    this.phase = Phase::Done;
                }
                cx.notify();
            },
        );
    }
}

impl Render for RegistryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.issues.iter().filter(|i| i.selected).count();
        let can_clean = (self.phase == Phase::Scanned || self.phase == Phase::Done) && selected > 0;
        let shown = self.issues.len().min(500);
        let rows = self.issues.iter().take(500).enumerate().map(|(i, issue)| {
            div().flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2p5()
                .rounded_md()
                .bg(theme::panel())
                .child(checkbox(SharedString::from(format!("reg-chk-{i}")), issue.selected, cx.listener(move |this, _, _, cx| {
                    this.issues[i].selected = !this.issues[i].selected;
                    this.armed.reset();
                    cx.notify();
                })))
                .child(
                    div()
                        .w(px(110.))
                        .text_size(px(12.))
                        .text_color(theme::accent())
                        .child(issue.kind),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .truncate()
                        .child(issue.detail.clone()),
                )
        });

        let status_text = self.status.view(Self::DEFAULT_STATUS);
        let status_el = match self.last_backup.clone() {
            Some(p) => div()
                .id("reg-backup-link")
                .flex_1()
                .min_w_0()
                .text_right()
                .text_size(px(12.))
                .text_color(theme::accent())
                .truncate()
                .cursor_pointer()
                .focusable()
                .focus(|s| s.text_color(theme::accent_hover()))
                .on_click(cx.listener(move |_, _, _, _| {
                    open_in_explorer(&backups_dir(), Some(p.as_path()));
                }))
                .child(status_text)
                .into_any_element(),
            None => div()
                .flex_1()
                .min_w_0()
                .text_right()
                .text_size(px(12.))
                .text_color(theme::muted())
                .truncate()
                .child(status_text)
                .into_any_element(),
        };

        let empty_text = match &self.phase {
            Phase::Scanning => "正在扫描…",
            Phase::Cleaning => "正在清理…",
            Phase::Scanned | Phase::Done => "未发现可疑注册表项",
            Phase::Idle => if self.cancelled { "扫描已取消，点击「扫描」重新开始" } else { "点击「扫描」检查无效卸载项与 DLL 引用" },
        };

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("注册表清理", "清理前自动导出 .reg 备份到 %APPDATA%\\MWCleaner\\backups"))
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .overflow_hidden()
                    .child(button("reg-scan", "扫描", true, !self.busy(), cx.listener(|this, _, _, cx| this.start_scan(cx))))
                    .when(self.phase == Phase::Scanning, |d| {
                        d.child(button("reg-cancel", "取消", false, true, cx.listener(|this, _, _, cx| {
                            if let Some(c) = &this.cancel {
                                c.store(true, Ordering::Relaxed);
                            }
                            this.cancelled = true;
                            this.phase = Phase::Idle;
                            this.status.set("扫描已取消");
                            cx.notify();
                        })))
                    })
                    .child(danger_button("reg-clean",
                        if self.armed.any() { "再次点击确认清理" } else { "清理勾选项" },
                        can_clean,
                        cx.listener(|this, _, _, cx| {
                            if this.armed.click(()) {
                                this.start_clean(cx);
                            } else {
                                cx.notify();
                            }
                        }),
                    ))
                    .child(button("reg-backups", "打开备份目录", false, true, cx.listener(|_, _, _, _| {
                        open_in_explorer(&backups_dir(), latest_backup().as_deref());
                    })))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::muted())
                            .child(format!("已勾选 {selected} 项")),
                    )
                    .child(status_el),
            )
            .when(self.issues.len() > shown, |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format!("仅显示前 {shown} 项，清理将作用于全部勾选项")),
                )
            })
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("reg-list").overflow_y_scroll()
                    .children(rows)
                    .when(self.issues.is_empty(), move |d| d.child(empty_state(empty_text))),
            )
    }
}
