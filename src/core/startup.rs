use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use winreg::enums::*;
use winreg::RegKey;
use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};

use super::app_data_dir;

const DISABLED_KEY: &str = "Software\\MWCleaner\\DisabledStartup";

#[derive(Clone)]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    pub description: String,
    pub source: String,
    pub is_folder: bool,
    pub enabled: bool,
}

pub fn expand_env(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split('%').enumerate() {
        if i % 2 == 1 {
            match std::env::var(part) {
                Ok(v) => out.push_str(&v),
                Err(_) => {
                    out.push('%');
                    out.push_str(part);
                    out.push('%');
                }
            }
        } else {
            out.push_str(part);
        }
    }
    out
}

pub fn exe_path(command: &str) -> Option<PathBuf> {
    let cmd = command.trim();
    let mut candidates: Vec<String> = Vec::new();
    if let Some(rest) = cmd.strip_prefix('"') {
        if let Some(p) = rest.split('"').next() {
            candidates.push(p.to_string());
        }
    }
    if let Some(first) = cmd.split_whitespace().next() {
        candidates.push(first.trim_matches('"').to_string());
    }
    candidates.push(cmd.to_string());
    candidates
        .into_iter()
        .map(|c| PathBuf::from(expand_env(&c)))
        .find(|p| p.is_file())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn file_description(path: &PathBuf) -> String {
    unsafe {
        let w: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return String::new();
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(PCWSTR(w.as_ptr()), 0, size, buf.as_mut_ptr() as *mut _)
            .is_err()
        {
            return String::new();
        }
        let mut langs: Vec<(u16, u16)> = Vec::new();
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if VerQueryValueW(
            buf.as_ptr() as *const _,
            PCWSTR(wide("\\VarFileInfo\\Translation").as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && !ptr.is_null()
            && len >= 4
        {
            let t = std::slice::from_raw_parts(ptr as *const u16, len as usize / 2);
            for pair in t.chunks(2) {
                if pair.len() == 2 {
                    langs.push((pair[0], pair[1]));
                }
            }
        }
        langs.push((0x0804, 0x04b0));
        langs.push((0x0409, 0x04b0));
        for (lang, cp) in langs {
            let q = wide(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
            let mut p2: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut l2 = 0u32;
            if VerQueryValueW(buf.as_ptr() as *const _, PCWSTR(q.as_ptr()), &mut p2, &mut l2)
                .as_bool()
                && !p2.is_null()
                && l2 > 0
            {
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(
                    p2 as *const u16,
                    l2 as usize,
                ));
                let s = s.trim_end_matches('\0').trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
        String::new()
    }
}

fn description_of(command: &str) -> String {
    exe_path(command)
        .map(|p| file_description(&p))
        .unwrap_or_default()
}

fn reg_sources() -> Vec<String> {
    vec![
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce".into(),
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce".into(),
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
    ]
}

fn strip_hive(source: &str) -> (&str, RegKey) {
    if let Some(rest) = source.strip_prefix("HKCU\\") {
        (rest, RegKey::predef(HKEY_CURRENT_USER))
    } else {
        (source.strip_prefix("HKLM\\").unwrap_or(source), RegKey::predef(HKEY_LOCAL_MACHINE))
    }
}

fn folder_sources() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        v.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        v.push(PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"));
    }
    v.into_iter().filter(|p| p.is_dir()).collect()
}

fn disabled_store() -> io::Result<RegKey> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(DISABLED_KEY)?;
    Ok(key)
}

fn store_id(item: &StartupItem) -> String {
    format!("{}|{}", item.source, item.name)
}

pub fn list() -> Vec<StartupItem> {
    let mut items = Vec::new();
    for source in reg_sources() {
        let (sub, hive) = strip_hive(&source);
        let Ok(key) = hive.open_subkey_with_flags(sub, KEY_READ) else {
            continue;
        };
        for name in key.enum_values().flatten().map(|(n, _)| n) {
            if let Ok(cmd) = key.get_value::<String, _>(&name) {
                items.push(StartupItem {
                    name,
                    description: description_of(&cmd),
                    command: cmd,
                    source: source.clone(),
                    is_folder: false,
                    enabled: true,
                });
            }
        }
    }
    for folder in folder_sources() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            items.push(StartupItem {
                name,
                description: description_of(&entry.path().to_string_lossy()),
                command: entry.path().to_string_lossy().to_string(),
                source: folder.to_string_lossy().to_string(),
                is_folder: true,
                enabled: true,
            });
        }
    }
    if let Ok(store) = disabled_store() {
        for (id, _) in store.enum_values().flatten() {
            let Some((source, name)) = id.split_once('|') else { continue };
            let Ok(command) = store.get_value::<String, _>(&id) else { continue };
            let is_folder = !source.starts_with("HKCU\\") && !source.starts_with("HKLM\\");
            items.push(StartupItem {
                name: name.to_string(),
                description: description_of(&command),
                command,
                source: source.to_string(),
                is_folder,
                enabled: false,
            });
        }
    }
    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

pub fn set_enabled(item: &StartupItem, enabled: bool) -> io::Result<()> {
    if item.enabled == enabled {
        return Ok(());
    }
    let store = disabled_store()?;
    let id = store_id(item);
    if enabled {
        let command: String = store.get_value(&id)?;
        if item.is_folder {
            let backup = app_data_dir().join("disabled-startup").join(&item.name);
            let target = PathBuf::from(&item.source).join(&item.name);
            fs::rename(&backup, &target)?;
        } else {
            let (sub, hive) = strip_hive(&item.source);
            let (key, _) = hive.create_subkey(sub)?;
            key.set_value(&item.name, &command)?;
        }
        store.delete_value(&id)?;
    } else if item.is_folder {
        let src = PathBuf::from(&item.source).join(&item.name);
        let backup_dir = app_data_dir().join("disabled-startup");
        fs::create_dir_all(&backup_dir)?;
        fs::rename(&src, backup_dir.join(&item.name))?;
        store.set_value(&id, &item.command)?;
    } else {
        let (sub, hive) = strip_hive(&item.source);
        let key = hive.open_subkey_with_flags(sub, KEY_READ | KEY_WRITE)?;
        let command: String = key.get_value(&item.name)?;
        store.set_value(&id, &command)?;
        key.delete_value(&item.name)?;
    }
    Ok(())
}

pub fn delete(item: &StartupItem) -> io::Result<()> {
    if item.enabled {
        // A3：启用项删除先走禁用备份（注册表命令/启动文件夹 lnk），可在已禁用列表启用还原
        return set_enabled(item, false);
    }
    let store = disabled_store()?;
    store.delete_value(store_id(item))?;
    if item.is_folder {
        let backup = app_data_dir().join("disabled-startup").join(&item.name);
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_enabled_backup_roundtrip() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run, _) = hkcu.create_subkey("Software\\MWCleanerTest\\Run").unwrap();
        run.set_value("MWCleanerTestEntry", &"cmd.exe /c exit 0").unwrap();
        let item = StartupItem {
            name: "MWCleanerTestEntry".into(),
            description: String::new(),
            command: "cmd.exe /c exit 0".into(),
            source: "HKCU\\Software\\MWCleanerTest\\Run".into(),
            is_folder: false,
            enabled: true,
        };
        delete(&item).unwrap();
        assert!(run.get_value::<String, _>("MWCleanerTestEntry").is_err());
        let store = disabled_store().unwrap();
        assert!(store.get_value::<String, _>(&store_id(&item)).is_ok());

        let disabled = StartupItem { enabled: false, ..item.clone() };
        set_enabled(&disabled, true).unwrap();
        assert_eq!(
            run.get_value::<String, _>("MWCleanerTestEntry").unwrap(),
            "cmd.exe /c exit 0"
        );
        assert!(store.get_value::<String, _>(&store_id(&item)).is_err());

        set_enabled(&item, false).unwrap();
        delete(&disabled).unwrap();
        hkcu.delete_subkey_all("Software\\MWCleanerTest").unwrap();
    }
}