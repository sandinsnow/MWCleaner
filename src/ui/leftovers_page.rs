use gpui::*;
use gpui::prelude::*;

use crate::core::leftovers::{self, Leftover};
use crate::core::fmt_size;
use crate::ui::{button, checkbox, danger_button, page_header, progress_bar, pump, size_bar, theme};

enum Msg {
    Progress(usize, usize, String),
    Scanned(Vec<Leftover>),
    Cleaned(u64, usize),
}

#[derive(PartialEq)]
enum Phase {
    Idle,
    Scanning,
    Scanned,
    Cleaning,
    Done,
}

pub struct LeftoversPage {
    phase: Phase,
    items: Vec<Leftover>,
    progress: (usize, usize, String),
    status: SharedString,
    armed: bool,
}

impl LeftoversPage {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            items: Vec::new(),
            progress: (0, 1, String::new()),
            status: "对比已安装程序列表，找出疑似残留目录（启发式判断，请人工确认后再勾选）".into(),
            armed: false,
        }
    }

    fn busy(&self) -> bool {
        self.phase == Phase::Scanning || self.phase == Phase::Cleaning
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Scanning;
        self.armed = false;
        cx.notify();
        pump(
            cx,
            |tx| {
                let items = leftovers::scan(&|i, t, l| {
                    let _ = tx.try_send(Msg::Progress(i, t, l.to_string()));
                });
                let _ = tx.try_send(Msg::Scanned(items));
            },
            |this, msg, cx| {
                match msg {
                    Msg::Progress(i, t, l) => this.progress = (i, t, l),
                    Msg::Scanned(items) => {
                        this.status = format!("发现 {} 个疑似残留目录，默认不勾选", items.len()).into();
                        this.items = items;
                        this.phase = Phase::Scanned;
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
        let items = self.items.clone();
        pump(
            cx,
            move |tx| {
                let (freed, failed) = leftovers::clean(&items);
                let _ = tx.try_send(Msg::Cleaned(freed, failed));
            },
            |this, msg, cx| {
                if let Msg::Cleaned(freed, failed) = msg {
                    this.status = format!("清理完成：释放 {}，失败 {failed} 项", fmt_size(freed)).into();
                    this.phase = Phase::Done;
                }
                cx.notify();
            },
        );
    }
}

impl Render for LeftoversPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_size: u64 = self.items.iter().filter(|i| i.selected).map(|i| i.size).sum();
        let has_selection = self.items.iter().any(|i| i.selected);
        let can_clean = (self.phase == Phase::Scanned || self.phase == Phase::Done) && has_selection;
        let max_size = self.items.iter().map(|i| i.size).max().unwrap_or(1);

        let rows = self.items.iter().enumerate().map(|(i, item)| {
            div().flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2p5()
                .rounded_md()
                .bg(theme::panel())
                .child(checkbox(SharedString::from(format!("lo-chk-{i}")), item.selected, cx.listener(move |this, _, _, cx| {
                    this.items[i].selected = !this.items[i].selected;
                    this.armed = false;
                    cx.notify();
                })))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(13.))
                                .truncate()
                                .child(item.path.to_string_lossy().to_string()),
                        )
                        .child(size_bar(item.size as f32 / max_size as f32, theme::accent())),
                )
                .child(div().w(px(90.)).text_right().child(fmt_size(item.size)))
        });

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("卸载残留", "Program Files / AppData / ProgramData 中无对应已安装程序的目录"))
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .child(button("lo-scan", "扫描", true, !self.busy(), cx.listener(|this, _, _, cx| this.start_scan(cx))))
                    .child(danger_button("lo-clean", 
                        if self.armed { "再次点击确认删除" } else { "删除勾选项" },
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
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::muted())
                            .child(format!("已勾选 {}", fmt_size(selected_size))),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .truncate()
                            .child(self.status.clone()),
                    ),
            )
            .when(self.busy(), |d| {
                let (i, t, _) = &self.progress;
                let f = if *t == 0 { 0.0 } else { *i as f32 / *t as f32 };
                d.child(progress_bar(f))
            })
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("lo-list").overflow_y_scroll()
                    .children(rows),
            )
    }
}