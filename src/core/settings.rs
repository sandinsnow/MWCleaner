use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

/// 主题模式常量归位于 core：持久化层与 ui::theme 共同引用，避免 core 反向依赖 ui。
pub const MODE_DARK: u32 = 0;
pub const MODE_LIGHT: u32 = 1;
pub const MODE_SYSTEM: u32 = 2;

const KEY: &str = "Software\\MWCleaner\\Settings";

pub fn load_u32(name: &str, default: u32) -> u32 {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey_with_flags(KEY, KEY_READ)
        .ok()
        .and_then(|k| k.get_value::<u32, _>(name).ok())
        .unwrap_or(default)
}

pub fn save_u32(name: &str, value: u32) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((k, _)) = hkcu.create_subkey(KEY) {
        let _ = k.set_value(name, &value);
    }
}

pub fn load_flag(name: &str, default: bool) -> bool {
    load_u32(name, default as u32) != 0
}

pub fn save_flag(name: &str, value: bool) {
    save_u32(name, value as u32);
}

pub fn load_theme() -> u32 {
    let v = load_u32("Theme", MODE_DARK);
    if v <= MODE_SYSTEM {
        v
    } else {
        MODE_DARK
    }
}

pub fn save_theme(mode: u32) {
    save_u32("Theme", mode);
}

/// B5：窗口位置尺寸记忆。x/y 允许负值（副屏在主屏左侧），按 i32 位转换存取。
pub struct WindowMemory {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub maximized: bool,
}

const WIN_X: &str = "WinX";
const WIN_Y: &str = "WinY";
const WIN_W: &str = "WinW";
const WIN_H: &str = "WinH";
const WIN_MAX: &str = "WinMax";

pub fn load_window_memory() -> Option<WindowMemory> {
    let maximized = load_u32(WIN_MAX, 0) != 0;
    let w = load_u32(WIN_W, 0);
    let h = load_u32(WIN_H, 0);
    // 从未记录过窗口尺寸时返回 None，维持默认居中
    if !maximized && (w == 0 || h == 0) {
        return None;
    }
    Some(WindowMemory {
        x: load_u32(WIN_X, 0) as i32,
        y: load_u32(WIN_Y, 0) as i32,
        w,
        h,
        maximized,
    })
}

pub fn save_window_memory(m: &WindowMemory) {
    save_u32(WIN_X, m.x as u32);
    save_u32(WIN_Y, m.y as u32);
    save_u32(WIN_W, m.w);
    save_u32(WIN_H, m.h);
    save_u32(WIN_MAX, m.maximized as u32);
}