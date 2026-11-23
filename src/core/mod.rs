pub mod bigfiles;
pub mod junk;
pub mod registry;
pub mod report;
pub mod services;
pub mod settings;
pub mod startup;
pub mod uninstall;
pub mod update;

use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

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

/// dir_size 的可取消同构版（C6：Windows.old 等几十 GB 目录的遍历可被中断）。
/// 返回 (size, count, cancelled)；cancelled 为 true 时 size/count 为已扫部分。
pub fn dir_size_cancelled(root: &Path, cancel: &AtomicBool) -> (u64, u64, bool) {
    use std::sync::atomic::Ordering;
    let mut size = 0u64;
    let mut count = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return (size, count, true);
        }
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return (size, count, true);
            }
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
    (size, count, false)
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

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    let dir = PathBuf::from(base).join("MWCleaner");
    let _ = fs::create_dir_all(&dir);
    dir
}

// ===== B4/S2：CSV 导出公共助手 =====

/// CSV 字段转义：含逗号/引号/换行时加引号，内部引号翻倍。
pub fn csv_escape(field: &str) -> String {
    // CSV 公式注入防护：以 = + - @ \t \r 开头的单元格加 ' 前缀，
    // 防 Excel/WPS 打开导出文件时将单元格按公式执行（OWASP CSV Injection 惯例）
    let risky = matches!(
        field.bytes().next(),
        Some(b'=') | Some(b'+') | Some(b'-') | Some(b'@') | Some(b'\t') | Some(b'\r')
    );
    let s = if risky { format!("'{field}") } else { field.to_string() };
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// HTML 文本转义：& 必须最先替换（避免二次转义）；' 转实体防属性上下文逃逸。
/// 与 csv_escape 对称，供 report.rs 全量动态文本防注入。
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&#39;")
}

/// 导出目录：`%APPDATA%\MWCleaner\exports`（不存在时创建）。
pub fn exports_dir() -> PathBuf {
    let dir = app_data_dir().join("exports");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// 写 CSV：UTF-8 带 BOM（Excel 打开中文不乱码），行以 CRLF 结尾。
pub fn write_csv_lines(path: &Path, rows: &[String]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = fs::File::create(path)?;
    f.write_all(&[0xEF, 0xBB, 0xBF])?;
    for row in rows {
        f.write_all(row.as_bytes())?;
        f.write_all(b"\r\n")?;
    }
    Ok(())
}

/// 当前时间戳（本地简化：按 UTC 折算），格式 `yyyymmdd-hhmmss`，用于导出文件名。
pub fn timestamp_name() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // civil_from_days：从 Unix 天数折算年月日（Howard Hinnant 算法）
    let days = (secs / 86400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let sod = secs % 86400;
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        y,
        m,
        d,
        sod / 3600,
        sod % 3600 / 60,
        sod % 60
    )
}

/// Windows 保留设备名（大小写不敏感；首个 '.' 前的 base 比对，覆盖 CON.txt 等带扩展名形式）。
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 文件名净化：替换 Windows 非法字符（含控制符）为 `_`；
/// 保留设备名加 `_` 前缀；结尾的 `.`/空格补 `_`（Windows 会静默吞掉，存在路径混淆风险）。
pub fn sanitize_filename(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();
    let base = s.split('.').next().unwrap_or(&s);
    if RESERVED_NAMES.iter().any(|r| base.eq_ignore_ascii_case(r)) {
        s = format!("_{s}");
    }
    if s.ends_with('.') || s.ends_with(' ') {
        s.push('_');
    }
    s
}

/// 展开 `%ENV%` 形式的路径片段；未知变量原样保留，避免误判为路径缺失。
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

/// 从命令行推断可执行文件绝对路径（支持引号包裹与环境变量）；找不到返回 None。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_escape_formula_injection() {
        // = + - @ \t \r 开头的字段加 ' 前缀（OWASP CSV 注入惯例）
        assert_eq!(csv_escape("=cmd"), "'=cmd");
        assert_eq!(csv_escape("+1"), "'+1");
        assert_eq!(csv_escape("@x"), "'@x");
        assert_eq!(csv_escape("-5"), "'-5");
        assert_eq!(csv_escape("\tz"), "'\tz");
        // \r 触发前缀后又含换行字符 → 前缀 + 引号包裹两层叠加
        assert_eq!(csv_escape("\r\n"), "\"'\r\n\"");
        // 正常内容不变；引号包裹逻辑不受影响
        assert_eq!(csv_escape("normal"), "normal");
        assert_eq!(csv_escape("C:\\a,b.txt"), "\"C:\\a,b.txt\"");
        // 风险前缀 + 逗号：前缀在引号包裹之内
        assert_eq!(csv_escape("=1,2"), "\"'=1,2\"");
    }

    #[test]
    fn html_escape_basics_and_injection() {
        // 全量五字符转义
        assert_eq!(html_escape("<script>&\"'"), "&lt;script&gt;&amp;&quot;&#39;");
        // & 最先替换：a&b<c 不会二次转义
        assert_eq!(html_escape("a&b<c"), "a&amp;b&lt;c");
        // 已转义文本不再嵌套（&amp; 的 & 被转义）
        assert_eq!(html_escape("&amp;"), "&amp;amp;");
        // 纯中文/空串不变
        assert_eq!(html_escape("启动项·服务"), "启动项·服务");
        assert_eq!(html_escape(""), "");
        // 属性上下文逃逸防护：引号全部转实体
        assert_eq!(html_escape("\" onmouseover=\"x"), "&quot; onmouseover=&quot;x");
    }

    #[test]
    fn sanitize_filename_reserved_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("con.txt"), "_con.txt");
        assert_eq!(sanitize_filename("LPT9.log"), "_LPT9.log");
        assert_eq!(sanitize_filename("Com1"), "_Com1");
        assert_eq!(sanitize_filename("PRN"), "_PRN");
        // 非保留名 / base 不匹配的，不动
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("report_final"), "report_final");
        assert_eq!(sanitize_filename("con_abc.txt"), "con_abc.txt");
    }

    #[test]
    fn sanitize_filename_trailing_dot_space() {
        // 结尾点/空格会被 Windows 静默吞掉 → 补 _
        assert_eq!(sanitize_filename("报告."), "报告._");
        assert_eq!(sanitize_filename("nul "), "nul _");
        assert_eq!(sanitize_filename("file . "), "file . _");
        // 中间空格不受影响
        assert_eq!(sanitize_filename("my file.txt"), "my file.txt");
    }
}