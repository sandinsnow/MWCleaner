# MWCleaner

> Windows 系统清理与优化工具。**Rust + gpui** 原生渲染，免安装**单文件 exe**，纯本地、离线、零遥测。

[![Release](https://github.com/sandinsnow/MWCleaner/actions/workflows/release.yml/badge.svg)](https://github.com/sandinsnow/MWCleaner/actions/workflows/release.yml)

MWCleaner 是一款 **Windows 桌面、纯 Rust**（gpui 0.2 原生 GPU 渲染，无 WebView、无 Electron）的系统清理与优化套件：垃圾清理、启动项管理、服务优化、注册表清理、大文件分析、软件卸载，六合一。所有操作都在本机完成，不联网、不上传任何数据。

**安全优先**是它的第一设计原则：破坏性操作一律两击确认；垃圾默认移入回收站而非直删；注册表清理前自动导出 `.reg` 备份；服务启动类型修改记录原值、一键还原；软件卸载走官方卸载器，退出后再定向扫描残留、人工确认才清理。

---

## 特性

- **垃圾清理**：系统临时文件、浏览器缓存、缩略图缓存等分类扫描，勾选后清理；默认**移入回收站**（可在设置关闭改为直删）；失败/被占用项保留在磁盘并计入失败数，绝不静默直删；扫描可随时取消。
- **启动项管理**：枚举注册表 Run 键与启动文件夹，显示名称/命令/来源；启用、禁用、删除（两击确认）；**全部 / 已启用 / 已禁用**状态筛选。
- **服务优化**：全量服务列表，中文显示名与描述；按 **建议优化 / 保持现状 / 不建议优化** 三分类给出建议徽标与筛选；可改启动类型（自动/手动/禁用），非建议类禁用需两击确认；**修改自动记录原值，行内「还原」按钮一键回滚**，重启后记录仍在；叠加 **全部状态 / 运行中 / 已停止** 状态筛选。
- **注册表清理**：扫描无效/残留键值，清理前自动导出 UTF-16 `.reg` 备份到 `%APPDATA%\MWCleaner\backups`；页内「打开备份目录」直达，还原走系统双击导入；扫描可取消。
- **大文件分析**：**磁盘卡片**展示各盘卷标、用量百分比与用量条（>75% 黄、>90% 红）、已用/总量/剩余；按盘符扫描，大文件阈值 **50 / 100 / 500MB 可配置**（设置页），大文件夹 ≥ 1GB；结果按大小列出，扫描可取消。
- **软件卸载**：枚举系统已安装软件，显示**软件图标**、名称、版本、发布者、占用大小；两击确认后启动**官方卸载器**，等待其退出后自动定向扫描该软件残留（安装目录、AppData/ProgramData 匹配目录、卸载键与厂商/产品注册表键），列表勾选后清理——文件走回收站、注册表先备份；卸载未完成时自动返回不做清理。
- **设置与主题**：深/浅色主题切换（精修双调色板）、回收站开关、大文件阈值，均持久化到注册表。
- **图形化界面**：分组侧栏（清理/优化/管理）+ 品牌标识 + 选中强调条，分类徽标柔和底色，状态筛选 chips，服务/启动项/软件图标获取显示。
- **便携单文件**：约 6MB 免安装 exe，crt 静态链接，无运行时依赖，放任意目录双击即用。

---

## 下载使用（Windows）

1. 到 [Releases](https://github.com/sandinsnow/MWCleaner/releases) 下载最新的 `MWCleaner.exe`（附 `.sha256` 校验文件）。
2. 放到任意目录，双击运行。无需安装。
3. 首次使用建议顺序：**垃圾清理** 扫描 → 勾选类别 → 清理（默认进回收站，可随时还原）；**服务优化** 先看「建议优化」分类，改动后如不满意用行内「还原」回滚。

> **运行前提**：Windows 10 1903+ / Windows 11，x64。应用需要**管理员权限**（服务与 HKLM 注册表操作），启动时会弹 UAC 确认。
>
> **未签名提示**：exe 未做代码签名，首次运行可能触发 SmartScreen 警告，选择「更多信息 → 仍要运行」即可。

---

## 从源码构建

### 环境要求

- **Windows 10/11**
- **Rust**（MSVC 工具链，host `x86_64-pc-windows-msvc`）
- **Windows SDK**（提供 `mt.exe`，用于注入应用 manifest）

### 命令

```bash
# 开发运行
cargo run

# 运行测试（垃圾/注册表模块单元测试，使用自建临时测试件，自动清理）
cargo test

# 构建 release
cargo build --release
```

### 注入 manifest（发布必需）

gpui 依赖 Common-Controls v6，且应用需要管理员权限——cargo 产物必须用 SDK 的 `mt.exe` 注入 [`assets/app.manifest`](assets/app.manifest) 后才能分发：

```bash
mt.exe -nologo -manifest assets/app.manifest \
  "-outputresource:target/release/mwcleaner.exe;#1"
```

产物 `target/release/mwcleaner.exe` 重命名为 `MWCleaner.exe` 即为分发单文件。`.cargo/config.toml` 已配置 `crt-static`，无需额外运行时。

---

## 发布流程

推送 `v*` 标签（如 `v0.1.0`）即触发 [`.github/workflows/release.yml`](.github/workflows/release.yml)：在 `windows-latest` 上 `cargo build --release --locked`、用 Windows SDK `mt.exe` 注入管理员 manifest 并回读校验、计算 SHA256，把 `MWCleaner.exe` 与 `MWCleaner.exe.sha256` 发布到 GitHub Releases。

---

## 项目结构

```
src/
  core/               # 业务核心：junk / startup / services / registry / bigfiles / uninstall / settings
  ui/                 # gpui 页面：六功能页 + 设置页 + 主题 + 图标获取
assets/
  app.manifest        # requireAdministrator + Common-Controls v6 + DPI 感知
  icon.ico / icon.png # 应用图标
build.rs              # winresource 嵌入图标与版本信息
.cargo/config.toml    # crt-static 静态链接
SPEC-v0.2.md          # v0.2 调整规格（安全网 + 体验项）
```

---

## 技术栈

| 层 | 选型 |
|---|---|
| UI 框架 | gpui 0.2（Zed 编辑器同款，原生 GPU 渲染） |
| 系统 API | windows-rs 0.58（服务/Shell/注册表/文件系统） |
| 注册表 | winreg 0.55 |
| 异步运行时 | smol 2 |
| 图标解码 | image 0.25 |
| 资源嵌入 | winresource（图标 + 版本信息） |

---

## 明确的非目标

为坚守「单文件、纯本地、真实有效」，以下能力**一律不做**：

- ❌ 托盘常驻 / 开机自启守护
- ❌ 自动更新（手动下载新版替换即可）
- ❌ 内存加速、注册表碎片整理等「伪优化」
- ❌ 驱动级清理 / 系统文件替换
- ❌ 任何联网行为与遥测

跨平台不在计划内（gpui 的 Windows 剪贴板/COM 面深度绑定 Win32）。

---

## 许可证

本项目基于 [MIT 许可证](LICENSE) 开源。