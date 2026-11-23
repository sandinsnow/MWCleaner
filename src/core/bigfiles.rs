use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use super::{is_reparse, junk};

pub struct DriveInfo {
    pub letter: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
}

pub fn drives() -> Vec<DriveInfo> {
    let mask = unsafe { GetLogicalDrives() };
    let mut result = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = format!("{}:\\", (b'A' + i as u8) as char);
        let wide: Vec<u16> = letter.encode_utf16().chain([0]).collect();
        let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        if kind != 3 {
            continue;
        }
        let mut total = 0u64;
        let mut free = 0u64;
        let mut vol = [0u16; 256];
        let mut label = String::new();
        unsafe {
            let _ = GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                None,
                Some(&mut total),
                Some(&mut free),
            );
            if GetVolumeInformationW(
                PCWSTR(wide.as_ptr()),
                Some(&mut vol),
                None,
                None,
                None,
                None,
            )
            .is_ok()
            {
                let end = vol.iter().position(|&c| c == 0).unwrap_or(vol.len());
                label = String::from_utf16_lossy(&vol[..end]);
            }
        }
        result.push(DriveInfo { letter, label, total, free });
    }
    result
}

#[derive(Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Clone)]
pub struct DirEntry {
    pub path: PathBuf,
    pub size: u64,
}

pub struct ScanResult {
    pub files: Vec<FileEntry>,
    pub dirs: Vec<DirEntry>,
    pub scanned_files: u64,
    pub scanned_bytes: u64,
    pub cancelled: bool,
}

struct Walker<'a> {
    min_file: u64,
    min_dir: u64,
    cancel: &'a AtomicBool,
    progress: &'a dyn Fn(u64, &str),
    files: Vec<FileEntry>,
    dirs: Vec<DirEntry>,
    scanned_files: u64,
    scanned_bytes: u64,
    /// COR-1：显式父子聚合——子目录 Finish 时把子树和累进此表，父目录 Finish 时一次取出。
    child_sums: std::collections::HashMap<PathBuf, u64>,
}

impl Walker<'_> {
    fn walk(&mut self, root: &Path) {
        // Enter(目录, 父目录)；Finish(目录, 自身文件合计, 父目录)
        enum Frame {
            Enter(PathBuf, Option<PathBuf>),
            Finish(PathBuf, u64, Option<PathBuf>),
        }
        let mut stack = vec![Frame::Enter(root.to_path_buf(), None)];
        while let Some(frame) = stack.pop() {
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            match frame {
                Frame::Enter(dir, parent) => {
                    let Ok(entries) = fs::read_dir(&dir) else { continue };
                    let mut total = 0u64;
                    let mut children = Vec::new();
                    for entry in entries.flatten() {
                        let Ok(md) = entry.metadata() else { continue };
                        if is_reparse(&md) {
                            continue;
                        }
                        let path = entry.path();
                        if md.is_dir() {
                            children.push(path);
                        } else {
                            let size = md.len();
                            total += size;
                            self.scanned_files += 1;
                            self.scanned_bytes += size;
                            if size >= self.min_file {
                                self.files.push(FileEntry { path, size });
                            }
                            if self.scanned_files % 5000 == 0 {
                                (self.progress)(self.scanned_files, &dir.to_string_lossy());
                            }
                        }
                    }
                    stack.push(Frame::Finish(dir.clone(), total, parent));
                    for c in children {
                        stack.push(Frame::Enter(c, Some(dir.clone())));
                    }
                }
                Frame::Finish(dir, total, parent) => {
                    // 子树合计 = 目录自身文件 + 已汇总的各直接子目录子树
                    let subtree =
                        total + self.child_sums.remove(&dir).unwrap_or(0);
                    if subtree >= self.min_dir {
                        self.dirs.push(DirEntry { path: dir, size: subtree });
                    }
                    if let Some(p) = parent {
                        *self.child_sums.entry(p).or_insert(0) += subtree;
                    }
                }
            }
        }
    }
}

pub fn scan(
    root: &Path,
    min_file: u64,
    min_dir: u64,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, &str),
) -> ScanResult {
    let mut walker = Walker {
        min_file,
        min_dir,
        cancel,
        progress,
        files: Vec::new(),
        dirs: Vec::new(),
        scanned_files: 0,
        scanned_bytes: 0,
        child_sums: std::collections::HashMap::new(),
    };
    walker.walk(root);
    walker.files.sort_by(|a, b| b.size.cmp(&a.size));
    walker.files.truncate(200);
    walker.dirs.sort_by(|a, b| b.size.cmp(&a.size));
    walker.dirs.truncate(100);
    ScanResult {
        files: walker.files,
        dirs: walker.dirs,
        scanned_files: walker.scanned_files,
        scanned_bytes: walker.scanned_bytes,
        cancelled: cancel.load(Ordering::Relaxed),
    }
}

/// 删除方式：回收站（可还原，默认）或永久删除（不可恢复）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    Recycle,
    Permanent,
}

fn path_eq_ignore_case(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

/// 已知系统关键路径：禁止在「大文件分析」页删除，避免误删导致系统不可启动。
/// 仅保护具体敏感路径与其直接子目录，不过度屏蔽普通盘符扫描结果。
pub fn is_protected(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "pagefile.sys" | "hiberfil.sys" | "swapfile.sys"
            | "$recycle.bin" | "system volume information"
    ) {
        return true;
    }
    let system_roots = [
        std::env::var("windir").unwrap_or_else(|_| "C:\\Windows".into()),
        std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into()),
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into()),
    ];
    system_roots.iter().any(|r| {
        let root = Path::new(r);
        path_eq_ignore_case(path, root)
            || path.parent()
                .map(|p| path_eq_ignore_case(p, root))
                .unwrap_or(false)
    })
}

/// 按指定方式删除单个文件或目录；目录仅在非重解析点时递归删除。
/// `owner_hwnd`：Shell 弹窗（如回收站容量确认）归属的主窗口 HWND，无窗口时传 0。
pub fn delete_path_mode(path: &Path, mode: DeleteMode, owner_hwnd: isize) -> std::io::Result<()> {
    match mode {
        DeleteMode::Recycle => {
            if junk::move_to_recycle_bin(owner_hwnd, &[path.to_path_buf()]) {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "移入回收站失败（文件仍保留在磁盘，可改用永久删除）",
                ))
            }
        }
        DeleteMode::Permanent => {
            let md = fs::symlink_metadata(path)?;
            if md.is_dir() && !is_reparse(&md) {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
        }
    }
}

/// 永久删除的兼容入口；UI 已改用 `delete_path_mode` 显式选择删除方式。
#[allow(dead_code)]
pub fn delete_path(path: &Path) -> std::io::Result<()> {
    delete_path_mode(path, DeleteMode::Permanent, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, n: usize) {
        fs::write(p, vec![7u8; n]).unwrap();
    }

    #[test]
    fn protected_system_paths() {
        let windir = std::env::var("windir").unwrap_or_else(|_| "C:\\Windows".into());
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        // 系统关键根目录及其直接子目录受保护（大小写不敏感）
        assert!(is_protected(Path::new(&windir)));
        assert!(is_protected(&PathBuf::from(&windir).join("System32")));
        assert!(is_protected(Path::new(&pf)));
        assert!(is_protected(&PathBuf::from(&pf).join("Common Files")));
        // 特殊系统目录与页面文件受保护
        assert!(is_protected(Path::new("C:\\$Recycle.Bin")));
        assert!(is_protected(Path::new("C:\\System Volume Information")));
        assert!(is_protected(Path::new("C:\\pagefile.sys")));
        assert!(is_protected(Path::new("D:\\hiberfil.sys")));
        // 普通用户文件/目录不受保护，避免正常扫描结果被禁删
        assert!(!is_protected(Path::new("D:\\Movies\\big.mkv")));
        assert!(!is_protected(Path::new("C:\\Users\\me\\huge.zip")));
    }

    #[test]
    fn scan_totals_and_cancel() {
        let base = std::env::temp_dir().join(format!("mwcleaner_bftest_{}", std::process::id()));
        let sub = base.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::create_dir_all(base.join("empty")).unwrap();
        write(&base.join("a.bin"), 3000);
        write(&sub.join("b.bin"), 2000);
        write(&sub.join("small.bin"), 10);
        let cancel = AtomicBool::new(false);
        let r = scan(&base, 1000, 2000, &cancel, &|_, _| {});
        assert!(!r.cancelled);
        assert_eq!(r.scanned_files, 3);
        assert_eq!(r.scanned_bytes, 5010);
        assert_eq!(r.files.len(), 2);
        assert_eq!(r.files[0].size, 3000);
        assert_eq!(r.dirs.len(), 2);
        assert_eq!(r.dirs.iter().find(|d| d.path == base).unwrap().size, 5010);
        assert_eq!(r.dirs.iter().find(|d| d.path == sub).unwrap().size, 2010);

        let r = scan(&base, 1000, 4000, &cancel, &|_, _| {});
        assert!(r.dirs.iter().all(|d| d.path != sub));

        cancel.store(true, Ordering::Relaxed);
        let r = scan(&base, 1000, 2000, &cancel, &|_, _| {});
        assert!(r.cancelled);
        assert_eq!(r.scanned_files, 0);
        assert!(r.files.is_empty());
        fs::remove_dir_all(&base).unwrap();
    }

    /// COR-1：三层嵌套 + 兄弟目录 + 空目录的显式父子聚合断言。
    #[test]
    fn scan_nested_aggregation() {
        let base = std::env::temp_dir().join(format!("mwcleaner_bfnest_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let l1a = base.join("l1a");
        let l2a = l1a.join("l2a");
        let l3a = l2a.join("l3a");
        let l1b = base.join("l1b");
        let empty = base.join("empty");
        for d in [&l2a, &l3a, &l1b, &empty] {
            fs::create_dir_all(d).unwrap();
        }
        write(&base.join("root.bin"), 1000);
        write(&l1a.join("a.bin"), 2000);
        write(&l2a.join("b.bin"), 3000);
        write(&l3a.join("c.bin"), 4000);
        write(&l1b.join("d.bin"), 5000);
        let cancel = AtomicBool::new(false);
        let r = scan(&base, 1000, 1000, &cancel, &|_, _| {});
        assert_eq!(r.scanned_files, 5);
        assert_eq!(r.scanned_bytes, 15000);
        let size_of = |p: &Path| r.dirs.iter().find(|d| d.path == p).map(|d| d.size);
        assert_eq!(size_of(&l3a), Some(4000));
        assert_eq!(size_of(&l2a), Some(7000));
        assert_eq!(size_of(&l1a), Some(9000));
        assert_eq!(size_of(&l1b), Some(5000));
        assert_eq!(size_of(&base), Some(15000));
        assert_eq!(size_of(&empty), None);
        // 空目录（0 < min_dir）不应进入结果
        assert!(!r.dirs.iter().any(|d| d.path == empty));
        fs::remove_dir_all(&base).unwrap();
    }
}