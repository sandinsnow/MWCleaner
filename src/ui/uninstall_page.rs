use gpui::*;
use gpui::prelude::*;

use crate::core::fmt_size;
use crate::core::{csv_escape, exports_dir, sanitize_filename, timestamp_name, write_csv_lines};
use crate::core::settings;
use crate::core::uninstall::{self, AppEntry, LeftoverItem, LeftoverKind};
use crate::ui::text_input::SearchInput;
use crate::ui::{badge, button, checkbox, danger_button, empty_state, filter_chips, icon, page_header, pump, start_status_ticker, theme, ArmedConfirm, Status};

enum Msg {
    IconsReady,
    Uninstalled(i32),
    StillInstalled(Vec<AppEntry>),
    Refreshed(Vec<AppEntry>),
    Leftovers(Vec<LeftoverItem>),
    Cleaned(uninstall::CleanReport),
}

#[derive(PartialEq)]
enum Phase {
    List,
    Uninstalling,
    Leftovers,
    Cleaning,
    Done,
}

pub struct UninstallPage {
    apps: Vec<AppEntry>,
    phase: Phase,
    current: Option<usize>,
    leftovers: Vec<LeftoverItem>,
    armed_uninstall: ArmedConfirm<usize>,
    armed_clean: ArmedConfirm<()>,
    status: Status,
    loading: bool,
    /// B1：软件列表搜索（name/publisher/install_dir 不区分大小写 contains）
    search: Entity<SearchInput>,
    /// S2：残留列表「路径 A→Z」排序开关（UI 层渲染顺序，不改数据）
    leftover_sort_path: bool,
}

impl UninstallPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| SearchInput::new("搜索已安装软件…", cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            apps: Vec::new(),
            phase: Phase::List,
            current: None,
            leftovers: Vec::new(),
            armed_uninstall: ArmedConfirm::new(),
            armed_clean: ArmedConfirm::new(),
            status: Status::new(),
            loading: true,
            search,
            leftover_sort_path: false,
        };
        start_status_ticker(cx);
        // PERF-1：首次加载改走后台 pump，避免 GetFileVersionInfo 阻塞 UI 线程。
        this.status.set("正在加载已安装软件…");
        this.load_apps(cx);
        this
    }

    fn warm_icons(&self, cx: &mut Context<Self>) {
        let sources: Vec<String> = self.apps.iter().map(|a| a.icon_source().to_string()).collect();
        pump(
            cx,
            move |tx| {
                unsafe {
                    let _ = windows::Win32::System::Com::CoInitializeEx(
                        None,
                        windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                    );
                }
                for s in &sources {
                    icon::for_binary(s);
                }
                let _ = tx.try_send(Msg::IconsReady);
            },
            |_, _, cx| cx.notify(),
        );
    }

    fn list_status(&self) -> SharedString {
        format!("共 {} 个已安装软件 · 卸载后自动扫描残留", self.apps.len()).into()
    }

    fn load_apps(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        pump(
            cx,
            |tx| {
                let _ = tx.try_send(Msg::Refreshed(uninstall::list()));
            },
            |this, msg, cx| {
                if let Msg::Refreshed(apps) = msg {
                    this.apps = apps;
                    this.loading = false;
                    this.status.set(this.list_status());
                    this.warm_icons(cx);
                }
                cx.notify();
            },
        );
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::List;
        self.current = None;
        self.leftovers.clear();
        self.armed_uninstall.reset();
        self.armed_clean.reset();
        self.status.set("正在刷新已安装软件…");
        cx.notify();
        self.load_apps(cx);
    }

    fn start_uninstall(&mut self, i: usize, cx: &mut Context<Self>) {
        let app = self.apps[i].clone();
        self.phase = Phase::Uninstalling;
        self.current = Some(i);
        self.armed_uninstall.reset();
        self.status.set(format!("正在卸载「{}」…", app.name));
        cx.notify();
        pump(
            cx,
            move |tx| {
                let code = uninstall::run_uninstaller(&app).unwrap_or(-1);
                let _ = tx.try_send(Msg::Uninstalled(code));
                let installed = uninstall::list();
                if installed.iter().any(|a| a.reg_key == app.reg_key) {
                    let _ = tx.try_send(Msg::StillInstalled(installed));
                    return;
                }
                let items = uninstall::scan_leftovers(&app, &installed);
                let _ = tx.try_send(Msg::Leftovers(items));
            },
            |this, msg, cx| {
                match msg {
                    Msg::Uninstalled(code) => {
                        this.status.set(format!("卸载程序已退出（代码 {code}），正在扫描残留…"));
                    }
                    Msg::StillInstalled(apps) => {
                        this.status.set("卸载未完成（软件仍在已安装列表），已返回列表");
                        this.phase = Phase::List;
                        this.current = None;
                        this.apps = apps;
                        this.warm_icons(cx);
                    }
                    Msg::Leftovers(items) => {
                        this.status.set(format!(
                            "发现 {} 项残留（默认不勾选，请人工确认后清理）",
                            items.len()
                        ));
                        this.leftovers = items;
                        this.phase = Phase::Leftovers;
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
    }

    fn start_clean(&mut self, owner_hwnd: isize, cx: &mut Context<Self>) {
        self.phase = Phase::Cleaning;
        self.armed_clean.reset();
        cx.notify();
        let items = self.leftovers.clone();
        pump(
            cx,
            move |tx| {
                let report = uninstall::clean(&items, settings::load_flag("RecycleBin", true), owner_hwnd);
                let _ = tx.try_send(Msg::Cleaned(report));
            },
            |this, msg, cx| {
                if let Msg::Cleaned(r) = msg {
                    let mut s = format!("清理完成：释放 {}，失败 {} 项", fmt_size(r.freed), r.failed);
                    if let Some(b) = &r.backup {
                        s.push_str(&format!(
                            " · 注册表已备份 {}",
                            b.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                        ));
                    }
                    this.status.set(s);
                    this.phase = Phase::Done;
                }
                cx.notify();
            },
        );
    }

    fn current_name(&self) -> String {
        self.current
            .and_then(|i| self.apps.get(i))
            .map(|a| a.name.clone())
            .unwrap_or_default()
    }

    /// B4：导出残留清单为 CSV（目录/注册表键 + 路径 + 大小字节）。
    fn export_leftovers(&mut self, cx: &mut Context<Self>) {
        let mut rows = vec!["类型,路径,大小字节".to_string()];
        for i in &self.leftovers {
            let kind = if i.kind == LeftoverKind::Dir { "文件" } else { "注册表键" };
            rows.push(format!("{kind},{},{}", csv_escape(&i.path), i.size));
        }
        let app = sanitize_filename(&self.current_name());
        let file = if app.is_empty() {
            format!("leftover-{}.csv", timestamp_name())
        } else {
            format!("leftover-{app}-{}.csv", timestamp_name())
        };
        let path = exports_dir().join(file);
        match write_csv_lines(&path, &rows) {
            Ok(()) => self.status.set(format!("已导出 {} 个条目到 {}", rows.len() - 1, path.display())),
            Err(e) => self.status.set(format!("导出失败：{e}")),
        }
        cx.notify();
    }
}

impl Render for UninstallPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("软件卸载", "卸载已安装软件，完成后自动扫描并清理其残留"));

        match self.phase {
            Phase::List => {
                // B1：搜索词实时过滤；空词匹配一切
                let q = self.search.read(cx).content().trim().to_lowercase();
                let rows = self.apps.iter().enumerate().filter(|(_, app)| {
                    q.is_empty()
                        || app.name.to_lowercase().contains(&q)
                        || app.publisher.to_lowercase().contains(&q)
                        || app.install_dir.to_lowercase().contains(&q)
                }).map(|(i, app)| {
                    let armed = self.armed_uninstall.is_armed(&i);
                    let im = icon::cached_binary(app.icon_source());
                    let sub = match (app.publisher.is_empty(), app.version.is_empty()) {
                        (false, false) => format!("{} · {}", app.publisher, app.version),
                        (false, true) => app.publisher.clone(),
                        (true, false) => format!("版本 {}", app.version),
                        (true, true) => "未知发布者".to_string(),
                    };
                    div().flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2p5()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme::border())
                        .bg(theme::panel())
                        .child(icon::icon_or_placeholder(&im))
                        .child(
                            div().flex_1().min_w_0().flex().flex_col().gap_0p5()
                                .child(div().text_size(px(13.)).truncate().child(app.name.clone()))
                                .child(div().text_size(px(11.)).text_color(theme::muted()).truncate().child(sub)),
                        )
                        .child(
                            div().w(px(90.)).text_right().text_size(px(12.))
                                .text_color(theme::muted())
                                .child(if app.size > 0 { fmt_size(app.size) } else { "—".to_string() }),
                        )
                        .child(danger_button(
                            SharedString::from(format!("un-arm-{i}")),
                            if armed { "确认卸载" } else { "卸载" },
                            true,
                            cx.listener(move |this, _, _, cx| {
                                if this.armed_uninstall.click(i) {
                                    this.start_uninstall(i, cx);
                                } else {
                                    cx.notify();
                                }
                            }),
                        ))
                }).collect::<Vec<_>>();
                let rows_empty = rows.is_empty();
                let status = self.status.view(self.list_status());
                root = root
                    .child(
                        div().flex().items_center().gap_3().min_w_0().overflow_hidden()
                            .child(button("un-refresh", "刷新", false, true, cx.listener(|this, _, _, cx| this.refresh(cx))))
                            .child(self.search.clone())
                            .child(div().flex_1().min_w_0().text_right().text_size(px(12.)).text_color(theme::muted()).truncate().child(status)),
                    )
                    .child(
                        div().flex().flex_col().flex_1().min_h_0().gap_2()
                            .id("un-list").overflow_y_scroll()
                            .children(rows)
                            .when(rows_empty, |d| d.child(empty_state(if self.loading {
                                "正在加载已安装软件…"
                            } else if q.is_empty() {
                                "未发现已安装软件"
                            } else {
                                "无匹配软件：调整搜索词"
                            }))),
                    );
            }
            Phase::Uninstalling | Phase::Cleaning => {
                let msg = if self.phase == Phase::Uninstalling {
                    format!("正在卸载「{}」——请在弹出的卸载窗口中完成操作，退出后自动扫描残留。", self.current_name())
                } else {
                    "正在清理残留…".to_string()
                };
                let default_status = self.list_status();
                root = root.child(
                    div().p_6().rounded(px(10.)).border_1().border_color(theme::border())
                        .bg(theme::panel())
                        .flex().flex_col().gap_2()
                        .child(div().text_size(px(14.)).child(msg))
                        .child(div().text_size(px(12.)).text_color(theme::muted()).child(self.status.view(default_status))),
                );
            }
            Phase::Leftovers | Phase::Done => {
                let selected_size: u64 = self.leftovers.iter().filter(|i| i.selected).map(|i| i.size).sum();
                let has_selection = self.leftovers.iter().any(|i| i.selected);
                // S2：路径 A→Z 单切换（UI 层排渲染顺序副本）；渲染顺序映射回原始 index，勾选/删除按原 index 定位
                let mut order: Vec<usize> = (0..self.leftovers.len()).collect();
                if self.leftover_sort_path {
                    order.sort_by(|&a, &b| {
                        self.leftovers[a].path.to_lowercase()
                            .cmp(&self.leftovers[b].path.to_lowercase())
                    });
                }
                let rows = order.into_iter().map(|i| {
                    let item = &self.leftovers[i];
                    let is_dir = item.kind == LeftoverKind::Dir;
                    div().flex()
                        .items_center()
                        .gap_3()
                        .px_4()
                        .py_2p5()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(theme::border())
                        .bg(theme::panel())
                        .child(checkbox(SharedString::from(format!("un-lo-chk-{i}")), item.selected, cx.listener(move |this, _, _, cx| {
                            this.leftovers[i].selected = !this.leftovers[i].selected;
                            this.armed_clean.reset();
                            cx.notify();
                        })))
                        .child(badge(
                            if is_dir { "目录" } else { "注册表" },
                            if is_dir { theme::accent() } else { theme::warning() },
                            if is_dir { theme::accent_soft() } else { theme::warning_soft() },
                        ))
                        .child(
                            div().flex_1().min_w_0().text_size(px(13.)).truncate()
                                .child(item.path.clone()),
                        )
                        .child(
                            div().w(px(90.)).text_right().text_size(px(12.))
                                .text_color(theme::muted())
                                .child(if is_dir { fmt_size(item.size) } else { "—".to_string() }),
                        )
                }).collect::<Vec<_>>();
                let lo_empty = rows.is_empty();
                let can_clean = self.phase == Phase::Leftovers && has_selection;
                let default_status = self.list_status();
                root = root
                    .child(
                        div().flex().items_center().gap_3().min_w_0().overflow_hidden()
                            .child(button("un-back", "返回软件列表", false, true, cx.listener(|this, _, _, cx| this.refresh(cx))))
                            .when(self.phase == Phase::Leftovers, |d| {
                                d.child(danger_button("un-lo-clean",
                                    if self.armed_clean.any() { "再次点击确认清理" } else { "清理勾选项" },
                                    can_clean,
                                    cx.listener(|this, _, window, cx| {
                                        if this.armed_clean.click(()) {
                                            this.start_clean(super::controls::window_hwnd(window), cx);
                                        } else {
                                            cx.notify();
                                        }
                                    }),
                                ))
                                .child(button("un-lo-export", "导出 CSV", false, !self.leftovers.is_empty(), cx.listener(|this, _, _, cx| {
                                    this.export_leftovers(cx);
                                })))
                                // S2：残留列表路径 A→Z 排序切换（与 B4 排序 chips 风格一致）
                                .children(filter_chips(
                                    "un-lo-sort",
                                    vec![
                                        (false, "默认顺序"),
                                        (true, "路径 A→Z"),
                                    ],
                                    self.leftover_sort_path,
                                    |f| cx.listener(move |this, _, _, cx| {
                                        this.leftover_sort_path = f;
                                        cx.notify();
                                    }),
                                ))
                                .child(
                                    div().text_size(px(12.)).text_color(theme::muted())
                                        .child(format!("已勾选 {}", fmt_size(selected_size))),
                                )
                            })
                            .child(div().flex_1().min_w_0().text_right().text_size(px(12.)).text_color(theme::muted()).truncate().child(self.status.view(default_status))),
                    )
                    .child(
                        div().text_size(px(11.)).text_color(theme::muted())
                            .child("文件默认移入回收站（设置中可改直删）；注册表项清理前自动备份到 %APPDATA%\\MWCleaner\\backups"),
                    )
                    .child(
                        div().flex().flex_col().flex_1().min_h_0().gap_2()
                            .id("un-lo-list").overflow_y_scroll()
                            .children(rows)
                            .when(lo_empty, |d| d.child(empty_state("未发现残留项"))),
                    );
            }
        }
        root
    }
}
