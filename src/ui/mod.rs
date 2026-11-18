pub mod bigfiles_page;
pub mod icon;
pub mod junk_page;
pub mod leftovers_page;
pub mod registry_page;
pub mod services_page;
pub mod settings_page;
pub mod startup_page;
pub mod theme;

use gpui::*;
use gpui::prelude::*;

use bigfiles_page::BigFilesPage;
use junk_page::JunkPage;
use leftovers_page::LeftoversPage;
use registry_page::RegistryPage;
use services_page::ServicesPage;
use settings_page::SettingsPage;
use startup_page::StartupPage;

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Junk,
    Startup,
    Services,
    Registry,
    BigFiles,
    Leftovers,
    Settings,
}

impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Junk => "垃圾清理",
            Page::Startup => "启动项管理",
            Page::Services => "服务优化",
            Page::Registry => "注册表清理",
            Page::BigFiles => "大文件分析",
            Page::Leftovers => "卸载残留",
            Page::Settings => "设置",
        }
    }
}

pub struct RootView {
    page: Page,
    junk: Entity<JunkPage>,
    startup: Entity<StartupPage>,
    services: Entity<ServicesPage>,
    registry: Entity<RegistryPage>,
    bigfiles: Entity<BigFilesPage>,
    leftovers: Entity<LeftoversPage>,
    settings: Entity<SettingsPage>,
}

impl RootView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            page: Page::Junk,
            junk: cx.new(|_| JunkPage::new()),
            startup: cx.new(|_| StartupPage::new()),
            services: cx.new(|cx| ServicesPage::new(cx)),
            registry: cx.new(|_| RegistryPage::new()),
            bigfiles: cx.new(|_| BigFilesPage::new()),
            leftovers: cx.new(|_| LeftoversPage::new()),
            settings: cx.new(|_| SettingsPage::new()),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = [
            Page::Junk,
            Page::Startup,
            Page::Services,
            Page::Registry,
            Page::BigFiles,
            Page::Leftovers,
            Page::Settings,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let active = self.page == p;
            div()
                .id(SharedString::from(format!("nav-{i}")))
                .mx_2()
                .px_3()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .text_color(if active { theme::text() } else { theme::muted() })
                .bg(if active { theme::panel_hover() } else { theme::bg() })
                .hover(|s| s.bg(theme::panel_hover()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = p;
                    cx.notify();
                }))
                .child(p.label())
        })
        .collect::<Vec<_>>();

        let content = match self.page {
            Page::Junk => self.junk.clone().into_any_element(),
            Page::Startup => self.startup.clone().into_any_element(),
            Page::Services => self.services.clone().into_any_element(),
            Page::Registry => self.registry.clone().into_any_element(),
            Page::BigFiles => self.bigfiles.clone().into_any_element(),
            Page::Leftovers => self.leftovers.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
        };

        div().flex()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .text_size(px(14.))
            .child(
                div().flex().flex_col()
                    .w(px(200.))
                    .h_full()
                    .border_r_1()
                    .border_color(theme::border())
                    .py_4()
                    .gap_1()
                    .child(
                        div()
                            .px_4()
                            .pb_3()
                            .text_size(px(18.))
                            .text_color(theme::accent())
                            .child("MWCleaner"),
                    )
                    .children(nav)
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_end()
                            .px_4()
                            .pb_2()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("v0.1.0 · 单文件版"),
                    ),
            )
            .child(div().flex_1().h_full().min_w_0().child(content))
    }
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .px_4()
        .py_1p5()
        .rounded_md()
        .text_size(px(13.))
        .child(label.into());
    if !enabled {
        return base.bg(theme::track()).text_color(theme::muted());
    }
    let base = base.cursor_pointer().on_click(on_click);
    if primary {
        base.bg(theme::accent())
            .text_color(rgb(0xffffff))
            .hover(|s| s.bg(theme::accent_hover()))
    } else {
        base.bg(theme::panel_hover())
            .text_color(theme::text())
            .hover(|s| s.bg(theme::border()))
    }
}

pub fn danger_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .px_4()
        .py_1p5()
        .rounded_md()
        .text_size(px(13.))
        .child(label.into());
    if !enabled {
        return base.bg(theme::track()).text_color(theme::muted());
    }
    base.cursor_pointer()
        .bg(theme::danger())
        .text_color(rgb(0xffffff))
        .hover(|s| s.bg(theme::danger_hover()))
        .on_click(on_click)
}

pub fn checkbox(
    id: impl Into<ElementId>,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .cursor_pointer()
        .border_color(if checked { theme::accent() } else { theme::border() })
        .bg(if checked { theme::accent() } else { theme::panel() })
        .text_size(px(11.))
        .text_color(rgb(0xffffff))
        .on_click(on_click)
        .when(checked, |d| d.child("✓"))
}

pub fn progress_bar(fraction: f32) -> Div {
    div()
        .h(px(6.))
        .w_full()
        .rounded_full()
        .bg(theme::track())
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(theme::accent())
                .w(relative(fraction.clamp(0.0, 1.0))),
        )
}

pub fn size_bar(fraction: f32, color: Rgba) -> Div {
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(theme::track())
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(color)
                .w(relative(fraction.clamp(0.0, 1.0))),
        )
}

pub fn usage_bar(fraction: f32) -> Div {
    let f = fraction.clamp(0.0, 1.0);
    let color = if f > 0.9 {
        theme::danger()
    } else if f > 0.75 {
        theme::warning()
    } else {
        theme::accent()
    };
    size_bar(f, color)
}

pub fn stacked_bar(segs: &[(f32, Rgba)]) -> Div {
    div()
        .h(px(8.))
        .w_full()
        .rounded_full()
        .bg(theme::track())
        .flex()
        .overflow_hidden()
        .children(
            segs.iter()
                .map(|(f, c)| div().h_full().bg(*c).w(relative(f.clamp(0.0, 1.0)))),
        )
}

pub fn chart_color(i: usize) -> Rgba {
    const PALETTE: [u32; 11] = [
        0x4f8cff, 0x3fb950, 0xd29922, 0xbc8cff, 0x39c5cf, 0xf778ba, 0xff9e64, 0x7ee787,
        0x58a6ff, 0xe3b341, 0x8b949e,
    ];
    rgb(PALETTE[i % PALETTE.len()])
}

pub fn dot(color: Rgba) -> Div {
    div().size(px(8.)).rounded_full().bg(color)
}

pub fn page_header(title: &'static str, desc: &'static str) -> Div {
    div().flex().flex_col()
        .gap_1()
        .child(div().text_size(px(20.)).child(title))
        .child(div().text_size(px(13.)).text_color(theme::muted()).child(desc))
}

pub fn pump<T: 'static, M: Send + 'static>(
    cx: &mut Context<T>,
    work: impl FnOnce(smol::channel::Sender<M>) + Send + 'static,
    mut apply: impl FnMut(&mut T, M, &mut Context<T>) + 'static,
) {
    let (tx, rx) = smol::channel::unbounded::<M>();
    std::thread::spawn(move || work(tx));
    cx.spawn(async move |this, cx| {
        while let Ok(msg) = rx.recv().await {
            if this.update(cx, |page, cx| apply(page, msg, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
}