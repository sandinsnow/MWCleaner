use gpui::*;
use gpui::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::core::junk::{self, failure_hint, CleanResult, FailKind, JunkCategory};
use crate::core::settings;
use crate::ui::{button, chart_color, checkbox, danger_button, dot, empty_state, page_header, progress_bar, pump, size_bar, stacked_bar, start_status_ticker, theme, ArmedConfirm, STATUS_TTL};

enum Msg {
    ScanProgress(u64, usize, usize, String),
    Scanned(u64, Vec<JunkCategory>),
    CleanProgress(u64, usize, usize, String),
    Cleaned(u64, CleanResult),
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
    armed: ArmedConfirm<()>,
    cancel: Option<Arc<AtomicBool>>,
    cancelled: bool,
    gen: u64,
    done_at: Option<Instant>,
}

impl JunkPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let this = Self {
            phase: Phase::Idle,
            cats: Vec::new(),
            progress: (0, 1, String::new()),
            result: None,
            armed: ArmedConfirm::new(),
            cancel: None,
            cancelled: false,
            gen: 0,
            done_at: None,
        };
        start_status_ticker(cx);
        this
    }

    fn busy(&self) -> bool {
        self.phase == Phase::Scanning || self.phase == Phase::Cleaning
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Scanning;
        self.result = None;
        self.armed.reset();
        self.cancelled = false;
        self.done_at = None;
        self.gen += 1;
        let gen = self.gen;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        cx.notify();
        pump(
            cx,
            move |tx| {
                let cats = junk::scan(
                    &|i, t, l| {
                        let _ = tx.try_send(Msg::ScanProgress(gen, i, t, l.to_string()));
                    },
                    &cancel,
                );
                let _ = tx.try_send(Msg::Scanned(gen, cats));
            },
            |this, msg, cx| {
                match msg {
                    Msg::ScanProgress(g, i, t, l) if g == this.gen => this.progress = (i, t, l),
                    Msg::Scanned(g, cats) if g == this.gen => {
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

    fn start_clean(&mut self, owner_hwnd: isize, cx: &mut Context<Self>) {
        self.phase = Phase::Cleaning;
        self.armed.reset();
        self.cancelled = false;
        self.done_at = None;
        self.gen += 1;
        let gen = self.gen;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        cx.notify();
        let cats = self.cats.clone();
        let to_recycle = settings::load_flag("RecycleBin", true);
        pump(
            cx,
            move |tx| {
                let r = junk::clean(
                    &cats,
                    &|i, t, l| {
                        let _ = tx.try_send(Msg::CleanProgress(gen, i, t, l.to_string()));
                    },
                    to_recycle,
                    &cancel,
                    owner_hwnd,
                );
                let _ = tx.try_send(Msg::Cleaned(gen, r));
            },
            |this, msg, cx| {
                match msg {
                    Msg::CleanProgress(g, i, t, l) if g == this.gen => this.progress = (i, t, l),
                    Msg::Cleaned(g, r) if g == this.gen => {
                        this.cancel = None;
                        this.result = Some(r);
                        this.phase = Phase::Done;
                        this.done_at = Some(Instant::now());
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
        // SEC-2：勾选回收站类别意味着永久清空，需要显式强提示。
        let has_recycle_selected = self.cats.iter().any(|c| c.recycle_bin && c.selected);
        // C6：勾选升级残留意味着强制直删且无法回退旧系统，独立红字警示（两者可并列）。
        let has_upgrade_selected = self.cats.iter().any(|c| c.upgrade_remnant && c.selected);

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
                    this.armed.reset();
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
                // B6：完成结果 8 秒后自动回落为简短提示，不永久占用状态栏。
                if self.done_at.map_or(true, |t| t.elapsed() >= STATUS_TTL) {
                    "清理完成，点击「扫描」可重新分析".into()
                } else {
                    // COR-3/口径区分：freed=已真正释放（直删/清空回收站），recycled_bytes=移入回收站暂未释放
                    let r = self.result.as_ref().unwrap();
                    let mut parts: Vec<String> = Vec::new();
                    if r.freed > 0 {
                        parts.push(format!("已永久释放 {}", crate::core::fmt_size(r.freed)));
                    }
                    if r.recycled_bytes > 0 {
                        parts.push(format!(
                            "移入回收站 {}（清空回收站后释放）",
                            crate::core::fmt_size(r.recycled_bytes)
                        ));
                    }
                    let tail = if parts.is_empty() { "无已清理项".to_string() } else { parts.join("，") };
                    if r.cancelled {
                        format!("已取消清理：{tail}，其余项保留在磁盘").into()
                    } else if r.failed > 0 {
                        // 按失败构成细分：占用给服务暂停指引，无权限建议保留，避免笼统「无权限」误导
                        let hint = failure_hint(r.fail_denied, r.fail_locked, r.fail_other)
                            .unwrap_or_else(|| format!("{} 项未能清理", r.failed));
                        format!("清理完成：{tail}，{hint}").into()
                    } else {
                        format!("清理完成：{tail}").into()
                    }
                }
            }
        };

        let empty_text = match &self.phase {
            Phase::Scanning => "正在扫描…",
            Phase::Cleaning => "正在清理…",
            Phase::Scanned | Phase::Done => "未发现可清理的垃圾文件",
            Phase::Idle => if self.cancelled { "扫描已取消，点击「扫描」重新开始" } else { "点击「扫描」分析可清理的垃圾文件" },
        };

        // 失败清单：展示前 5 个失败路径 + 原因分类，与结果文案同样在 TTL 后隐藏。
        // r.cancelled 时隐藏清单：取消态下用户主动终止，列取消前已发生的失败反而令人
        // 困惑，信息丢失可接受（取舍）。
        let failure_rows: Vec<SharedString> = match self.result.as_ref() {
            Some(r)
                if self.phase == Phase::Done
                    && r.failed > 0
                    && !r.cancelled
                    && self.done_at.map_or(true, |t| t.elapsed() < STATUS_TTL) =>
            {
                let mut lines: Vec<SharedString> = Vec::new();
                if r.failures.is_empty() {
                    // 无逐文件明细（如清空回收站整体失败，Shell 不提供逐文件归因）：
                    // 给总量说明行，避免空清单只有「等 N 项」的观感
                    lines.push(SharedString::from(format!(
                        "{} 项失败（Shell 未提供逐文件明细）",
                        r.failed
                    )));
                } else {
                    lines.extend(r.failures.iter().take(5).map(|(p, k)| {
                        let why = match k {
                            FailKind::Locked => "被占用",
                            FailKind::Denied => "无权限",
                            FailKind::ReadOnly => "只读",
                            FailKind::Other => "其他",
                        };
                        SharedString::from(format!("{}（{why}）", p.display()))
                    }));
                    if r.failed as usize > 5 {
                        lines.push(SharedString::from(format!("……等 {} 项失败", r.failed)));
                    }
                }
                lines
            }
            _ => Vec::new(),
        };
        let has_failures = !failure_rows.is_empty();

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("垃圾清理", "扫描并清理临时文件、浏览器缓存、回收站等"))
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .overflow_hidden()
                    .child(button("junk-scan", "扫描", true, !self.busy(), cx.listener(|this, _, _, cx| this.start_scan(cx))))
                    .when(self.busy(), |d| {
                        d.child(button("junk-cancel", "取消", false, true, cx.listener(|this, _, _, cx| {
                            if let Some(c) = &this.cancel {
                                c.store(true, Ordering::Relaxed);
                            }
                            this.cancelled = true;
                            if this.phase == Phase::Scanning {
                                this.phase = Phase::Idle;
                            }
                            cx.notify();
                        })))
                    })
                    .child(danger_button("junk-clean",
                        if self.armed.any() {
                            if has_upgrade_selected {
                                // C6：升级残留警示文案优先（强制直删、不可恢复）
                                "确认：永久删除 Windows 升级残留"
                            } else if has_recycle_selected { "确认：将永久清空回收站" } else { "再次点击确认清理" }
                        } else { "清理勾选项" },
                        can_clean,
                        cx.listener(|this, _, window, cx| {
                            if this.armed.click(()) {
                                this.start_clean(super::controls::window_hwnd(window), cx);
                            } else {
                                cx.notify();
                            }
                        }),
                    ))
                    .when(has_upgrade_selected, |d| {
                        d.child(div().text_size(px(11.)).text_color(theme::danger())
                            .child("警告：Windows 升级残留删除后无法回退到旧版 Windows，且不经过回收站，不可恢复"))
                    })
                    .when(has_recycle_selected, |d| {
                        d.child(div().text_size(px(11.)).text_color(theme::danger())
                            .child("警告：已勾选「回收站」，清理将永久清空回收站，不可恢复"))
                    })
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
            .when(has_failures, move |d| {
                d.child(
                    div().flex().flex_col()
                        .gap_1()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(theme::panel())
                        .children(failure_rows.into_iter().map(|s| {
                            div().text_size(px(11.)).text_color(theme::muted()).truncate().child(s)
                        })),
                )
            })
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("junk-list").overflow_y_scroll()
                    .children(rows)
                    .when(self.cats.is_empty(), move |d| d.child(empty_state(empty_text))),
            )
    }
}
