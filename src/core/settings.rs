use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
use winreg::RegKey;

use crate::ui::theme::{MODE_DARK, MODE_SYSTEM};

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

pub fn init() {
    crate::ui::theme::set_mode(load_theme());
}