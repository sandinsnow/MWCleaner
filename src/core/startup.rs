use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use winreg::enums::*;
use winreg::RegKey;
use windows::core::{BSTR, GUID, Interface, PCWSTR, VARIANT};
use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, ITaskDefinition, ITaskFolder, ITaskService,
    TASK_ACTION_EXEC, TASK_ACTION_TYPE, TASK_ENUM_HIDDEN, TASK_TRIGGER_BOOT,
    TASK_TRIGGER_LOGON, TASK_TRIGGER_TYPE2,
};

use super::app_data_dir;

const DISABLED_KEY: &str = "Software\\MWCleaner\\DisabledStartup";

// Task Scheduler 2.0（Vista+）C 类 TaskScheduler 的 CLSID；
// windows-0.58.0 仅预定义了 1.0 遗留接口的 CLSID_CTaskScheduler，2.0 值需内联。
const CLSID_TASKSCHEDULER2: GUID = GUID::from_u128(0x0f87369f_a4e5_4cfc_bd3e_73e6154572dd);

/// C4 数据编码约定：task_items 构造的条目 source = "计划任务" + IRegisteredTask::Path 全路径；
/// UI 展示与 set_task_enabled 回传均以该前缀剥离取全路径。
const TASK_SOURCE_PREFIX: &str = "计划任务";

#[derive(Clone)]
pub struct StartupItem {
    pub name: String,
    pub command: String,
    pub description: String,
    pub source: String,
    pub is_folder: bool,
    pub enabled: bool,
    /// C4：计划任务来源——禁用走 COM SetEnabled 分流、UI 隐藏删除按钮。
    /// 现有全部构造点（reg/folder/disabled 循环 + tests）均为 false。
    pub is_task: bool,
}

// 通用工具函数已归位到 core::mod；此处 re-export 保持旧调用点兼容。
#[allow(unused_imports)]
pub use super::{exe_path, expand_env};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn file_description(path: &PathBuf) -> String {
    unsafe {
        let w: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return String::new();
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(PCWSTR(w.as_ptr()), 0, size, buf.as_mut_ptr() as *mut _)
            .is_err()
        {
            return String::new();
        }
        let mut langs: Vec<(u16, u16)> = Vec::new();
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if VerQueryValueW(
            buf.as_ptr() as *const _,
            PCWSTR(wide("\\VarFileInfo\\Translation").as_ptr()),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            && !ptr.is_null()
            && len >= 4
        {
            let t = std::slice::from_raw_parts(ptr as *const u16, len as usize / 2);
            for pair in t.chunks(2) {
                if pair.len() == 2 {
                    langs.push((pair[0], pair[1]));
                }
            }
        }
        langs.push((0x0804, 0x04b0));
        langs.push((0x0409, 0x04b0));
        for (lang, cp) in langs {
            let q = wide(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
            let mut p2: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut l2 = 0u32;
            if VerQueryValueW(buf.as_ptr() as *const _, PCWSTR(q.as_ptr()), &mut p2, &mut l2)
                .as_bool()
                && !p2.is_null()
                && l2 > 0
            {
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(
                    p2 as *const u16,
                    l2 as usize,
                ));
                let s = s.trim_end_matches('\0').trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
        String::new()
    }
}

fn description_of(command: &str) -> String {
    exe_path(command)
        .map(|p| file_description(&p))
        .unwrap_or_default()
}

fn reg_sources() -> Vec<String> {
    vec![
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
        "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce".into(),
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
        "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\RunOnce".into(),
        "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run".into(),
    ]
}

fn strip_hive(source: &str) -> (&str, RegKey) {
    if let Some(rest) = source.strip_prefix("HKCU\\") {
        (rest, RegKey::predef(HKEY_CURRENT_USER))
    } else {
        (source.strip_prefix("HKLM\\").unwrap_or(source), RegKey::predef(HKEY_LOCAL_MACHINE))
    }
}

fn folder_sources() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        v.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        v.push(PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"));
    }
    v.into_iter().filter(|p| p.is_dir()).collect()
}

fn disabled_store() -> io::Result<RegKey> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(DISABLED_KEY)?;
    Ok(key)
}

fn store_id(item: &StartupItem) -> String {
    format!("{}|{}", item.source, item.name)
}

fn source_hash(source: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

fn backup_file(item: &StartupItem) -> PathBuf {
    app_data_dir()
        .join("disabled-startup")
        .join(format!("{}#{:016x}", item.name, source_hash(&item.source)))
}

// v0.4.1 及更早版本的启动文件夹备份仅以 item.name 命名
fn legacy_backup_file(item: &StartupItem) -> PathBuf {
    app_data_dir().join("disabled-startup").join(&item.name)
}

pub fn list() -> Vec<StartupItem> {
    let mut items = Vec::new();
    for source in reg_sources() {
        let (sub, hive) = strip_hive(&source);
        let Ok(key) = hive.open_subkey_with_flags(sub, KEY_READ) else {
            continue;
        };
        for name in key.enum_values().flatten().map(|(n, _)| n) {
            if let Ok(cmd) = key.get_value::<String, _>(&name) {
                items.push(StartupItem {
                    name,
                    description: description_of(&cmd),
                    command: cmd,
                    source: source.clone(),
                    is_folder: false,
                    enabled: true,
                    is_task: false,
                });
            }
        }
    }
    for folder in folder_sources() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            items.push(StartupItem {
                name,
                description: description_of(&entry.path().to_string_lossy()),
                command: entry.path().to_string_lossy().to_string(),
                source: folder.to_string_lossy().to_string(),
                is_folder: true,
                enabled: true,
                is_task: false,
            });
        }
    }
    // C4：第四来源——计划任务中含登录/启动触发器的任务（COM 失败降级为空）
    items.extend(task_items());
    if let Ok(store) = disabled_store() {
        for (id, _) in store.enum_values().flatten() {
            let Some((source, name)) = id.split_once('|') else { continue };
            let Ok(command) = store.get_value::<String, _>(&id) else { continue };
            let is_folder = !source.starts_with("HKCU\\") && !source.starts_with("HKLM\\");
            items.push(StartupItem {
                name: name.to_string(),
                description: description_of(&command),
                command,
                source: source.to_string(),
                is_folder,
                enabled: false,
                is_task: false,
            });
        }
    }
    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

pub fn set_enabled(item: &StartupItem, enabled: bool) -> io::Result<()> {
    // C4 分流：计划任务走 COM SetEnabled，永不触达 DisabledStartup 存储
    if item.is_task {
        if item.enabled == enabled {
            return Ok(());
        }
        let path = item.source.strip_prefix(TASK_SOURCE_PREFIX).unwrap_or(&item.source);
        return set_task_enabled(path, enabled);
    }
    if item.enabled == enabled {
        return Ok(());
    }
    let store = disabled_store()?;
    let id = store_id(item);
    if enabled {
        let command: String = store.get_value(&id)?;
        if item.is_folder {
            let target = PathBuf::from(&item.source).join(&item.name);
            let backup = backup_file(item);
            let backup = if backup.exists() { backup } else { legacy_backup_file(item) };
            fs::rename(&backup, &target)?;
        } else {
            let (sub, hive) = strip_hive(&item.source);
            let (key, _) = hive.create_subkey(sub)?;
            key.set_value(&item.name, &command)?;
        }
        store.delete_value(&id)?;
    } else if item.is_folder {
        let src = PathBuf::from(&item.source).join(&item.name);
        let backup_dir = app_data_dir().join("disabled-startup");
        fs::create_dir_all(&backup_dir)?;
        fs::rename(&src, backup_file(item))?;
        store.set_value(&id, &item.command)?;
    } else {
        let (sub, hive) = strip_hive(&item.source);
        let key = hive.open_subkey_with_flags(sub, KEY_READ | KEY_WRITE)?;
        let command: String = key.get_value(&item.name)?;
        store.set_value(&id, &command)?;
        key.delete_value(&item.name)?;
    }
    Ok(())
}

pub fn delete(item: &StartupItem) -> io::Result<()> {
    if item.enabled {
        // A3：启用项删除先走禁用备份（注册表命令/启动文件夹 lnk），可在已禁用列表启用还原
        return set_enabled(item, false);
    }
    let store = disabled_store()?;
    store.delete_value(store_id(item))?;
    if item.is_folder {
        let backup = backup_file(item);
        if backup.exists() {
            let _ = fs::remove_file(backup);
        } else {
            let _ = fs::remove_file(legacy_backup_file(item));
        }
    }
    Ok(())
}

// ===== C4：计划任务自启源（Task Scheduler 2.0 COM，只读枚举 + 禁用/启用） =====

/// COM 初始化配对助手：S_OK/S_FALSE（均为成功初始化）时执行闭包并在结束后 CoUninitialize 配对；
/// 初始化失败（如 RPC_E_CHANGED_MODE——线程已被其他模式初始化）不配对。
/// 供 task_items / set_task_enabled / warm_icons 三处复用，统一与 warm_icons 既有先例相同的模式。
pub fn with_com_init<R>(f: impl FnOnce() -> R) -> R {
    unsafe {
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let initialized = hr.is_ok();
        let result = f();
        if initialized {
            CoUninitialize();
        }
        result
    }
}

/// 枚举计划任务中含「登录/启动」触发器的任务（启动项页第四来源）。
/// COM 初始化或创建失败 → 返回空 Vec（前三个来源不受影响，UI 无感）；
/// 权限受限的子文件夹/单任务 → 跳过该项，绝不整体失败、绝不 panic。
pub fn task_items() -> Vec<StartupItem> {
    let mut items = Vec::new();
    // 整体失败（无 Task Scheduler 服务等）→ 静默降级为空来源
    let _ = with_com_init(|| collect_task_items(&mut items));
    items
}

/// 连接本机 Task Scheduler 服务并递归根文件夹；仅顶层创建/连接失败整体报错。
fn collect_task_items(items: &mut Vec<StartupItem>) -> windows::core::Result<()> {
    unsafe {
        let svc: ITaskService = CoCreateInstance(&CLSID_TASKSCHEDULER2, None, CLSCTX_INPROC_SERVER)?;
        // 四个 VARIANT 默认 VT_EMPTY：本机 + 当前用户，凭据缺失即为官方约定的「不指定」
        let empty = VARIANT::default();
        svc.Connect(&empty, &empty, &empty, &empty)?;
        let root = svc.GetFolder(&BSTR::from("\\"))?;
        walk_task_folder(&root, items);
        Ok(())
    }
}

/// 递归枚举：先扫当前文件夹的任务，再下钻子文件夹；
/// 任一文件夹/任务失败只影响自身，不中断兄弟节点。
fn walk_task_folder(folder: &ITaskFolder, items: &mut Vec<StartupItem>) {
    unsafe {
        if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN.0 as i32) {
            if let Ok(count) = tasks.Count() {
                // COM 集合为 1-based；索引参数按生成签名收 VARIANT
                for i in 1..=count {
                    let idx = VARIANT::from(i);
                    if let Ok(task) = tasks.get_Item(&idx) {
                        if let Some(item) = task_to_startup_item(&task) {
                            items.push(item);
                        }
                    }
                }
            }
        }
        if let Ok(subs) = folder.GetFolders(0) {
            if let Ok(count) = subs.Count() {
                for i in 1..=count {
                    // get_Item 直接返回子文件夹对象，无需 GetFolder 二次解析
                    let idx = VARIANT::from(i);
                    if let Ok(sub) = subs.get_Item(&idx) {
                        walk_task_folder(&sub, items);
                    }
                }
            }
        }
    }
}

/// 单任务 → StartupItem；触发器/定义解析失败返回 None（跳过该任务）。
fn task_to_startup_item(task: &IRegisteredTask) -> Option<StartupItem> {
    unsafe {
        let path = task.Path().ok()?.to_string();
        let def: ITaskDefinition = task.Definition().ok()?;
        // SPEC 原文：只枚举登录/启动触发的任务，其余（如每日 TimeTrigger）不出现
        let triggers = def.Triggers().ok()?;
        let mut label: Option<&'static str> = None;
        // ITriggerCollection 为双接口出参风格：Count/get_Item 手写出参
        let mut count = 0i32;
        if triggers.Count(&mut count).is_ok() {
            for i in 1..=count {
                if let Ok(trigger) = triggers.get_Item(i) {
                    let mut t = TASK_TRIGGER_TYPE2(0);
                    if trigger.Type(&mut t).is_ok()
                        && matches!(t, TASK_TRIGGER_LOGON | TASK_TRIGGER_BOOT)
                    {
                        label = Some(task_trigger_label(t));
                        break;
                    }
                }
            }
        }
        let label = label?;
        let enabled = task.Enabled().map(|v| v.as_bool()).unwrap_or(true);
        // command：首个 EXEC action；无 EXEC action 时回落任务全路径
        let command = exec_command(&def).unwrap_or_else(|| path.clone());
        let name = path.rsplit('\\').next().unwrap_or(&path).to_string();
        Some(StartupItem {
            name,
            command,
            description: label.to_string(),
            source: format!("{TASK_SOURCE_PREFIX}{path}"),
            is_folder: false,
            enabled,
            is_task: true,
        })
    }
}

/// 取任务首个 EXEC action 的 Path+Arguments；无 EXEC action 返回 None。
fn exec_command(def: &ITaskDefinition) -> Option<String> {
    unsafe {
        let actions = def.Actions().ok()?;
        // IActionCollection 同为出参风格
        let mut count = 0i32;
        if actions.Count(&mut count).is_err() {
            return None;
        }
        for i in 1..=count {
            let Ok(action) = actions.get_Item(i) else { continue };
            let mut t = TASK_ACTION_TYPE(0);
            if action.Type(&mut t).is_err() || t != TASK_ACTION_EXEC {
                continue;
            }
            let exec: IExecAction = action.cast().ok()?;
            let mut path = BSTR::new();
            if exec.Path(&mut path).is_err() {
                continue;
            }
            let mut args = BSTR::new();
            let args = exec.Arguments(&mut args).ok().map(|_| args).unwrap_or_default();
            let path = path.to_string();
            return Some(format_task_command(&path, &args.to_string()));
        }
        None
    }
}

/// EXEC 命令拼装：参数为空时仅保留程序路径。
pub fn format_task_command(path: &str, args: &str) -> String {
    let args = args.trim();
    if args.is_empty() {
        path.to_string()
    } else {
        format!("{path} {args}")
    }
}

/// 触发类型中文描述。
pub fn task_trigger_label(t: TASK_TRIGGER_TYPE2) -> &'static str {
    match t {
        TASK_TRIGGER_LOGON => "登录时触发",
        TASK_TRIGGER_BOOT => "系统启动时触发",
        _ => "计划任务",
    }
}

/// 计划任务启停（IRegisteredTask::SetEnabled）。task_path = IRegisteredTask::Path 全路径。
/// 纯 COM 调用，不走 schtasks 子进程、不改任务 XML；失败需注明可能缺管理员权限。
pub fn set_task_enabled(task_path: &str, enabled: bool) -> io::Result<()> {
    let result = with_com_init(|| -> windows::core::Result<()> {
        unsafe {
            let svc: ITaskService =
                CoCreateInstance(&CLSID_TASKSCHEDULER2, None, CLSCTX_INPROC_SERVER)?;
            let empty = VARIANT::default();
            svc.Connect(&empty, &empty, &empty, &empty)?;
            // GetTask 挂在 ITaskFolder 上：拆父文件夹 + 叶子名（根级任务父为 "\\"）
            let (parent, leaf) = match task_path.rsplit_once('\\') {
                Some(("", leaf)) => ("\\", leaf),
                Some((parent, leaf)) => (parent, leaf),
                None => ("\\", task_path),
            };
            let folder = svc.GetFolder(&BSTR::from(parent))?;
            let task = folder.GetTask(&BSTR::from(leaf))?;
            task.SetEnabled(if enabled { VARIANT_TRUE } else { VARIANT_FALSE })
        }
    });
    result.map_err(|e| {
        let msg = e.to_string();
        io::Error::new(io::ErrorKind::Other, msg)
    })
}

// ===== B5：本程序自身的开机自启开关（HKCU Run 键，无需管理员权限） =====

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const SELF_RUN_VALUE: &str = "MWCleaner";

/// 设置页可选开关：Run 键下 "MWCleaner" 值存在且指向自身 exe。
pub fn self_run_enabled() -> bool {
    let Some(v) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .ok()
        .and_then(|k| k.get_value::<String, _>(SELF_RUN_VALUE).ok())
    else {
        return false;
    };
    run_value_points_to_self(&v)
}

/// C6 观察项防护：Run 值必须指向自身 exe——防用户/其他软件改写同名值后本页误报「已开启」。
fn run_value_points_to_self(v: &str) -> bool {
    if v.trim().is_empty() {
        return false;
    }
    let Some(exe) = std::env::current_exe().ok() else {
        return false;
    };
    // exe_path：剥引号/取首 token/expand_env，兼容 "<self>" --flag 形式
    super::exe_path(v).map(|p| same_file(&exe, &p)).unwrap_or(false)
}

/// 路径同一性：canonicalize（解析大小写/8.3 短名/相对路径）优先，失败回落小写字符串比对。
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase(),
    }
}

/// 写入/删除 "MWCleaner" 值，指向自身 exe 路径（带引号，兼容含空格路径）。
pub fn set_self_run(enabled: bool) -> io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY)?;
    if enabled {
        let exe = std::env::current_exe()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("无法定位自身路径：{e}")))?;
        key.set_value(SELF_RUN_VALUE, &format!("\"{}\"", exe.display()))?;
    } else {
        // 值不存在时删除会报错，视为已关闭
        let _ = key.delete_value(SELF_RUN_VALUE);
    }
    Ok(())
}

// ===== C1：启动项三分类建议（仿 services.rs advice 模式） =====

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartupAdviceCategory {
    /// 建议优化（更新器/遥测/推广类）
    Recommend,
    /// 保持现状（系统/输入法/安全/驱动类，未知项亦归此类）
    Keep,
    /// 不建议优化（已知高危，禁用需格外谨慎）
    Caution,
}

impl StartupAdviceCategory {
    pub fn label(self) -> &'static str {
        match self {
            StartupAdviceCategory::Recommend => "建议优化",
            StartupAdviceCategory::Keep => "保持现状",
            StartupAdviceCategory::Caution => "不建议优化",
        }
    }
}

#[derive(Clone, Copy)]
pub struct StartupAdvice {
    pub category: StartupAdviceCategory,
    pub reason: &'static str,
}

/// 关键词规则表：依次匹配 name 再 command，先 Keep（防误伤系统组件）后 Recommend，最后 Caution。
/// 未命中 → Keep 且 reason 为空（UI 不显示徽标，等同服务页「未知=保持现状」）。
const ADVICE_RULES: &[(&str, StartupAdviceCategory, &str)] = &[
    // —— Keep：系统相关，用户可自行取舍，禁用前应知情 ——
    ("ctfmon", StartupAdviceCategory::Keep, "输入法相关组件，禁用会影响中文输入"),
    ("ime", StartupAdviceCategory::Keep, "输入法相关组件，禁用会影响中文输入"),
    ("onedrive", StartupAdviceCategory::Keep, "系统同步组件，是否保留由用户决定"),
    ("defender", StartupAdviceCategory::Keep, "安全软件，不建议经第三方工具改动"),
    ("antivirus", StartupAdviceCategory::Keep, "安全软件，不建议经第三方工具改动"),
    ("hips", StartupAdviceCategory::Keep, "安全软件主防组件，不建议禁用"),
    ("kaspersky", StartupAdviceCategory::Keep, "安全软件，不建议经第三方工具改动"),
    ("norton", StartupAdviceCategory::Keep, "安全软件，不建议经第三方工具改动"),
    ("mcafee", StartupAdviceCategory::Keep, "安全软件，不建议经第三方工具改动"),
    ("avast", StartupAdviceCategory::Keep, "安全软件，不建议经第三方工具改动"),
    ("driver", StartupAdviceCategory::Keep, "驱动辅助组件，禁用可能影响硬件功能"),
    ("audio", StartupAdviceCategory::Keep, "音频驱动组件，禁用可能影响声音输出"),
    ("realtek", StartupAdviceCategory::Keep, "音频驱动组件，禁用可能影响声音输出"),
    ("bluetooth", StartupAdviceCategory::Keep, "蓝牙组件，禁用会影响蓝牙外设"),
    ("printer", StartupAdviceCategory::Keep, "打印相关组件，禁用会影响打印功能"),
    ("touchpad", StartupAdviceCategory::Keep, "触控板驱动，禁用会影响手势功能"),
    // —— Recommend：厂商更新器 / 遥测 / 推广类，通常无保留价值 ——
    ("update", StartupAdviceCategory::Recommend, "厂商更新器，建议禁用（可手动检查更新）"),
    ("updater", StartupAdviceCategory::Recommend, "厂商更新器，建议禁用（可手动检查更新）"),
    ("upgrade", StartupAdviceCategory::Recommend, "升级程序，建议禁用（可手动检查更新）"),
    ("telemetry", StartupAdviceCategory::Recommend, "遥测数据上报，建议禁用"),
    ("crashreport", StartupAdviceCategory::Recommend, "崩溃上报组件，建议禁用"),
    ("reporter", StartupAdviceCategory::Recommend, "数据上报组件，建议禁用"),
    ("promo", StartupAdviceCategory::Recommend, "推广弹窗组件，建议禁用"),
    ("advert", StartupAdviceCategory::Recommend, "广告推广组件，建议禁用"),
    ("assistant", StartupAdviceCategory::Recommend, "厂商助手类组件，建议禁用"),
    ("notify", StartupAdviceCategory::Recommend, "通知推送组件，建议禁用"),
    // —— Caution：已知高危钩子 ——
    ("shutdown", StartupAdviceCategory::Caution, "关机钩子类条目，改动可能影响系统关机流程"),
];

/// 启动项建议：name 优先于 command，逐条规则匹配；未命中返回 Keep 且 reason 为空。
pub fn startup_advice(name: &str, command: &str) -> StartupAdvice {
    for field in [name, command] {
        let f = field.to_lowercase();
        for (kw, cat, reason) in ADVICE_RULES {
            let hit = match *kw {
                // 词边界匹配，防子串误伤：Runtime/Lifetime 不再命中输入法规则
                "ime" => contains_word(&f, kw),
                _ => f.contains(kw),
            };
            if hit {
                return StartupAdvice { category: *cat, reason };
            }
        }
    }
    StartupAdvice { category: StartupAdviceCategory::Keep, reason: "" }
}

/// 词边界匹配：命中位置左右任一侧紧邻 ASCII 字母/数字时视为词内不命中。
/// 路径分隔符、空格、连字符等均为合法边界（如 `\ime\cache`、`Windows IME` 命中）。
fn contains_word(hay: &str, word: &str) -> bool {
    let bytes = hay.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(word) {
        let i = start + pos;
        let end = i + word.len();
        let left_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let right_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if left_ok && right_ok {
            return true;
        }
        start = i + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_run_value_must_point_to_self() {
        // 指向他者 exe → false（cmd.exe 必然存在，且 ≠ 测试进程自身）
        assert!(!run_value_points_to_self("C:\\Windows\\System32\\cmd.exe"));
        // 空值 / 不存在的路径 → false
        assert!(!run_value_points_to_self(""));
        assert!(!run_value_points_to_self("  "));
        assert!(!run_value_points_to_self("C:\\__mw_not_exist_exe__.exe"));
        // 指向测试进程自身：裸路径 + 带引号带参数形式（exe_path 取首 token）
        let exe = std::env::current_exe().unwrap();
        assert!(run_value_points_to_self(&exe.display().to_string()));
        assert!(run_value_points_to_self(&format!("\"{}\" --flag", exe.display())));
    }

    #[test]
    fn same_file_case_and_missing() {
        let base = std::env::temp_dir().join(format!("mwcleaner_sametest_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let f = base.join("Target File.txt");
        fs::write(&f, b"x").unwrap();
        // 大小写不同的同一文件 → canonicalize 归一后相同
        assert!(same_file(&f, &base.join("target file.txt")));
        // 两个不同文件 → false
        let g = base.join("other.txt");
        fs::write(&g, b"y").unwrap();
        assert!(!same_file(&f, &g));
        // 不存在的路径：canonicalize 失败 → 回落小写字符串比对
        assert!(same_file(&base.join("Missing.TXT"), &base.join("missing.txt")));
        assert!(!same_file(&f, &base.join("missing.txt")));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn advice_categories() {
        // Recommend：更新器/遥测/推广
        assert_eq!(startup_advice("Adobe Updater", "").category, StartupAdviceCategory::Recommend);
        assert_eq!(startup_advice("", "C:\\x\\telemetry.exe").category, StartupAdviceCategory::Recommend);
        // name 优先于 command：name 命中 Keep 时不看 command
        let adv = startup_advice("OneDrive", "C:\\upd\\update.exe");
        assert_eq!(adv.category, StartupAdviceCategory::Keep);
        // Keep：输入法/安全/驱动
        assert_eq!(startup_advice("ctfmon", "").category, StartupAdviceCategory::Keep);
        assert!(startup_advice("ctfmon", "").reason.contains("输入法"));
        assert_eq!(startup_advice("MyAntivirus Pro", "").category, StartupAdviceCategory::Keep);
        // Caution
        assert_eq!(startup_advice("", "shutdown -h").category, StartupAdviceCategory::Caution);
        // 未命中：Keep 且无徽标
        let unknown = startup_advice("MyApp", "myapp.exe");
        assert_eq!(unknown.category, StartupAdviceCategory::Keep);
        assert_eq!(unknown.reason, "");
    }

    #[test]
    fn ime_keyword_word_boundary() {
        // QA 第 2 轮 Bug B：子串误伤修复——"ime" 必须词边界命中
        // Runtime/Lifetime 等 name 不再误显示输入法文案
        let runtime = startup_advice("Runtime Broker", "");
        assert_eq!(runtime.category, StartupAdviceCategory::Keep);
        assert_eq!(runtime.reason, "");
        assert_eq!(startup_advice("Lifetime Cleaner", "").reason, "");
        // 独立词 "IME" 正常命中
        assert!(startup_advice("Windows IME", "").reason.contains("输入法"));
        // 路径段 \ime\ 正常命中
        assert!(startup_advice("", "C:\\Windows\\ime\\tool.exe").reason.contains("输入法"));
        // 词内出现（如 kime.exe 的 kime）不命中
        assert_eq!(startup_advice("kime host", "").reason, "");
    }

    #[test]
    fn contains_word_basics() {
        assert!(contains_word("a ime b", "ime"));
        assert!(contains_word("\\ime\\", "ime"));
        assert!(contains_word("ime", "ime"));
        assert!(!contains_word("runtime", "ime"));
        assert!(!contains_word("lifetime", "ime"));
        assert!(!contains_word("", "ime"));
    }

    #[test]
    fn format_task_command_basics() {
        // 参数为空：仅保留程序路径（不尾随空格）
        assert_eq!(format_task_command("C:\\x\\a.exe", ""), "C:\\x\\a.exe");
        assert_eq!(format_task_command("C:\\x\\a.exe", "   "), "C:\\x\\a.exe");
        // 带参数：单空格连接
        assert_eq!(
            format_task_command("C:\\x\\a.exe", "-f --flag=1"),
            "C:\\x\\a.exe -f --flag=1"
        );
        // 路径含空格原样保留（不改写引号语义，与任务 XML 记录一致）
        assert_eq!(
            format_task_command("C:\\Program Files\\app.exe", "/s"),
            "C:\\Program Files\\app.exe /s"
        );
    }

    #[test]
    fn task_trigger_label_mapping() {
        use windows::Win32::System::TaskScheduler::{TASK_TRIGGER_EVENT, TASK_TRIGGER_TIME};
        assert_eq!(task_trigger_label(TASK_TRIGGER_LOGON), "登录时触发");
        assert_eq!(task_trigger_label(TASK_TRIGGER_BOOT), "系统启动时触发");
        // 登录/启动以外触发类型统一兜底文案（不会出现在列表，仅防御）
        assert_eq!(task_trigger_label(TASK_TRIGGER_TIME), "计划任务");
        assert_eq!(task_trigger_label(TASK_TRIGGER_EVENT), "计划任务");
    }

    #[test]
    fn task_item_source_roundtrip() {
        // source 编码约定：前缀剥离后应还原为 IRegisteredTask::Path 全路径
        let path = "\\Microsoft\\Windows\\DiskCleanup\\SilentCleanup";
        let item = StartupItem {
            name: "SilentCleanup".into(),
            command: "cleanmgr.exe /sagerun:1".into(),
            description: task_trigger_label(TASK_TRIGGER_LOGON).into(),
            source: format!("{TASK_SOURCE_PREFIX}{path}"),
            is_folder: false,
            enabled: true,
            is_task: true,
        };
        assert_eq!(item.source.strip_prefix(TASK_SOURCE_PREFIX), Some(path));
        assert_eq!(item.name, item.source.rsplit('\\').next().unwrap());
    }

    #[test]
    fn delete_enabled_backup_roundtrip() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run, _) = hkcu.create_subkey("Software\\MWCleanerTest\\Run").unwrap();
        run.set_value("MWCleanerTestEntry", &"cmd.exe /c exit 0").unwrap();
        let item = StartupItem {
            name: "MWCleanerTestEntry".into(),
            description: String::new(),
            command: "cmd.exe /c exit 0".into(),
            source: "HKCU\\Software\\MWCleanerTest\\Run".into(),
            is_folder: false,
            enabled: true,
            is_task: false,
        };
        delete(&item).unwrap();
        assert!(run.get_value::<String, _>("MWCleanerTestEntry").is_err());
        let store = disabled_store().unwrap();
        assert!(store.get_value::<String, _>(&store_id(&item)).is_ok());

        let disabled = StartupItem { enabled: false, ..item.clone() };
        set_enabled(&disabled, true).unwrap();
        assert_eq!(
            run.get_value::<String, _>("MWCleanerTestEntry").unwrap(),
            "cmd.exe /c exit 0"
        );
        assert!(store.get_value::<String, _>(&store_id(&item)).is_err());

        set_enabled(&item, false).unwrap();
        delete(&disabled).unwrap();
        hkcu.delete_subkey_all("Software\\MWCleanerTest").unwrap();
    }

    #[test]
    fn folder_backup_same_name_two_sources() {
        let base = std::env::temp_dir().join(format!("mwcleaner_sftest_{}", std::process::id()));
        let src_a = base.join("a");
        let src_b = base.join("b");
        fs::create_dir_all(&src_a).unwrap();
        fs::create_dir_all(&src_b).unwrap();
        fs::write(src_a.join("MWCleanerTestSame.lnk"), b"aaa").unwrap();
        fs::write(src_b.join("MWCleanerTestSame.lnk"), b"bbb").unwrap();
        let mk = |src: &std::path::Path| StartupItem {
            name: "MWCleanerTestSame.lnk".into(),
            description: String::new(),
            command: src.join("MWCleanerTestSame.lnk").to_string_lossy().to_string(),
            source: src.to_string_lossy().to_string(),
            is_folder: true,
            enabled: true,
            is_task: false,
        };
        let a = mk(&src_a);
        let b = mk(&src_b);
        set_enabled(&a, false).unwrap();
        set_enabled(&b, false).unwrap();
        assert!(!src_a.join("MWCleanerTestSame.lnk").exists());
        assert!(!src_b.join("MWCleanerTestSame.lnk").exists());
        assert!(backup_file(&a).exists());
        assert!(backup_file(&b).exists());

        let da = StartupItem { enabled: false, ..a.clone() };
        let db = StartupItem { enabled: false, ..b.clone() };
        set_enabled(&da, true).unwrap();
        set_enabled(&db, true).unwrap();
        assert_eq!(fs::read(src_a.join("MWCleanerTestSame.lnk")).unwrap(), b"aaa");
        assert_eq!(fs::read(src_b.join("MWCleanerTestSame.lnk")).unwrap(), b"bbb");
        let store = disabled_store().unwrap();
        assert!(store.get_value::<String, _>(&store_id(&a)).is_err());
        assert!(store.get_value::<String, _>(&store_id(&b)).is_err());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn folder_legacy_backup_restore() {
        let base = std::env::temp_dir().join(format!("mwcleaner_sltest_{}", std::process::id()));
        let src = base.join("src");
        fs::create_dir_all(&src).unwrap();
        let item = StartupItem {
            name: "MWCleanerTestLegacy.lnk".into(),
            description: String::new(),
            command: src.join("MWCleanerTestLegacy.lnk").to_string_lossy().to_string(),
            source: src.to_string_lossy().to_string(),
            is_folder: true,
            enabled: false,
            is_task: false,
        };
        let dir = app_data_dir().join("disabled-startup");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("MWCleanerTestLegacy.lnk"), b"legacy").unwrap();
        disabled_store()
            .unwrap()
            .set_value(&store_id(&item), &item.command)
            .unwrap();
        set_enabled(&item, true).unwrap();
        assert_eq!(fs::read(src.join("MWCleanerTestLegacy.lnk")).unwrap(), b"legacy");
        assert!(disabled_store()
            .unwrap()
            .get_value::<String, _>(&store_id(&item))
            .is_err());
        fs::remove_dir_all(&base).unwrap();
    }
}