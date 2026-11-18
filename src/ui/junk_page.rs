use gpui::*;
use gpui::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::core::junk::{self, CleanResult, JunkCategory};
use crate::core::settings;
use crate::ui::{button, chart_color, checkbox, danger_button, dot, page_header, progress_bar, pump, size_bar, stacked_bar, theme};

enum Msg {
    ScanProgress(usize, usize, String),
    Scanned(Vec<JunkCategory>),
    CleanProgress(usize, usize, String),
    Cleaned(CleanResult),
}

#[derive(PartialEq)]
enum Phase {
    Idle,
    Scanning,
    Scanned,
    Cleaning,
    Done,
}

pub struct JunkPage {
    phase: Phase,
    cats: Vec<JunkCategory>,
    progress: (usize, usize, String),
    result: Option<CleanResult>,
    armed: bool,
    cancel: Option<Arc<AtomicBool>>,
    cancelled: bool,
}

impl JunkPage {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            cats: Vec::new(),
            progress: (0, 1, String::new()),
            result: None,
            armed: false,
            cancel: None,
            cancelled: false,
        }
    }

    fn busy(&self) -> bool {
        self.phase == Phase::Scanning || self.phase == Phase::Cleaning
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Scanning;
        self.result = None;
        self.armed = false;
        self.cancelled = false;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        cx.notify();
        pump(
            cx,
            move |tx| {
                let cats = junk::scan(
                    &|i, t, l| {
                        let _ = tx.try_send(Msg::ScanProgress(i, t, l.to_string()));
                    },
                    &cancel,
                );
                let _ = tx.try_send(Msg::Scanned(cats));
            },
            |this, msg, cx| {
                match msg {
                    Msg::ScanProgress(i, t, l) => this.progress = (i, t, l),
                    Msg::Scanned(cats) => {
                        this.cancel = None;
                        if this.cancelled {
                            this.phase = Phase::Idle;
                        } else {
                            this.cats = cats;
                            this.phase = Phase::Scanned;
                        }
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
    }

    fn start_clean(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Cleaning;
        self.armed = false;
        cx.notify();
        let cats = self.cats.clone();
        let to_recycle = settings::load_flag("RecycleBin", true);
        pump(
            cx,
            move |tx| {
                let r = junk::clean(
                    &cats,
                    &|i, t, l| {
                        let _ = tx.try_send(Msg::CleanProgress(i, t, l.to_string()));
                    },
                    to_recycle,
                );
                let _ = tx.try_send(Msg::Cleaned(r));
            },
            |this, msg, cx| {
                match msg {
                    Msg::CleanProgress(i, t, l) => this.progress = (i, t, l),
                    Msg::Cleaned(r) => {
                        this.result = Some(r);
                        this.phase = Phase::Done;
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
    }

}

impl Render for JunkPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_size: u64 = self.cats.iter().filter(|c| c.selected).map(|c| c.size).sum();
        let total_size: u64 = self.cats.iter().map(|c| c.size).sum();
        let has_selection = self.cats.iter().any(|c| c.selected && c.size > 0);
        let can_clean = (self.phase == Phase::Scanned || self.phase == Phase::Done) && has_selection;
        let max_size = self.cats.iter().map(|c| c.size).max().unwrap_or(1);

        let rows = self.cats.iter().enumerate().map(|(i, cat)| {
            let size = crate::core::fmt_size(cat.size);
            let count = cat.file_count;
            div().flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_3()
                .rounded_md()
                .bg(theme::panel())
                .child(checkbox(SharedString::from(format!("junk-chk-{i}")), cat.selected, cx.listener(move |this, _, _, cx| {
                    this.cats[i].selected = !this.cats[i].selected;
                    this.armed = false;
                    cx.notify();
                })))
                .child(
                    div().flex().flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div().flex().items_center().gap_2()
                                .child(dot(chart_color(i)))
                                .child(div().child(cat.name)),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .child(cat.description),
                        )
                        .child(size_bar(cat.size as f32 / max_size as f32, chart_color(i))),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(format!("{count} 项")),
                )
                .child(div().w(px(90.)).text_right().child(size))
        });

        let status: SharedString = match &self.phase {
            Phase::Idle => {
                if self.cancelled {
                    "扫描已取消".into()
                } else {
                    "点击「扫描」分析可清理的垃圾文件".into()
                }
            }
            Phase::Scanning => format!("正在扫描：{}", self.progress.2).into(),
            Phase::Scanned => format!(
                "扫描完成，共发现 {} 垃圾，已勾选 {}",
                crate::core::fmt_size(total_size),
                crate::core::fmt_size(selected_size)
            )
            .into(),
            Phase::Cleaning => format!("正在清理：{}", self.progress.2).into(),
            Phase::Done => {
                let r = self.result.as_ref().unwrap();
                if r.recycled {
                    format!(
                        "清理完成：{} 已移入回收站，{} 项被占用或无权限已跳过",
                        crate::core::fmt_size(r.freed),
                        r.failed
                    )
                    .into()
                } else {
                    format!(
                        "清理完成：释放 {}，{} 个文件被占用或无权限已跳过",
                        crate::core::fmt_size(r.freed),
                        r.failed
                    )
                    .into()
                }
            }
        };

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("垃圾清理", "扫描并清理临时文件、浏览器缓存、回收站等"))
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .child(button("junk-scan", "扫描", true, !self.busy(), cx.listener(|this, _, _, cx| this.start_scan(cx))))
                    .when(self.phase == Phase::Scanning, |d| {
                        d.child(button("junk-cancel", "取消", false, true, cx.listener(|this, _, _, cx| {
                            if let Some(c) = &this.cancel {
                                c.store(true, Ordering::Relaxed);
                            }
                            this.cancelled = true;
                            this.phase = Phase::Idle;
                            cx.notify();
                        })))
                    })
                    .child(danger_button("junk-clean", 
                        if self.armed { "再次点击确认清理" } else { "清理勾选项" },
                        can_clean,
                        cx.listener(|this, _, _, cx| {
                            if this.armed {
                                this.start_clean(cx);
                            } else {
                                this.armed = true;
                                cx.notify();
                            }
                        }),
                    ))
                    .child(div().flex_1().min_w_0().text_right().text_size(px(13.)).text_color(theme::muted()).truncate().child(status)),
            )
            .when(self.busy(), |d| {
                let (i, t, _) = &self.progress;
                let f = if *t == 0 { 0.0 } else { *i as f32 / *t as f32 };
                d.child(progress_bar(f))
            })
            .when(total_size > 0 && !self.busy(), |d| {
                let segs: Vec<(f32, Rgba)> = self.cats.iter().enumerate()
                    .filter(|(_, c)| c.size > 0)
                    .map(|(i, c)| (c.size as f32 / total_size as f32, chart_color(i)))
                    .collect();
                d.child(stacked_bar(&segs))
            })
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("junk-list").overflow_y_scroll()
                    .children(rows),
            )
    }
}