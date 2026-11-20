use gpui::*;
use gpui::prelude::*;

use crate::core::services::{self, ServiceInfo};
use crate::ui::{button, dot, icon, page_header, pump, theme};

enum Msg {
    Loaded(Result<Vec<ServiceInfo>, String>),
    IconsReady,
}

pub struct ServicesPage {
    services: Vec<ServiceInfo>,
    loading: bool,
    status: SharedString,
    filter: Option<services::AdviceCategory>,
    state_filter: Option<bool>,
    armed: Option<(usize, u32)>,
    undo: std::collections::HashMap<String, u32>,
}

impl ServicesPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            services: Vec::new(),
            loading: true,
            status: "正在加载服务列表…".into(),
            filter: None,
            state_filter: None,
            armed: None,
            undo: std::collections::HashMap::new(),
        };
        this.reload(cx);
        this
    }

    fn refresh_undo(&mut self) {
        self.undo = self
            .services
            .iter()
            .filter_map(|s| services::original_start_type(&s.name).map(|v| (s.name.clone(), v)))
            .collect();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.armed = None;
        cx.notify();
        pump(
            cx,
            |tx| {
                let _ = tx.try_send(Msg::Loaded(services::list()));
            },
            |this, msg, cx| {
                match msg {
                    Msg::Loaded(Ok(list)) => {
                        let rec = list.iter().filter(|s| {
                            services::advice(&s.name).category == services::AdviceCategory::Recommend
                        }).count();
                        let caution = list.iter().filter(|s| {
                            services::advice(&s.name).category == services::AdviceCategory::Caution
                        }).count();
                        this.status = format!(
                            "共 {} 服务 · 建议 {} · 不建议 {}",
                            list.len(), rec, caution
                        ).into();
                        this.services = list;
                        this.armed = None;
                        this.refresh_undo();
                        let binaries: Vec<String> =
                            this.services.iter().map(|s| s.binary.clone()).collect();
                        pump(
                            cx,
                            move |tx| {
                                unsafe {
                                    let _ = windows::Win32::System::Com::CoInitializeEx(
                                        None,
                                        windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
                                    );
                                }
                                for b in &binaries {
                                    icon::for_binary(b);
                                }
                                let _ = tx.try_send(Msg::IconsReady);
                            },
                            |_, _, cx| cx.notify(),
                        );
                    }
                    Msg::Loaded(Err(e)) => this.status = e.into(),
                    Msg::IconsReady => {}
                }
                this.loading = false;
                cx.notify();
            },
        );
    }

    fn change(&mut self, i: usize, target: u32, cx: &mut Context<Self>) {
        let name = self.services[i].name.clone();
        let orig = self.services[i].start_type;
        match services::set_start_type(&name, target) {
            Ok(()) => {
                self.services[i].start_type = target;
                match services::original_start_type(&name) {
                    Some(rec) if rec == target => services::clear_original(&name),
                    Some(_) => {}
                    None if orig != 0 => services::record_original(&name, orig),
                    None => {}
                }
                self.refresh_undo();
                self.status = format!(
                    "「{}」启动类型已改为{}",
                    self.services[i].display,
                    services::start_type_text(target)
                )
                .into();
            }
            Err(e) => self.status = e.into(),
        }
        self.armed = None;
        cx.notify();
    }

    fn undo(&mut self, i: usize, cx: &mut Context<Self>) {
        let name = self.services[i].name.clone();
        if let Some(orig) = services::original_start_type(&name) {
            match services::set_start_type(&name, orig) {
                Ok(()) => {
                    self.services[i].start_type = orig;
                    services::clear_original(&name);
                    self.refresh_undo();
                    self.status = format!(
                        "「{}」已还原为{}",
                        self.services[i].display,
                        services::start_type_text(orig)
                    )
                    .into();
                }
                Err(e) => self.status = e.into(),
            }
        }
        self.armed = None;
        cx.notify();
    }
}

impl Render for ServicesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.services.iter().enumerate().filter(|(_, s)| {
            self.filter.map_or(true, |c| services::advice(&s.name).category == c)
                && self.state_filter.map_or(true, |running| {
                    if running { s.state == 4 } else { s.state == 1 }
                })
        }).map(|(i, s)| {
            let adv = services::advice(&s.name);
            let running = s.state == 4;
            let type_buttons = [2u32, 3, 4].into_iter().map(|t| {
                let current = s.start_type == t;
                let armed = self.armed == Some((i, t));
                let label = match t {
                    2 => "自动",
                    3 => "手动",
                    _ => "禁用",
                };
                div()
                    .id(SharedString::from(format!("svc-{i}-{t}")))
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .bg(if current { theme::accent() } else { theme::track() })
                    .text_color(if current { rgb(0xffffff) } else { theme::muted() })
                    .hover(move |d| d.bg(if current { theme::accent_hover() } else { theme::panel_hover() }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if t == 4
                            && adv.category != services::AdviceCategory::Recommend
                            && this.armed != Some((i, t))
                        {
                            this.armed = Some((i, t));
                            this.status = if adv.category == services::AdviceCategory::Caution {
                                format!(
                                    "「{}」不建议优化：{}。再次点击「禁用」确认",
                                    this.services[i].display, adv.reason
                                ).into()
                            } else {
                                "该服务无明确优化建议，再次点击「禁用」确认".into()
                            };
                            cx.notify();
                        } else {
                            this.change(i, t, cx);
                        }
                    }))
                    .child(if armed { "确认?" } else { label })
            });
            div().flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2p5()
                .rounded_md()
                .bg(theme::panel())
                .child(icon::icon_or_placeholder(&icon::cached_binary(&s.binary)))
                .child(
                    div().flex().flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            div().flex()
                                .gap_2()
                                .items_center()
                                .child(div().truncate().child(s.display.clone()))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme::muted())
                                        .child(s.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .px_1p5()
                                        .rounded(px(4.))
                                        .bg(match adv.category {
                                            services::AdviceCategory::Recommend => theme::accent_soft(),
                                            services::AdviceCategory::Keep => theme::track(),
                                            services::AdviceCategory::Caution => theme::danger_soft(),
                                        })
                                        .text_color(match adv.category {
                                            services::AdviceCategory::Recommend => theme::accent(),
                                            services::AdviceCategory::Keep => theme::muted(),
                                            services::AdviceCategory::Caution => theme::danger(),
                                        })
                                        .child(adv.category.label()),
                                ),
                        )
                        .when(!s.description.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .truncate()
                                    .child(s.description.clone()),
                            )
                        })
                        .when(!adv.reason.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(if adv.category == services::AdviceCategory::Caution {
                                        theme::danger()
                                    } else {
                                        theme::warning()
                                    })
                                    .truncate()
                                    .child(adv.reason),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .w(px(70.))
                        .child(dot(if running { theme::success() } else { theme::muted() }))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if running { theme::success() } else { theme::muted() })
                                .child(services::state_text(s.state)),
                        ),
                )
                .child(div().flex().gap_1().children(type_buttons))
                .when(self.undo.contains_key(&s.name), |d| {
                    d.child(button(SharedString::from(format!("svc-undo-{i}")), "还原", false, true, cx.listener(move |this, _, _, cx| this.undo(i, cx))))
                })
        });

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("服务优化", "按建议类别管理启动类型；保持现状/不建议优化的服务禁用前需二次确认"))
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .overflow_hidden()
                    .child(button("svc-refresh", "刷新", false, !self.loading, cx.listener(|this, _, _, cx| this.reload(cx))))
                    .children({
                        let opts: [(Option<services::AdviceCategory>, &'static str); 4] = [
                            (None, "全部"),
                            (Some(services::AdviceCategory::Recommend), "建议优化"),
                            (Some(services::AdviceCategory::Keep), "保持现状"),
                            (Some(services::AdviceCategory::Caution), "不建议优化"),
                        ];
                        opts.into_iter().enumerate().map(|(fi, (f, label))| {
                            let active = self.filter == f;
                            div()
                                .id(SharedString::from(format!("svc-filter-{fi}")))
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
                                    this.armed = None;
                                    cx.notify();
                                }))
                                .child(label)
                        })
                    })
                    .child(div().w(px(1.)).h(px(16.)).bg(theme::track()))
                    .children({
                        let opts: [(Option<bool>, &'static str); 3] = [
                            (None, "全部状态"),
                            (Some(true), "运行中"),
                            (Some(false), "已停止"),
                        ];
                        opts.into_iter().enumerate().map(|(si, (f, label))| {
                            let active = self.state_filter == f;
                            div()
                                .id(SharedString::from(format!("svc-state-{si}")))
                                .px_2()
                                .py_0p5()
                                .rounded(px(4.))
                                .text_size(px(12.))
                                .cursor_pointer()
                                .bg(if active { theme::accent() } else { theme::track() })
                                .text_color(if active { rgb(0xffffff) } else { theme::muted() })
                                .hover(move |d| d.bg(if active { theme::accent_hover() } else { theme::panel_hover() }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.state_filter = f;
                                    this.armed = None;
                                    cx.notify();
                                }))
                                .child(label)
                        })
                    })
                    .child(div().flex_1().min_w_0().text_right().text_size(px(13.)).text_color(theme::muted()).truncate().child(self.status.clone())),
            )
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("svc-list").overflow_y_scroll()
                    .children(rows),
            )
    }
}