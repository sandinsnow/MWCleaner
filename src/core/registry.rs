use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use winreg::enums::*;
use winreg::types::FromRegValue;
use winreg::{RegKey, RegValue};

use super::app_data_dir;

/// 保守判缺失：先展开 %env%；相对路径与访问错误一律视为"存在"，避免误删有效项。
fn path_missing(raw: &str) -> bool {
    let expanded = super::expand_env(raw);
    let p = Path::new(&expanded);
    if !p.is_absolute() {
        return false;
    }
    matches!(p.try_exists(), Ok(false))
}

#[derive(Clone)]
pub struct RegIssue {
    pub key: String,
    pub value_name: Option<String>,
    pub kind: &'static str,
    pub detail: String,
    pub selected: bool,
}

fn split_hive(full: &str) -> (RegKey, &str) {
    if let Some(rest) = full.strip_prefix("HKCU\\") {
        (RegKey::predef(HKEY_CURRENT_USER), rest)
    } else {
        (
            RegKey::predef(HKEY_LOCAL_MACHINE),
            full.strip_prefix("HKLM\\").unwrap_or(full),
        )
    }
}

fn hive_long(full: &str) -> &str {
    if full.starts_with("HKCU\\") {
        "HKEY_CURRENT_USER"
    } else {
        "HKEY_LOCAL_MACHINE"
    }
}

fn get_string(key: &RegKey, name: &str) -> Option<String> {
    key.get_value::<String, _>(name).ok().filter(|s| !s.is_empty())
}

fn exe_from_command(cmd: &str) -> Option<PathBuf> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return None;
    }
    if let Some(rest) = cmd.strip_prefix('"') {
        return rest.split('"').next().map(PathBuf::from);
    }
    let lower = cmd.to_lowercase();
    if let Some(pos) = lower.find(".exe") {
        return Some(PathBuf::from(&cmd[..pos + 4]));
    }
    cmd.split_whitespace().next().map(PathBuf::from)
}

fn uninstall_roots() -> Vec<String> {
    vec![
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall".into(),
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall".into(),
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall".into(),
    ]
}

fn scan_uninstall(issues: &mut Vec<RegIssue>, cancel: &AtomicBool) {
    for root in uninstall_roots() {
        let (hive, sub) = split_hive(&root);
        let Ok(key) = hive.open_subkey_with_flags(sub, KEY_READ) else { continue };
        for sub_name in key.enum_keys().flatten() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let full = format!("{root}\\{sub_name}");
            let Ok(entry) = key.open_subkey_with_flags(&sub_name, KEY_READ) else { continue };
            if entry.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }
            let display = get_string(&entry, "DisplayName").unwrap_or_else(|| sub_name.clone());
            let uninstall = get_string(&entry, "UninstallString");
            let location = get_string(&entry, "InstallLocation");
            if let Some(cmd) = &uninstall {
                if cmd.to_lowercase().contains("msiexec") {
                    continue;
                }
            }
            let exe_missing = match &uninstall {
                Some(cmd) => exe_from_command(cmd).map(|p| path_missing(&p.to_string_lossy())).unwrap_or(true),
                None => true,
            };
            let loc_missing = location.as_ref().map(|p| path_missing(p)).unwrap_or(true);
            if exe_missing && loc_missing {
                issues.push(RegIssue {
                    key: full,
                    value_name: None,
                    kind: "无效卸载项",
                    detail: format!("{display}（卸载程序与安装目录均不存在）"),
                    selected: true,
                });
            }
        }
    }
}

fn scan_shared_dlls(issues: &mut Vec<RegIssue>, cancel: &AtomicBool) {
    for root in [
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\SharedDLLs",
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\SharedDLLs",
    ] {
        let (hive, sub) = split_hive(root);
        let Ok(key) = hive.open_subkey_with_flags(sub, KEY_READ) else { continue };
        for (name, _) in key.enum_values().flatten() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            if path_missing(&name) {
                issues.push(RegIssue {
                    key: root.to_string(),
                    value_name: Some(name.clone()),
                    kind: "缺失共享 DLL",
                    detail: name,
                    selected: true,
                });
            }
        }
    }
}

fn scan_app_paths(issues: &mut Vec<RegIssue>, cancel: &AtomicBool) {
    for root in [
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths",
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\App Paths",
    ] {
        let (hive, sub) = split_hive(root);
        let Ok(key) = hive.open_subkey_with_flags(sub, KEY_READ) else { continue };
        for sub_name in key.enum_keys().flatten() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let Ok(entry) = key.open_subkey_with_flags(&sub_name, KEY_READ) else { continue };
            let Some(exe) = get_string(&entry, "") else { continue };
            if path_missing(&exe) {
                issues.push(RegIssue {
                    key: format!("{root}\\{sub_name}"),
                    value_name: None,
                    kind: "无效 App Path",
                    detail: format!("{sub_name} → {exe}"),
                    selected: true,
                });
            }
        }
    }
}

pub fn scan(cancel: &AtomicBool) -> Vec<RegIssue> {
    let mut issues = Vec::new();
    scan_uninstall(&mut issues, cancel);
    if cancel.load(Ordering::Relaxed) {
        return issues;
    }
    scan_shared_dlls(&mut issues, cancel);
    if cancel.load(Ordering::Relaxed) {
        return issues;
    }
    scan_app_paths(&mut issues, cancel);
    issues
}

fn reg_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn value_line(name: &str, v: &RegValue) -> String {
    let n = if name.is_empty() {
        "@".to_string()
    } else {
        format!("\"{}\"", reg_escape(name))
    };
    match v.vtype {
        REG_SZ => {
            let s = String::from_reg_value(v).unwrap_or_default();
            if s.chars().any(|c| c.is_control()) {
                let bytes: Vec<u8> = s.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect();
                format!("{n}=hex(1):{}", hex_list(&bytes))
            } else {
                format!("{n}=\"{}\"", reg_escape(&s))
            }
        }
        REG_EXPAND_SZ => {
            let s = String::from_reg_value(v).unwrap_or_default();
            let bytes: Vec<u8> = s.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect();
            format!("{n}=hex(2):{}", hex_list(&bytes))
        }
        REG_DWORD => {
            let d = u32::from_reg_value(v).unwrap_or(0);
            format!("{n}=dword:{d:08x}")
        }
        REG_MULTI_SZ => format!("{n}=hex(7):{}", hex_list(&v.bytes)),
        _ => format!("{n}=hex:{}", hex_list(&v.bytes)),
    }
}

fn hex_list(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(",")
}

fn export_key(out: &mut String, hive: &RegKey, path: &str, display_path: &str) -> io::Result<()> {
    out.push_str(&format!("\n[{display_path}]\n"));
    let key = hive.open_subkey_with_flags(path, KEY_READ)?;
    for (name, v) in key.enum_values().flatten() {
        out.push_str(&value_line(&name, &v));
        out.push('\n');
    }
    for sub in key.enum_keys().flatten() {
        export_key(out, hive, &format!("{path}\\{sub}"), &format!("{display_path}\\{sub}"))?;
    }
    Ok(())
}

fn write_backup(issues: &[RegIssue]) -> io::Result<PathBuf> {
    let mut text = String::from("Windows Registry Editor Version 5.00\n");
    for issue in issues {
        let (hive, sub) = split_hive(&issue.key);
        let display = format!("{}\\{}", hive_long(&issue.key), sub);
        match &issue.value_name {
            None => export_key(&mut text, &hive, sub, &display)?,
            Some(name) => {
                text.push_str(&format!("\n[{display}]\n"));
                let key = hive.open_subkey_with_flags(sub, KEY_READ)?;
                let v = key.get_raw_value(name)?;
                text.push_str(&value_line(name, &v));
                text.push('\n');
            }
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = app_data_dir().join("backups");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "registry-{stamp}-{}.reg",
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let mut bytes: Vec<u8> = vec![0xFF, 0xFE];
    bytes.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
    fs::write(&path, bytes)?;
    Ok(path)
}

fn delete_key_recursive(hive: &RegKey, path: &str) -> io::Result<()> {
    let key = hive.open_subkey_with_flags(path, KEY_READ | KEY_WRITE)?;
    for sub in key.enum_keys().flatten() {
        delete_key_recursive(hive, &format!("{path}\\{sub}"))?;
    }
    drop(key);
    hive.delete_subkey(path)
}

pub struct CleanOutcome {
    pub deleted: usize,
    pub failed: usize,
    pub backup: PathBuf,
}

pub fn backup_and_clean(issues: &[RegIssue]) -> Result<CleanOutcome, String> {
    let selected: Vec<_> = issues.iter().filter(|i| i.selected).collect();
    if selected.is_empty() {
        return Err("没有勾选任何项目".into());
    }
    let refs: Vec<RegIssue> = selected.iter().map(|i| (*i).clone()).collect();
    let backup = write_backup(&refs).map_err(|e| format!("备份不完整或写入失败，已中止清理: {e}"))?;
    let mut deleted = 0;
    let mut failed = 0;
    for issue in &selected {
        let (hive, sub) = split_hive(&issue.key);
        let r = match &issue.value_name {
            None => delete_key_recursive(&hive, sub),
            Some(name) => hive
                .open_subkey_with_flags(sub, KEY_WRITE)
                .and_then(|k| k.delete_value(name)),
        };
        if r.is_ok() {
            deleted += 1;
        } else {
            failed += 1;
        }
    }
    Ok(CleanOutcome { deleted, failed, backup })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_cancel() {
        let c = AtomicBool::new(true);
        assert!(scan(&c).is_empty());
    }

    #[test]
    fn backup_restore_roundtrip() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (k, _) = hkcu.create_subkey("Software\\MWCleanerTest\\Sub").unwrap();
        k.set_value("V", &"hello").unwrap();
        k.set_value("D", &7u32).unwrap();
        let issue = RegIssue {
            key: "HKCU\\Software\\MWCleanerTest\\Sub".into(),
            value_name: None,
            kind: "test",
            detail: String::new(),
            selected: true,
        };
        let o = backup_and_clean(&[issue]).unwrap();
        assert_eq!(o.deleted, 1);
        assert!(o.backup.is_file());
        assert!(hkcu.open_subkey("Software\\MWCleanerTest\\Sub").is_err());
        let st = std::process::Command::new("reg.exe")
            .arg("import")
            .arg(&o.backup)
            .status()
            .unwrap();
        assert!(st.success());
        let k2 = hkcu.open_subkey("Software\\MWCleanerTest\\Sub").unwrap();
        assert_eq!(k2.get_value::<String, _>("V").unwrap(), "hello");
        assert_eq!(k2.get_value::<u32, _>("D").unwrap(), 7);
        let _ = delete_key_recursive(&hkcu, "Software\\MWCleanerTest");
        let _ = fs::remove_file(&o.backup);
    }

    #[test]
    fn missing_path_conservative() {
        assert!(!path_missing("relative\\path.exe"));
        assert!(!path_missing("%TEMP%"));
        assert!(!path_missing("C:\\Windows\\explorer.exe"));
        assert!(path_missing("C:\\mwcleaner_definitely_missing_xyz.exe"));
    }

    #[test]
    fn reg_sz_control_chars_hex() {
        let v = RegValue {
            bytes: "a\nb".encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect(),
            vtype: REG_SZ,
        };
        assert!(value_line("x", &v).starts_with("\"x\"=hex(1):"));
        let plain = RegValue {
            bytes: "ab".encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect(),
            vtype: REG_SZ,
        };
        assert_eq!(value_line("x", &plain), "\"x\"=\"ab\"");
    }
}