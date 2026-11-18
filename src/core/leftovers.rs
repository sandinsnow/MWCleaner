use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use winreg::enums::*;
use winreg::RegKey;

use super::dir_size;

#[derive(Clone)]
pub struct Leftover {
    pub path: PathBuf,
    pub size: u64,
    pub selected: bool,
}

const WHITELIST: &[&str] = &[
    "microsoft",
    "windows",
    "commonfiles",
    "packages",
    "temp",
    "mwcleaner",
    "packagecache",
    "applicationdata",
    // 每用户安装容器与 SDK 目录，不是残留
    "programs",
    "windowskits",
];

fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

fn uninstall_roots() -> Vec<(RegKey, &'static str)> {
    vec![
        (RegKey::predef(HKEY_LOCAL_MACHINE), "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (RegKey::predef(HKEY_LOCAL_MACHINE), "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (RegKey::predef(HKEY_CURRENT_USER), "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ]
}

fn installed_identifiers() -> HashSet<String> {
    let mut ids = HashSet::new();
    for (hive, root) in uninstall_roots() {
        let Ok(key) = hive.open_subkey_with_flags(root, KEY_READ) else { continue };
        for sub in key.enum_keys().flatten() {
            let Ok(entry) = key.open_subkey_with_flags(&sub, KEY_READ) else { continue };
            for field in ["DisplayName", "Publisher"] {
                if let Ok(v) = entry.get_value::<String, _>(field) {
                    let n = normalize(&v);
                    if n.len() >= 4 {
                        ids.insert(n);
                    }
                }
            }
            if let Ok(loc) = entry.get_value::<String, _>("InstallLocation") {
                if let Some(leaf) = PathBuf::from(loc.trim_end_matches(['\\', '/']))
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                {
                    let n = normalize(&leaf);
                    if n.len() >= 4 {
                        ids.insert(n);
                    }
                }
            }
        }
    }
    ids
}

fn scan_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramData", "LOCALAPPDATA", "APPDATA"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}

pub fn scan(progress: &dyn Fn(usize, usize, &str)) -> Vec<Leftover> {
    let ids = installed_identifiers();
    let roots = scan_roots();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in &roots {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for entry in entries.flatten() {
            let Ok(md) = entry.metadata() else { continue };
            if !md.is_dir() || super::is_reparse(&md) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let n = normalize(&name);
            if n.len() < 4 || WHITELIST.contains(&n.as_str()) {
                continue;
            }
            let in_use = ids.iter().any(|id| id.contains(&n) || n.contains(id));
            if !in_use {
                candidates.push(entry.path());
            }
        }
    }
    let total = candidates.len();
    let mut result = Vec::new();
    for (i, path) in candidates.into_iter().enumerate() {
        progress(i, total, &path.to_string_lossy());
        let (size, _) = dir_size(&path);
        result.push(Leftover { path, size, selected: false });
    }
    result.sort_by(|a, b| b.size.cmp(&a.size));
    result
}

pub fn clean(items: &[Leftover]) -> (u64, usize) {
    let mut freed = 0u64;
    let mut failed = 0usize;
    for item in items.iter().filter(|i| i.selected) {
        match fs::remove_dir_all(&item.path) {
            Ok(()) => freed += item.size,
            Err(_) => failed += 1,
        }
    }
    (freed, failed)
}