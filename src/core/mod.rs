pub mod bigfiles;
pub mod junk;
pub mod registry;
pub mod services;
pub mod settings;
pub mod startup;
pub mod uninstall;
pub mod update;

use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub fn is_reparse(md: &fs::Metadata) -> bool {
    md.file_type().is_symlink() || md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", v, UNITS[i])
    }
}

pub fn dir_size(root: &Path) -> (u64, u64) {
    let mut size = 0u64;
    let mut count = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(md) = entry.metadata() else { continue };
            if is_reparse(&md) {
                continue;
            }
            if md.is_dir() {
                stack.push(entry.path());
            } else {
                size += md.len();
                count += 1;
            }
        }
    }
    (size, count)
}

pub fn delete_contents(root: &Path) -> (u64, u64) {
    let mut freed = 0u64;
    let mut failed = 0u64;
    let Ok(entries) = fs::read_dir(root) else {
        return (0, 0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(md) = fs::symlink_metadata(&path) else {
            failed += 1;
            continue;
        };
        if is_reparse(&md) {
            continue;
        }
        if md.is_dir() {
            let (f, e) = delete_contents(&path);
            freed += f;
            failed += e;
            if fs::remove_dir(&path).is_err() {
                failed += 1;
            }
        } else {
            let len = md.len();
            if fs::remove_file(&path).is_ok() {
                freed += len;
            } else {
                failed += 1;
            }
        }
    }
    (freed, failed)
}

pub fn app_data_dir() -> std::path::PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = std::path::PathBuf::from(base).join("MWCleaner");
    let _ = fs::create_dir_all(&dir);
    dir
}