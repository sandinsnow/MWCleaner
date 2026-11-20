pub mod about_page;
pub mod bigfiles_page;
pub mod icon;
pub mod junk_page;
pub mod registry_page;
pub mod services_page;
pub mod settings_page;
pub mod startup_page;
pub mod theme;
pub mod uninstall_page;

use gpui::*;
use gpui::prelude::*;
use std::sync::{Arc, OnceLock};

use about_page::AboutPage;
use bigfiles_page::BigFilesPage;
use junk_page::JunkPage;
use registry_page::RegistryPage;
use services_page::ServicesPage;
use settings_page::SettingsPage;
use startup_page::StartupPage;
use uninstall_page::UninstallPage;

#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Junk,
    Startup,
    Services,
    Registry,
    BigFiles,
    Uninstall,
    Settings,
    About,
}

impl Page {
    fn label(self) -> &'static str {
        match self {
            Page::Junk => "垃圾清理",
            Page::Startup => "启动项管理",
            Page::Services => "服务优化",
            Page::Registry => "注册表清理",
            Page::BigFiles => "大文件分析",
            Page::Uninstall => "软件卸载",
            Page::Settings => "设置",
            Page::About => "关于",
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
    uninstall: Entity<UninstallPage>,
    settings: Entity<SettingsPage>,
    about: Entity<AboutPage>,
}

impl RootView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            page: Page::Junk,
            junk: cx.new(|_| JunkPage::new()),
            startup: cx.new(|cx| StartupPage::new(cx)),
            services: cx.new(|cx| ServicesPage::new(cx)),
            registry: cx.new(|_| RegistryPage::new()),
            bigfiles: cx.new(|_| BigFilesPage::new()),
            uninstall: cx.new(|cx| UninstallPage::new(cx)),
            settings: cx.new(|_| SettingsPage::new()),
            about: cx.new(|_| AboutPage::new()),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups: [(&str, &[Page]); 3] = [
            ("清理", &[Page::Junk, Page::Registry, Page::BigFiles]),
            ("优化", &[Page::Startup, Page::Services]),
            ("管理", &[Page::Uninstall]),
        ];
        let nav_item = |this: &RootView, p: Page, cx: &mut Context<Self>| {
            let active = this.page == p;
            div()
                .id(SharedString::from(format!("nav-{}", p.label())))
                .mx_2()
                .px_2()
                .py_1p5()
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .text_size(px(13.))
                .text_color(if active { theme::text() } else { theme::muted() })
                .bg(if active { theme::accent_soft() } else { theme::bg() })
                .hover(move |s| s.bg(if active { theme::accent_soft() } else { theme::panel_hover() }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = p;
                    cx.notify();
                }))
                .child(
                    div().w(px(3.)).h(px(14.)).rounded_full()
                        .bg(if active { theme::accent() } else { Rgba { r: 0., g: 0., b: 0., a: 0. } }),
                )
                .child(p.label())
        };

        let content = match self.page {
            Page::Junk => self.junk.clone().into_any_element(),
            Page::Startup => self.startup.clone().into_any_element(),
            Page::Services => self.services.clone().into_any_element(),
            Page::Registry => self.registry.clone().into_any_element(),
            Page::BigFiles => self.bigfiles.clone().into_any_element(),
            Page::Uninstall => self.uninstall.clone().into_any_element(),
            Page::Settings => self.settings.clone().into_any_element(),
            Page::About => self.about.clone().into_any_element(),
        };

        let maximized = window.is_maximized();
        let cap_base = || {
            div()
                .w(px(46.))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .flex_none()
        };
        let titlebar = div()
            .flex()
            .h(px(40.))
            .flex_none()
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .w(px(208.))
                    .flex_none()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_r_1()
                    .border_color(theme::border())
                    .children(brand_icon().map(|im| img(im).size(px(20.)).flex_none()))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("MWCleaner"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .px_4()
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .truncate()
                            .child(self.page.label()),
                    ),
            )
            .child(
                cap_base()
                    .window_control_area(WindowControlArea::Min)
                    .hover(|s| s.bg(theme::panel_hover()))
                    .child(div().w(px(10.)).h(px(1.)).bg(theme::text())),
            )
            .child(
                cap_base()
                    .window_control_area(WindowControlArea::Max)
                    .hover(|s| s.bg(theme::panel_hover()))
                    .child(if maximized {
                        div()
                            .relative()
                            .w(px(11.))
                            .h(px(11.))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right_0()
                                    .w(px(8.))
                                    .h(px(8.))
                                    .border_t_1()
                                    .border_r_1()
                                    .border_color(theme::text()),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .bottom_0()
                                    .left_0()
                                    .w(px(8.))
                                    .h(px(8.))
                                    .border_1()
                                    .border_color(theme::text()),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .w(px(10.))
                            .h(px(10.))
                            .border_1()
                            .border_color(theme::text())
                            .into_any_element()
                    }),
            )
            .child(
                cap_base()
                    .window_control_area(WindowControlArea::Close)
                    .hover(|s| s.bg(theme::danger_soft()))
                    .child(div().text_size(px(16.)).child("×")),
            );

        div().flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .text_size(px(14.))
            .child(titlebar)
            .child(
                div().flex().flex_1().min_h_0()
            .child(
                div().flex().flex_col()
                    .w(px(208.))
                    .h_full()
                    .border_r_1()
                    .border_color(theme::border())
                    .pb_4()
                    .pt_1()
                    .children(groups.into_iter().map(|(label, pages)| {
                        div().flex().flex_col().gap_0p5().pt_2()
                            .child(
                                div().px_4().pb_0p5()
                                    .text_size(px(11.))
                                    .text_color(theme::muted())
                                    .child(label),
                            )
                            .children(pages.iter().map(|&p| nav_item(self, p, cx)))
                    }))
                    .child(div().flex_1())
                    .child(nav_item(self, Page::Settings, cx))
                    .child(nav_item(self, Page::About, cx))
                    .child(
                        div()
                            .px_4()
                            .pt_2()
                            .pb_1()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(format!("v{} · 单文件版", env!("CARGO_PKG_VERSION"))),
                    ),
            )
            .child(div().flex_1().h_full().min_w_0().child(content))
            )
    }
}

pub fn brand_icon() -> Option<Arc<RenderImage>> {
    static ICON: OnceLock<Option<Arc<RenderImage>>> = OnceLock::new();
    ICON.get_or_init(|| {
        let bytes = include_bytes!("../../assets/icon.png");
        let mut im = image::load_from_memory(bytes).ok()?.to_rgba8();
        // gpui Windows 表面按 BGRA 字节序取样
        for p in im.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        Some(Arc::new(RenderImage::new(vec![image::Frame::new(im)])))
    })
    .clone()
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
        .rounded(px(8.))
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
        .rounded(px(8.))
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

pub fn badge(label: impl Into<SharedString>, fg: Rgba, bg: Rgba) -> Div {
    div()
        .px_2()
        .py_0p5()
        .rounded(px(4.))
        .text_size(px(11.))
        .bg(bg)
        .text_color(fg)
        .child(label.into())
}

pub fn page_header(title: &'static str, desc: &'static str) -> Div {
    div().flex().flex_col()
        .gap_1()
        .child(
            div().text_size(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
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