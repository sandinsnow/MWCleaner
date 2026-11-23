//! B1/S1：自绘单行搜索输入框，完整支持中文 IME。
//!
//! 实现要点（对应 gpui 0.2.2 源码证据，见 .workbuddy/review/spec-执行计划.md §0.1）：
//! - 直接键入走 WM_CHAR → `replace_text_in_range`；控制字符（含退格 0x08）被平台
//!   `is_control()` 过滤，退格/清空在 `on_key_down` 处理；
//! - 中文 IME 组合走 `replace_and_mark_text_in_range` / `unmark_text`，组合串按普通
//!   文本上屏渲染（V1 不做下划线）；
//! - paint 阶段经 canvas 调 `window.handle_input` 挂载 `ElementInputHandler`，
//!   仅当本输入框持焦点时平台才会路由输入；
//! - V1 简化：selection 恒为文末空选区（caret），无拖选/粘贴/光标闪烁；
//!   `bounds_for_range` 返回输入框整体 bounds，候选窗定位于输入框。

use gpui::*;
use std::ops::Range;

use crate::ui::theme;

/// 纯逻辑：在指定 UTF-8 字节区间替换文本，返回替换后的区间（供单测与 IME 复用）。
fn replace_range(content: &mut String, byte_range: Range<usize>, text: &str) -> Range<usize> {
    let start = byte_range.start;
    content.replace_range(byte_range, text);
    start..start + text.len()
}

/// 纯逻辑：UTF-16 码元偏移 → UTF-8 字节偏移（InputHandler 协议使用 UTF-16 坐标）。
/// 偏移落在 surrogate pair 内部时向前取整到 pair 起始字节（与 `byte_to_utf16` 的边界回退对称）。
fn utf16_to_byte(content: &str, n: usize) -> usize {
    let mut units = 0;
    for (byte_off, ch) in content.char_indices() {
        if units >= n {
            return byte_off;
        }
        let next = units + ch.len_utf16();
        if next > n {
            // n 落在该字符内部（如 😀 的低位代理）→ 取整到字符起始
            return byte_off;
        }
        units = next;
    }
    content.len()
}

/// 纯逻辑：UTF-8 字节偏移 → UTF-16 码元偏移；非字符边界时向前取整到边界（防御性）。
fn byte_to_utf16(content: &str, b: usize) -> usize {
    let mut b = b.min(content.len());
    while b > 0 && !content.is_char_boundary(b) {
        b -= 1;
    }
    content[..b].chars().map(|c| c.len_utf16()).sum()
}

pub struct SearchInput {
    content: String,
    /// IME 组合态区域（UTF-8 字节区间）；组合中每次更新替换此区域
    marked: Option<Range<usize>>,
    focus: FocusHandle,
    placeholder: &'static str,
    /// 只读开关：批量执行等场景禁用输入（默认开启）
    enabled: bool,
}

impl SearchInput {
    pub fn new(placeholder: &'static str, cx: &mut Context<Self>) -> Self {
        Self {
            content: String::new(),
            marked: None,
            focus: cx.focus_handle(),
            placeholder,
            enabled: true,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[allow(dead_code)]
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }
}

impl EntityInputHandler for SearchInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let start = utf16_to_byte(&self.content, range_utf16.start);
        let end = utf16_to_byte(&self.content, range_utf16.end);
        let s = self.content.get(start..end)?;
        *adjusted_range = Some(range_utf16);
        Some(s.to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        // V1：selection 恒为文末空选区（caret）
        let caret = self.content.chars().map(|c| c.len_utf16()).sum();
        Some(UTF16Selection { range: caret..caret, reversed: false })
    }

    fn marked_text_range(&self, _window: &mut Window, _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|r| {
            byte_to_utf16(&self.content, r.start)..byte_to_utf16(&self.content, r.end)
        })
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        replacement_range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let range = if let Some(r) = replacement_range {
            let start = utf16_to_byte(&self.content, r.start);
            let end = utf16_to_byte(&self.content, r.end);
            start..end
        } else if let Some(m) = self.marked.clone() {
            // IME 上屏：替换组合串区域
            self.marked = None;
            m
        } else {
            // 直接键入：caret 恒在文末（V1）
            self.content.len()..self.content.len()
        };
        replace_range(&mut self.content, range, text);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.enabled {
            return;
        }
        let range = if let Some(r) = range_utf16 {
            let start = utf16_to_byte(&self.content, r.start);
            let end = utf16_to_byte(&self.content, r.end);
            start..end
        } else if let Some(m) = self.marked.clone() {
            // 组合串更新：整体替换上一次的组合区域
            m
        } else {
            self.content.len()..self.content.len()
        };
        self.marked = Some(replace_range(&mut self.content, range, new_text));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // V1：候选窗定位于整个输入框而非光标点，可用性足够
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

impl Render for SearchInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let composing = self.marked.is_some();
        let empty = self.content.is_empty();
        let enabled = self.enabled;
        let focus = self.focus.clone();
        let entity = cx.entity();

        div()
            .id("search-input")
            .key_context("SearchInput")
            .track_focus(&self.focus)
            .relative()
            .w(px(220.))
            .h(px(28.))
            .px_2()
            .rounded(px(6.))
            .border_1()
            .border_color(if !enabled {
                theme::track()
            } else if focused {
                theme::accent()
            } else {
                theme::border()
            })
            .bg(theme::panel())
            .hover(move |s| {
                s.border_color(if !enabled {
                    theme::track()
                } else if focused {
                    theme::accent()
                } else {
                    theme::panel_hover()
                })
            })
            .cursor(if enabled { CursorStyle::IBeam } else { CursorStyle::Arrow })
            .flex()
            .items_center()
            // 点击聚焦；失焦后平台自动停止路由输入，无需手动注销
            .on_click(cx.listener(|this, _, window, _| {
                if !this.enabled {
                    return;
                }
                this.focus.focus(window);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.enabled || !this.focus.is_focused(window) {
                    return;
                }
                match event.keystroke.key.as_str() {
                    // 退格：控制字符被平台过滤不进 IME 通道，在此删除文末字符（V1 caret 恒在文末）
                    "backspace" => {
                        this.content.pop();
                        this.marked = None;
                        cx.notify();
                    }
                    // Esc 清空搜索
                    "escape" => {
                        this.content.clear();
                        this.marked = None;
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .child(if empty && !composing {
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .child(self.placeholder)
                    .into_any_element()
            } else {
                // 组合串按普通文本上屏渲染（V1 不做下划线）
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .truncate()
                    .child(self.content.clone())
                    .into_any_element()
            })
            .child(
                // paint 阶段挂载输入处理器：焦点在本框时 WM_CHAR/IME 路由到此
                canvas(
                    |bounds, _, _| bounds,
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
                    },
                )
                .absolute()
                .inset_0(),
            )
    }
}

#[cfg(test)]
mod tests {
    // 注意：不能用 `use super::*`——gpui 导出了名为 `test` 的宏，会遮蔽内置 #[test]
    use super::{byte_to_utf16, replace_range, utf16_to_byte};
    use std::ops::Range;

    #[test]
    fn insert_and_backspace() {
        let mut c = String::new();
        let caret = c.len()..c.len();
        replace_range(&mut c, caret, "abc");
        assert_eq!(c, "abc");
        let caret = c.len()..c.len();
        replace_range(&mut c, caret, "中文");
        assert_eq!(c, "abc中文");
        // 退格删最后一个字符（与 on_key_down 的 content.pop 行为一致）
        c.pop();
        assert_eq!(c, "abc中");
        c.pop();
        assert_eq!(c, "abc");
    }

    #[test]
    fn ime_mark_replacement() {
        // 模拟 IME 组合流：插入组合串 → 更新组合串 → 上屏替换 → unmark
        let mut c = String::new();
        let mut marked: Option<Range<usize>> = None;
        // 首次组合 "ni"
        let empty = c.len()..c.len();
        marked = Some(replace_range(&mut c, marked.clone().unwrap_or(empty), "ni"));
        assert_eq!(c, "ni");
        // 更新组合 "nihao"：替换已有组合区域
        marked = Some(replace_range(&mut c, marked.clone().unwrap(), "nihao"));
        assert_eq!(c, "nihao");
        // 上屏 "你好"：replace_text_in_range(None) 语义——替换组合区并清除标记
        let m = marked.take().unwrap();
        replace_range(&mut c, m, "你好");
        assert_eq!(c, "你好");
        assert!(marked.is_none());
        // 之后直接键入追加
        let caret = c.len()..c.len();
        replace_range(&mut c, caret, "abc");
        assert_eq!(c, "你好abc");
    }

    #[test]
    fn utf16_offset_conversion() {
        // 中(3B/1u16) a(1B/1u16) 文(3B/1u16) b(1B/1u16) 😀(4B/2u16)：共 12 字节 / 6 码元
        let c = "中a文b😀";
        assert_eq!(utf16_len_for_test(c), 6);
        assert_eq!(utf16_to_byte(c, 4), 8);  // "中a文b" = 4 码元 = 8 字节
        assert_eq!(utf16_to_byte(c, 5), 8);  // 偏移落在 😀 代理对内部 → 向前取整到 pair 起始
        assert_eq!(utf16_to_byte(c, 6), 12); // 字符串末尾
        assert_eq!(byte_to_utf16(c, 6), 2);  // 字节 6 落在 '文' 中间 → 回退到边界 4（"中a"）
        assert_eq!(byte_to_utf16(c, 8), 4);
        assert_eq!(byte_to_utf16(c, 12), 6);
    }

    fn utf16_len_for_test(s: &str) -> usize {
        s.chars().map(|c| c.len_utf16()).sum()
    }

    #[test]
    fn filter_word_extraction() {
        // 页面过滤逻辑约定：trim 后转小写再 contains；空词匹配一切
        let mut c = String::new();
        let caret = c.len()..c.len();
        replace_range(&mut c, caret, "  Chrome Cache  ");
        let q = c.trim().to_lowercase();
        assert_eq!(q, "chrome cache");
        assert!("Google Chrome Cache Data".to_lowercase().contains(&q));
        assert!(!"edge cache".to_lowercase().contains(&q));
        // 空搜索词匹配一切（各页 rows 过滤的 q.is_empty() 分支）
        let empty = String::new().trim().to_lowercase();
        assert!(empty.is_empty());
    }
}
