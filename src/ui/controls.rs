use gpui::*;
use gpui::prelude::*;
use std::time::{Duration, Instant};

use crate::ui::theme;

pub fn transparent() -> Rgba {
    Rgba { r: 0., g: 0., b: 0., a: 0. }
}

/// SEC-3：取窗口 HWND（isize），用于把 Shell 弹窗（回收站容量确认等）归属到主窗口。
/// 必须全限定调用 trait 方法——`Window` 自带的固有 `window_handle()` 返回的是 AnyWindowHandle。
pub fn window_hwnd(window: &Window) -> isize {
    raw_window_handle::HasWindowHandle::window_handle(window)
        .ok()
        .and_then(|h| match h.as_raw() {
            raw_window_handle::RawWindowHandle::Win32(w) => Some(w.hwnd.get() as isize),
            _ => None,
        })
        .unwrap_or(0)
}

fn focus_ring(s: StyleRefinement) -> StyleRefinement {
    s.border_color(theme::accent())
}

/// 用资源管理器打开目录；`select` 提供时用 /select 定位到该文件（上提自 registry_page）。
pub fn open_in_explorer(dir: &std::path::Path, select: Option<&std::path::Path>) {
    let _ = std::fs::create_dir_all(dir);
    let mut cmd = std::process::Command::new("explorer.exe");
    match select {
        Some(p) => cmd.arg(format!("/select,{}", p.display())),
        None => cmd.arg(dir),
    };
    let _ = cmd.spawn();
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
        .border_1()
        .border_color(transparent())
        .text_size(px(13.))
        .child(label.into());
    if !enabled {
        return base.bg(theme::track()).text_color(theme::muted());
    }
    let base = base
        .cursor_pointer()
        .focusable()
        .focus(focus_ring)
        .on_click(on_click);
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
        .border_1()
        .border_color(transparent())
        .text_size(px(13.))
        .child(label.into());
    if !enabled {
        return base.bg(theme::track()).text_color(theme::muted());
    }
    base.cursor_pointer()
        .focusable()
        .focus(focus_ring)
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
        .focusable()
        .focus(focus_ring)
        .border_color(if checked { theme::accent() } else { theme::border() })
        .bg(if checked { theme::accent() } else { theme::panel() })
        .text_size(px(11.))
        .text_color(rgb(0xffffff))
        .on_click(on_click)
        .when(checked, |d| d.child("✓"))
}

/// 筛选 chips：选中态 hover 保留选中底色，避免 hover 覆盖发白。
pub fn filter_chips<T: Copy + PartialEq + 'static, L: Fn(&ClickEvent, &mut Window, &mut App) + 'static>(
    id_prefix: &str,
    opts: Vec<(T, &'static str)>,
    active: T,
    mut make_listener: impl FnMut(T) -> L,
) -> Vec<Stateful<Div>> {
    opts.into_iter()
        .enumerate()
        .map(|(i, (val, label))| {
            let is_active = val == active;
            let listener = make_listener(val);
            div()
                .id(SharedString::from(format!("{id_prefix}-{i}")))
                .px_2()
                .py_0p5()
                .rounded(px(4.))
                .border_1()
                .border_color(transparent())
                .text_size(px(12.))
                .cursor_pointer()
                .focusable()
                .focus(focus_ring)
                .bg(if is_active { theme::accent() } else { theme::track() })
                .text_color(if is_active { rgb(0xffffff) } else { theme::muted() })
                .hover(move |d| d.bg(if is_active { theme::accent_hover() } else { theme::panel_hover() }))
                .on_click(listener)
                .child(label)
        })
        .collect()
}

/// 两击确认状态机：首次点击武装（返回 false），同键再次点击执行（返回 true）。
pub struct ArmedConfirm<K: PartialEq + Clone> {
    armed: Option<K>,
}

impl<K: PartialEq + Clone> ArmedConfirm<K> {
    pub fn new() -> Self {
        Self { armed: None }
    }

    pub fn reset(&mut self) {
        self.armed = None;
    }

    pub fn any(&self) -> bool {
        self.armed.is_some()
    }

    pub fn is_armed(&self, key: &K) -> bool {
        self.armed.as_ref() == Some(key)
    }

    pub fn click(&mut self, key: K) -> bool {
        if self.is_armed(&key) {
            self.armed = None;
            true
        } else {
            self.armed = Some(key);
            false
        }
    }
}

/// 临时状态消息的存活时长；超时后自动回落到各页默认提示。
pub const STATUS_TTL: Duration = Duration::from_secs(8);

/// 带时间戳的状态栏文本。业务只管 `set`，渲染时用 `view(default)` 决定是否回落。
pub struct Status {
    text: SharedString,
    set_at: Option<Instant>,
    /// 持久态：不受 8s TTL 影响，常驻显示直到下一条消息覆盖（D3：导出完成/失败态）。
    persistent: bool,
}

impl Status {
    pub fn new() -> Self {
        Self { text: SharedString::from(""), set_at: None, persistent: false }
    }

    /// 记录一条新的临时状态（8 秒后回落），并重置持久标记。
    pub fn set(&mut self, text: impl Into<SharedString>) {
        self.text = text.into();
        self.set_at = Some(Instant::now());
        self.persistent = false;
    }

    /// 记录一条持久状态：常驻显示，直到下一条 set/set_persistent 覆盖。
    pub fn set_persistent(&mut self, text: impl Into<SharedString>) {
        self.text = text.into();
        self.set_at = Some(Instant::now());
        self.persistent = true;
    }

    /// 持久消息始终显示；临时消息在 8 秒内显示，否则回落到传入的默认提示。
    pub fn view(&self, default: impl Into<SharedString>) -> SharedString {
        match self.set_at {
            Some(_) if self.persistent => self.text.clone(),
            Some(t) if t.elapsed() < STATUS_TTL => self.text.clone(),
            _ => default.into(),
        }
    }
}

impl Default for Status {
    fn default() -> Self {
        Self::new()
    }
}

/// 每 1 秒唤醒一次重绘，使 `Status::view` 的超时回落无需用户操作即可生效。
/// Entity 释放后 `update` 返回错误，循环自动退出。
pub fn start_status_ticker<T: 'static>(cx: &mut Context<T>) {
    cx.spawn(async move |this, cx| loop {
        cx.background_executor().timer(Duration::from_secs(1)).await;
        if this.update(cx, |_, cx| cx.notify()).is_err() {
            break;
        }
    })
    .detach();
}

pub fn empty_state(text: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.))
        .text_color(theme::muted())
        .child(text.into())
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