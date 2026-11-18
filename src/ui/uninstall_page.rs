use gpui::*;
use gpui::prelude::*;

use crate::core::fmt_size;
use crate::core::settings;
use crate::core::uninstall::{self, AppEntry, LeftoverItem, LeftoverKind};
use crate::ui::{badge, button, checkbox, danger_button, icon, page_header, pump, theme};

enum Msg {
    IconsReady,
    Uninstalled(i32),
    StillInstalled,
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
    armed_uninstall: Option<usize>,
    armed_clean: bool,
    status: SharedString,
}

impl UninstallPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let apps = uninstall::list();
        let mut this = Self {
            apps,
            phase: Phase::List,
            current: None,
            leftovers: Vec::new(),
            armed_uninstall: None,
            armed_clean: false,
            status: SharedString::from(""),
        };
        this.warm_icons(cx);
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

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.apps = uninstall::list();
        self.phase = Phase::List;
        self.current = None;
        self.leftovers.clear();
        self.armed_uninstall = None;
        self.armed_clean = false;
        self.status = self.list_status();
        self.warm_icons(cx);
        cx.notify();
    }

    fn start_uninstall(&mut self, i: usize, cx: &mut Context<Self>) {
        let app = self.apps[i].clone();
        self.phase = Phase::Uninstalling;
        self.current = Some(i);
        self.armed_uninstall = None;
        self.status = format!("正在卸载「{}」…", app.name).into();
        cx.notify();
        pump(
            cx,
            move |tx| {
                let code = uninstall::run_uninstaller(&app).unwrap_or(-1);
                let _ = tx.try_send(Msg::Uninstalled(code));
                let installed = uninstall::list();
                if installed.iter().any(|a| a.reg_key == app.reg_key) {
                    let _ = tx.try_send(Msg::StillInstalled);
                    return;
                }
                let items = uninstall::scan_leftovers(&app, &installed);
                let _ = tx.try_send(Msg::Leftovers(items));
            },
            |this, msg, cx| {
                match msg {
                    Msg::Uninstalled(code) => {
                        this.status = format!("卸载程序已退出（代码 {code}），正在扫描残留…").into();
                    }
                    Msg::StillInstalled => {
                        this.status = "卸载未完成（软件仍在已安装列表），已返回列表".into();
                        this.phase = Phase::List;
                        this.current = None;
                        this.apps = uninstall::list();
                    }
                    Msg::Leftovers(items) => {
                        this.status = format!(
                            "发现 {} 项残留（默认不勾选，请人工确认后清理）",
                            items.len()
                        ).into();
                        this.leftovers = items;
                        this.phase = Phase::Leftovers;
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
    }

    fn start_clean(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Cleaning;
        self.armed_clean = false;
        cx.notify();
        let items = self.leftovers.clone();
        pump(
            cx,
            move |tx| {
                let report = uninstall::clean(&items, settings::load_flag("RecycleBin", true));
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
                    this.status = s.into();
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
                let rows = self.apps.iter().enumerate().map(|(i, app)| {
                    let armed = self.armed_uninstall == Some(i);
                    let im = icon::for_binary(app.icon_source());
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
                                if this.armed_uninstall == Some(i) {
                                    this.start_uninstall(i, cx);
                                } else {
                                    this.armed_uninstall = Some(i);
                                    cx.notify();
                                }
                            }),
                        ))
                });
                let status = if self.status.is_empty() { self.list_status() } else { self.status.clone() };
                root = root
                    .child(
                        div().flex().items_center().gap_3()
                            .child(button("un-refresh", "刷新", false, true, cx.listener(|this, _, _, cx| this.refresh(cx))))
                            .child(div().flex_1().min_w_0().text_right().text_size(px(12.)).text_color(theme::muted()).truncate().child(status)),
                    )
                    .child(
                        div().flex().flex_col().flex_1().min_h_0().gap_2()
                            .id("un-list").overflow_y_scroll()
                            .children(rows),
                    );
            }
            Phase::Uninstalling | Phase::Cleaning => {
                let msg = if self.phase == Phase::Uninstalling {
                    format!("正在卸载「{}」——请在弹出的卸载窗口中完成操作，退出后自动扫描残留。", self.current_name())
                } else {
                    "正在清理残留…".to_string()
                };
                root = root.child(
                    div().p_6().rounded(px(10.)).border_1().border_color(theme::border())
                        .bg(theme::panel())
                        .flex().flex_col().gap_2()
                        .child(div().text_size(px(14.)).child(msg))
                        .child(div().text_size(px(12.)).text_color(theme::muted()).child(self.status.clone())),
                );
            }
            Phase::Leftovers | Phase::Done => {
                let selected_size: u64 = self.leftovers.iter().filter(|i| i.selected).map(|i| i.size).sum();
                let has_selection = self.leftovers.iter().any(|i| i.selected);
                let rows = self.leftovers.iter().enumerate().map(|(i, item)| {
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
                            this.armed_clean = false;
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
                });
                let can_clean = self.phase == Phase::Leftovers && has_selection;
                root = root
                    .child(
                        div().flex().items_center().gap_3()
                            .child(button("un-back", "返回软件列表", false, true, cx.listener(|this, _, _, cx| this.refresh(cx))))
                            .when(self.phase == Phase::Leftovers, |d| {
                                d.child(danger_button("un-lo-clean",
                                    if self.armed_clean { "再次点击确认清理" } else { "清理勾选项" },
                                    can_clean,
                                    cx.listener(|this, _, _, cx| {
                                        if this.armed_clean {
                                            this.start_clean(cx);
                                        } else {
                                            this.armed_clean = true;
                                            cx.notify();
                                        }
                                    }),
                                ))
                                .child(
                                    div().text_size(px(12.)).text_color(theme::muted())
                                        .child(format!("已勾选 {}", fmt_size(selected_size))),
                                )
                            })
                            .child(div().flex_1().min_w_0().text_right().text_size(px(12.)).text_color(theme::muted()).truncate().child(self.status.clone())),
                    )
                    .child(
                        div().text_size(px(11.)).text_color(theme::muted())
                            .child("文件默认移入回收站（设置中可改直删）；注册表项清理前自动备份到 %APPDATA%\\MWCleaner\\backups"),
                    )
                    .child(
                        div().flex().flex_col().flex_1().min_h_0().gap_2()
                            .id("un-lo-list").overflow_y_scroll()
                            .children(rows),
                    );
            }
        }
        root
    }
}