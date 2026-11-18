#![windows_subsystem = "windows"]

mod core;
mod ui;

use gpui::*;
use ui::RootView;

fn main() {
    core::settings::init();
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("MWCleaner · Windows 清理与优化".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| RootView::new(cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}