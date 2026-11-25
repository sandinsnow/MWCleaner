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

/// 垃圾清理失败分类：区分无权限 / 被占用 / 只读 / 其他，供 UI 给出针对性指引。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailKind {
    /// 无权限（ERROR_ACCESS_DENIED，系统保护文件，管理员通常也无权）
    Denied,
    /// 被其他程序占用（ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION）
    Locked,
    /// 只读属性且清除失败
    ReadOnly,
    /// 其他原因（Shell 拒绝、路径失效等）
    Other,
}

/// 失败明细只保留前 20 条（控制内存），总量仍由计数器完整统计。
pub const MAX_FAILURE_DETAILS: usize = 20;

/// 纯函数分类：只读属性优先归类；5=无权限，32/33=被占用，其余=其他。
pub fn classify_delete_error(err: &std::io::Error, readonly_attr: bool) -> FailKind {
    if readonly_attr {
        return FailKind::ReadOnly;
    }
    match err.raw_os_error() {
        Some(5) => FailKind::Denied,
        Some(32) | Some(33) => FailKind::Locked,
        _ => FailKind::Other,
    }
}

/// 一次清理的失败/释放聚合统计：计数不设上限，明细只留前 MAX_FAILURE_DETAILS 条。
#[derive(Default, Clone, Debug)]
pub struct DeleteStats {
    pub freed: u64,
    pub failed: u64,
    pub denied: u64,
    pub locked: u64,
    pub readonly: u64,
    pub other: u64,
    pub details: Vec<(PathBuf, FailKind)>,
}

impl DeleteStats {
    fn record(&mut self, path: &Path, kind: FailKind) {
        self.failed += 1;
        match kind {
            FailKind::Denied => self.denied += 1,
            FailKind::Locked => self.locked += 1,
            FailKind::ReadOnly => self.readonly += 1,
            FailKind::Other => self.other += 1,
        }
        if self.details.len() < MAX_FAILURE_DETAILS {
            self.details.push((path.to_path_buf(), kind));
        }
    }

    /// 无具体路径的失败（如清空回收站整体失败）。
    fn record_count(&mut self, kind: FailKind) {
        self.failed += 1;
        match kind {
            FailKind::Denied => self.denied += 1,
            FailKind::Locked => self.locked += 1,
            FailKind::ReadOnly => self.readonly += 1,
            FailKind::Other => self.other += 1,
        }
    }
}

/// 删除前清除文件的只读属性。Windows 上只读属性会使 `fs::remove_file` 返回
/// PermissionDenied（删除文件只需目录写权限，只读位会阻止），清位是标准做法。
/// 返回 true 表示该文件原本带只读属性（无论清除是否成功）。
/// 取舍：清只读后若删除失败（如并发占用），不恢复只读位——垃圾文件保留可删状态
/// 更利于重试清理，且清理目标本就是删除，恢复逻辑不值得复杂化。
pub fn clear_readonly(path: &Path) -> bool {
    let Ok(md) = fs::symlink_metadata(path) else {
        return false;
    };
    if !md.permissions().readonly() {
        return false;
    }
    let mut perms = md.permissions();
    perms.set_readonly(false);
    let _ = fs::set_permissions(path, perms);
    true
}

pub fn delete_contents(root: &Path, stats: &mut DeleteStats) {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(err) => {
            // 根目录不存在：无内容可清理，不算失败；其他错误（权限/占用）如实记一条
            if err.kind() != std::io::ErrorKind::NotFound {
                stats.record(root, classify_delete_error(&err, false));
            }
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(md) = fs::symlink_metadata(&path) else {
            stats.record(&path, FailKind::Other);
            continue;
        };
        if is_reparse(&md) {
            continue;
        }
        if md.is_dir() {
            delete_contents(&path, stats);
            if let Err(e) = fs::remove_dir(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    stats.record(&path, classify_delete_error(&e, false));
                }
            }
        } else {
            let len = md.len();
            let was_readonly = md.permissions().readonly();
            if was_readonly {
                clear_readonly(&path);
            }
            match fs::remove_file(&path) {
                Ok(()) => stats.freed += len,
                Err(e) => {
                    // TOCTOU：symlink_metadata 与 remove_file 之间文件被第三方删除
                    // （ERROR_FILE_NOT_FOUND）= 目标已达成，豁免不计失败（与根目录/remove_dir 口径一致）
                    if e.raw_os_error() != Some(2) {
                        stats.record(&path, classify_delete_error(&e, was_readonly));
                    }
                }
            }
        }
    }
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
    fn classify_delete_error_mapping() {
        // 5=ACCESS_DENIED → 无权限；32/33=sharing/lock violation → 被占用
        let e5 = std::io::Error::from_raw_os_error(5);
        let e32 = std::io::Error::from_raw_os_error(32);
        let e33 = std::io::Error::from_raw_os_error(33);
        let e2 = std::io::Error::from_raw_os_error(2);
        assert_eq!(classify_delete_error(&e5, false), FailKind::Denied);
        assert_eq!(classify_delete_error(&e32, false), FailKind::Locked);
        assert_eq!(classify_delete_error(&e33, false), FailKind::Locked);
        assert_eq!(classify_delete_error(&e2, false), FailKind::Other);
        // 只读属性优先归类（便于针对性提示「只读」而非笼统「无权限」）
        assert_eq!(classify_delete_error(&e5, true), FailKind::ReadOnly);
        assert_eq!(classify_delete_error(&e32, true), FailKind::ReadOnly);
        // 无 OS 错误码的自造错误归为 Other
        let custom = std::io::Error::new(std::io::ErrorKind::Other, "custom");
        assert_eq!(classify_delete_error(&custom, false), FailKind::Other);
    }

    #[test]
    fn delete_stats_details_capped_at_20() {
        // 计数完整累计，明细只留前 20 条
        let mut stats = DeleteStats::default();
        for i in 0..30 {
            stats.record(&PathBuf::from(format!("p{i}")), FailKind::Locked);
        }
        assert_eq!(stats.failed, 30);
        assert_eq!(stats.locked, 30);
        assert_eq!(stats.details.len(), MAX_FAILURE_DETAILS);
        assert_eq!(stats.details[0].0, PathBuf::from("p0"));
        assert_eq!(stats.details[19].0, PathBuf::from("p19"));
    }

    #[test]
    fn delete_contents_clears_readonly() {
        // 真实构造只读文件：清除属性后直删应成功（修复前 failed=1 PermissionDenied）
        let d = std::env::temp_dir().join(format!("mwcleaner_ro_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let f = d.join("ro.txt");
        fs::write(&f, b"data").unwrap();
        let mut perms = fs::metadata(&f).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&f, perms).unwrap();
        assert!(fs::metadata(&f).unwrap().permissions().readonly());
        let mut stats = DeleteStats::default();
        delete_contents(&d, &mut stats);
        assert_eq!(stats.failed, 0, "只读文件清属性后应可删除");
        assert_eq!(stats.freed, 4);
        assert!(!f.exists());
        assert!(stats.details.is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn delete_contents_missing_root_not_counted() {
        // 根目录不存在：无内容可清理，不算失败（与旧行为一致）
        let missing = std::env::temp_dir().join(format!("mwcleaner_missing_{}", std::process::id()));
        let _ = fs::remove_dir_all(&missing);
        let mut stats = DeleteStats::default();
        delete_contents(&missing, &mut stats);
        assert_eq!(stats.failed, 0);
    }

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