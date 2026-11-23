use gpui::*;
use std::sync::{Arc, OnceLock};

use crate::ui::theme;

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

pub fn titlebar(page_label: &'static str, maximized: bool) -> Div {
    let cap_base = || {
        div()
            .w(px(46.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
    };
    div()
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
                        .child(page_label),
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
        )
}