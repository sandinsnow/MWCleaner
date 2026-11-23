use gpui::*;
use gpui::prelude::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::core::bigfiles::{self, DeleteMode, DriveInfo, ScanResult};
use crate::core::{csv_escape, exports_dir, fmt_size, timestamp_name, write_csv_lines};
use crate::ui::{button, danger_button, empty_state, filter_chips, open_in_explorer, page_header, pump, size_bar, start_status_ticker, theme, usage_bar, ArmedConfirm, Status};

enum Msg {
    Progress(u64, String),
    Done(ScanResult),
    Deleted(PathBuf, DeleteMode, std::io::Result<()>),
}

#[derive(PartialEq)]
enum Tab {
    Files,
    Dirs,
}

/// B4：列表排序（core 固定大小降序不变，UI 层排渲染副本）
#[derive(Clone, Copy, PartialEq)]
enum SortMode {
    SizeDesc,
    SizeAsc,
    Name,
}

pub struct BigFilesPage {
    drives: Vec<DriveInfo>,
    drive: Option<String>,
    scanning: bool,
    cancel: Option<Arc<AtomicBool>>,
    result: Option<ScanResult>,
    progress: (u64, String),
    status: Status,
    tab: Tab,
    sort: SortMode,
    /// 最近一次成功导出的 CSV 路径（用于「打开目录」定位）
    exported: Option<PathBuf>,
    armed_delete: ArmedConfirm<PathBuf>,
}

impl BigFilesPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let drives = bigfiles::drives();
        let drive = drives.first().map(|d| d.letter.clone());
        let this = Self {
            drives,
            drive,
            scanning: false,
            cancel: None,
            result: None,
            progress: (0, String::new()),
            status: Status::new(),
            tab: Tab::Files,
            sort: SortMode::SizeDesc,
            exported: None,
            armed_delete: ArmedConfirm::new(),
        };
        start_status_ticker(cx);
        this
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        let Some(letter) = self.drive.clone() else { return };
        self.scanning = true;
        self.result = None;
        self.armed_delete.reset();
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
                        // COR-4：取消时明确标注结果不完整，避免误读为全盘统计。
                        this.status.set(format!(
                            "扫描{}：{} 个文件，共 {}；大文件 {} 个，大文件夹 {} 个",
                            if r.cancelled { "已取消（仅部分结果）" } else { "完成" },
                            r.scanned_files,
                            fmt_size(r.scanned_bytes),
                            r.files.len(),
                            r.dirs.len()
                        ));
                        this.result = Some(r);
                        this.scanning = false;
                        this.cancel = None;
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
    }

    /// SEC-1：按用户显式选择的方式删除；回收站失败时保留文件并给出可操作提示。
    /// SEC-3：传主窗口 HWND，让 Shell 弹窗归属主窗口。
    fn delete(&mut self, path: PathBuf, mode: DeleteMode, owner_hwnd: isize, cx: &mut Context<Self>) {
        self.armed_delete.reset();
        cx.notify();
        pump(
            cx,
            move |tx| {
                let r = bigfiles::delete_path_mode(&path, mode, owner_hwnd);
                let _ = tx.try_send(Msg::Deleted(path, mode, r));
            },
            |this, msg, cx| {
                if let Msg::Deleted(path, mode, r) = msg {
                    let mode_text = if mode == DeleteMode::Recycle { "移入回收站" } else { "永久删除" };
                    match r {
                        Ok(()) => {
                            if let Some(res) = &mut this.result {
                                res.files.retain(|f| f.path != path);
                                res.dirs.retain(|d| d.path != path);
                            }
                            this.status.set(if mode == DeleteMode::Recycle {
                                format!("已移入回收站 {}（可在回收站还原）", path.display())
                            } else {
                                format!("已永久删除 {}（不可恢复）", path.display())
                            });
                        }
                        Err(e) => this.status.set(format!("{mode_text}失败：{e}")),
                    }
                }
                cx.notify();
            },
        );
    }

    /// B4：导出当前扫描结果全量（文件 + 文件夹）为 CSV；成功后可用「打开目录」定位。
    fn export_csv(&mut self, cx: &mut Context<Self>) {
        let Some(r) = &self.result else { return };
        let mut rows = vec!["类型,大小字节,大小可读,路径".to_string()];
        for f in &r.files {
            rows.push(format!(
                "文件,{},{},{}",
                f.size,
                fmt_size(f.size),
                csv_escape(&f.path.display().to_string())
            ));
        }
        for d in &r.dirs {
            rows.push(format!(
                "文件夹,{},{},{}",
                d.size,
                fmt_size(d.size),
                csv_escape(&d.path.display().to_string())
            ));
        }
        let path = exports_dir().join(format!("bigfiles-{}.csv", timestamp_name()));
        match write_csv_lines(&path, &rows) {
            Ok(()) => {
                self.exported = Some(path.clone());
                self.status.set(format!(
                    "已导出 {} 个条目到 {}",
                    rows.len() - 1,
                    path.display()
                ));
            }
            Err(e) => self.status.set(format!("导出失败：{e}")),
        }
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
                .focusable()
                .focus(|s| s.border_color(theme::accent()))
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
            let mut entries: Vec<(PathBuf, u64)> = if self.tab == Tab::Files {
                r.files.iter().map(|f| (f.path.clone(), f.size)).collect()
            } else {
                r.dirs.iter().map(|d| (d.path.clone(), d.size)).collect()
            };
            // B4：UI 层排序副本（core 固定大小降序不变），默认「大小↓」与现状一致
            match self.sort {
                SortMode::SizeDesc => entries.sort_by(|a, b| b.1.cmp(&a.1)),
                SortMode::SizeAsc => entries.sort_by(|a, b| a.1.cmp(&b.1)),
                SortMode::Name => entries.sort_by(|a, b| {
                    a.0.to_string_lossy().to_lowercase()
                        .cmp(&b.0.to_string_lossy().to_lowercase())
                }),
            }
            let max_size = entries.iter().map(|(_, s)| *s).max().unwrap_or(1);
            let entries_empty = entries.is_empty();
            list = list.children(entries.into_iter().enumerate().map(|(i, (path, size))| {
                let armed = self.armed_delete.is_armed(&path);
                let protected = bigfiles::is_protected(&path);
                let p1 = path.clone();
                let p2 = path.clone();
                let p3 = path.clone();
                let p4 = path.clone();
                // SEC-1：两击确认后展开双选项，默认高亮可还原的「移入回收站」。
                let action: AnyElement = if protected {
                    danger_button(
                        SharedString::from(format!("bf-del-{i}")),
                        "受保护",
                        false,
                        |_, _, _| {},
                    )
                    .into_any_element()
                } else if armed {
                    div().flex().items_center().gap_2()
                        .child(div().text_size(px(11.)).text_color(theme::warning()).child("选择删除方式："))
                        .child(button(
                            SharedString::from(format!("bf-recycle-{i}")),
                            "移入回收站（可还原）",
                            true,
                            true,
                            cx.listener(move |this, _, window, cx| {
                                this.delete(p2.clone(), DeleteMode::Recycle, super::controls::window_hwnd(window), cx);
                            }),
                        ))
                        .child(danger_button(
                            SharedString::from(format!("bf-perma-{i}")),
                            "永久删除（不可恢复）",
                            true,
                            cx.listener(move |this, _, _, cx| {
                                this.delete(p3.clone(), DeleteMode::Permanent, 0, cx);
                            }),
                        ))
                        .child(button(
                            SharedString::from(format!("bf-armcancel-{i}")),
                            "取消",
                            false,
                            true,
                            cx.listener(move |this, _, _, cx| {
                                this.armed_delete.reset();
                                cx.notify();
                            }),
                        ))
                        .into_any_element()
                } else {
                    danger_button(
                        SharedString::from(format!("bf-del-{i}")),
                        "删除",
                        true,
                        cx.listener(move |this, _, _, cx| {
                            let _ = this.armed_delete.click(p4.clone());
                            cx.notify();
                        }),
                    )
                    .into_any_element()
                };
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
                    .child(action)
            }));
            if entries_empty {
                list = list.child(empty_state(if self.tab == Tab::Files {
                    "未发现达到阈值的大文件"
                } else {
                    "未发现 ≥ 1GB 的大文件夹"
                }));
            }
        } else {
            list = list.child(empty_state("选择盘符后点击「扫描」开始分析"));
        }

        let default_status: SharedString = format!(
            "选择盘符后开始扫描（大文件 ≥ {}MB，大文件夹 ≥ 1GB）",
            crate::core::settings::load_u32("BigFileMinMB", 100)
        )
        .into();
        let status_text = self.status.view(default_status);

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("大文件分析", "找出占用空间最大的文件与文件夹；删除默认进回收站"))
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
                    .child(div().w(px(1.)).h(px(16.)).bg(theme::track()))
                    // B4：排序 chips 与 CSV 导出（扫描完成后可用）
                    .children(filter_chips(
                        "bf-sort",
                        vec![
                            (SortMode::SizeDesc, "大小↓"),
                            (SortMode::SizeAsc, "大小↑"),
                            (SortMode::Name, "名称"),
                        ],
                        self.sort,
                        |f| cx.listener(move |this, _, _, cx| {
                            this.sort = f;
                            cx.notify();
                        }),
                    ))
                    .child(button("bf-export", "导出 CSV", false, self.result.is_some(), cx.listener(|this, _, _, cx| {
                        this.export_csv(cx);
                    })))
                    .when(self.exported.is_some(), |d| {
                        let exported = self.exported.clone().unwrap();
                        d.child(button("bf-open-exports", "打开目录", false, true, cx.listener(move |_, _, _, _| {
                            open_in_explorer(&exports_dir(), Some(exported.as_path()));
                        })))
                    })
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
