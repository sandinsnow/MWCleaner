# MWCleaner v0.2 调整 Spec（提案稿，待圈定范围）

基线：v0.1 单文件 5,893,120 字节。六模块（垃圾/启动项/服务/注册表/大文件/残留）+ 图形化 + 中文名称描述 + 应用图标 + 主题设置 + 服务建议三分类。
约束（继承）：Rust + gpui 0.2.2、单文件、Win10 1903+；破坏性操作两击确认；验证只读 + 自建测试件；每轮迭代最简范围。
工作量估级：S ≈ <1h，M ≈ 1–3h，L ≈ >3h（含本机编译与点击验证）。

## P0 安全网（建议必做）

- **A1 垃圾清理可选回收站**（M）
  现状：`core::junk::clean` 直接 `remove_file/remove_dir`，不可恢复。
  改动：`SHFileOperationW`（FOF_ALLOWUNDO | FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI）移入回收站；设置页开关「清理时移入回收站」默认开；回收站失败/被占用项保留在磁盘并计入失败数（不静默直删）；清理结果文案区分「已移入回收站/已删除」。
  验收：自建临时目录+文件走清理流程，回收站可见、可还原；开关关闭后直删。
- **A2 服务修改可撤销**（S）
  现状：改启动类型后无原值记录。
  改动：HKCU\Software\MWCleaner\ServiceUndo 存 name→原 start_type；被改行显示「还原」按钮；还原后清除记录；刷新/重启后记录仍在。
  验收：自建测试服务不可行 → 用清单内服务改手动再还原，核对注册表 Start 值回原值（此操作可逆，允许实机）。
- **A3 启动项删除备份与还原**（S–M）
  现状：删除 Run 值/启动文件夹链接不可恢复（禁用已有 DisabledStartup 存储）。
  改动：删除前把原值（name/command/source）写入 backups\startup-<ts>.json；启动项页「还原已删除」入口列表+一键写回。
  验收：自建 HKCU Run 测试值删除→还原，值与原文一致。
- **A4 注册表备份管理入口**（S）
  现状：清理前已自动导出 UTF-16 .reg 到 %APPDATA%\MWCleaner\backups 并在状态栏显示路径；无管理入口。
  改动：注册表页「打开备份目录」按钮（explorer 定位）；状态栏备份路径可点击定位；还原走系统双击 .reg，应用内不解析（写明文案）。
  验收：按钮打开目录且选中最新备份文件。

## P1 体验（选 2–3）

- **B1 列表搜索框**（M–L）：服务/启动项/残留按名称或路径过滤。gpui 0.2.2 无现成 TextInput（仅平台层提及），需自绘单行输入：焦点态 + window 键事件（字符/退格/清空）+ 光标闪烁可省。降级方案：不做输入框，改「首字母/类别 chips」过滤（S）。建议先降级方案收口。
- **B2 大文件阈值可配置**（S）：现状硬编码 100MB（ui/bigfiles_page.rs:64）。设置页 chips 50/100/500MB，持久化同 Theme；扫描页显示当前阈值。
- **B3 垃圾/注册表扫描可取消**（S）：复用大文件页 AtomicBool 取消模式；扫描中显示「取消」按钮与当前路径（progress 回调已带路径）。
- **B4 排序与导出**（M）：大文件/残留按大小降序开关；结果 CSV 导出到 %APPDATA%\MWCleaner\exports + explorer 定位（gpui 无原生保存对话框，不做 IFileSaveDialog）。
- **B5 设置扩展**（S/项）：A1 开关、B2 阈值、开机自启（写自身 Run 键，可关）、记住窗口位置尺寸（HKCU 存 bounds，启动恢复）。
- **B6 文案与状态打磨**（S）：状态消息 8s 自动淡出回默认提示；空结果/失败态文案统一「为什么+下一步」短句。

## P2 范围扩展（选 0–1）

- **C1 启动项三分类建议**（M）：复用服务 advice 模式（更新器/遥测=建议优化，安全/输入法/驱动=保持现状，未知=保持现状），徽标+筛选同服务页。
- **C2 服务批量应用建议优化**（M）：一键把 Recommend 类改为其建议类型（禁用或手动），两击确认+逐项结果报告；依赖 A2 撤销。
- **C3 服务依赖警告**（S–M）：禁用前 EnumDependentServicesW 列出仍在运行的依赖服务，确认提示中展示。
- **C4 计划任务自启源**（L）：ITaskService COM 枚举登录/启动触发任务，纳入启动项页第四来源；只读+禁用两击。
- **C5 残留交叉校验**（M）：与卸载注册表键交叉比对降低误报；追加残留注册表键扫描（HKCU/HKLM Software 下已卸载厂商键）。
- **C6 Windows.old/更新残留类别**（M）：垃圾页新增类别，带「删除后无法回退旧系统」警示与两击。

## P3 包装与外观

- **D1 exe 版本信息+图标**（M）：winresource 嵌 VersionInfo（中文产品名/版本/版权）与 .ico（源图用 ImageGen 或手绘 svg → PowerShell System.Drawing 转 ico）；资源管理器/任务栏观感统一。
- **D2 窗口最小尺寸/侧栏收起**（S）：最小 900x600；侧栏收起按钮留 48px 图标条（需先有 D1 图标或文字缩写）。
- **D3 HTML 清理报告**（L，低优先）：扫描结果单文件 HTML 导出。

## 非目标（本轮明确不做）

托盘常驻、自动更新、跨平台、多语言、内存/注册表"优化"类伪功能、注册表碎片整理、驱动级清理。

## 建议下一轮最简范围

A1 + A2 + A4 + B2 + B3（安全网三件 + 两个 S 级体验）， stretch 可选 B1 降级方案。
理由：全部可逆/只读验证友好，无新 COM 面，单轮可收口；C 类留待范围确认后单独迭代。

## 验收与验证约定

- 破坏性路径只用自建测试件验证（HKCU 测试键、临时目录）；A2 允许实机可逆改还原。
- 每个 UI 断言配 PrintWindow 截图并 Read；截图与注册表/文件状态成对核对（用户可能同时操作窗口）。
- 发布流水线不变：release build → mt.exe 注入管理员 manifest（保留 Common-Controls v6）→ dist 复制 → 回读 grep → 同名 asInvoker 副本实跑 → 清理副本。

---

## v0.4.x 实施状态（追加说明，不改写上方原始提案）

> 本节由编码实现后追加，用于标记各 P 项的**真实落地状态**；图例：✅ 已完成　🔄 机制等价（与 spec 字面不同但达成目标）　🟡 部分完成　❌ 未做/暂缓。
> 基线为 v0.4.1（含 UI 拆分重构），范围遵循「核心稳妥」确认结论。

### 完成度总表（截至 SPEC 终轮）

| 分组 | 状态 | 说明 |
|---|---|---|
| P0 | A1 ✅ A2 ✅ A3 🔄 A4 ✅ | 安全目标 4/4 达成；A3 采用「删除启用项=禁用+备份、在已禁用中启用还原」，未做独立 JSON 备份文件与还原页 |
| P1 | B1 ✅ B2 ✅ B3 ✅ B4 ✅ B5 ✅ B6 ✅ | **P1 六项全部落地**：B1 自绘输入框（完整 IME）三页接入；B4 排序 chips + CSV 导出；B5 记住窗口位置 + 开机自启开关；B6 状态 8s 淡出 |
| P2 | C1 ✅ C2 ✅ C3 ✅ C4 ✅ C5 ✅ C6 ✅ | **P2 六项全部落地**：C2 服务批量应用（四条安全红线+逐项撤销）；C3 服务依赖警告（EnumDependentServicesW）；C4 计划任务自启源（ITaskService 纯 COM，只读+两击禁用）；C5 残留交叉校验；C6 Windows 升级残留类别（强制直删+四层防护） |
| P3 | D1 ✅ D2 ✅ D3 ✅ | **P3 全部落地**：D3 单文件 HTML 报告（五节快照、双主题、零外部资源、全量转义） |
| 专项 | 检查更新 ✅ | WinHttp 真实请求 + 代理回退 + JSON 解析 + 版本比较 + about 页状态机；用户主动触发，非 stub |
| 收口 | SEC-3 ✅ COR-1 ✅ COR-3 ✅ | 暂缓项全部落地：删除弹窗 HWND 归属主窗口；遍历显式父子聚合；回收站清理区分「取消/失败」 |
| **总体** | **代码层面 100%** | **SPEC-v0.2 全部 P0–P3 项落地（A3 机制等价 🔄）；最终确认依赖实机走查（见观察项节）** |

### SPEC 终轮实际落地（C4 + D3，SPEC 清零）

- **C4 计划任务自启源**：windows crate 启用 `Win32_System_TaskScheduler` feature（零新依赖）；`task_items()` 走正统 COM（CoInitializeEx → CoCreateInstance 内联 CLSID_TASKSCHEDULER2 → Connect → 递归 GetFolders/GetTasks(TASK_ENUM_HIDDEN)），仅保留 Logon/Boot 触发任务，command 取首个 EXEC action；COM 顶层失败静默降级空 Vec、逐项失败跳过；`with_com_init` 配对助手三处复用（task_items/set_task_enabled/warm_icons，顺带消除 warm_icons 历史泄漏）；禁用走 `IRegisteredTask::SetEnabled` 纯 COM 两击确认，**永不触达 DisabledStartup 存储**；is_task 项不渲染删除按钮（SPEC 只读+禁用边界）；启动项页第四来源自然融入建议徽标/筛选/搜索。
- **D3 HTML 清理报告**：`core/report.rs` 五节快照（垃圾类别/大文件 Top30+文件夹 Top15/注册表发现/启动项全表+建议/服务 Recommend+Caution 两表）+ `build_report_html` 纯拼装可单测；单文件零外部资源（内联 CSS + prefers-color-scheme 双主题 + 系统字体栈 + 无 JS/外链，单测断言）；动态文本全量 `html_escape`（& 首替换防二次转义，注入样本单测）；设置页「报告」面板导出 + 分阶段进度 + `open_in_explorer` 定位；`Status` 增持久变体（完成/失败态常驻，`set()` 重置防永久吞）。
- 工程师修正设计文档两处 windows API 签名偏差（get_Item 收 `Param<VARIANT>`、Count/Path/Arguments 出参风格），QA 逐条对照 crate 源码确认修正正确。

### SPEC 收尾轮实际落地（C2/C3/C6 + 观察项收口）

- **C2 服务批量应用建议优化**：`Advice.suggested` 逐条赋目标类型（可禁用→4；SysMain/PcaSvc/WerSvc/WinHttpAutoProxySvc→3；WSearch→0 排除出批量）；`batch_candidates` 四条安全红线（仅 Recommend+suggested≠0、已达标排除、start_type∉{2,3,4} 绝不触碰、有 ServiceUndo 记录的排除——尊重用户手动操作）；批量确认→Running 后台串行逐项 set_start_type+record_original（幂等、逐项可行内还原）→Done 报告「成功 X/失败 Y+明细」；Running 期间刷新/筛选/行内按钮/搜索框全部守卫。
- **C3 服务依赖警告**：`active_dependents`（OpenServiceW SERVICE_ENUMERATE_DEPENDENTS + EnumDependentServicesW(SERVICE_ACTIVE) 双调用扩容，零新增 feature）；单条禁用统一两击（首击后台查依赖，gen+armed 双校验防串台，回包 (i,t) 精确匹配防迟到回包误清「检查中」）；确认文案列出运行中依赖；枚举失败降级「（未能枚举依赖，请谨慎）」绝不误判为「无依赖」；改手动不查。
- **C6 Windows 升级残留**：探测 Windows.old/$WINDOWS.~BT/$Windows.~WS（C:\ESD 明确不纳入）；默认不勾选 + 勾选红字警示「删除后无法回退到旧版 Windows，且不经过回收站」+ 两击「确认：永久删除 Windows 升级残留」；**强制直删连根移除**（回收站装不下几十 GB 且与释放空间目标冲突，安全由四层防护补足）；`dir_size_cancelled` 可取消变体支持数十 GB 扫描中断；完成后「已释放 X」对比。
- **CleanResult 口径修正**：删除 `recycled: bool`，新增 `recycled_bytes`——freed=已真正释放（直删+清空回收站），recycled_bytes=移入回收站待释放体积；完成文案双段并列「已永久释放 X + 移入回收站 Y（清空回收站后释放）」，混合勾选不再把回收站体积冒充已释放。单测断言同步加强（freed==0 等式断言）。
- **观察项收口**：csv_escape 公式注入防护（`=+-@\t\r` 开头加 `'` 前缀，OWASP 惯例）；sanitize_filename 处理 22 个 Windows 保留名（首点前 base 比对）与结尾点/空格；self_run_enabled 校验 Run 值指向自身（canonicalize same_file）；SearchInput 增 `enabled` 开关（批量 Running 期间禁用，IME 两通道全封堵）。

### SPEC 执行轮实际落地（B1/B4/B5/D2 + C1 + 收口）

- **SEC-3 HWND 归属**：新增 `raw-window-handle 0.6` 依赖（与 gpui 内部同版本，仅 trait 引用）；`controls::window_hwnd` 全限定 trait 调用取真 HWND 随 pump 闭包拷贝入后台线程；`SHFileOperationW`/`SHEmptyRecycleBinW` 弹窗归属主窗口（超容量确认框不再无归属）。
- **COR-3**：`RecycleOutcome{Done,Cancelled,Failed}` + `outcome_from` 纯函数，单文件与目录两分支三态一致；`fAnyOperationsAborted`/`E_ABORT` 区分用户取消与失败，文案相应区分。
- **COR-1**：bigfiles 遍历改 `Frame::Enter/Finish(parent)` + `child_sums: HashMap` 显式父子聚合，删除栈内反查与 seen 去重；新增三层嵌套聚合单测。
- **B5**：`WindowMemory`（WinX/Y/W/H/Max，负坐标副屏支持）启动恢复（校验 ≥900×600 且中心点可见，防拔副屏）；`on_window_should_close` 保存；设置页「启动」面板开机自启开关（HKCU Run 值 "MWCleaner"，带引号处理空格）；README 非目标措辞对齐「开机自启守护 ≠ 可选开关」。
- **D2**：`window_min_size` 900×600；侧栏 208↔48px 折叠（`Page::short_label` 两字缩写，收起态保留选中强调）。
- **B1**：新建 `ui/text_input.rs` SearchInput——`EntityInputHandler` 全 8 方法 + `window.handle_input` 挂载 + UTF-16↔UTF-8 偏移双向转换（surrogate pair 取整对称）+ IME 组合（marked text）+ WM_CHAR 直接键入 + 退格/Esc 键绑定；**完整支持中文输入法**；services/startup/uninstall 三页实时过滤接入。
- **B4**：`csv_escape/exports_dir/write_csv_lines`（UTF-8 BOM + CRLF，Excel 中文兼容）+ `timestamp_name`（civil_from_days，无新依赖）；大文件页排序 chips（大小↓/↑/名称）+ 全量导出 CSV + explorer 定位；卸载残留页导出 + 路径 A→Z 排序（index 副本渲染防勾选错位）。
- **C1**：`startup_advice` 27 条规则（Keep 16 前置防误伤 → Recommend 10 → Caution 1；"ime" 关键词词边界匹配防 Runtime 误报）；徽标 + 「建议优化/保持现状」筛选 chips，与服务页模式对称。

### 观察项（不阻断，后续迭代候选）

- ~~csv_escape 无公式注入防护~~ → 收尾轮已收口（OWASP `'` 前缀）。
- ~~self_run_enabled 不校验指向自身；sanitize_filename 保留名~~ → 收尾轮已收口。
- ~~warm_icons COM 初始化未配对~~ → 终轮已收口（`with_com_init` 助手三处统一）。
- IME 组合中失焦时组合串按已输入文本残留（V1 局限）。
- `timestamp_name` 按 UTC 折算（精确到分钟足够）。
- 验证环境缺口：`backup_restore_roundtrip` 需在 reg.exe 可执行环境跑通一次闭环（当前 48 用例 47 过，实机应 48/48）；`cargo test` 裸跑会因回收站弹窗挂死，必须 `timeout 180 -- --test-threads=1`。
- **实机走查（发布前最后一道人工验收，终轮新增 5 项）**：① taskschd.msc 对照登录/启动触发任务条数与来源（TimeTrigger 任务不出现）；② 禁用→msc 复核→启用恢复闭环；③ 禁用后刷新列表禁用项仍出现且可再启用（GetTasks 枚举契约）；④ HTML 断网打开 + 深浅色切换 + 特殊字符行无裸标记；⑤ 导出全程 UI 不冻结、阶段文案渐进、完成后定位文件。
- 实机走查（发布前最后一道人工验收）：服务批量确认文案与依赖清单实显、批量执行回显与逐项还原、真实 Windows.old 场景删除后类别消失、混合勾选双段文案。

### 本轮实际落地（核心稳妥范围）

- **T01 分层去耦**：`MODE_*` 常量上移 `core/settings.rs`，`ui/theme.rs` 反向引用 core，消除 `core → ui` 反向依赖；`expand_env/exe_path` 归位 `core/mod.rs`，`core/startup.rs` 保留 `pub use` 兼容；`main.rs` 改用 `core::settings::load_theme()` 初始化主题。
- **T02 安全网**：
  - SEC-1 大文件/文件夹删除改为**双选项**——两击确认后展开「移入回收站（可还原，默认高亮）」与「永久删除（不可恢复）」，回收站复用 `junk::move_to_recycle_bin`（FOF_ALLOWUNDO）；扩充黑名单至 `C:\Windows`、`C:\Program Files`、`C:\Program Files (x86)` 根目录及直接子目录、`$Recycle.Bin`、`System Volume Information`。
  - SEC-2 「回收站」类别默认取消勾选，勾选时确认按钮与状态栏显示「将永久清空回收站，不可恢复」强提示。
  - COR-2 启动项「删除」启用项文案改为「已移除并备份（可在已禁用中启用恢复）」。
- **T05 文档一致性**：README 补「检查更新」特性并把「零联网/任何联网一律不做」统一为「除用户在关于页主动检查更新外不联网、无遥测」；SPEC 追加本节。about 页文案原本清晰，未改动。
- **低风险优化**：PERF-1 启动项/卸载页首次加载改异步 pump；B6 状态消息 8s 自动淡出（`Status` + `start_status_ticker`）；CON-2 图标缓存改双重检查加锁，慢速 GDI extract 在锁外执行；COR-4 取消扫描状态标注「仅部分结果」。

### 上轮暂缓/不做（SPEC 执行轮已收口或维持不做）

- ~~B1 搜索框、B4 CSV 导出与排序开关、D2 窗口最小尺寸与侧栏折叠、B5 记住窗口位置、开机自启开关~~ → **SPEC 执行轮已全部落地**（见上节）。
- ~~COR-1（大文件遍历 Finish 帧可读性重构）~~ → 已落地（显式父子聚合 + 单测）。
- ~~COR-3（回收失败区分「用户取消」与「权限失败」）~~ → 已落地（`fAnyOperationsAborted` 可靠区分）。
- ~~SEC-3（删除弹窗传应用 HWND）~~ → 已落地（raw-window-handle 0.6 取真 HWND）。
- C2/C3/C4/C6、D3 HTML 报告：**维持不做**（C 类按 spec「选 0–1」已选 C1；C4/D3 为 L 级边缘项）。
- 任何新框架引入、pump 架构重写、`build.rs` 修改：不做。