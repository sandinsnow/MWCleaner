#![windows_subsystem = "windows"]
#![recursion_limit = "256"]

mod core;
mod ui;

use gpui::*;
use ui::RootView;

actions!(mwcleaner, [FocusNext, FocusPrev]);

const MIN_W: f32 = 900.;
const MIN_H: f32 = 600.;

/// 校验恢复的窗口 bounds 是否落在任一显示器的可见范围内（防拔掉副屏后窗口消失）。
/// 简化判定：窗口中心点位于某个显示器 bounds 内即视为可见。
fn bounds_visible(b: &Bounds<Pixels>, cx: &App) -> bool {
    let cx_p = b.origin.x + b.size.width / 2.0;
    let cy_p = b.origin.y + b.size.height / 2.0;
    cx.displays().iter().any(|d| {
        let db = d.bounds();
        cx_p >= db.origin.x
            && cx_p <= db.origin.x + db.size.width
            && cy_p >= db.origin.y
            && cy_p <= db.origin.y + db.size.height
    })
}

/// B5：按上次会话恢复窗口位置尺寸；无记录、尺寸不合法或不可见时回落默认居中。
fn initial_bounds(cx: &mut App) -> WindowBounds {
    let default_bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
    match core::settings::load_window_memory() {
        Some(m) if m.maximized => WindowBounds::Maximized(default_bounds),
        Some(m) if m.w >= MIN_W as u32 && m.h >= MIN_H as u32 => {
            let restored = Bounds {
                origin: point(px(m.x as f32), px(m.y as f32)),
                size: size(px(m.w as f32), px(m.h as f32)),
            };
            if bounds_visible(&restored, cx) {
                WindowBounds::Windowed(restored)
            } else {
                WindowBounds::Windowed(default_bounds)
            }
        }
        _ => WindowBounds::Windowed(default_bounds),
    }
}

fn main() {
    ui::theme::set_mode(core::settings::load_theme());
    Application::new().run(|cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("tab", FocusNext, None),
            KeyBinding::new("shift-tab", FocusPrev, None),
        ]);
        cx.on_action(|_: &FocusNext, cx| {
            if let Some(w) = cx.active_window() {
                let _ = w.update(cx, |_, window, _| window.focus_next());
            }
        });
        cx.on_action(|_: &FocusPrev, cx| {
            if let Some(w) = cx.active_window() {
                let _ = w.update(cx, |_, window, _| window.focus_prev());
            }
        });
        let bounds = initial_bounds(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                // D2：窗口最小尺寸 900×600（Windows 经 WM_GETMINMAXINFO 完整支持）
                window_min_size: Some(size(px(MIN_W), px(MIN_H))),
                titlebar: Some(TitlebarOptions {
                    title: Some("MWCleaner · Windows 清理与优化".into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                // B5：窗口关闭前记忆位置尺寸；最大化只记标志，不记最大化后的尺寸
                window.on_window_should_close(cx, |window, _cx| {
                    let b = window.bounds();
                    core::settings::save_window_memory(&core::settings::WindowMemory {
                        x: f32::from(b.origin.x) as i32,
                        y: f32::from(b.origin.y) as i32,
                        w: u32::from(b.size.width.round()),
                        h: u32::from(b.size.height.round()),
                        maximized: window.is_maximized(),
                    });
                    true
                });
                cx.new(|cx| RootView::new(cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
