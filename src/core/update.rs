use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_NAMED_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

pub const REPO_URL: &str = "https://github.com/sandinsnow/MWCleaner";
pub const RELEASES_URL: &str = "https://github.com/sandinsnow/MWCleaner/releases";
pub const ISSUES_URL: &str = "https://github.com/sandinsnow/MWCleaner/issues";

pub struct ReleaseInfo {
    pub tag: String,
    pub url: String,
    pub download_url: Option<String>,
    pub published: String,
    pub notes: String,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn parse_ver(s: &str) -> Option<(u32, u32, u32)> {
    let t = s.trim().trim_start_matches('v');
    let mut it = t.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    Some((a, b, c))
}

pub fn is_newer(remote: &str, current: &str) -> bool {
    match (parse_ver(remote), parse_ver(current)) {
        (Some(r), Some(c)) => r > c,
        _ => false,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_err() -> u32 {
    unsafe { GetLastError().0 }
}

// 直连失败后的回退代理：取系统配置值，不看 ProxyEnable 开关（代理软件常关开关但端口仍在）
fn system_proxy() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;
    let k = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            KEY_READ,
        )
        .ok()?;
    let s: String = k.get_value("ProxyServer").ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        return None;
    }
    if s.contains('=') {
        let pick = s
            .split(';')
            .find(|p| p.trim_start().starts_with("https="))
            .or_else(|| s.split(';').find(|p| p.trim_start().starts_with("http=")))?;
        let v = pick.split_once('=')?.1.trim().to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    } else {
        Some(s)
    }
}

fn http_get_latest(proxy: Option<&str>) -> Result<String, String> {
    unsafe {
        let agent = wide(&format!("MWCleaner/{}", current_version()));
        let (access, proxy_w) = match proxy {
            Some(p) => (WINHTTP_ACCESS_TYPE_NAMED_PROXY, wide(p)),
            None => (WINHTTP_ACCESS_TYPE_NO_PROXY, wide("")),
        };
        let session = WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            access,
            PCWSTR(proxy_w.as_ptr()),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err(format!("WinHttpOpen 失败 ({})", last_err()));
        }
        let host = wide("api.github.com");
        let conn = WinHttpConnect(session, PCWSTR(host.as_ptr()), 443, 0);
        if conn.is_null() {
            let _ = WinHttpCloseHandle(session);
            return Err(format!("连接失败 ({})", last_err()));
        }
        let verb = wide("GET");
        let path = wide("/repos/sandinsnow/MWCleaner/releases/latest");
        let req = WinHttpOpenRequest(
            conn,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if req.is_null() {
            let _ = WinHttpCloseHandle(conn);
            let _ = WinHttpCloseHandle(session);
            return Err(format!("创建请求失败 ({})", last_err()));
        }
        let result = read_response(req);
        let _ = WinHttpCloseHandle(req);
        let _ = WinHttpCloseHandle(conn);
        let _ = WinHttpCloseHandle(session);
        result
    }
}

unsafe fn read_response(req: *mut c_void) -> Result<String, String> {
    let _ = WinHttpSetTimeouts(req, 5000, 8000, 8000, 8000);
    let headers = wide(&format!(
        "User-Agent: MWCleaner/{}\r\nAccept: application/vnd.github+json",
        current_version()
    ));
    WinHttpSendRequest(req, Some(&headers[..headers.len() - 1]), None, 0, 0, 0)
        .map_err(|_| format!("发送失败 ({})", last_err()))?;
    WinHttpReceiveResponse(req, std::ptr::null_mut())
        .map_err(|_| format!("接收失败 ({})", last_err()))?;
    let mut status: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    let mut idx = 0u32;
    WinHttpQueryHeaders(
        req,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some(&mut status as *mut u32 as *mut c_void),
        &mut len,
        &mut idx,
    )
    .map_err(|_| format!("查询状态失败 ({})", last_err()))?;
    if status == 404 {
        return Err("仓库尚未发布 Release".into());
    }
    if status != 200 {
        return Err(format!("GitHub 返回 HTTP {status}"));
    }
    let mut body = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let mut read = 0u32;
        WinHttpReadData(req, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut read)
            .map_err(|_| format!("读取失败 ({})", last_err()))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&buf[..read as usize]);
        if body.len() > 2_000_000 {
            return Err("响应过大".into());
        }
    }
    String::from_utf8(body).map_err(|_| "响应编码无效".into())
}

pub fn check_latest() -> Result<ReleaseInfo, String> {
    let body = match http_get_latest(None) {
        Ok(b) => b,
        Err(direct) => match system_proxy() {
            Some(p) => http_get_latest(Some(&p))
                .map_err(|via| format!("{direct}；代理重试：{via}"))?,
            None => return Err(direct),
        },
    };
    parse_release(&body)
}

pub fn parse_release(body: &str) -> Result<ReleaseInfo, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("解析响应失败：{e}"))?;
    let tag = v["tag_name"]
        .as_str()
        .ok_or("响应缺少 tag_name")?
        .to_string();
    let url = v["html_url"].as_str().unwrap_or(RELEASES_URL).to_string();
    let download_url = v["assets"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["browser_download_url"].as_str())
        .map(|s| s.to_string());
    let published = v["published_at"]
        .as_str()
        .unwrap_or("")
        .split('T')
        .next()
        .unwrap_or("")
        .to_string();
    let notes = v["body"].as_str().unwrap_or("").trim().to_string();
    Ok(ReleaseInfo {
        tag,
        url,
        download_url,
        published,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ver_compare() {
        assert!(is_newer("v0.4.0", "0.3.2"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.3.10", "0.3.2"));
        assert!(!is_newer("v0.3.2", "0.3.2"));
        assert!(!is_newer("v0.3.1", "0.3.2"));
        assert!(!is_newer("garbage", "0.3.2"));
        assert_eq!(parse_ver("v0.3"), Some((0, 3, 0)));
        assert_eq!(parse_ver("0.3.2"), Some((0, 3, 2)));
        assert_eq!(parse_ver("x"), None);
    }

    #[test]
    fn parse_release_fields() {
        let body = r#"{"tag_name":"v0.4.0","html_url":"https://github.com/sandinsnow/MWCleaner/releases/tag/v0.4.0","published_at":"2026-11-19T08:00:00Z","body":"notes line","assets":[{"browser_download_url":"https://example.com/MWCleaner.exe"}]}"#;
        let r = parse_release(body).unwrap();
        assert_eq!(r.tag, "v0.4.0");
        assert_eq!(r.published, "2026-11-19");
        assert_eq!(r.notes, "notes line");
        assert_eq!(r.download_url.as_deref(), Some("https://example.com/MWCleaner.exe"));
        assert!(parse_release("{}").is_err());
        assert!(parse_release("not json").is_err());
    }
}