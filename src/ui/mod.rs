pub mod about_page;
pub mod bigfiles_page;
pub mod controls;
pub mod icon;
pub mod junk_page;
pub mod pump;
pub mod registry_page;
pub mod services_page;
pub mod settings_page;
pub mod startup_page;
pub mod text_input;
pub mod theme;
pub mod titlebar;
pub mod uninstall_page;

pub use controls::*;
pub use pump::pump;
pub use titlebar::brand_icon;

use gpui::*;
use gpui::prelude::*;
use std::time::Duration;

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

    /// D2：侧栏收起态的两字缩写
    fn short_label(self) -> &'static str {
        match self {
            Page::Junk => "垃圾",
            Page::Startup => "启动",
            Page::Services => "服务",
            Page::Registry => "注册",
            Page::BigFiles => "大件",
            Page::Uninstall => "卸载",
            Page::Settings => "设置",
            Page::About => "关于",
        }
    }
}

const SIDEBAR_EXPANDED: f32 = 208.;
const SIDEBAR_COLLAPSED: f32 = 48.;

pub struct RootView {
    page: Page,
    /// D2：侧栏收起态（48px 缩写条 ↔ 208px 全称）
    sidebar_collapsed: bool,
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
        let this = Self {
            page: Page::Junk,
            sidebar_collapsed: false,
            junk: cx.new(|cx| JunkPage::new(cx)),
            startup: cx.new(|cx| StartupPage::new(cx)),
            services: cx.new(|cx| ServicesPage::new(cx)),
            registry: cx.new(|cx| RegistryPage::new(cx)),
            bigfiles: cx.new(|cx| BigFilesPage::new(cx)),
            uninstall: cx.new(|cx| UninstallPage::new(cx)),
            settings: cx.new(|cx| SettingsPage::new(cx)),
            about: cx.new(|_| AboutPage::new()),
        };
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            if theme::refresh_system() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        this
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups: [(&str, &[Page]); 3] = [
            ("清理", &[Page::Junk, Page::Registry, Page::BigFiles]),
            ("优化", &[Page::Startup, Page::Services]),
            ("管理", &[Page::Uninstall]),
        ];
        let collapsed = self.sidebar_collapsed;
        let nav_item = |this: &RootView, p: Page, cx: &mut Context<Self>| {
            let active = this.page == p;
            let label = if collapsed { p.short_label() } else { p.label() };
            div()
                .id(SharedString::from(format!("nav-{}", p.label())))
                .mx_2()
                .px_2()
                .py_1p5()
                .rounded(px(8.))
                .border_1()
                .border_color(transparent())
                .flex()
                .items_center()
                .gap_2()
                .when(collapsed, |d| d.justify_center())
                .cursor_pointer()
                .focusable()
                .focus(|s| s.border_color(theme::accent()))
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
                        .bg(if active { theme::accent() } else { transparent() }),
                )
                .child(
                    // 收起态选中项：accent 色加粗，配合竖条与底色依然一眼可辨
                    div()
                        .when(active && collapsed, |d| d.text_color(theme::accent()).font_weight(FontWeight::SEMIBOLD))
                        .child(label),
                )
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

        let sidebar = div().flex().flex_col()
            .w(px(if collapsed { SIDEBAR_COLLAPSED } else { SIDEBAR_EXPANDED }))
            .h_full()
            .border_r_1()
            .border_color(theme::border())
            .pb_4()
            .pt_1();
        let sidebar = if collapsed {
            sidebar
                .children(groups.into_iter().flat_map(|(_, pages)| {
                    pages.iter().copied().map(|p| nav_item(self, p, cx)).collect::<Vec<_>>()
                }))
                .child(div().flex_1())
                .child(nav_item(self, Page::Settings, cx))
                .child(nav_item(self, Page::About, cx))
        } else {
            sidebar
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
                    div().px_4().pt_2().pb_1()
                        .text_size(px(11.))
                        .text_color(theme::muted())
                        .child(format!("v{} · 单文件版", env!("CARGO_PKG_VERSION"))),
                )
        };
        let sidebar = sidebar.child(
            // D2：收起/展开切换按钮，位于侧栏底部版本文字位置
            div().flex().justify_center().pt_1()
                .child(
                    div().id("sidebar-toggle")
                        .px_2()
                        .py_0p5()
                        .rounded(px(6.))
                        .text_size(px(13.))
                        .text_color(theme::muted())
                        .bg(theme::track())
                        .hover(|s| s.bg(theme::panel_hover()).text_color(theme::text()))
                        .cursor_pointer()
                        .child(if collapsed { "›" } else { "‹" })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.sidebar_collapsed = !this.sidebar_collapsed;
                            cx.notify();
                        })),
                ),
        );

        div().flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .text_size(px(14.))
            .child(titlebar::titlebar(self.page.label(), window.is_maximized()))
            .child(
                div().flex().flex_1().min_h_0()
            .child(sidebar)
            .child(div().flex_1().h_full().min_w_0().child(content))
            )
    }
}
