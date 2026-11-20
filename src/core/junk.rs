use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    SHFileOperationW, SHFILEOPSTRUCTW, SHQueryRecycleBinW, SHEmptyRecycleBinW, FO_DELETE,
    FOF_ALLOWUNDO, FOF_NOERRORUI, FOF_SILENT, SHQUERYRBINFO,
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
};

use super::{delete_contents, dir_size, is_reparse};

#[derive(Clone)]
pub struct JunkCategory {
    pub name: &'static str,
    pub description: &'static str,
    pub roots: Vec<PathBuf>,
    pub size: u64,
    pub file_count: u64,
    pub selected: bool,
    pub recycle_bin: bool,
}

pub struct CleanResult {
    pub freed: u64,
    pub failed: u64,
    pub recycled: bool,
    pub cancelled: bool,
}

fn env_dir(var: &str, sub: &str) -> Option<PathBuf> {
    std::env::var(var).ok().map(|v| PathBuf::from(v).join(sub))
}

fn existing(dirs: Vec<Option<PathBuf>>) -> Vec<PathBuf> {
    dirs.into_iter()
        .flatten()
        .filter(|p| p.is_dir())
        .collect()
}

fn browser_cache_roots(user_data: PathBuf) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Ok(profiles) = fs::read_dir(&user_data) else {
        return roots;
    };
    for profile in profiles.flatten() {
        let name = profile.file_name().to_string_lossy().to_string();
        if name != "Default" && !name.starts_with("Profile ") && name != "Guest Profile" {
            continue;
        }
        for sub in ["Cache", "Code Cache", "GPUCache"] {
            let p = profile.path().join(sub);
            if p.is_dir() {
                roots.push(p);
            }
        }
    }
    roots
}

fn firefox_cache_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(profiles) = env_dir("LOCALAPPDATA", "Mozilla\\Firefox\\Profiles") else {
        return roots;
    };
    let Ok(entries) = fs::read_dir(profiles) else {
        return roots;
    };
    for entry in entries.flatten() {
        let p = entry.path().join("cache2");
        if p.is_dir() {
            roots.push(p);
        }
    }
    roots
}

fn thumbnail_files() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Some(dir) = env_dir("LOCALAPPDATA", "Microsoft\\Windows\\Explorer") else {
        return roots;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return roots;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if (name.starts_with("thumbcache_") || name.starts_with("iconcache_"))
            && name.ends_with(".db")
        {
            roots.push(entry.path());
        }
    }
    roots
}

pub fn build_categories() -> Vec<JunkCategory> {
    let windir = std::env::var("windir").unwrap_or_else(|_| "C:\\Windows".into());
    let w = PathBuf::from(&windir);
    let mut cats = vec![
        JunkCategory {
            name: "用户临时文件",
            description: "当前用户 Temp 目录中的临时文件",
            roots: existing(vec![std::env::var("TEMP").ok().map(PathBuf::from)]),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "系统临时文件",
            description: "Windows\\Temp 目录中的临时文件",
            roots: vec![w.join("Temp")].into_iter().filter(|p| p.is_dir()).collect(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "回收站",
            description: "所有驱动器回收站中的文件",
            roots: Vec::new(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: true,
        },
        JunkCategory {
            name: "Chrome 缓存",
            description: "Chrome 浏览器缓存（不影响书签和密码）",
            roots: env_dir("LOCALAPPDATA", "Google\\Chrome\\User Data")
                .map(browser_cache_roots)
                .unwrap_or_default(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "Edge 缓存",
            description: "Edge 浏览器缓存（不影响书签和密码）",
            roots: env_dir("LOCALAPPDATA", "Microsoft\\Edge\\User Data")
                .map(browser_cache_roots)
                .unwrap_or_default(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "Firefox 缓存",
            description: "Firefox 浏览器磁盘缓存",
            roots: firefox_cache_roots(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "Windows 更新缓存",
            description: "已下载的安装包（SoftwareDistribution\\Download）",
            roots: vec![w.join("SoftwareDistribution\\Download")]
                .into_iter()
                .filter(|p| p.is_dir())
                .collect(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "传递优化缓存",
            description: "Windows 更新的 P2P 分发缓存",
            roots: vec![w.join(
                "ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization\\Cache",
            )]
            .into_iter()
            .filter(|p| p.is_dir())
            .collect(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "缩略图缓存",
            description: "资源管理器缩略图/图标缓存（会自动重建）",
            roots: thumbnail_files(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "崩溃转储与日志",
            description: "程序崩溃转储、系统小型转储文件",
            roots: {
                let mut v = existing(vec![
                    env_dir("LOCALAPPDATA", "CrashDumps"),
                    Some(w.join("Minidump")),
                ]);
                let mem = w.join("MEMORY.DMP");
                if mem.is_file() {
                    v.push(mem);
                }
                v
            },
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        },
        JunkCategory {
            name: "预读取文件",
            description: "Prefetch 记录（清理后短期内程序启动略慢，默认不选）",
            roots: vec![w.join("Prefetch")].into_iter().filter(|p| p.is_dir()).collect(),
            size: 0,
            file_count: 0,
            selected: false,
            recycle_bin: false,
        },
    ];
    cats.retain(|c| c.recycle_bin || !c.roots.is_empty());
    cats
}

fn recycle_bin_size() -> (u64, u64) {
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if SHQueryRecycleBinW(PCWSTR::null(), &mut info).is_ok() {
            (info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64)
        } else {
            (0, 0)
        }
    }
}

pub fn scan(progress: &dyn Fn(usize, usize, &str), cancel: &AtomicBool) -> Vec<JunkCategory> {
    let mut cats = build_categories();
    let total = cats.len();
    for (i, cat) in cats.iter_mut().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(i, total, cat.name);
        if cat.recycle_bin {
            let (size, count) = recycle_bin_size();
            cat.size = size;
            cat.file_count = count;
            continue;
        }
        for root in &cat.roots {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if root.is_file() {
                if let Ok(md) = root.metadata() {
                    cat.size += md.len();
                    cat.file_count += 1;
                }
            } else {
                let (size, count) = dir_size(root);
                cat.size += size;
                cat.file_count += count;
            }
        }
    }
    progress(total, total, "完成");
    cats
}

fn wide_list(paths: &[PathBuf]) -> Vec<u16> {
    let mut v = Vec::new();
    for p in paths {
        v.extend(p.to_string_lossy().encode_utf16());
        v.push(0);
    }
    v.push(0);
    v
}

pub(crate) unsafe fn recycle_paths(paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return true;
    }
    let from = wide_list(paths);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        // 不带 FOF_NOCONFIRMATION：超回收站容量时由 Shell 弹窗征求永久删除同意，不静默直删
        fFlags: (FOF_ALLOWUNDO | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    SHFileOperationW(&mut op) == 0
}

fn entry_size(p: &Path) -> u64 {
    if p.is_dir() {
        dir_size(p).0
    } else {
        p.metadata().map(|m| m.len()).unwrap_or(0)
    }
}

fn clean_dir_recycle(root: &Path) -> (u64, u64) {
    let Ok(entries) = fs::read_dir(root) else {
        return (0, 1);
    };
    let items: Vec<(PathBuf, u64)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| fs::symlink_metadata(p).map(|m| !is_reparse(&m)).unwrap_or(false))
        .map(|p| {
            let size = entry_size(&p);
            (p, size)
        })
        .collect();
    if items.is_empty() {
        return (0, 0);
    }
    let paths: Vec<PathBuf> = items.iter().map(|(p, _)| p.clone()).collect();
    unsafe {
        recycle_paths(&paths);
    }
    let mut freed = 0u64;
    let mut failed = 0u64;
    for (p, size) in &items {
        if p.exists() {
            failed += 1;
        } else {
            freed += size;
        }
    }
    (freed, failed)
}

pub fn clean(
    cats: &[JunkCategory],
    progress: &dyn Fn(usize, usize, &str),
    to_recycle: bool,
    cancel: &AtomicBool,
) -> CleanResult {
    let selected: Vec<_> = cats.iter().filter(|c| c.selected).collect();
    let total = selected.len();
    let mut freed = 0u64;
    let mut failed = 0u64;
    for (i, cat) in selected.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(i, total, cat.name);
        if cat.recycle_bin {
            let before = recycle_bin_size().0;
            let ok = unsafe {
                SHEmptyRecycleBinW(
                    HWND::default(),
                    PCWSTR::null(),
                    SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
                )
                .is_ok()
            };
            if ok {
                freed += before;
            } else {
                failed += 1;
            }
            continue;
        }
        for root in &cat.roots {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            if root.is_file() {
                let len = root.metadata().map(|m| m.len()).unwrap_or(0);
                let ok = if to_recycle {
                    (unsafe { recycle_paths(&[root.clone()]) }) && !root.exists()
                } else {
                    fs::remove_file(root).is_ok()
                };
                if ok {
                    freed += len;
                } else {
                    failed += 1;
                }
            } else if to_recycle {
                let (f, e) = clean_dir_recycle(root);
                freed += f;
                failed += e;
            } else {
                let (f, e) = delete_contents(root);
                freed += f;
                failed += e;
            }
        }
    }
    let cancelled = cancel.load(Ordering::Relaxed);
    progress(total, total, if cancelled { "已取消" } else { "完成" });
    CleanResult { freed, failed, recycled: to_recycle, cancelled }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mwcleaner_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("a.txt"), b"hello").unwrap();
        fs::write(d.join("b.bin"), vec![7u8; 2048]).unwrap();
        d
    }

    fn cat(root: PathBuf) -> JunkCategory {
        JunkCategory {
            name: "test",
            description: "",
            roots: vec![root],
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
        }
    }

    fn bin_count() -> i64 {
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            let _ = SHQueryRecycleBinW(PCWSTR::null(), &mut info);
        }
        info.i64NumItems
    }

    #[test]
    fn recycle_roundtrip() {
        let d = test_dir("rectest");
        let before = bin_count();
        let r = clean(&[cat(d.clone())], &|_, _, _| {}, true, &AtomicBool::new(false));
        assert_eq!(r.failed, 0);
        assert!(r.recycled);
        assert!(r.freed >= 2053);
        assert!(fs::read_dir(&d).unwrap().next().is_none());
        assert!(bin_count() >= before + 2);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn direct_delete() {
        let d = test_dir("directtest");
        let r = clean(&[cat(d.clone())], &|_, _, _| {}, false, &AtomicBool::new(false));
        assert_eq!(r.failed, 0);
        assert!(!r.recycled);
        assert!(fs::read_dir(&d).unwrap().next().is_none());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn scan_cancel() {
        let c = AtomicBool::new(true);
        let cats = scan(&|_, _, _| {}, &c);
        assert!(cats.iter().all(|c| c.size == 0));
    }

    #[test]
    fn clean_cancel() {
        let d = test_dir("cleancancel");
        let c = AtomicBool::new(true);
        let r = clean(&[cat(d.clone())], &|_, _, _| {}, false, &c);
        assert!(r.cancelled);
        assert_eq!(r.freed, 0);
        assert!(fs::read_dir(&d).unwrap().next().is_some());
        let _ = fs::remove_dir_all(&d);
    }
}