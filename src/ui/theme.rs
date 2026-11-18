use gpui::Rgba;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub const MODE_DARK: u32 = 0;
pub const MODE_LIGHT: u32 = 1;
pub const MODE_SYSTEM: u32 = 2;

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

pub fn bg() -> Rgba { pair(0x17181c, 0xf5f6f8) }
pub fn panel() -> Rgba { pair(0x202128, 0xffffff) }
pub fn panel_hover() -> Rgba { pair(0x2a2b34, 0xeceef2) }
pub fn border() -> Rgba { pair(0x33343e, 0xd8dbe1) }
pub fn text() -> Rgba { pair(0xe8e8ec, 0x1f2328) }
pub fn muted() -> Rgba { pair(0x9a9ca6, 0x697077) }
pub fn accent() -> Rgba { pair(0x4f8cff, 0x2f6feb) }
pub fn accent_hover() -> Rgba { pair(0x6ba1ff, 0x4f8cff) }
pub fn danger() -> Rgba { pair(0xe5534b, 0xcf222e) }
pub fn danger_hover() -> Rgba { pair(0xf06a62, 0xa40e26) }
pub fn success() -> Rgba { pair(0x3fb950, 0x1a7f37) }
pub fn track() -> Rgba { pair(0x2c2d36, 0xe6e8ec) }
pub fn warning() -> Rgba { pair(0xd29922, 0x9a6700) }