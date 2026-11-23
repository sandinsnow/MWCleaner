use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{E_ABORT, HWND};
use windows::Win32::UI::Shell::{
    SHFileOperationW, SHFILEOPSTRUCTW, SHQueryRecycleBinW, SHEmptyRecycleBinW, FO_DELETE,
    FOF_ALLOWUNDO, FOF_NOERRORUI, FOF_SILENT, SHQUERYRBINFO,
    SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
};

use super::{delete_contents, dir_size, dir_size_cancelled, is_reparse};

#[derive(Clone)]
pub struct JunkCategory {
    pub name: &'static str,
    pub description: &'static str,
    pub roots: Vec<PathBuf>,
    pub size: u64,
    pub file_count: u64,
    pub selected: bool,
    pub recycle_bin: bool,
    /// C6：Windows 升级残留——默认不勾选、清理强制直删、UI 强警示
    pub upgrade_remnant: bool,
}

pub struct CleanResult {
    /// 已真正释放的体积（直删 + 清空回收站）
    pub freed: u64,
    /// 移入回收站、清空回收站后才会释放的体积（不与 freed 重复计入）
    pub recycled_bytes: u64,
    pub failed: u64,
    pub cancelled: bool,
}

/// COR-3：回收站操作结果——用 `fAnyOperationsAborted` 区分用户取消与权限/占用失败。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecycleOutcome {
    /// 全部完成
    Done,
    /// 用户在 Shell 弹窗中取消（如超回收站容量确认框）
    Cancelled,
    /// Shell 拒绝（权限不足、路径失效等），文件仍保留
    Failed,
}

/// 纯函数映射，便于单测覆盖。
fn outcome_from(code: i32, aborted: bool) -> RecycleOutcome {
    if code == 0 {
        RecycleOutcome::Done
    } else if aborted {
        RecycleOutcome::Cancelled
    } else {
        RecycleOutcome::Failed
    }
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
            upgrade_remnant: false,
        },
        JunkCategory {
            name: "系统临时文件",
            description: "Windows\\Temp 目录中的临时文件",
            roots: vec![w.join("Temp")].into_iter().filter(|p| p.is_dir()).collect(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
            upgrade_remnant: false,
        },
        JunkCategory {
            name: "回收站",
            description: "所有驱动器回收站中的文件（清空不可恢复，默认不勾选）",
            roots: Vec::new(),
            size: 0,
            file_count: 0,
            selected: false,
            recycle_bin: true,
            upgrade_remnant: false,
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
            upgrade_remnant: false,
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
            upgrade_remnant: false,
        },
        JunkCategory {
            name: "Firefox 缓存",
            description: "Firefox 浏览器磁盘缓存",
            roots: firefox_cache_roots(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
            upgrade_remnant: false,
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
            upgrade_remnant: false,
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
            upgrade_remnant: false,
        },
        JunkCategory {
            name: "缩略图缓存",
            description: "资源管理器缩略图/图标缓存（会自动重建）",
            roots: thumbnail_files(),
            size: 0,
            file_count: 0,
            selected: true,
            recycle_bin: false,
            upgrade_remnant: false,
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
            upgrade_remnant: false,
        },
        JunkCategory {
            name: "预读取文件",
            description: "Prefetch 记录（清理后短期内程序启动略慢，默认不选）",
            roots: vec![w.join("Prefetch")].into_iter().filter(|p| p.is_dir()).collect(),
            size: 0,
            file_count: 0,
            selected: false,
            recycle_bin: false,
            upgrade_remnant: false,
        },
        JunkCategory {
            name: "Windows 升级残留",
            description: "Windows.old 与系统升级临时目录（删除后无法回退旧版 Windows，默认不勾选）",
            roots: {
                // C:\ESD 可能含用户自留安装介质，明确不纳入
                let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
                let d = PathBuf::from(&drive);
                vec![
                    d.join("Windows.old"),
                    d.join("$WINDOWS.~BT"),
                    d.join("$Windows.~WS"),
                ]
                .into_iter()
                .filter(|p| p.is_dir())
                .collect()
            },
            size: 0,
            file_count: 0,
            selected: false,
            recycle_bin: false,
            upgrade_remnant: true,
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
            } else if cat.upgrade_remnant {
                // C6：Windows.old 等几十 GB 数万文件，走可取消遍历（中断后保留已扫部分）
                let (size, count, _) = dir_size_cancelled(root, cancel);
                cat.size += size;
                cat.file_count += count;
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

/// SEC-3：带 HWND 归属的回收站删除核心。Shell 弹窗（如超容量确认）模态归属主窗口。
pub(crate) unsafe fn recycle_paths_owner(hwnd: isize, paths: &[PathBuf]) -> RecycleOutcome {
    if paths.is_empty() {
        return RecycleOutcome::Done;
    }
    let from = wide_list(paths);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND(hwnd as *mut _),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        // 不带 FOF_NOCONFIRMATION：超回收站容量时由 Shell 弹窗征求永久删除同意，不静默直删
        fFlags: (FOF_ALLOWUNDO | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        ..Default::default()
    };
    let code = SHFileOperationW(&mut op);
    outcome_from(code, op.fAnyOperationsAborted.as_bool())
}

/// 将一组路径移入回收站（FOF_ALLOWUNDO，可还原）。
/// 返回 false 表示 Shell 拒绝或用户取消，调用方应保留原文件并给出明确提示。
pub fn move_to_recycle_bin(owner_hwnd: isize, paths: &[PathBuf]) -> bool {
    unsafe { recycle_paths_owner(owner_hwnd, paths) == RecycleOutcome::Done }
}

fn entry_size(p: &Path) -> u64 {
    if p.is_dir() {
        dir_size(p).0
    } else {
        p.metadata().map(|m| m.len()).unwrap_or(0)
    }
}

fn clean_dir_recycle(root: &Path, owner_hwnd: isize) -> (u64, u64, bool) {
    let Ok(entries) = fs::read_dir(root) else {
        return (0, 1, false);
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
        return (0, 0, false);
    }
    let paths: Vec<PathBuf> = items.iter().map(|(p, _)| p.clone()).collect();
    let outcome = unsafe { recycle_paths_owner(owner_hwnd, &paths) };
    let cancelled = outcome == RecycleOutcome::Cancelled;
    let mut freed = 0u64;
    let mut failed = 0u64;
    for (p, size) in &items {
        if p.exists() {
            // 用户取消不算失败：文件按预期保留在磁盘
            if !cancelled {
                failed += 1;
            }
        } else {
            freed += size;
        }
    }
    (freed, failed, cancelled)
}

pub fn clean(
    cats: &[JunkCategory],
    progress: &dyn Fn(usize, usize, &str),
    to_recycle: bool,
    cancel: &AtomicBool,
    owner_hwnd: isize,
) -> CleanResult {
    let selected: Vec<_> = cats.iter().filter(|c| c.selected).collect();
    let total = selected.len();
    let mut freed = 0u64;
    let mut failed = 0u64;
    let mut recycled_bytes = 0u64;
    let mut shell_cancelled = false;
    for (i, cat) in selected.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(i, total, cat.name);
        if cat.recycle_bin {
            let before = recycle_bin_size().0;
            let result = unsafe {
                SHEmptyRecycleBinW(
                    HWND(owner_hwnd as *mut _),
                    PCWSTR::null(),
                    SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
                )
            };
            if result.is_ok() {
                freed += before;
            } else if result.unwrap_err().code() == E_ABORT {
                shell_cancelled = true;
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
                if to_recycle {
                    // COR-3：单文件路径与目录分支同样三态处理，取消不算失败
                    let outcome = unsafe { recycle_paths_owner(owner_hwnd, &[root.clone()]) };
                    if outcome == RecycleOutcome::Cancelled {
                        shell_cancelled = true;
                    } else if outcome == RecycleOutcome::Done && !root.exists() {
                        // 移入回收站：空间清空回收站后才释放
                        recycled_bytes += len;
                    } else {
                        failed += 1;
                    }
                } else if fs::remove_file(root).is_ok() {
                    freed += len;
                } else {
                    failed += 1;
                }
            } else if cat.upgrade_remnant {
                // C6：升级残留强制直删（设置页回收站开关对此类别无效），清空后移除根目录。
                // 根壳移除后下次扫描该类别自动消失；失败项照常计入 failed，不静默。
                let (f, e) = delete_contents(root);
                freed += f;
                failed += e;
                let _ = fs::remove_dir(root);
            } else if to_recycle {
                let (f, e, cancelled) = clean_dir_recycle(root, owner_hwnd);
                // 移入回收站：体积计入 recycled_bytes，与真正释放的 freed 区分口径
                recycled_bytes += f;
                failed += e;
                shell_cancelled |= cancelled;
            } else {
                let (f, e) = delete_contents(root);
                freed += f;
                failed += e;
            }
        }
    }
    let cancelled = cancel.load(Ordering::Relaxed) || shell_cancelled;
    progress(total, total, if cancelled { "已取消" } else { "完成" });
    CleanResult { freed, recycled_bytes, failed, cancelled }
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
            upgrade_remnant: false,
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
        let r = clean(&[cat(d.clone())], &|_, _, _| {}, true, &AtomicBool::new(false), 0);
        assert_eq!(r.failed, 0);
        // 回收站口径：体积计入 recycled_bytes（清空回收站后才释放），freed 保持 0
        assert_eq!(r.freed, 0);
        assert!(r.recycled_bytes >= 2053);
        assert!(fs::read_dir(&d).unwrap().next().is_none());
        assert!(bin_count() >= before + 2);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn direct_delete() {
        let d = test_dir("directtest");
        let r = clean(&[cat(d.clone())], &|_, _, _| {}, false, &AtomicBool::new(false), 0);
        assert_eq!(r.failed, 0);
        assert_eq!(r.recycled_bytes, 0);
        assert!(r.freed >= 2053);
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
        let r = clean(&[cat(d.clone())], &|_, _, _| {}, false, &c, 0);
        assert!(r.cancelled);
        assert_eq!(r.freed, 0);
        assert!(fs::read_dir(&d).unwrap().next().is_some());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn recycle_outcome_mapping() {
        // COR-3：SHFileOperationW 返回值 + fAnyOperationsAborted → 三态映射
        assert_eq!(outcome_from(0, false), RecycleOutcome::Done);
        assert_eq!(outcome_from(0, true), RecycleOutcome::Done);
        assert_eq!(outcome_from(0x454, true), RecycleOutcome::Cancelled);
        assert_eq!(outcome_from(0x454, false), RecycleOutcome::Failed);
        assert_eq!(outcome_from(-1, true), RecycleOutcome::Cancelled);
        assert_eq!(outcome_from(-1, false), RecycleOutcome::Failed);
    }

    #[test]
    fn dir_size_cancelled_basics() {
        // 正常完成：cancelled = false，统计与 dir_size 一致
        let d = test_dir("dsc_ok");
        let (s, c, cancelled) = dir_size_cancelled(&d, &AtomicBool::new(false));
        assert!(!cancelled);
        let (s2, c2) = dir_size(&d);
        assert_eq!((s, c), (s2, c2));
        // 预置取消：立即中断，cancelled = true
        let (s3, c3, cancelled3) = dir_size_cancelled(&d, &AtomicBool::new(true));
        assert!(cancelled3);
        assert_eq!((s3, c3), (0, 0));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn upgrade_remnant_force_direct_delete() {
        // C6：升级残留类别无视 to_recycle 开关，强制直删并连根移除
        let d = test_dir("upgdirect");
        let mut c = cat(d.clone());
        c.upgrade_remnant = true;
        let r = clean(&[c], &|_, _, _| {}, true, &AtomicBool::new(false), 0);
        assert_eq!(r.failed, 0);
        assert_eq!(r.recycled_bytes, 0, "升级残留强制直删，不经过回收站");
        assert!(r.freed >= 2053);
        // 根目录已连根移除（普通直删分支只清空内容、保留根壳）
        assert!(!d.exists());
    }

    #[test]
    fn upgrade_remnant_category_probing() {
        // 三目录均不存在于临时盘 → 探测只认真实存在的目录；类别默认不勾选
        let cats = build_categories();
        if let Some(c) = cats.iter().find(|c| c.upgrade_remnant) {
            assert_eq!(c.name, "Windows 升级残留");
            assert!(!c.selected, "升级残留必须默认不勾选");
            assert!(!c.recycle_bin);
            for root in &c.roots {
                assert!(root.is_dir(), "探测清单只应包含真实存在的目录");
            }
        }
    }
}