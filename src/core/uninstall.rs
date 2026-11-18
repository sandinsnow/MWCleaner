use std::collections::HashSet;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use winreg::enums::*;
use winreg::RegKey;

use super::{dir_size, is_reparse};

#[derive(Clone)]
pub struct AppEntry {
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub size: u64,
    pub icon: String,
    pub uninstall: String,
    pub install_dir: String,
    pub reg_key: String,
}

impl AppEntry {
    pub fn icon_source(&self) -> &str {
        if !self.icon.is_empty() {
            &self.icon
        } else {
            &self.uninstall
        }
    }
}

fn uninstall_roots() -> Vec<(RegKey, &'static str)> {
    vec![
        (RegKey::predef(HKEY_LOCAL_MACHINE), "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (RegKey::predef(HKEY_LOCAL_MACHINE), "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (RegKey::predef(HKEY_CURRENT_USER), "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ]
}

fn str_val(k: &RegKey, name: &str) -> String {
    k.get_value::<String, _>(name).unwrap_or_default()
}

pub fn list() -> Vec<AppEntry> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();
    for (hive, root) in uninstall_roots() {
        let hive_tag = if hive.raw_handle() == RegKey::predef(HKEY_CURRENT_USER).raw_handle() {
            "HKCU"
        } else {
            "HKLM"
        };
        let Ok(key) = hive.open_subkey_with_flags(root, KEY_READ) else { continue };
        for sub in key.enum_keys().flatten() {
            let Ok(entry) = key.open_subkey_with_flags(&sub, KEY_READ) else { continue };
            if entry.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }
            let name = str_val(&entry, "DisplayName");
            let uninstall = str_val(&entry, "UninstallString");
            let quiet = str_val(&entry, "QuietUninstallString");
            if name.is_empty() || (uninstall.is_empty() && quiet.is_empty()) {
                continue;
            }
            let version = str_val(&entry, "DisplayVersion");
            let publisher = str_val(&entry, "Publisher");
            let dedup = format!("{}|{}|{}", name.to_lowercase(), version.to_lowercase(), publisher.to_lowercase());
            if !seen.insert(dedup) {
                continue;
            }
            let size_kb = entry.get_value::<u32, _>("EstimatedSize").unwrap_or(0) as u64;
            apps.push(AppEntry {
                name,
                version,
                publisher,
                size: size_kb * 1024,
                icon: str_val(&entry, "DisplayIcon"),
                uninstall: if uninstall.is_empty() { quiet } else { uninstall },
                install_dir: str_val(&entry, "InstallLocation"),
                reg_key: format!("{hive_tag}\\{root}\\{sub}"),
            });
        }
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

fn parse_command(raw: &str) -> Option<(String, String)> {
    let cmd = raw.trim();
    if cmd.is_empty() {
        return None;
    }
    if let Some(rest) = cmd.strip_prefix('"') {
        let end = rest.find('"')?;
        let exe = rest[..end].to_string();
        let args = rest[end + 1..].trim().to_string();
        Some((exe, args))
    } else {
        let mut it = cmd.splitn(2, char::is_whitespace);
        let exe = it.next()?.to_string();
        let args = it.next().unwrap_or("").trim().to_string();
        Some((exe, args))
    }
}

/// 启动官方卸载器并等待其退出，返回退出码。
pub fn run_uninstaller(app: &AppEntry) -> Result<i32, String> {
    let (exe, mut args) = parse_command(&app.uninstall)
        .ok_or_else(|| "无法解析卸载命令".to_string())?;
    // MSI 的 /I 是维护模式，改 /X 直接进入卸载
    if exe.to_ascii_lowercase().ends_with("msiexec.exe") {
        args = args.replace("/I", "/X").replace("/i", "/X");
    }
    let mut cmd = Command::new(&exe);
    if !args.is_empty() {
        cmd.raw_arg(&args);
    }
    let child = cmd.spawn().or_else(|_| {
        Command::new("cmd.exe")
            .raw_arg(format!("/C \"{}\"", app.uninstall))
            .spawn()
    });
    let mut child = child.map_err(|e| format!("启动卸载程序失败：{e}"))?;
    let status = child.wait().map_err(|e| format!("等待卸载程序失败：{e}"))?;
    Ok(status.code().unwrap_or(-1))
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

const WHITELIST: &[&str] = &[
    "microsoft", "windows", "commonfiles", "packages", "temp", "mwcleaner",
    "packagecache", "applicationdata", "programs", "windowskits",
];

#[derive(Clone, PartialEq)]
pub enum LeftoverKind {
    Dir,
    Reg,
}

#[derive(Clone)]
pub struct LeftoverItem {
    pub kind: LeftoverKind,
    pub path: String,
    pub size: u64,
    pub selected: bool,
}

fn scan_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramData", "LOCALAPPDATA", "APPDATA"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect()
}

fn leaf_norm(path: &str) -> String {
    PathBuf::from(path.trim_end_matches(['\\', '/']))
        .file_name()
        .map(|s| normalize(&s.to_string_lossy()))
        .unwrap_or_default()
}

/// 定向扫描某软件的残留：安装目录、各程序数据根下匹配目录、卸载注册表键与厂商/产品键。
pub fn scan_leftovers(app: &AppEntry, installed: &[AppEntry]) -> Vec<LeftoverItem> {
    let mut items = Vec::new();
    let mut ids: Vec<String> = vec![normalize(&app.name), normalize(&app.publisher), leaf_norm(&app.install_dir)];
    ids.retain(|s| s.len() >= 4);

    // 其他仍在安装的软件占用的目录名，避免误报
    let others: Vec<String> = installed
        .iter()
        .filter(|o| o.reg_key != app.reg_key)
        .flat_map(|o| [normalize(&o.name), leaf_norm(&o.install_dir)])
        .filter(|s| s.len() >= 4)
        .collect();

    let mut dirs: Vec<PathBuf> = Vec::new();
    let install = PathBuf::from(app.install_dir.trim_end_matches(['\\', '/']));
    if !app.install_dir.is_empty() && install.is_dir() {
        dirs.push(install);
    }
    for root in scan_roots() {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let Ok(md) = entry.metadata() else { continue };
            if !md.is_dir() || is_reparse(&md) {
                continue;
            }
            let n = normalize(&entry.file_name().to_string_lossy());
            if n.len() < 4 || WHITELIST.contains(&n.as_str()) {
                continue;
            }
            if others.iter().any(|o| o == &n) {
                continue;
            }
            if ids.iter().any(|id| id.contains(&n) || n.contains(id)) {
                let p = entry.path();
                if !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }
    }
    for d in dirs {
        let (size, _) = dir_size(&d);
        items.push(LeftoverItem {
            kind: LeftoverKind::Dir,
            path: d.to_string_lossy().to_string(),
            size,
            selected: false,
        });
    }

    let mut reg_keys: Vec<String> = vec![app.reg_key.clone()];
    let publisher = app.publisher.trim();
    let product = app.name.trim();
    if !publisher.is_empty() && !product.is_empty() {
        for hive in ["HKCU", "HKLM"] {
            reg_keys.push(format!("{hive}\\Software\\{publisher}\\{product}"));
            reg_keys.push(format!("{hive}\\Software\\WOW6432Node\\{publisher}\\{product}"));
        }
        // 厂商根键仅在没有其他同厂商软件时才视为残留
        let same_publisher = installed.iter().any(|o| {
            o.reg_key != app.reg_key && normalize(&o.publisher) == normalize(publisher)
        });
        if !same_publisher {
            for hive in ["HKCU", "HKLM"] {
                reg_keys.push(format!("{hive}\\Software\\{publisher}"));
            }
        }
    }
    let mut seen = HashSet::new();
    for key in reg_keys {
        if !seen.insert(key.to_lowercase()) {
            continue;
        }
        let (hive, sub) = if let Some(rest) = key.strip_prefix("HKCU\\") {
            (RegKey::predef(HKEY_CURRENT_USER), rest)
        } else {
            (RegKey::predef(HKEY_LOCAL_MACHINE), key.strip_prefix("HKLM\\").unwrap_or(&key))
        };
        if hive.open_subkey_with_flags(sub, KEY_READ).is_ok() {
            items.push(LeftoverItem {
                kind: LeftoverKind::Reg,
                path: key,
                size: 0,
                selected: false,
            });
        }
    }
    items
}

pub struct CleanReport {
    pub freed: u64,
    pub failed: usize,
    pub backup: Option<PathBuf>,
}

pub fn clean(items: &[LeftoverItem], to_recycle: bool) -> CleanReport {
    let mut freed = 0u64;
    let mut failed = 0usize;
    let mut regs = Vec::new();
    for item in items.iter().filter(|i| i.selected) {
        match item.kind {
            LeftoverKind::Dir => {
                let p = PathBuf::from(&item.path);
                let ok = if to_recycle {
                    (unsafe { super::junk::recycle_paths(&[p.clone()]) }) && !p.exists()
                } else {
                    fs::remove_dir_all(&p).is_ok()
                };
                if ok {
                    freed += item.size;
                } else {
                    failed += 1;
                }
            }
            LeftoverKind::Reg => regs.push(super::registry::RegIssue {
                key: item.path.clone(),
                value_name: None,
                kind: "卸载残留",
                detail: String::new(),
                selected: true,
            }),
        }
    }
    let mut backup = None;
    if !regs.is_empty() {
        match super::registry::backup_and_clean(&regs) {
            Ok(o) => {
                failed += o.failed;
                backup = Some(o.backup);
            }
            Err(_) => failed += regs.len(),
        }
    }
    CleanReport { freed, failed, backup }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(dir: &PathBuf) -> AppEntry {
        AppEntry {
            name: "MWCleanerTestApp".into(),
            version: "1.0".into(),
            publisher: "MWCleanerTestVendor".into(),
            size: 0,
            icon: String::new(),
            uninstall: format!("\"{}\" /S", dir.join("unins.exe").display()),
            install_dir: dir.to_string_lossy().to_string(),
            reg_key: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MWCleanerTestApp".into(),
        }
    }

    #[test]
    fn parse_command_forms() {
        let (exe, args) = parse_command("\"C:\\Program Files\\App\\unins000.exe\" /SILENT").unwrap();
        assert_eq!(exe, "C:\\Program Files\\App\\unins000.exe");
        assert_eq!(args, "/SILENT");
        let (exe, args) = parse_command("MsiExec.exe /I{GUID-123}").unwrap();
        assert_eq!(exe, "MsiExec.exe");
        assert_eq!(args, "/I{GUID-123}");
        assert!(parse_command("").is_none());
    }

    #[test]
    fn leftover_scan_and_clean_roundtrip() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let base = std::env::temp_dir().join(format!("mwcleaner_untest_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("data")).unwrap();
        fs::write(base.join("data\\a.bin"), vec![3u8; 4096]).unwrap();

        // 自建卸载注册表项 + 厂商产品键
        let (uk, _) = hkcu.create_subkey(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MWCleanerTestApp",
        ).unwrap();
        uk.set_value("DisplayName", &"MWCleanerTestApp").unwrap();
        uk.set_value("UninstallString", &"cmd.exe /c exit 0").unwrap();
        uk.set_value("InstallLocation", &base.to_string_lossy().to_string()).unwrap();
        let (pk, _) = hkcu.create_subkey("Software\\MWCleanerTestVendor\\MWCleanerTestApp").unwrap();
        pk.set_value("V", &"x").unwrap();

        let app = test_app(&base);
        let found = scan_leftovers(&app, &[app.clone()]);
        let has_dir = found.iter().any(|i| i.kind == LeftoverKind::Dir && i.path == base.to_string_lossy());
        let has_uk = found.iter().any(|i| i.kind == LeftoverKind::Reg && i.path == app.reg_key);
        let has_pk = found.iter().any(|i| i.kind == LeftoverKind::Reg
            && i.path == "HKCU\\Software\\MWCleanerTestVendor\\MWCleanerTestApp");
        assert!(has_dir && has_uk && has_pk, "found: {:?}", found.iter().map(|i| &i.path).collect::<Vec<_>>());

        let selected: Vec<LeftoverItem> = found.into_iter().map(|mut i| { i.selected = true; i }).collect();
        let report = clean(&selected, true);
        assert_eq!(report.failed, 0);
        assert!(report.freed >= 4096);
        assert!(!base.exists());
        assert!(report.backup.is_some());
        assert!(hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MWCleanerTestApp").is_err());
        assert!(hkcu.open_subkey("Software\\MWCleanerTestVendor").is_err());

        // 清理测试备份文件；回收站中的测试件由外部脚本按来源路径还原清理
        if let Some(b) = report.backup {
            let _ = fs::remove_file(b);
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn run_uninstaller_exit_code() {
        let app = AppEntry {
            name: "t".into(),
            version: String::new(),
            publisher: String::new(),
            size: 0,
            icon: String::new(),
            uninstall: "cmd.exe /c exit 7".into(),
            install_dir: String::new(),
            reg_key: String::new(),
        };
        assert_eq!(run_uninstaller(&app).unwrap(), 7);
    }
}