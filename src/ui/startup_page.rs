use gpui::*;
use gpui::prelude::*;

use crate::core::startup::{self, StartupAdviceCategory, StartupItem};
use crate::ui::text_input::SearchInput;
use crate::ui::{badge, button, danger_button, empty_state, filter_chips, icon, page_header, pump, start_status_ticker, theme, ArmedConfirm, Status};

enum Msg {
    Loaded(Vec<StartupItem>),
    IconsReady,
}

pub struct StartupPage {
    items: Vec<StartupItem>,
    loading: bool,
    status: Status,
    armed_delete: ArmedConfirm<usize>,
    /// C4：计划任务禁用两击确认（启用方向非破坏性，单击直达）
    armed_task: ArmedConfirm<usize>,
    filter: Option<bool>,
    /// C1：三分类建议筛选
    advice_filter: Option<StartupAdviceCategory>,
    /// B1：列表搜索（name/command/description 不区分大小写 contains）
    search: Entity<SearchInput>,
}

impl StartupPage {
    const DEFAULT_STATUS: &'static str = "管理开机自启动项，禁用不会删除原程序";

    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| SearchInput::new("搜索启动项…", cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            items: Vec::new(),
            loading: true,
            status: Status::new(),
            armed_delete: ArmedConfirm::new(),
            armed_task: ArmedConfirm::new(),
            filter: None,
            advice_filter: None,
            search,
        };
        start_status_ticker(cx);
        this.reload(cx);
        this
    }

    /// PERF-1：枚举注册表 Run 键与启动文件夹放到后台线程，避免首次渲染阻塞 UI。
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.armed_delete.reset();
        self.armed_task.reset();
        cx.notify();
        pump(
            cx,
            move |tx| {
                let _ = tx.try_send(Msg::Loaded(startup::list()));
            },
            |this, msg, cx| {
                if let Msg::Loaded(items) = msg {
                    this.items = items;
                    this.loading = false;
                    this.warm_icons(cx);
                }
                cx.notify();
            },
        );
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.status.set("正在刷新启动项…");
        self.reload(cx);
    }

    fn warm_icons(&self, cx: &mut Context<Self>) {
        let commands: Vec<String> = self.items.iter().map(|i| i.command.clone()).collect();
        pump(
            cx,
            move |tx| {
                // QA 观察项 1：改用 core 的 with_com_init，初始化成功时配对 CoUninitialize
                startup::with_com_init(|| {
                    for c in &commands {
                        icon::for_command(c);
                    }
                });
                let _ = tx.try_send(Msg::IconsReady);
            },
            |_, _, cx| cx.notify(),
        );
    }

    fn toggle(&mut self, i: usize, cx: &mut Context<Self>) {
        let item = self.items[i].clone();
        let target = !item.enabled;
        // C4：计划任务禁用方向两击确认（武装后首击不执行）；启用为恢复操作，单击直达
        if item.is_task && !target && !self.armed_task.is_armed(&i) {
            self.armed_task.click(i);
            self.status.set(format!(
                "禁用计划任务「{}」将阻止其自动运行（可随时点击「启用」恢复）。再次点击「禁用」确认",
                item.name
            ));
            cx.notify();
            return;
        }
        self.armed_task.reset();
        match startup::set_enabled(&item, target) {
            Ok(()) => {
                self.status.set(if item.is_task {
                    if target {
                        format!("已启用计划任务「{}」", item.name)
                    } else {
                        format!("已禁用计划任务「{}」（可点击「启用」恢复）", item.name)
                    }
                } else {
                    format!("已{}「{}」", if target { "启用" } else { "禁用" }, item.name)
                });
            }
            Err(e) => {
                // C4：系统级计划任务常见权限拦截，失败文案注明
                self.status.set(if item.is_task {
                    format!("操作失败：{e}（部分系统任务需要管理员权限）")
                } else {
                    format!("操作失败：{e}")
                });
            }
        }
        self.reload(cx);
    }

    fn delete(&mut self, i: usize, cx: &mut Context<Self>) {
        let item = self.items[i].clone();
        match startup::delete(&item) {
            Ok(()) => {
                // COR-2：启用项的「删除」实为「禁用 + 备份」，文案如实说明可恢复路径。
                self.status.set(if item.enabled {
                    format!("已移除并备份「{}」（可在「已禁用」中启用恢复）", item.name)
                } else {
                    format!("已删除「{}」", item.name)
                });
            }
            Err(e) => {
                self.status.set(format!("删除失败：{e}"));
            }
        }
        self.reload(cx);
    }
}

impl Render for StartupPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // B1：搜索词实时过滤；C1：三分类建议筛选
        let q = self.search.read(cx).content().trim().to_lowercase();
        let shown = self
            .items
            .iter()
            .filter(|item| {
                self.filter.map_or(true, |e| item.enabled == e)
                    && self.advice_filter.map_or(true, |c| {
                        startup::startup_advice(&item.name, &item.command).category == c
                    })
                    && (q.is_empty()
                        || item.name.to_lowercase().contains(&q)
                        || item.command.to_lowercase().contains(&q)
                        || item.description.to_lowercase().contains(&q))
            })
            .count();
        let rows = self.items.iter().enumerate().filter(|(_, item)| {
            self.filter.map_or(true, |e| item.enabled == e)
                && self.advice_filter.map_or(true, |c| {
                    startup::startup_advice(&item.name, &item.command).category == c
                })
                && (q.is_empty()
                    || item.name.to_lowercase().contains(&q)
                    || item.command.to_lowercase().contains(&q)
                    || item.description.to_lowercase().contains(&q))
        }).map(|(i, item)| {
            let enabled = item.enabled;
            let armed = self.armed_delete.is_armed(&i);
            let adv = startup::startup_advice(&item.name, &item.command);
            let source = if item.is_task {
                // C4：detail 行直接显示「计划任务\全路径 · 触发描述」
                item.source.clone()
            } else if item.is_folder {
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
                .child(icon::icon_or_placeholder(&icon::cached_command(&item.command)))
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
                                )
                                // C1：建议徽标（未命中规则的项不显示）
                                .when(!adv.reason.is_empty(), |d| {
                                    d.child(badge(
                                        adv.category.label(),
                                        match adv.category {
                                            StartupAdviceCategory::Recommend => theme::accent(),
                                            StartupAdviceCategory::Keep => theme::muted(),
                                            StartupAdviceCategory::Caution => theme::danger(),
                                        },
                                        match adv.category {
                                            StartupAdviceCategory::Recommend => theme::accent_soft(),
                                            StartupAdviceCategory::Keep => theme::track(),
                                            StartupAdviceCategory::Caution => theme::danger_soft(),
                                        },
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .truncate()
                                .child(item.command.clone()),
                        )
                        .when(!adv.reason.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(if adv.category == StartupAdviceCategory::Caution {
                                        theme::danger()
                                    } else {
                                        theme::warning()
                                    })
                                    .truncate()
                                    .child(adv.reason),
                            )
                        })
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
                // C4：计划任务只读边界——不渲染删除按钮，固定宽占位保持行对齐
                .child(if item.is_task {
                    div().w(px(64.))
                } else {
                    div().child(danger_button(SharedString::from(format!("startup-del-{i}")),
                        if armed { "确认删除" } else { "删除" },
                        true,
                        cx.listener(move |this, _, _, cx| {
                            if this.armed_delete.click(i) {
                                this.delete(i, cx);
                            } else {
                                cx.notify();
                            }
                        }),
                    ))
                })
        }).collect::<Vec<_>>();
        let rows_empty = rows.is_empty();

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("启动项管理", "注册表 Run 键、启动文件夹与计划任务（登录/启动触发）"))
            .child(
                div().flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .overflow_hidden()
                    .child(button("startup-refresh", "刷新", false, true, cx.listener(|this, _, _, cx| {
                        this.refresh(cx);
                    })))
                    .child(self.search.clone())
                    .children(filter_chips(
                        "startup-state",
                        vec![
                            (None, "全部"),
                            (Some(true), "已启用"),
                            (Some(false), "已禁用"),
                        ],
                        self.filter,
                        |f| cx.listener(move |this, _, _, cx| {
                            this.filter = f;
                            this.armed_delete.reset();
                            this.armed_task.reset();
                            cx.notify();
                        }),
                    ))
                    .child(div().w(px(1.)).h(px(16.)).bg(theme::track()))
                    // C1：三分类建议筛选 chips（与状态 chips 同行，用分隔线区分）
                    .children(filter_chips(
                        "startup-advice",
                        vec![
                            (None, "全部建议"),
                            (Some(StartupAdviceCategory::Recommend), "建议优化"),
                            (Some(StartupAdviceCategory::Keep), "保持现状"),
                            (Some(StartupAdviceCategory::Caution), "不建议优化"),
                        ],
                        self.advice_filter,
                        |f| cx.listener(move |this, _, _, cx| {
                            this.advice_filter = f;
                            this.armed_delete.reset();
                            this.armed_task.reset();
                            cx.notify();
                        }),
                    ))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::muted())
                            .child(if self.filter.is_some() || self.advice_filter.is_some() || !q.is_empty() {
                                format!("{shown} / {} 项", self.items.len())
                            } else {
                                format!("共 {} 项", self.items.len())
                            }),
                    )
                    .child(div().flex_1().min_w_0().text_right().text_size(px(13.)).text_color(theme::muted()).truncate()
                        .child(self.status.view(Self::DEFAULT_STATUS))),
            )
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("startup-list").overflow_y_scroll()
                    .children(rows)
                    .when(rows_empty, |d| d.child(empty_state(
                        if self.loading {
                            "正在加载启动项…"
                        } else if self.items.is_empty() {
                            "未发现自启动项"
                        } else {
                            "无匹配项：调整筛选条件或搜索词"
                        }
                    ))),
            )
    }
}
