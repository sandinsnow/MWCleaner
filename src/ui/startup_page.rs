use gpui::*;

use crate::core::startup::{self, StartupItem};
use crate::ui::{button, danger_button, icon, page_header, theme};

pub struct StartupPage {
    items: Vec<StartupItem>,
    status: SharedString,
    armed_delete: Option<usize>,
    filter: Option<bool>,
}

impl StartupPage {
    pub fn new() -> Self {
        Self {
            items: startup::list(),
            status: "管理开机自启动项，禁用不会删除原程序".into(),
            armed_delete: None,
            filter: None,
        }
    }

    fn toggle(&mut self, i: usize, cx: &mut Context<Self>) {
        let item = self.items[i].clone();
        let target = !item.enabled;
        match startup::set_enabled(&item, target) {
            Ok(()) => {
                self.status = format!("已{}「{}」", if target { "启用" } else { "禁用" }, item.name).into();
            }
            Err(e) => {
                self.status = format!("操作失败：{e}").into();
            }
        }
        self.items = startup::list();
        self.armed_delete = None;
        cx.notify();
    }

    fn delete(&mut self, i: usize, cx: &mut Context<Self>) {
        let item = self.items[i].clone();
        match startup::delete(&item) {
            Ok(()) => {
                self.status = format!("已删除「{}」", item.name).into();
            }
            Err(e) => {
                self.status = format!("删除失败：{e}").into();
            }
        }
        self.items = startup::list();
        self.armed_delete = None;
        cx.notify();
    }
}

impl Render for StartupPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self
            .items
            .iter()
            .filter(|item| self.filter.map_or(true, |e| item.enabled == e))
            .count();
        let rows = self.items.iter().enumerate().filter(|(_, item)| {
            self.filter.map_or(true, |e| item.enabled == e)
        }).map(|(i, item)| {
            let enabled = item.enabled;
            let armed = self.armed_delete == Some(i);
            let source = if item.is_folder {
                "启动文件夹".to_string()
            } else if item.source.starts_with("HKCU") {
                "注册表 · 当前用户".to_string()
            } else {
                "注册表 · 所有用户".to_string()
            };
            div().flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_3()
                .rounded_md()
                .bg(theme::panel())
                .child(icon::icon_or_placeholder(&icon::for_command(&item.command)))
                .child(
                    div().flex().flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            div().flex()
                                .gap_2()
                                .items_center()
                                .child(div().child(item.name.clone()))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .px_1p5()
                                        .rounded(px(4.))
                                        .bg(if enabled { theme::success_soft() } else { theme::track() })
                                        .text_color(if enabled { theme::success() } else { theme::muted() })
                                        .child(if enabled { "已启用" } else { "已禁用" }),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .truncate()
                                .child(item.command.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::muted())
                                .truncate()
                                .child(if item.description.is_empty() {
                                    source
                                } else {
                                    format!("{source} · {}", item.description)
                                }),
                        ),
                )
                .child(button(SharedString::from(format!("startup-toggle-{i}")),
                    if enabled { "禁用" } else { "启用" },
                    false,
                    true,
                    cx.listener(move |this, _, _, cx| this.toggle(i, cx)),
                ))
                .child(danger_button(SharedString::from(format!("startup-del-{i}")), 
                    if armed { "确认删除" } else { "删除" },
                    true,
                    cx.listener(move |this, _, _, cx| {
                        if this.armed_delete == Some(i) {
                            this.delete(i, cx);
                        } else {
                            this.armed_delete = Some(i);
                            cx.notify();
                        }
                    }),
                ))
        });

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("启动项管理", "注册表 Run 键与启动文件夹中的自启动项"))
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .child(button("startup-refresh", "刷新", false, true, cx.listener(|this, _, _, cx| {
                        this.items = startup::list();
                        this.armed_delete = None;
                        cx.notify();
                    })))
                    .children({
                        let opts: [(Option<bool>, &'static str); 3] = [
                            (None, "全部"),
                            (Some(true), "已启用"),
                            (Some(false), "已禁用"),
                        ];
                        opts.into_iter().enumerate().map(|(si, (f, label))| {
                            let active = self.filter == f;
                            div()
                                .id(SharedString::from(format!("startup-state-{si}")))
                                .px_2()
                                .py_0p5()
                                .rounded(px(4.))
                                .text_size(px(12.))
                                .cursor_pointer()
                                .bg(if active { theme::accent() } else { theme::track() })
                                .text_color(if active { rgb(0xffffff) } else { theme::muted() })
                                .hover(move |d| d.bg(if active { theme::accent_hover() } else { theme::panel_hover() }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.filter = f;
                                    this.armed_delete = None;
                                    cx.notify();
                                }))
                                .child(label)
                        })
                    })
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::muted())
                            .child(if self.filter.is_some() {
                                format!("{shown} / {} 项", self.items.len())
                            } else {
                                format!("共 {} 项", self.items.len())
                            }),
                    )
                    .child(div().flex_1().min_w_0().text_right().text_size(px(13.)).text_color(theme::muted()).truncate().child(self.status.clone())),
            )
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("startup-list").overflow_y_scroll()
                    .children(rows),
            )
    }
}