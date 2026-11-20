use gpui::*;

use crate::core::settings;
use crate::ui::{checkbox, page_header, theme};

pub struct SettingsPage;

impl SettingsPage {
    pub fn new() -> Self {
        Self
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = theme::mode();
        let options = [
            (theme::MODE_DARK, "深色"),
            (theme::MODE_LIGHT, "浅色"),
            (theme::MODE_SYSTEM, "跟随系统"),
        ];
        let buttons = options.into_iter().map(|(m, label)| {
            let active = current == m;
            div()
                .id(SharedString::from(format!("theme-{m}")))
                .px_4()
                .py_1p5()
                .rounded_md()
                .text_size(px(13.))
                .cursor_pointer()
                .bg(if active { theme::accent() } else { theme::panel_hover() })
                .text_color(if active { rgb(0xffffff) } else { theme::text() })
                .hover(|d| d.bg(if active { theme::accent_hover() } else { theme::border() }))
                .on_click(cx.listener(move |_, _, _, cx| {
                    theme::set_mode(m);
                    settings::save_theme(m);
                    cx.notify();
                }))
                .child(label)
        });

        div().flex().flex_col()
            .size_full()
            .p_6()
            .gap_4()
            .child(page_header("设置", "应用外观与偏好"))
            .child(
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("主题"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("选择应用配色，切换立即生效；「跟随系统」读取 Windows 的应用主题设置"),
                    )
                    .child(div().flex().gap_2().children(buttons)),
            )
            .child(
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("清理"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("垃圾清理的删除方式；移入回收站后可从回收站还原"),
                    )
                    .child(
                        div().flex().items_center().gap_2()
                            .child(checkbox("set-recycle", settings::load_flag("RecycleBin", true), cx.listener(|_, _, _, cx| {
                                settings::save_flag("RecycleBin", !settings::load_flag("RecycleBin", true));
                                cx.notify();
                            })))
                            .child(div().text_size(px(13.)).child("清理时移入回收站")),
                    ),
            )
            .child(
                div().flex().flex_col()
                    .gap_3()
                    .px_4()
                    .py_4()
                    .rounded_md()
                    .bg(theme::panel())
                    .child(div().text_size(px(15.)).child("大文件分析"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("扫描时忽略小于阈值的文件"),
                    )
                    .child(div().flex().gap_2().children({
                        let current = settings::load_u32("BigFileMinMB", 100);
                        [50u32, 100, 500].into_iter().map(move |v| {
                            let active = current == v;
                            div()
                                .id(SharedString::from(format!("set-minmb-{v}")))
                                .px_4()
                                .py_1p5()
                                .rounded_md()
                                .text_size(px(13.))
                                .cursor_pointer()
                                .bg(if active { theme::accent() } else { theme::panel_hover() })
                                .text_color(if active { rgb(0xffffff) } else { theme::text() })
                                .hover(move |d| d.bg(if active { theme::accent_hover() } else { theme::border() }))
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    settings::save_u32("BigFileMinMB", v);
                                    cx.notify();
                                }))
                                .child(format!("{v} MB"))
                        })
                    })),
            )
    }
}