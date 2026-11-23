//! D3：HTML 清理报告——单文件零外部资源（内联 CSS、无 JS/图片/外部字体），
//! 跟随查看环境的 `prefers-color-scheme` 双主题；所有动态文本经 html_escape 防注入
//! （任务名、路径、服务描述均来自系统/第三方）。

use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use super::{
    bigfiles, exports_dir, fmt_size, html_escape, junk, registry, services, settings, startup,
    timestamp_name,
};

/// 五节扫描快照（0.3 决策）：垃圾/大文件/注册表/启动项/服务建议。
/// 不含卸载残留（交互式单应用扫描）与清理历史（无持久化机制）。
pub struct ReportInput<'a> {
    pub junk: &'a [junk::JunkCategory],
    /// None = 大文件节标「未扫描」
    pub bigfiles: Option<&'a bigfiles::ScanResult>,
    pub reg: &'a [registry::RegIssue],
    pub startup: &'a [startup::StartupItem],
    pub services: &'a [services::ServiceInfo],
}

/// 纯字符串拼装（无 IO，可单测）：返回完整 HTML 文档。
pub fn build_report_html(input: &ReportInput) -> String {
    let mut html = String::with_capacity(16 * 1024);
    html.push_str("<!DOCTYPE html>\n");
    html.push_str("<html lang=\"zh-CN\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n",
    );
    html.push_str("<title>MWCleaner 清理报告</title>\n");
    push_css(&mut html);
    html.push_str("</head>\n<body>\n");

    // 头部：标题 + 生成时间 + 版本
    html.push_str("<header><h1>MWCleaner 清理报告</h1><p class=\"muted\">");
    html.push_str(&format!(
        "生成时间 {} · 版本 v{}",
        html_escape(&readable_time(&timestamp_name())),
        html_escape(env!("CARGO_PKG_VERSION"))
    ));
    html.push_str("</p></header>\n");

    push_junk_section(&mut html, input.junk);
    push_bigfiles_section(&mut html, input.bigfiles);
    push_registry_section(&mut html, input.reg);
    push_startup_section(&mut html, input.startup);
    push_services_section(&mut html, input.services);

    html.push_str("<footer>本报告由 MWCleaner 生成，只反映生成时刻的扫描快照；清理操作请在应用内执行。</footer>\n");
    html.push_str("</body>\n</html>\n");
    html
}

/// 即时采集并落盘：在 pump 后台线程内整串调用，stage 回调驱动分阶段进度文案。
/// 最重为大文件全盘扫描（分钟级，与 bigfiles 页同参数）；cancel 预留（本轮 UI 不暴露取消）。
pub fn export_report(
    stage: &dyn Fn(&'static str),
    cancel: &AtomicBool,
) -> io::Result<PathBuf> {
    let noop_junk = |_: usize, _: usize, _: &str| {};
    let noop_big = |_: u64, _: &str| {};

    stage("扫描垃圾类别…");
    let junk = junk::scan(&noop_junk, cancel);

    stage("扫描注册表…");
    let reg = registry::scan(cancel);

    stage("扫描大文件…");
    // 与 bigfiles 页初始选中一致：取第一个固定磁盘；无盘则本节标「未扫描」
    let big = bigfiles::drives()
        .first()
        .map(|d| {
            bigfiles::scan(
                std::path::Path::new(&d.letter),
                settings::load_u32("BigFileMinMB", 100) as u64 * 1024 * 1024,
                1024 * 1024 * 1024,
                cancel,
                &noop_big,
            )
        });
    let bigfiles_ref = big.as_ref();

    stage("读取启动项…");
    let startup = startup::list();

    stage("读取服务建议…");
    let services = services::list().unwrap_or_default();

    let html = build_report_html(&ReportInput {
        junk: &junk,
        bigfiles: bigfiles_ref,
        reg: &reg,
        startup: &startup,
        services: &services,
    });
    let path = exports_dir().join(format!("report-{}.html", timestamp_name()));
    std::fs::write(&path, html)?;
    Ok(path)
}

// ===== 五节拼装 =====

fn push_junk_section(html: &mut String, junk: &[junk::JunkCategory]) {
    html.push_str("<section><h2>垃圾扫描</h2>");
    if junk.is_empty() {
        html.push_str(EMPTY_PLACEHOLDER);
    } else {
        let (total_size, total_count) = junk
            .iter()
            .fold((0u64, 0u64), |(s, c), k| (s + k.size, c + k.file_count));
        html.push_str("<table><thead><tr><th>类别</th><th>说明</th><th class=\"num\">体积</th><th class=\"num\">文件数</th></tr></thead><tbody>");
        for k in junk {
            html.push_str(&format!(
                "<tr><td>{}</td><td class=\"muted\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                html_escape(k.name),
                html_escape(k.description),
                html_escape(&fmt_size(k.size)),
                k.file_count
            ));
        }
        html.push_str(&format!(
            "<tr class=\"total\"><td>合计</td><td></td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
            html_escape(&fmt_size(total_size)),
            total_count
        ));
        html.push_str("</tbody></table>");
    }
    html.push_str("</section>\n");
}

fn push_bigfiles_section(html: &mut String, big: Option<&bigfiles::ScanResult>) {
    html.push_str("<section><h2>大文件分析</h2>");
    let Some(big) = big else {
        html.push_str("<p class=\"muted\">未扫描大文件</p></section>\n");
        return;
    };
    html.push_str(&format!(
        "<p class=\"muted\">共扫描 {} 个文件（{}），按大小降序列出文件 Top30 与文件夹 Top15。</p>",
        big.scanned_files,
        html_escape(&fmt_size(big.scanned_bytes))
    ));
    html.push_str("<h3>文件 Top30</h3>");
    if big.files.is_empty() {
        html.push_str(EMPTY_PLACEHOLDER);
    } else {
        html.push_str("<table><thead><tr><th class=\"num\">#</th><th class=\"num\">大小</th><th>路径</th></tr></thead><tbody>");
        for (i, f) in big.files.iter().take(30).enumerate() {
            html.push_str(&format!(
                "<tr><td class=\"num\">{}</td><td class=\"num\">{}（{} 字节）</td><td class=\"path\">{}</td></tr>",
                i + 1,
                html_escape(&fmt_size(f.size)),
                f.size,
                html_escape(&f.path.display().to_string())
            ));
        }
        html.push_str("</tbody></table>");
    }
    html.push_str("<h3>文件夹 Top15</h3>");
    if big.dirs.is_empty() {
        html.push_str(EMPTY_PLACEHOLDER);
    } else {
        html.push_str("<table><thead><tr><th class=\"num\">#</th><th class=\"num\">大小</th><th>路径</th></tr></thead><tbody>");
        for (i, d) in big.dirs.iter().take(15).enumerate() {
            html.push_str(&format!(
                "<tr><td class=\"num\">{}</td><td class=\"num\">{}（{} 字节）</td><td class=\"path\">{}</td></tr>",
                i + 1,
                html_escape(&fmt_size(d.size)),
                d.size,
                html_escape(&d.path.display().to_string())
            ));
        }
        html.push_str("</tbody></table>");
    }
    html.push_str("</section>\n");
}

fn push_registry_section(html: &mut String, reg: &[registry::RegIssue]) {
    html.push_str("<section><h2>注册表发现</h2>");
    if reg.is_empty() {
        html.push_str("<p class=\"muted\">未发现注册表问题</p></section>\n");
        return;
    }
    html.push_str("<table><thead><tr><th>键</th><th>值名</th><th>类型</th><th>说明</th></tr></thead><tbody>");
    for issue in reg {
        html.push_str(&format!(
            "<tr><td class=\"path\">{}</td><td class=\"path\">{}</td><td>{}</td><td class=\"muted\">{}</td></tr>",
            html_escape(&issue.key),
            html_escape(issue.value_name.as_deref().unwrap_or("-")),
            html_escape(issue.kind),
            html_escape(&issue.detail)
        ));
    }
    html.push_str("</tbody></table></section>\n");
}

fn push_startup_section(html: &mut String, items: &[startup::StartupItem]) {
    html.push_str("<section><h2>启动项</h2>");
    if items.is_empty() {
        html.push_str("<p class=\"muted\">未发现自启动项</p></section>\n");
        return;
    }
    html.push_str("<table><thead><tr><th>名称</th><th>来源</th><th>状态</th><th>建议</th></tr></thead><tbody>");
    for item in items {
        let adv = startup::startup_advice(&item.name, &item.command);
        let advice_text = if adv.reason.is_empty() {
            adv.category.label().to_string()
        } else {
            format!("{}（{}）", adv.category.label(), adv.reason)
        };
        html.push_str(&format!(
            "<tr><td>{}</td><td class=\"path\">{}</td><td>{}</td><td class=\"muted\">{}</td></tr>",
            html_escape(&item.name),
            html_escape(&item.source),
            if item.enabled { "已启用" } else { "已禁用" },
            html_escape(&advice_text)
        ));
    }
    html.push_str("</tbody></table></section>\n");
}

/// 服务建议：只列 Recommend + Caution 两张表，Keep 汇总一句（0.3 决策）。
fn push_services_section(html: &mut String, list: &[services::ServiceInfo]) {
    html.push_str("<section><h2>服务建议</h2>");
    if list.is_empty() {
        html.push_str("<p class=\"muted\">未读取到服务列表</p></section>\n");
        return;
    }
    let mut recommend = Vec::new();
    let mut caution = Vec::new();
    let mut keep = 0usize;
    for svc in list {
        match services::advice(&svc.name).category {
            services::AdviceCategory::Recommend => recommend.push(svc),
            services::AdviceCategory::Caution => caution.push(svc),
            services::AdviceCategory::Keep => keep += 1,
        }
    }
    html.push_str("<h3>建议优化</h3>");
    if recommend.is_empty() {
        html.push_str(EMPTY_PLACEHOLDER);
    } else {
        push_service_table(html, &recommend);
    }
    html.push_str("<h3>不建议优化（已知高危，禁用需格外谨慎）</h3>");
    if caution.is_empty() {
        html.push_str(EMPTY_PLACEHOLDER);
    } else {
        push_service_table(html, &caution);
    }
    html.push_str(&format!(
        "<p class=\"muted\">另有 {keep} 项服务保持现状（系统/安全相关，未逐一列出）。</p>"
    ));
    html.push_str("</section>\n");
}

fn push_service_table(html: &mut String, list: &[&services::ServiceInfo]) {
    html.push_str("<table><thead><tr><th>服务名</th><th>显示名</th><th>启动类型</th><th>建议</th></tr></thead><tbody>");
    for svc in list {
        let adv = services::advice(&svc.name);
        html.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"muted\">{}</td></tr>",
            html_escape(&svc.name),
            html_escape(&svc.display),
            html_escape(services::start_type_text(svc.start_type)),
            html_escape(adv.reason)
        ));
    }
    html.push_str("</tbody></table>");
}

// ===== 骨架助手 =====

const EMPTY_PLACEHOLDER: &str = "<p class=\"muted\">本次未扫描/未发现</p>";

/// `yyyymmdd-hhmmss` → `yyyy-mm-dd hh:mm:ss`（仅用于报告头部的可读时间）。
fn readable_time(ts: &str) -> String {
    let b = ts.as_bytes();
    if ts.len() == 15 && b[8] == b'-' && b.iter().all(|c| c.is_ascii_digit() || *c == b'-') {
        format!(
            "{}-{}-{} {}:{}:{}",
            &ts[0..4],
            &ts[4..6],
            &ts[6..8],
            &ts[9..11],
            &ts[11..13],
            &ts[13..15]
        )
    } else {
        ts.to_string()
    }
}

/// 内联双主题 CSS：CSS 变量 + prefers-color-scheme 切换，系统字体栈，零外部资源。
fn push_css(html: &mut String) {
    html.push_str("<style>\n");
    html.push_str(
        ":root{--bg:#f6f7f9;--panel:#ffffff;--text:#1f2328;--muted:#6e7781;--border:#d0d7de;--accent:#0969da;}\n",
    );
    html.push_str(
        "@media (prefers-color-scheme: dark){:root{--bg:#0d1117;--panel:#161b22;--text:#e6edf3;--muted:#8b949e;--border:#30363d;--accent:#4493f8;}}\n",
    );
    html.push_str(
        "*{box-sizing:border-box;}body{margin:0 auto;max-width:960px;padding:24px 16px;background:var(--bg);color:var(--text);font-family:-apple-system,\"Segoe UI\",\"Microsoft YaHei\",system-ui,sans-serif;font-size:14px;line-height:1.5;}\n",
    );
    html.push_str("h1{font-size:22px;margin:0 0 4px;}h2{font-size:17px;margin:28px 0 8px;border-bottom:1px solid var(--border);padding-bottom:6px;}h3{font-size:14px;margin:18px 0 6px;}\n");
    html.push_str("header{padding:16px;background:var(--panel);border:1px solid var(--border);border-radius:8px;}\n");
    html.push_str(".muted{color:var(--muted);font-size:12px;}\n");
    html.push_str("section{margin-top:12px;background:var(--panel);border:1px solid var(--border);border-radius:8px;padding:12px 16px;}\n");
    html.push_str("table{width:100%;border-collapse:collapse;margin:8px 0;font-size:13px;}\n");
    html.push_str("th{background:var(--bg);text-align:left;padding:6px 8px;border-bottom:2px solid var(--border);}\n");
    html.push_str("td{padding:6px 8px;border-bottom:1px solid var(--border);vertical-align:top;word-break:break-all;}\n");
    html.push_str("th.num,td.num{text-align:right;white-space:nowrap;}td.path{color:var(--muted);font-family:Consolas,monospace;font-size:12px;}\n");
    html.push_str("tr.total td{font-weight:600;background:var(--bg);}\n");
    html.push_str("footer{margin:24px 0 8px;color:var(--muted);font-size:12px;text-align:center;}\n");
    html.push_str("</style>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_has_all_sections_and_placeholders() {
        let input = ReportInput {
            junk: &[],
            bigfiles: None,
            reg: &[],
            startup: &[],
            services: &[],
        };
        let html = build_report_html(&input);
        // 五节齐全
        for title in ["垃圾扫描", "大文件分析", "注册表发现", "启动项", "服务建议"] {
            assert!(html.contains(&format!("<h2>{title}</h2>")), "缺节：{title}");
        }
        // 空节占位：垃圾节通用占位 + 其余各节专属占位
        assert!(html.contains("本次未扫描/未发现"));
        for ph in ["未扫描大文件", "未发现注册表问题", "未发现自启动项", "未读取到服务列表"] {
            assert!(html.contains(ph), "缺占位：{ph}");
        }
        // 单文件零外部资源：无脚本、无外链、无图片
        assert!(!html.contains("<script"), "不允许 JS");
        assert!(!html.contains("http://") && !html.contains("https://"), "不允许外部资源");
        assert!(html.contains("prefers-color-scheme"));
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("lang=\"zh-CN\""));
    }

    #[test]
    fn dynamic_text_is_escaped_injection_samples() {
        use crate::core::bigfiles::{DirEntry, FileEntry, ScanResult};
        use crate::core::registry::RegIssue;
        use crate::core::startup::StartupItem;
        use crate::core::{junk, services};

        // 注入样本：任务名/路径/服务描述中的 HTML 不得以裸标记输出
        let junk = vec![junk::JunkCategory {
            name: "<script>alert(1)</script>",
            description: "desc & \"quoted\"",
            roots: vec![],
            size: 1024,
            file_count: 2,
            selected: false,
            recycle_bin: false,
            upgrade_remnant: false,
        }];
        let big = ScanResult {
            files: vec![FileEntry { path: "<img src=x onerror=y>.exe".into(), size: 4096 }],
            dirs: vec![DirEntry { path: "\"</table>\"".into(), size: 8192 }],
            scanned_files: 1,
            scanned_bytes: 4096,
            cancelled: false,
        };
        let reg = vec![RegIssue {
            key: "HKCU\\<b>key</b>".into(),
            value_name: Some("<i>v</i>".into()),
            kind: "值类型",
            detail: "&amp; raw".into(),
            selected: false,
        }];
        let startup_items = vec![StartupItem {
            name: "<script>evil()</script>".into(),
            command: "cmd /c <iframe>".into(),
            description: String::new(),
            source: "HKCU\\Run".into(),
            is_folder: false,
            enabled: true,
            is_task: false,
        }];
        let svc = vec![services::ServiceInfo {
            name: "<script>x</script>".into(),
            display: "<b>展示名</b>".into(),
            description: String::new(),
            binary: String::new(),
            state: 4,
            start_type: 2,
        }];
        let input = ReportInput {
            junk: &junk,
            bigfiles: Some(&big),
            reg: &reg,
            startup: &startup_items,
            services: &svc,
        };
        let html = build_report_html(&input);
        // 裸标记不得出现（排除骨架自身的合法标签）
        for raw in ["<script>alert", "<img src=x", "<iframe>", "<b>展示名", "</table>\""] {
            assert!(!html.contains(raw), "注入未转义：{raw}");
        }
        // 转义形态存在（"&amp; raw" 的 & 先转义 → &amp;amp; raw）
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(html.contains("&amp;amp; raw"));
        // Top30/Top15 截断与排序数据出现
        assert!(html.contains("4096 字节"));
    }

    #[test]
    fn readable_time_formats_timestamp_name() {
        assert_eq!(readable_time("20261123-143005"), "2026-11-23 14:30:05");
        // 非法输入原样回落
        assert_eq!(readable_time("oops"), "oops");
        assert_eq!(readable_time(""), "");
    }
}
