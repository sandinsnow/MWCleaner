use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_MORE_DATA;
use windows::Win32::System::Services::*;
use windows::Win32::UI::Shell::SHLoadIndirectString;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub display: String,
    pub description: String,
    pub binary: String,
    pub state: u32,
    pub start_type: u32,
}

fn resolve_indirect(s: &str) -> String {
    if !s.starts_with('@') {
        return s.to_string();
    }
    let mut buf = vec![0u16; 1024];
    unsafe {
        if SHLoadIndirectString(PCWSTR(to_wide(s).as_ptr()), &mut buf, None).is_ok() {
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            if len > 0 {
                return String::from_utf16_lossy(&buf[..len]);
            }
        }
    }
    s.to_string()
}

fn service_description(name: &str) -> String {
    let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(
        format!("SYSTEM\\CurrentControlSet\\Services\\{name}"),
        KEY_READ,
    ) else {
        return String::new();
    };
    match key.get_value::<String, _>("Description") {
        Ok(v) => resolve_indirect(&v),
        Err(_) => String::new(),
    }
}

pub fn state_text(state: u32) -> &'static str {
    match state {
        1 => "已停止",
        2 => "正在启动",
        3 => "正在停止",
        4 => "运行中",
        5 => "正在继续",
        6 => "正在暂停",
        7 => "已暂停",
        _ => "未知",
    }
}

pub fn start_type_text(t: u32) -> &'static str {
    match t {
        0 => "引导",
        1 => "系统",
        2 => "自动",
        3 => "手动",
        4 => "已禁用",
        _ => "未知",
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum AdviceCategory {
    Recommend,
    Keep,
    Caution,
}

impl AdviceCategory {
    pub fn label(self) -> &'static str {
        match self {
            AdviceCategory::Recommend => "建议优化",
            AdviceCategory::Keep => "保持现状",
            AdviceCategory::Caution => "不建议优化",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Advice {
    pub category: AdviceCategory,
    pub reason: &'static str,
    /// C2：批量应用建议的目标启动类型；0 = 该规则不参与批量（按需人工决定）
    pub suggested: u32,
}

pub fn advice(name: &str) -> Advice {
    // C2：suggested 语义——4=可禁用类参与批量禁用；3=可改手动类参与批量改手动；
    // 0=不参与批量（WSearch 作者本意按需选择；Keep/Caution 全部为安全红线）
    let disable = |reason: &'static str| Advice { category: AdviceCategory::Recommend, reason, suggested: 4 };
    let manual = |reason: &'static str| Advice { category: AdviceCategory::Recommend, reason, suggested: 3 };
    let caution = |reason: &'static str| Advice { category: AdviceCategory::Caution, reason, suggested: 0 };
    match name {
        "DiagTrack" => disable("遥测数据收集，可禁用以减少后台上传"),
        "dmwappushservice" => disable("WAP 推送消息路由，个人电脑通常无用"),
        "Fax" => disable("传真服务，无传真设备可禁用"),
        "RemoteRegistry" => disable("远程注册表访问，建议禁用以降低攻击面"),
        "MapsBroker" => disable("离线地图管理，不用离线地图可禁用"),
        "lfsvc" => disable("地理位置服务，不用定位可禁用"),
        "WalletService" => disable("钱包服务，极少使用"),
        "RetailDemo" => disable("零售演示模式，个人电脑无用"),
        "WMPNetworkSvc" => disable("媒体播放器网络共享，通常无用"),
        "XblAuthManager" | "XblGameSave" | "XboxNetApiSvc" => disable("Xbox 相关，不玩 Xbox 游戏可禁用"),
        "TabletInputService" => disable("触摸键盘/手写面板，无触摸屏可禁用"),
        "Spooler" => disable("打印后台处理，无打印机可禁用（有漏洞史）"),
        "SysMain" => manual("预读取加速，SSD 上收益有限，可改手动"),
        "WSearch" => Advice { category: AdviceCategory::Recommend, reason: "搜索索引，禁用后文件搜索变慢，按需选择", suggested: 0 },
        "WerSvc" => manual("错误报告上传，可改手动"),
        "SharedAccess" => disable("Internet 连接共享，不做热点/共享可禁用"),
        "wisvc" => disable("Windows Insider 服务，非预览用户可禁用"),
        "WbioSrvc" => disable("生物识别框架，不用指纹/面部登录可禁用"),
        "icssvc" => disable("移动热点服务，不用热点可禁用"),
        "LanmanServer" => disable("文件/打印共享服务端，不做共享可禁用（有漏洞史）"),
        "PcaSvc" => manual("程序兼容助手弹窗，可改手动"),
        "SCardSvr" => disable("智能卡服务，无智能卡可禁用"),
        "SessionEnv" | "TermService" => disable("远程桌面相关，不用远程桌面可禁用"),
        "ShellHWDetection" => disable("自动播放服务，禁用以降低自动运行风险"),
        "WebClient" => disable("WebDAV 客户端，不用可禁用（有漏洞史）"),
        "WinHttpAutoProxySvc" => manual("WPAD 代理自动发现，可改手动（有漏洞史）"),
        "RpcSs" | "RpcEptMapper" | "DcomLaunch" => caution("COM/RPC 基础组件，大量系统功能依赖，禁用会导致系统异常"),
        "Power" => caution("电源策略管理，禁用影响电源与性能调度"),
        "PlugPlay" => caution("即插即用设备枚举，禁用后硬件无法识别"),
        "ProfSvc" => caution("用户配置文件加载，禁用后无法登录桌面"),
        "SamSs" => caution("安全账户管理，禁用影响登录与凭据"),
        "LSM" | "CoreMessagingRegistrar" => caution("会话/消息基础组件，系统交互依赖"),
        "Schedule" => caution("任务计划程序，系统与软件维护任务依赖"),
        "EventLog" => caution("Windows 事件日志，故障排查与审计依赖"),
        "Winmgmt" => caution("WMI 管理接口，大量管理工具依赖"),
        "Dhcp" | "Dnscache" | "NlaSvc" | "netprofm" | "nsi" | "Wcmsvc" => caution("网络基础组件，禁用会导致断网或网络识别异常"),
        "BFE" | "MpsSvc" => caution("防火墙基础引擎/防火墙服务，禁用失去网络防护"),
        "WinDefend" | "WdNisSvc" | "SecurityHealthService" | "wscsvc" => caution("Windows 安全中心/防病毒组件，禁用降低系统防护"),
        "NgcSvc" | "NgcCtnrSvc" => caution("Windows Hello 凭据容器，禁用影响 PIN/面部登录"),
        "TrustedInstaller" => caution("Windows 模块安装器，更新与系统组件维护依赖"),
        "CryptSvc" => caution("加密服务与证书校验，禁用影响更新与签名验证"),
        "StateRepository" => caution("应用状态仓库，UWP/开始菜单依赖"),
        "AppXSvc" | "ClipSVC" | "InstallService" | "LicenseManager" => caution("应用部署/许可组件，禁用影响商店应用安装与运行"),
        "AudioSrv" | "AudioEndpointBuilder" => caution("音频服务，禁用后系统无声"),
        "Themes" => caution("主题管理，禁用后界面回退经典样式"),
        "WlanSvc" => caution("无线网卡配置，禁用后 Wi-Fi 不可用"),
        "bthserv" => caution("蓝牙支持服务，禁用后蓝牙不可用"),
        "SENS" => caution("系统事件通知，登录/网络事件订阅依赖"),
        "SystemEventsBroker" | "TimeBrokerSvc" | "BrokerInfrastructure" => caution("后台任务触发/基础结构，禁用影响后台任务运行"),
        "FontCache" => caution("字体缓存服务，禁用后文本渲染变慢"),
        "LanmanWorkstation" => caution("SMB 客户端，禁用后无法访问网络共享/映射盘"),
        "msiserver" => caution("Windows Installer，禁用后无法安装 msi 包"),
        "sppsvc" => caution("软件保护平台，禁用影响系统/Office 激活"),
        "TokenBroker" | "wlidsvc" => caution("账户令牌/登录助手，禁用影响商店与 Microsoft 账户登录"),
        "UsoSvc" | "WaaSMedicSvc" | "wuauserv" => caution("Windows 更新组件，禁用失去安全更新与更新自愈"),
        "VSS" => caution("卷影复制，禁用影响还原点与备份"),
        "hidserv" => caution("人机接口设备服务，禁用影响外设按键/快捷键"),
        "StorSvc" => caution("存储设置管理，禁用影响存储感知与磁盘管理"),
        _ => {
            const PER_USER: [&str; 8] = [
                "UserDataSvc", "OneSyncSvc", "MessagingService", "PimIndexMaintenanceSvc",
                "UnistoreSvc", "CDPUserSvc", "PrintWorkflowUserSvc", "WpnUserService",
            ];
            if PER_USER.iter().any(|p| name.starts_with(p)) {
                return caution("每用户同步/通知组件，禁用影响邮件、日历与通知同步");
            }
            Advice { category: AdviceCategory::Keep, reason: "", suggested: 0 }
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const UNDO_KEY: &str = "Software\\MWCleaner\\ServiceUndo";

pub fn original_start_type(name: &str) -> Option<u32> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(UNDO_KEY, KEY_READ)
        .ok()?
        .get_value::<u32, _>(name)
        .ok()
}

pub fn record_original(name: &str, orig: u32) {
    if let Ok((k, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(UNDO_KEY) {
        if k.get_value::<u32, _>(name).is_err() {
            let _ = k.set_value(name, &orig);
        }
    }
}

pub fn clear_original(name: &str) {
    if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(UNDO_KEY, KEY_WRITE) {
        let _ = k.delete_value(name);
    }
}

unsafe fn pwstr_to_string(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0;
    while *p.0.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len))
}

pub fn list() -> Result<Vec<ServiceInfo>, String> {
    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE)
            .map_err(|e| format!("打开服务管理器失败: {e}"))?;
        let mut buf: Vec<u64> = vec![0; 32 * 1024];
        let mut needed = 0u32;
        let mut count = 0u32;
        loop {
            let r = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(std::slice::from_raw_parts_mut(
                    buf.as_mut_ptr() as *mut u8,
                    buf.len() * 8,
                )),
                &mut needed,
                &mut count,
                None,
                PCWSTR::null(),
            );
            match r {
                Ok(()) => break,
                Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => {
                    buf.resize((needed as usize + 4096 + 7) / 8, 0);
                }
                Err(e) => {
                    let _ = CloseServiceHandle(scm);
                    return Err(format!("枚举服务失败: {e}"));
                }
            }
        }
        let statuses = std::slice::from_raw_parts(
            buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW,
            count as usize,
        );
        let mut result = Vec::new();
        for s in statuses {
            let name = pwstr_to_string(s.lpServiceName);
            let display = resolve_indirect(&pwstr_to_string(s.lpDisplayName));
            let description = service_description(&name);
            let state = s.ServiceStatusProcess.dwCurrentState.0;
            let (start_type, binary) = query_config(scm, &name).unwrap_or((0, String::new()));
            result.push(ServiceInfo { name, display, description, binary, state, start_type });
        }
        let _ = CloseServiceHandle(scm);
        result.sort_by(|a, b| a.display.to_lowercase().cmp(&b.display.to_lowercase()));
        Ok(result)
    }
}

unsafe fn query_config(scm: SC_HANDLE, name: &str) -> Option<(u32, String)> {
    let wide = to_wide(name);
    let svc = OpenServiceW(scm, PCWSTR(wide.as_ptr()), SERVICE_QUERY_CONFIG).ok()?;
    let mut buf = vec![0u64; 1024];
    let mut needed = 0u32;
    let mut out = None;
    loop {
        let ptr = buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;
        match QueryServiceConfigW(svc, Some(ptr), (buf.len() * 8) as u32, &mut needed) {
            Ok(()) => {
                let cfg = &*ptr;
                out = Some((cfg.dwStartType.0, pwstr_to_string(cfg.lpBinaryPathName)));
                break;
            }
            Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => {
                buf.resize((needed as usize + 4096 + 7) / 8, 0);
            }
            Err(_) => break,
        }
    }
    let _ = CloseServiceHandle(svc);
    out
}

pub fn set_start_type(name: &str, start_type: u32) -> Result<(), String> {
    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map_err(|e| format!("打开服务管理器失败: {e}"))?;
        let wide = to_wide(name);
        let svc = OpenServiceW(scm, PCWSTR(wide.as_ptr()), SERVICE_CHANGE_CONFIG)
            .map_err(|e| format!("打开服务 {name} 失败: {e}"))?;
        let r = ChangeServiceConfigW(
            svc,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            SERVICE_START_TYPE(start_type),
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        );
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);
        r.map_err(|e| format!("修改服务 {name} 失败: {e}"))
    }
}

// ===== C2：服务批量应用建议优化 =====

/// C2：逐项执行结果（display 名 + 成败）
pub struct BatchItemResult {
    /// 服务键名（用于回查列表行）
    pub name: String,
    pub display: String,
    pub target: u32,
    pub err: Option<String>,
}

/// C2：批量候选筛选。安全红线四条：
/// ① 只取 advice.category == Recommend 且 suggested != 0 的服务；
/// ② 当前 start_type 已是目标类型的不列；
/// ③ start_type ∈ {2,3,4} 之外（引导 0/系统 1）绝不触碰——SCM 语义特殊；
/// ④ 已有 undo 记录 = 用户手动改过，尊重既有操作，排除。
pub fn batch_candidates(services: &[ServiceInfo]) -> Vec<(usize, u32)> {
    batch_candidates_filtered(services, &|n| original_start_type(n).is_some())
}

/// 可注入 undo 判定的内部变体（纯函数，便于单测）。
fn batch_candidates_filtered(services: &[ServiceInfo], has_undo: &dyn Fn(&str) -> bool) -> Vec<(usize, u32)> {
    services
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let adv = advice(&s.name);
            if adv.category != AdviceCategory::Recommend || adv.suggested == 0 {
                return None; // 红线①：仅 Recommend 且参与批量
            }
            if s.start_type == adv.suggested {
                return None; // 红线②：已是目标状态
            }
            if !matches!(s.start_type, 2 | 3 | 4) {
                return None; // 红线③：引导/系统启动类型绝不触碰
            }
            if has_undo(&s.name) {
                return None; // 红线④：用户手动改过，尊重既有操作
            }
            Some((i, adv.suggested))
        })
        .collect()
}

// ===== C3：服务依赖警告 =====

/// C3：枚举仍处于运行中的依赖服务显示名。查询失败返回 Err（调用方按「未能确认依赖」降级）。
/// 仿 query_config 双调用 + ERROR_MORE_DATA 扩容模式：
/// OpenSCManagerW(SC_MANAGER_CONNECT) → OpenServiceW(SERVICE_ENUMERATE_DEPENDENTS)
/// → EnumDependentServicesW(svc, SERVICE_ACTIVE, ...) 循环扩容
/// → 读 ENUM_SERVICE_STATUSW.lpDisplayName（resolve_indirect 解 @ 引用）
pub fn active_dependents(name: &str) -> Result<Vec<String>, String> {
    unsafe {
        let scm = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT)
            .map_err(|e| format!("打开服务管理器失败: {e}"))?;
        let wide = to_wide(name);
        let svc = match OpenServiceW(scm, PCWSTR(wide.as_ptr()), SERVICE_ENUMERATE_DEPENDENTS) {
            Ok(s) => s,
            Err(e) => {
                let _ = CloseServiceHandle(scm);
                return Err(format!("打开服务 {name} 失败: {e}"));
            }
        };
        let mut buf: Vec<u64> = vec![0; 16 * 1024];
        let mut needed = 0u32;
        let mut count = 0u32;
        loop {
            let r = EnumDependentServicesW(
                svc,
                SERVICE_ACTIVE,
                Some(buf.as_mut_ptr() as *mut ENUM_SERVICE_STATUSW),
                (buf.len() * 8) as u32,
                &mut needed,
                &mut count,
            );
            match r {
                Ok(()) => break,
                Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => {
                    buf.resize((needed as usize + 4096 + 7) / 8, 0);
                }
                Err(e) => {
                    let _ = CloseServiceHandle(svc);
                    let _ = CloseServiceHandle(scm);
                    return Err(format!("枚举依赖失败: {e}"));
                }
            }
        }
        // SERVICE_ACTIVE 过滤后仅含运行中的依赖服务，直接收集显示名
        let statuses =
            std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUSW, count as usize);
        let mut out: Vec<String> = statuses
            .iter()
            .map(|s| resolve_indirect(&pwstr_to_string(s.lpDisplayName)))
            .collect();
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);
        out.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str, start_type: u32) -> ServiceInfo {
        ServiceInfo {
            name: name.into(),
            display: name.into(),
            description: String::new(),
            binary: String::new(),
            state: 1,
            start_type,
        }
    }

    #[test]
    fn advice_suggested_targets() {
        // 可禁用类 → 4
        for n in [
            "DiagTrack", "Fax", "RemoteRegistry", "Spooler", "XblAuthManager",
            "LanmanServer", "SessionEnv", "TermService", "WebClient", "icssvc",
        ] {
            assert_eq!(advice(n).suggested, 4, "{n} 应为禁用目标");
        }
        // 可改手动类 → 3
        for n in ["SysMain", "PcaSvc", "WerSvc", "WinHttpAutoProxySvc"] {
            assert_eq!(advice(n).suggested, 3, "{n} 应为手动目标");
        }
        // WSearch：作者本意按需选择 → 0 不参与批量
        assert_eq!(advice("WSearch").suggested, 0);
        // Keep / Caution 全部 → 0
        assert_eq!(advice("RpcSs").suggested, 0);
        assert_eq!(advice("Power").suggested, 0);
        assert_eq!(advice("wuauserv").suggested, 0);
        assert_eq!(advice("UnknownSvc").suggested, 0);
        assert_eq!(advice("UserDataSvc_abc").suggested, 0);
    }

    #[test]
    fn batch_candidates_rules() {
        // 注入式 undo 判定：只有 "Fax" 视为已有 undo 记录
        let has_undo = |n: &str| n == "Fax";
        let list = vec![
            svc("Fax", 2),           // Recommend/4，但有 undo → 排除（红线④）
            svc("DiagTrack", 2),     // Recommend/4 → 命中
            svc("DiagTrack", 4),     // 已是目标 → 排除（红线②）
            svc("SysMain", 2),       // Recommend/3 → 命中 target=3
            svc("SysMain", 3),       // 已是目标 → 排除
            svc("WSearch", 2),       // suggested=0 → 排除
            svc("RpcSs", 2),         // Caution → 排除（红线①）
            svc("UnknownSvc", 2),    // Keep → 排除
            svc("Spooler", 1),       // 系统启动类型 → 排除（红线③）
            svc("Spooler", 0),       // 引导启动类型 → 排除（红线③）
        ];
        let got = batch_candidates_filtered(&list, &has_undo);
        assert_eq!(got, vec![(1, 4), (3, 3)]);
        // start_type ∈ {2,3,4} 正常命中：已是禁用态不再列；手动态禁用建议照样命中
        let ok = vec![svc("DiagTrack", 4)];
        assert!(batch_candidates_filtered(&ok, &|_| false).is_empty());
        let ok2 = vec![svc("DiagTrack", 3)];
        assert_eq!(batch_candidates_filtered(&ok2, &|_| false), vec![(0, 4)]);
    }

    #[test]
    fn active_dependents_smoke() {
        // RpcSs 调用不 panic 即可（其 ACL 可能拒绝标准用户枚举，成败依环境而定）
        let _ = active_dependents("RpcSs");
        // 不存在的服务 → Err（ERROR_SERVICE_DOES_NOT_EXIST，无需任何特权）
        assert!(active_dependents("__mw_no_such_service__").is_err());
    }
}