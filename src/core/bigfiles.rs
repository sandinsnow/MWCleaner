use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
};

use super::is_reparse;

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
}

impl Walker<'_> {
    fn walk(&mut self, dir: &Path) -> u64 {
        if self.cancel.load(Ordering::Relaxed) {
            return 0;
        }
        let mut total = 0u64;
        let Ok(entries) = fs::read_dir(dir) else { return 0 };
        for entry in entries.flatten() {
            let Ok(md) = entry.metadata() else { continue };
            if is_reparse(&md) {
                continue;
            }
            let path = entry.path();
            if md.is_dir() {
                total += self.walk(&path);
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
        if total >= self.min_dir && !self.dirs.iter().any(|d| d.path == dir) {
            self.dirs.push(DirEntry { path: dir.to_path_buf(), size: total });
        }
        total
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

pub fn delete_path(path: &Path) -> std::io::Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.is_dir() && !is_reparse(&md) {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}