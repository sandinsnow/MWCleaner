use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::core::bigfiles::{self, DriveInfo, ScanResult};
use crate::core::fmt_size;
use crate::ui::{button, danger_button, page_header, pump, size_bar, theme, usage_bar};

enum Msg {
    Progress(u64, String),
    Done(ScanResult),
}

#[derive(PartialEq)]
enum Tab {
    Files,
    Dirs,
}

pub struct BigFilesPage {
    drives: Vec<DriveInfo>,
    drive: Option<String>,
    scanning: bool,
    cancel: Option<Arc<AtomicBool>>,
    result: Option<ScanResult>,
    progress: (u64, String),
    status: SharedString,
    tab: Tab,
    armed_delete: Option<PathBuf>,
}

impl BigFilesPage {
    pub fn new() -> Self {
        let drives = bigfiles::drives();
        let drive = drives.first().map(|d| d.letter.clone());
        Self {
            drives,
            drive,
            scanning: false,
            cancel: None,
            result: None,
            progress: (0, String::new()),
            status: SharedString::from(""),
            tab: Tab::Files,
            armed_delete: None,
        }
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        let Some(letter) = self.drive.clone() else { return };
        self.scanning = true;
        self.result = None;
        self.armed_delete = None;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        cx.notify();
        pump(
            cx,
            move |tx| {
                let r = bigfiles::scan(
                    &PathBuf::from(&letter),
                    crate::core::settings::load_u32("BigFileMinMB", 100) as u64 * 1024 * 1024,
                    1024 * 1024 * 1024,
                    &cancel,
                    &|n, dir| {
                        let _ = tx.try_send(Msg::Progress(n, dir.to_string()));
                    },
                );
                let _ = tx.try_send(Msg::Done(r));
            },
            |this, msg, cx| {
                match msg {
                    Msg::Progress(n, dir) => this.progress = (n, dir),
                    Msg::Done(r) => {
                        this.status = format!(
                            "扫描{}：{} 个文件，共 {}；大文件 {} 个，大文件夹 {} 个",
                            if r.cancelled { "已取消" } else { "完成" },
                            r.scanned_files,
                            fmt_size(r.scanned_bytes),
                            r.files.len(),
                            r.dirs.len()
                        )
                        .into();
                        this.result = Some(r);
                        this.scanning = false;
                        this.cancel = None;
                    }
                }
                cx.notify();
            },
        );
    }

    fn delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match bigfiles::delete_path(&path) {
            Ok(()) => {
                if let Some(r) = &mut self.result {
                    r.files.retain(|f| f.path != path);
                    r.dirs.retain(|d| d.path != path);
                }
                self.status = format!("已删除 {}", path.display()).into();
            }
            Err(e) => self.status = format!("删除失败：{e}").into(),
        }
        self.armed_delete = None;
        cx.notify();
    }
}

impl Render for BigFilesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let drive_cards = self.drives.iter().map(|d| {
            let letter = d.letter.clone();
            let active = self.drive.as_deref() == Some(letter.as_str());
            let used = d.total.saturating_sub(d.free);
            let frac = if d.total > 0 { used as f32 / d.total as f32 } else { 0. };
            let pct = (frac * 100.0).round() as u32;
            let vol_label = if d.label.is_empty() { "本地磁盘".to_string() } else { d.label.clone() };
            let letter_disp = letter.trim_end_matches('\\').to_string();
            div()
                .id(SharedString::from(format!("drive-{letter}")))
                .w(px(230.))
                .p_3()
                .rounded(px(10.))
                .border_1()
                .cursor_pointer()
                .flex()
                .flex_col()
                .gap_2()
                .border_color(if active { theme::accent() } else { theme::border() })
                .bg(if active { theme::accent_soft() } else { theme::panel() })
                .hover(move |s| {
                    s.border_color(theme::accent())
                        .bg(if active { theme::accent_soft() } else { theme::panel_hover() })
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.drive = Some(letter.clone());
                    cx.notify();
                }))
                .child(
                    div().flex().items_center().gap_2()
                        .child(
                            div().text_size(px(18.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if active { theme::accent() } else { theme::text() })
                                .child(letter_disp),
                        )
                        .child(
                            div().flex_1().min_w_0()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .truncate()
                                .child(vol_label),
                        )
                        .child(
                            div().text_size(px(12.))
                                .text_color(theme::muted())
                                .child(format!("{pct}%")),
                        ),
                )
                .child(usage_bar(frac))
                .child(
                    div().text_size(px(11.))
                        .text_color(theme::muted())
                        .child(format!("已用 {} / {}", fmt_size(used), fmt_size(d.total))),
                )
                .child(
                    div().text_size(px(11.))
                        .text_color(theme::muted())
                        .child(format!("剩余 {}", fmt_size(d.free))),
                )
        });

        let list_min = if self.result.is_some() { px(90.) } else { px(0.) };
        let mut list = div().flex().flex_col().flex_1().min_h(list_min).gap_2().id("bf-list").overflow_y_scroll();
        if let Some(r) = &self.result {
            let entries: Vec<(PathBuf, u64)> = if self.tab == Tab::Files {
                r.files.iter().map(|f| (f.path.clone(), f.size)).collect()
            } else {
                r.dirs.iter().map(|d| (d.path.clone(), d.size)).collect()
            };
            let max_size = entries.iter().map(|(_, s)| *s).max().unwrap_or(1);
            list = list.children(entries.into_iter().enumerate().map(|(i, (path, size))| {
                let armed = self.armed_delete.as_ref() == Some(&path);
                let protected = matches!(
                    path.file_name().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()).as_deref(),
                    Some("pagefile.sys" | "hiberfil.sys" | "swapfile.sys")
                );
                let p1 = path.clone();
                let p2 = path.clone();
                let p3 = path.clone();
                div().flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(theme::panel())
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
                                    .child(path.to_string_lossy().to_string()),
                            )
                            .child(size_bar(size as f32 / max_size as f32, theme::accent())),
                    )
                    .child(div().w(px(90.)).text_right().child(fmt_size(size)))
                    .child(button(SharedString::from(format!("bf-loc-{i}")), "定位", false, true, cx.listener(move |_, _, _, _| {
                        let _ = std::process::Command::new("explorer")
                            .arg(format!("/select,\"{}\"", p1.display()))
                            .spawn();
                    })))
                    .child(danger_button(SharedString::from(format!("bf-del-{i}")),
                        if protected { "系统文件" } else if armed { "确认删除" } else { "删除" },
                        !protected,
                        cx.listener(move |this, _, _, cx| {
                            if this.armed_delete.as_ref() == Some(&p2) {
                                this.delete(p3.clone(), cx);
                            } else {
                                this.armed_delete = Some(p2.clone());
                                cx.notify();
                            }
                        }),
                    ))
            }));
        }

        let status_text: SharedString = if self.result.is_none() {
            format!(
                "选择盘符后开始扫描（大文件 ≥ {}MB，大文件夹 ≥ 1GB）",
                crate::core::settings::load_u32("BigFileMinMB", 100)
            )
            .into()
        } else {
            self.status.clone()
        };

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("大文件分析", "找出占用空间最大的文件与文件夹"))
            .child(
                div().id("bf-drives").flex().flex_wrap().gap_3().items_center()
                    .flex_shrink()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(drive_cards)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(format!(
                                "大文件阈值 {} MB（设置中可调）",
                                crate::core::settings::load_u32("BigFileMinMB", 100)
                            )),
                    ),
            )
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .overflow_hidden()
                    .child(button("bf-scan", "扫描", true, !self.scanning && self.drive.is_some(), cx.listener(|this, _, _, cx| this.start_scan(cx))))
                    .when(self.scanning, |d| {
                        d.child(button("bf-cancel", "取消", false, true, cx.listener(|this, _, _, _cx| {
                            if let Some(c) = &this.cancel {
                                c.store(true, Ordering::Relaxed);
                            }
                        })))
                    })
                    .child(button("bf-tab-files",
                        if self.tab == Tab::Files { "▶ 大文件" } else { "大文件" },
                        false,
                        true,
                        cx.listener(|this, _, _, cx| { this.tab = Tab::Files; cx.notify(); }),
                    ))
                    .child(button("bf-tab-dirs",
                        if self.tab == Tab::Dirs { "▶ 大文件夹" } else { "大文件夹" },
                        false,
                        true,
                        cx.listener(|this, _, _, cx| { this.tab = Tab::Dirs; cx.notify(); }),
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_right()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .truncate()
                            .child(status_text),
                    ),
            )
            .when(self.scanning, |d| {
                d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .truncate()
                            .child(format!("已扫描 {} 个文件 · {}", self.progress.0, self.progress.1)),
                    )
            })
            .child(list)
    }
}