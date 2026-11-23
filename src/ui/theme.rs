use gpui::Rgba;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub use crate::core::settings::{MODE_DARK, MODE_LIGHT, MODE_SYSTEM};

static MODE: AtomicU8 = AtomicU8::new(MODE_DARK as u8);
static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn mode() -> u32 {
    MODE.load(Ordering::Relaxed) as u32
}

pub fn set_mode(m: u32) {
    let m = m.min(2);
    MODE.store(m as u8, Ordering::Relaxed);
    LIGHT.store(system_light(m), Ordering::Relaxed);
}

fn system_light(m: u32) -> bool {
    match m {
        MODE_LIGHT => true,
        MODE_SYSTEM => apps_use_light_theme(),
        _ => false,
    }
}

fn apps_use_light_theme() -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey_with_flags(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        KEY_READ,
    )
    .ok()
    .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme").ok())
    .map(|v| v != 0)
    .unwrap_or(false)
}

fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// 跟随系统模式下重读系统主题键；解析结果变化时返回 true（调用方负责重绘）。
pub fn refresh_system() -> bool {
    if mode() != MODE_SYSTEM {
        return false;
    }
    let now = apps_use_light_theme();
    LIGHT.swap(now, Ordering::Relaxed) != now
}

fn rgb(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

fn pair(dark: u32, light: u32) -> Rgba {
    rgb(if is_light() { light } else { dark })
}

pub fn bg() -> Rgba { pair(0x13151a, 0xf3f5f8) }
pub fn panel() -> Rgba { pair(0x1c1f26, 0xffffff) }
pub fn panel_hover() -> Rgba { pair(0x262a33, 0xeef1f5) }
pub fn border() -> Rgba { pair(0x2e333e, 0xdde1e8) }
pub fn text() -> Rgba { pair(0xe6e8ee, 0x1c2128) }
pub fn muted() -> Rgba { pair(0x8b91a0, 0x626b76) }
pub fn accent() -> Rgba { pair(0x5b9dff, 0x2f6feb) }
pub fn accent_hover() -> Rgba { pair(0x74adff, 0x4f8cff) }
pub fn danger() -> Rgba { pair(0xf0615c, 0xcf222e) }
pub fn danger_hover() -> Rgba { pair(0xf77b76, 0xa40e26) }
pub fn success() -> Rgba { pair(0x43c95f, 0x1a7f37) }
pub fn track() -> Rgba { pair(0x262a33, 0xe6e9ee) }
pub fn warning() -> Rgba { pair(0xdba32c, 0x9a6700) }

pub fn accent_soft() -> Rgba { pair(0x24344d, 0xe3edfd) }
pub fn danger_soft() -> Rgba { pair(0x3d2426, 0xfde7e9) }
pub fn success_soft() -> Rgba { pair(0x1f3327, 0xe2f4e8) }
pub fn warning_soft() -> Rgba { pair(0x3a3120, 0xfbf0d9) }