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
}

pub fn advice(name: &str) -> Advice {
    let recommend = |reason: &'static str| Advice { category: AdviceCategory::Recommend, reason };
    let caution = |reason: &'static str| Advice { category: AdviceCategory::Caution, reason };
    match name {
        "DiagTrack" => recommend("遥测数据收集，可禁用以减少后台上传"),
        "dmwappushservice" => recommend("WAP 推送消息路由，个人电脑通常无用"),
        "Fax" => recommend("传真服务，无传真设备可禁用"),
        "RemoteRegistry" => recommend("远程注册表访问，建议禁用以降低攻击面"),
        "MapsBroker" => recommend("离线地图管理，不用离线地图可禁用"),
        "lfsvc" => recommend("地理位置服务，不用定位可禁用"),
        "WalletService" => recommend("钱包服务，极少使用"),
        "RetailDemo" => recommend("零售演示模式，个人电脑无用"),
        "WMPNetworkSvc" => recommend("媒体播放器网络共享，通常无用"),
        "XblAuthManager" | "XblGameSave" | "XboxNetApiSvc" => recommend("Xbox 相关，不玩 Xbox 游戏可禁用"),
        "TabletInputService" => recommend("触摸键盘/手写面板，无触摸屏可禁用"),
        "Spooler" => recommend("打印后台处理，无打印机可禁用（有漏洞史）"),
        "SysMain" => recommend("预读取加速，SSD 上收益有限，可改手动"),
        "WSearch" => recommend("搜索索引，禁用后文件搜索变慢，按需选择"),
        "WerSvc" => recommend("错误报告上传，可改手动"),
        "SharedAccess" => recommend("Internet 连接共享，不做热点/共享可禁用"),
        "wisvc" => recommend("Windows Insider 服务，非预览用户可禁用"),
        "WbioSrvc" => recommend("生物识别框架，不用指纹/面部登录可禁用"),
        "icssvc" => recommend("移动热点服务，不用热点可禁用"),
        "LanmanServer" => recommend("文件/打印共享服务端，不做共享可禁用（有漏洞史）"),
        "PcaSvc" => recommend("程序兼容助手弹窗，可改手动"),
        "SCardSvr" => recommend("智能卡服务，无智能卡可禁用"),
        "SessionEnv" | "TermService" => recommend("远程桌面相关，不用远程桌面可禁用"),
        "ShellHWDetection" => recommend("自动播放服务，禁用以降低自动运行风险"),
        "WebClient" => recommend("WebDAV 客户端，不用可禁用（有漏洞史）"),
        "WinHttpAutoProxySvc" => recommend("WPAD 代理自动发现，可改手动（有漏洞史）"),
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
            Advice { category: AdviceCategory::Keep, reason: "" }
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
        let mut buf: Vec<u8> = vec![0; 256 * 1024];
        let mut needed = 0u32;
        let mut count = 0u32;
        loop {
            let r = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                Some(buf.as_mut_slice()),
                &mut needed,
                &mut count,
                None,
                PCWSTR::null(),
            );
            match r {
                Ok(()) => break,
                Err(e) if e.code() == ERROR_MORE_DATA.to_hresult() => {
                    buf.resize(needed as usize + 4096, 0);
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
            let (start_type, binary) = query_config(scm, &name).unwrap_or((3, String::new()));
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
    let ptr = buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW;
    let mut needed = 0u32;
    let ok = QueryServiceConfigW(svc, Some(ptr), 8192, &mut needed).is_ok();
    let _ = CloseServiceHandle(svc);
    if ok {
        let cfg = &*ptr;
        Some((cfg.dwStartType.0, pwstr_to_string(cfg.lpBinaryPathName)))
    } else {
        None
    }
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