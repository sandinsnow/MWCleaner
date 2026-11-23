use gpui::*;
use gpui::prelude::*;

use crate::core::services::{self, ServiceInfo};
use crate::ui::text_input::SearchInput;
use crate::ui::{button, danger_button, dot, empty_state, filter_chips, icon, page_header, pump, start_status_ticker, theme, ArmedConfirm, Status};

enum Msg {
    Loaded(Result<Vec<ServiceInfo>, String>),
    IconsReady,
    /// C3：单条禁用依赖检查结果（gen, index, target=4, 依赖清单或失败原因）
    DepCheck(u64, usize, u32, Result<Vec<String>, String>),
    /// C2：批量确认阶段的依赖聚合结果（gen, display 名, 依赖清单或失败）
    BatchDeps(u64, String, Result<Vec<String>, String>),
    /// C2：批量执行进度（gen, 已完成数, 总数, display 名）
    BatchProgress(u64, usize, usize, String),
    /// C2：批量执行完成（gen, 逐项结果）
    BatchFinished(u64, Vec<services::BatchItemResult>),
}

/// C2：批量应用状态机。
enum BatchPhase {
    Idle,
    /// 两击确认中：携带待执行目标（index, target）；依赖查询结果聚合在页面字段里
    Confirming { targets: Vec<(usize, u32)> },
    Running,
    /// 执行完成：报告常驻状态栏，直至下次操作
    Done(BatchReport),
}

struct BatchReport {
    ok: usize,
    /// (display, err)
    failed: Vec<(String, String)>,
}

fn batch_report_text(r: &BatchReport) -> String {
    if r.failed.is_empty() {
        format!("批量应用完成：成功 {} 项，均已记录原值，可逐项点击「还原」恢复", r.ok)
    } else {
        let detail: Vec<String> = r.failed.iter().map(|(d, e)| format!("{d}（{e}）")).collect();
        format!(
            "批量应用完成：成功 {} 项、失败 {} 项：{}。失败项未做任何修改",
            r.ok,
            r.failed.len(),
            detail.join("、")
        )
    }
}

pub struct ServicesPage {
    services: Vec<ServiceInfo>,
    loading: bool,
    status: Status,
    filter: Option<services::AdviceCategory>,
    state_filter: Option<bool>,
    /// B1：列表搜索（name/display/binary/description 不区分大小写 contains）
    search: Entity<SearchInput>,
    armed: ArmedConfirm<(usize, u32)>,
    undo: std::collections::HashMap<String, u32>,
    /// 代际号：reload/批量启动时 +1，使在途后台消息（依赖查询等）失效防串台
    gen: u64,
    /// C3：单条禁用依赖查询进行中 (index, target=4)
    dep_checking: Option<(usize, u32)>,
    /// C2：批量状态机
    batch: BatchPhase,
    /// C2：批量确认阶段聚合的运行中依赖 (display, 依赖显示名列表)
    batch_deps: Vec<(String, Vec<String>)>,
    /// C2：批量确认阶段是否有依赖枚举失败（降级注记）
    batch_dep_failed: bool,
}

impl ServicesPage {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| SearchInput::new("搜索服务…", cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            services: Vec::new(),
            loading: true,
            status: Status::new(),
            filter: None,
            state_filter: None,
            search,
            armed: ArmedConfirm::new(),
            undo: std::collections::HashMap::new(),
            gen: 0,
            dep_checking: None,
            batch: BatchPhase::Idle,
            batch_deps: Vec::new(),
            batch_dep_failed: false,
        };
        start_status_ticker(cx);
        this.reload(cx);
        this
    }

    /// 状态栏的常驻默认提示：未加载完成时提示加载，否则给出统计概览。
    fn status_default(&self) -> SharedString {
        if self.services.is_empty() {
            return "正在加载服务列表…".into();
        }
        // C2：批量完成报告常驻，直至下次操作
        if let BatchPhase::Done(r) = &self.batch {
            return batch_report_text(r).into();
        }
        let rec = self.services.iter().filter(|s| {
            services::advice(&s.name).category == services::AdviceCategory::Recommend
        }).count();
        let caution = self.services.iter().filter(|s| {
            services::advice(&s.name).category == services::AdviceCategory::Caution
        }).count();
        format!("共 {} 服务 · 建议 {} · 不建议 {}", self.services.len(), rec, caution).into()
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
        self.armed.reset();
        self.dep_checking = None;
        self.batch = BatchPhase::Idle;
        self.search.update(cx, |s, _| s.set_enabled(true));
        self.gen += 1;
        cx.notify();
        pump(
            cx,
            |tx| {
                let _ = tx.try_send(Msg::Loaded(services::list()));
            },
            |this, msg, cx| {
                match msg {
                    Msg::Loaded(Ok(list)) => {
                        this.services = list;
                        this.armed.reset();
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
                    Msg::Loaded(Err(e)) => this.status.set(e),
                    Msg::IconsReady => {}
                    _ => {}
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
                self.status.set(format!(
                    "「{}」启动类型已改为{}",
                    self.services[i].display,
                    services::start_type_text(target)
                ));
            }
            Err(e) => self.status.set(e),
        }
        self.armed.reset();
        self.dep_checking = None;
        self.batch = BatchPhase::Idle;
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
                    self.status.set(format!(
                        "「{}」已还原为{}",
                        self.services[i].display,
                        services::start_type_text(orig)
                    ));
                }
                Err(e) => self.status.set(e),
            }
        }
        self.armed.reset();
        self.dep_checking = None;
        self.batch = BatchPhase::Idle;
        cx.notify();
    }

    /// C3：禁用确认文案。deps 为 None 时（首击瞬间）按基础文案；
    /// 有运行中依赖时列出依赖清单；查询失败附加降级注记。
    fn disable_confirm_text(
        &self,
        i: usize,
        deps: Option<&Result<Vec<String>, String>>,
    ) -> String {
        let s = &self.services[i];
        let adv = services::advice(&s.name);
        let base = match adv.category {
            services::AdviceCategory::Caution => format!(
                "「{}」不建议优化：{}。再次点击「禁用」确认",
                s.display, adv.reason
            ),
            services::AdviceCategory::Recommend => format!(
                "「{}」建议优化：{}。再次点击「禁用」确认",
                s.display, adv.reason
            ),
            _ => "该服务无明确优化建议，再次点击「禁用」确认".to_string(),
        };
        if let Some(r) = deps {
            match r {
                Ok(list) if !list.is_empty() => {
                    return format!(
                        "禁用「{}」前发现 {} 个正在运行的依赖服务：{}。禁用后这些服务可能无法启动或功能异常。再次点击「禁用」确认",
                        s.display,
                        list.len(),
                        list.join("、")
                    );
                }
                Err(_) => return format!("{base}（未能枚举依赖，请谨慎）"),
                Ok(_) => {}
            }
        }
        base
    }

    /// C3：首击禁用 → 后台查运行中依赖，回 UI 后展示含依赖清单的确认文案（armed 保持等第二击）。
    fn start_dep_check(&mut self, i: usize, cx: &mut Context<Self>) {
        let name = self.services[i].name.clone();
        let display = self.services[i].display.clone();
        self.dep_checking = Some((i, 4));
        self.status.set(format!("正在检查「{display}」的服务依赖…"));
        cx.notify();
        let gen = self.gen;
        pump(
            cx,
            move |tx| {
                let r = services::active_dependents(&name);
                let _ = tx.try_send(Msg::DepCheck(gen, i, 4, r));
            },
            |this, msg, cx| {
                if let Msg::DepCheck(g, i, t, r) = msg {
                    if g == this.gen && this.armed.is_armed(&(i, t)) {
                        let text = this.disable_confirm_text(i, Some(&r));
                        this.status.set(text);
                    }
                    // 仅当回包对应当前检查目标时清除「检查中」标记：
                    // A 查询在途时改点 B，A 的迟到回包不应清掉 B 的标记
                    if this.dep_checking == Some((i, t)) {
                        this.dep_checking = None;
                    }
                }
                cx.notify();
            },
        );
    }

    /// C2：批量确认文案（依赖结果聚合展示）。
    fn batch_confirm_text(&self, targets: &[(usize, u32)]) -> String {
        let n = targets.len();
        let disable = targets.iter().filter(|(_, t)| *t == 4).count();
        let manual = n - disable;
        let mut s = format!(
            "应用建议优化：将修改 {n} 项服务（禁用 {disable} 项、改手动 {manual} 项）。保持现状/不建议优化的服务不受影响。"
        );
        if !self.batch_deps.is_empty() {
            let parts: Vec<String> = self
                .batch_deps
                .iter()
                .map(|(name, deps)| format!("{name} ←（被 {} 依赖）", deps.join("、")))
                .collect();
            s.push_str(&format!(
                "其中 {} 项存在正在运行的依赖服务：{}。禁用后这些依赖服务可能无法启动或功能异常。",
                self.batch_deps.len(),
                parts.join("、")
            ));
        }
        if self.batch_dep_failed {
            s.push_str("部分服务未能枚举依赖，请谨慎。");
        }
        s.push_str("再次点击确认执行");
        s
    }

    /// C2：一击进入 Confirming，同时后台对目标=禁用(4)的候选逐项聚合查询依赖。
    fn start_batch_confirm(&mut self, cx: &mut Context<Self>) {
        let targets = services::batch_candidates(&self.services);
        if targets.is_empty() {
            self.status
                .set("没有可应用建议的服务（建议项已是目标状态，或此前已手动调整）");
            cx.notify();
            return;
        }
        self.gen += 1; // 使在途单条依赖查询失效
        let gen = self.gen;
        self.batch = BatchPhase::Confirming { targets: targets.clone() };
        self.batch_deps.clear();
        self.batch_dep_failed = false;
        self.armed.reset();
        self.dep_checking = None;
        let text = self.batch_confirm_text(&targets);
        self.status.set(text);
        cx.notify();
        // 查询用服务键名，展示用 display 名
        let names: Vec<(String, String)> = targets
            .iter()
            .filter(|(_, t)| *t == 4)
            .map(|(i, _)| {
                (self.services[*i].name.clone(), self.services[*i].display.clone())
            })
            .collect();
        pump(
            cx,
            move |tx| {
                for (name, display) in names {
                    let r = services::active_dependents(&name);
                    let _ = tx.try_send(Msg::BatchDeps(gen, display, r));
                }
            },
            |this, msg, cx| {
                if let Msg::BatchDeps(g, display, r) = msg {
                    if g == this.gen {
                        match &r {
                            Ok(deps) if !deps.is_empty() => {
                                this.batch_deps.push((display, deps.clone()));
                            }
                            Ok(_) => {}
                            Err(_) => this.batch_dep_failed = true,
                        }
                        // 仅在仍处于确认阶段时刷新确认文案
                        if let BatchPhase::Confirming { targets } = &this.batch {
                            let text = this.batch_confirm_text(targets);
                            this.status.set(text);
                        }
                    }
                }
                cx.notify();
            },
        );
    }

    /// C2：二击执行——后台串行逐项 set_start_type + record_original，进度逐项回显。
    fn start_batch(&mut self, targets: Vec<(usize, u32)>, cx: &mut Context<Self>) {
        self.gen += 1;
        let gen = self.gen;
        self.batch = BatchPhase::Running;
        self.armed.reset();
        self.dep_checking = None;
        // 批量执行期间禁用搜索框输入，完成后在 BatchFinished 恢复
        self.search.update(cx, |s, _| s.set_enabled(false));
        cx.notify();
        // 快照执行清单：SCM 调用全部在后台线程完成
        let items: Vec<(String, String, u32, u32)> = targets
            .iter()
            .map(|(i, t)| {
                (
                    self.services[*i].name.clone(),
                    self.services[*i].display.clone(),
                    *t,
                    self.services[*i].start_type,
                )
            })
            .collect();
        pump(
            cx,
            move |tx| {
                let total = items.len();
                let mut results: Vec<services::BatchItemResult> = Vec::new();
                for (k, (name, display, target, orig)) in items.into_iter().enumerate() {
                    let err = services::set_start_type(&name, target).err();
                    if err.is_none() {
                        // 与 change() 一致的撤销语义：首次才写原值，回写同值则清除
                        match services::original_start_type(&name) {
                            Some(rec) if rec == target => services::clear_original(&name),
                            Some(_) => {}
                            None => services::record_original(&name, orig),
                        }
                    }
                    let _ = tx.try_send(Msg::BatchProgress(gen, k + 1, total, display.clone()));
                    results.push(services::BatchItemResult {
                        name,
                        display,
                        target,
                        err: err.map(|e| e.to_string()),
                    });
                }
                let _ = tx.try_send(Msg::BatchFinished(gen, results));
            },
            |this, msg, cx| {
                match msg {
                    Msg::BatchProgress(g, i, n, display) if g == this.gen => {
                        this.status.set(format!("正在应用建议优化：{i}/{n}（{display}）"));
                    }
                    Msg::BatchFinished(g, results) if g == this.gen => {
                        let ok = results.iter().filter(|r| r.err.is_none()).count();
                        let mut failed: Vec<(String, String)> = Vec::new();
                        for r in &results {
                            if let Some(e) = &r.err {
                                failed.push((r.display.clone(), e.clone()));
                            } else if let Some(s) =
                                this.services.iter_mut().find(|s| s.name == r.name)
                            {
                                s.start_type = r.target;
                            }
                        }
                        this.refresh_undo();
                        let report = BatchReport { ok, failed };
                        let text = batch_report_text(&report);
                        this.batch = BatchPhase::Done(report);
                        this.search.update(cx, |s, _| s.set_enabled(true));
                        this.status.set(text);
                    }
                    _ => {}
                }
                cx.notify();
            },
        );
    }
}

impl Render for ServicesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // B1：搜索词实时过滤（cx.observe 驱动重绘）；空词匹配一切
        let q = self.search.read(cx).content().trim().to_lowercase();
        let matches = |s: &ServiceInfo| {
            q.is_empty()
                || s.name.to_lowercase().contains(&q)
                || s.display.to_lowercase().contains(&q)
                || s.binary.to_lowercase().contains(&q)
                || s.description.to_lowercase().contains(&q)
        };
        // C2：批量执行期间全部交互守卫忽略
        let running = matches!(self.batch, BatchPhase::Running);
        let batch_targets = services::batch_candidates(&self.services);
        let rows = self.services.iter().enumerate().filter(|(_, s)| {
            matches(s)
            && self.filter.map_or(true, |c| services::advice(&s.name).category == c)
                && self.state_filter.map_or(true, |running| {
                    if running { s.state == 4 } else { s.state == 1 }
                })
        }).map(|(i, s)| {
            let adv = services::advice(&s.name);
            let running_state = s.state == 4;
            // C2：确认阶段给候选行加 accent 边框高亮
            let in_batch = match &self.batch {
                BatchPhase::Confirming { targets } => targets.iter().any(|(j, _)| *j == i),
                _ => false,
            };
            let type_buttons = [2u32, 3, 4].into_iter().map(|t| {
                let current = s.start_type == t;
                let armed = self.armed.is_armed(&(i, t));
                let checking = self.dep_checking == Some((i, t));
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
                    .border_1()
                    .border_color(crate::ui::transparent())
                    .text_size(px(12.))
                    .cursor_pointer()
                    .focusable()
                    .focus(|s| s.border_color(theme::accent()))
                    .bg(if current { theme::accent() } else { theme::track() })
                    .text_color(if current { rgb(0xffffff) } else { theme::muted() })
                    .hover(move |d| d.bg(if current { theme::accent_hover() } else { theme::panel_hover() }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if running {
                            return;
                        }
                        if t == 4 {
                            // C3：所有禁用统一两击，首击后台查运行中依赖（Recommend 亦然）
                            if !this.armed.click((i, t)) {
                                this.start_dep_check(i, cx);
                            } else {
                                this.change(i, t, cx);
                            }
                        } else {
                            this.change(i, t, cx);
                        }
                    }))
                    .child(if armed {
                        "确认?"
                    } else if checking {
                        "检查中"
                    } else {
                        label
                    })
            });
            div().flex()
                .items_center()
                .gap_3()
                .px_4()
                .py_2p5()
                .rounded_md()
                .border_1()
                .border_color(if in_batch { theme::accent() } else { crate::ui::transparent() })
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
                        .child(dot(if running_state { theme::success() } else { theme::muted() }))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(if running_state { theme::success() } else { theme::muted() })
                                .child(services::state_text(s.state)),
                        ),
                )
                .child(div().flex().gap_1().children(type_buttons))
                .when(self.undo.contains_key(&s.name), |d| {
                    d.child(button(SharedString::from(format!("svc-undo-{i}")), "还原", false, true, cx.listener(move |this, _, _, cx| {
                        if running {
                            return;
                        }
                        this.undo(i, cx)
                    })))
                })
        }).collect::<Vec<_>>();
        let rows_empty = rows.is_empty();
        let status = self.status.view(self.status_default());

        // C2：批量按钮文案随状态机变化；Running 期间禁用
        let batch_label: SharedString = match &self.batch {
            BatchPhase::Running => "正在应用…".into(),
            BatchPhase::Confirming { .. } => "再次点击确认执行".into(),
            _ => "批量应用建议优化".into(),
        };

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
                    .child(button("svc-refresh", "刷新", false, !self.loading && !running, cx.listener(|this, _, _, cx| this.reload(cx))))
                    // C2：候选非空才渲染批量按钮
                    .when(!batch_targets.is_empty() || running, |d| {
                        d.child(danger_button("svc-apply", batch_label, !running && !self.loading, cx.listener(|this, _, _, cx| {
                            if matches!(this.batch, BatchPhase::Running) {
                                return;
                            }
                            match &this.batch {
                                BatchPhase::Confirming { targets } => {
                                    let targets = targets.clone();
                                    this.start_batch(targets, cx);
                                }
                                _ => this.start_batch_confirm(cx),
                            }
                        })))
                    })
                    .child(self.search.clone())
                    .children(filter_chips(
                        "svc-filter",
                        vec![
                            (None, "全部"),
                            (Some(services::AdviceCategory::Recommend), "建议优化"),
                            (Some(services::AdviceCategory::Keep), "保持现状"),
                            (Some(services::AdviceCategory::Caution), "不建议优化"),
                        ],
                        self.filter,
                        |f| cx.listener(move |this, _, _, cx| {
                            if running {
                                return;
                            }
                            this.filter = f;
                            this.armed.reset();
                            this.gen += 1;
                            cx.notify();
                        }),
                    ))
                    .child(div().w(px(1.)).h(px(16.)).bg(theme::track()))
                    .children(filter_chips(
                        "svc-state",
                        vec![
                            (None, "全部状态"),
                            (Some(true), "运行中"),
                            (Some(false), "已停止"),
                        ],
                        self.state_filter,
                        |f| cx.listener(move |this, _, _, cx| {
                            if running {
                                return;
                            }
                            this.state_filter = f;
                            this.armed.reset();
                            this.gen += 1;
                            cx.notify();
                        }),
                    ))
                    .child(div().flex_1().min_w_0().text_right().text_size(px(13.)).text_color(theme::muted()).truncate().child(status)),
            )
            .child(
                div().flex().flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("svc-list").overflow_y_scroll()
                    .children(rows)
                    .when(rows_empty, |d| d.child(empty_state(if self.loading { "正在加载服务列表…" } else { "无匹配的服务：调整筛选条件或搜索词" }))),
            )
    }
}
