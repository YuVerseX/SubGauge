# 开发与打包

## 环境与依赖

开发目标为 Windows x64。需要 Node.js 24、Rust MSVC 1.90 或以上、Microsoft C++ Build Tools 的 x64 C++ 工具、Windows SDK 和 WebView2。

Rust 最低声明依据锁定依赖的 MSRV；本机实测版本为 1.99.0，不表示已在 1.90 实际构建。运行应用只需 Windows 和 WebView2，不需 Node、Rust 或 C++ 编译工具。Windows 11 的实际兼容验证尚未完成。

前端依赖精确版本写入 `package.json`，安装使用 `npm ci` 与 `package-lock.json`。Rust 依赖由 `Cargo.lock` 锁定，检查、测试和 Release 构建使用 `--locked`；升级依赖需单独验证，不通过日常构建隐式更新。

`scripts/desktop.ps1` 为当前子进程补入用户 `.cargo/bin`，不修改系统 PATH。PowerShell 5.1 可运行现有脚本；脚本代码保持 ASCII，配置明确按 UTF-8 读取，中文说明存放于 UTF-8 Markdown。系统编译工具安装可能需要 Windows UAC。

## 目录职责

| 路径 | 内容 |
| --- | --- |
| `src/` | Vue 界面、类型、原生适配、显式示例与样式 |
| `src-tauri/src/` | Rust 原生核心、窗口与回归测试 |
| `src-tauri/capabilities/` | 本地浮窗和详情权限 |
| `src-tauri/icons/` | 配置引用的 Windows 图标 |
| `app-icon.svg` | 图标原始矢量资源 |
| `scripts/` | 桌面命令、免安装打包、资源观察 |
| `.github/workflows/windows.yml` | 自动检查、构建及上传构建产物，不自动公开发布 |
| `docs/` | 产品、接入、架构、使用、验收与发布说明；截图只使用示例数据 |
| `node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/` | 依赖与生成输出，Git 忽略 |
| `release/` | 本地安装包、ZIP、校验文件与资源观察，Git 忽略 |

原生职责细分见 [implementation-plan.md](implementation-plan.md)。当前应用没有全量历史数据库、自动更新或远端同步。

## 启动与预览

```powershell
npm ci
npm run desktop
```

真实登录在桌面应用里完成。浏览器预览使用：

```powershell
npm run dev
# http://127.0.0.1:1420/?demo=1
```

示例必须显式开启，浏览器不接收真实登录。Vite 忽略 Rust 和分发输出目录，避免构建时监听锁定文件及误重载。

## 自动检查

```powershell
# 完整本地检查，任一失败即停止
npm run verify
# 或分别执行
npm run typecheck
npm run build
npm run test:layout
npm run check:rust
npm run fmt:rust
npm run lint:rust
npm run test:rust
```

`build` 包含 Vue/TypeScript 类型检查与生产前端构建。Rust lint 为 `cargo clippy --locked --lib --tests -- -D warnings`，格式命令只检查、不改写文件。业务回归覆盖时间、费用、缓存率、分页、会话与旧响应隔离。

`test:layout` 使用锁定的 Playwright、无头 Microsoft Edge 和 Tauri 官方 IPC 模拟运行实际 Vue 页面，只使用示例数据，不操作桌面鼠标。它检查卡片测量、展开/收起、动态内容、菜单与较矮工作区；不能替代 Windows 原生 DPI 验收。脚本自动启动并关闭临时 Vite 服务，报告和截图位于忽略目录 `release/validation/`。本机须已安装 Edge，也可用 `SUBGAUGE_BROWSER_CHANNEL=chromium` 选择已安装的 Playwright Chromium。

CI 运行相同检查入口，并生成、上传安装包、ZIP 与 SHA256 文件。工作流指定 Windows Server 2022、Node.js 24.20.0 和 Rust 1.99.0，显式安装锁定 Playwright 所需 Chromium；本机布局默认使用 Edge。失败时也尝试保存示例布局报告与截图。runner 镜像仍可能更新；工作流尚未远端执行，不能将本机通过等同于 CI 通过，也不能视为 Windows 10/11 桌面兼容验收。

不为静态文案添加永久测试；关键业务规则或公共接口变更应保留有回归价值的测试。临时探针完成后清理。不要提交真实凭据、完整私有响应、用户配置、登录缓存或本机观察 CSV。

## 打包顺序与产物

```powershell
# 1. 构建 Release EXE 与 NSIS 安装包
npm run package
# 2. 从已有 Release 制作免安装 ZIP、复制安装包、生成校验文件
npm run package:portable
```

`package:portable` 不执行编译，须先成功构建当前源码。它核对 npm、Cargo、Tauri 版本，以及 Release EXE 和安装器内部的 `ProductVersion`；ZIP 只收录 `SubGauge.exe` 和使用说明 `README.md`，不收录 staging 目录残留文件、用户配置或验证数据。

| 产物 | 位置 |
| --- | --- |
| Release EXE | `src-tauri/target/release/subgauge.exe` |
| 原始 NSIS | `src-tauri/target/release/bundle/nsis/SubGauge_<version>_x64-setup.exe` |
| 分发 NSIS | `release/SubGauge_<version>_x64-setup.exe` |
| 解压目录 | `release/SubGauge-<version>-windows-x64/` |
| 免安装 ZIP | `release/SubGauge-<version>-windows-x64.zip` |
| 校验文件 | 分发安装包、ZIP 同路径加 `.sha256` |

Tauri 打包会修改 EXE 的 bundle 标识，安装目录 EXE 与打包前 EXE 可能有不同哈希；应分别识别安装包与 ZIP，不能据此判断会话或统计不同。

现有分发为未签名 Windows x64 当前用户安装。两种版本使用相同 `app.subgauge.desktop` 标识、配置目录与单实例机制，升级保留配置。免安装不意味着会话能跨 Windows 用户或跨电脑迁移。

生成构建文件不代表已经完成安装、跨系统或桌面验收。发布前以 [validation.md](validation.md) 核对适用版本、测试范围和未执行项，并按 [发布准备](releasing.md) 检查公开源码及完整历史。

## 用户数据与安全边界

应用数据在 `%LOCALAPPDATA%\app.subgauge.desktop`，与工作区生成目录分离。`accounts.v1.json` 为版本化配置与 DPAPI 加密会话，`window.json` 保存窗口位置及两态逻辑尺寸；不得将这些文件用于示例夹具或打包。

配置损坏或版本未知时保留原文件并报告错误；会话解密失败时要求该账号重新登录。DPAPI 为当前用户保护，不能宣称阻止以相同 Windows 用户身份运行的程序访问。密码不保存，前端状态不持有访问或刷新令牌，错误不回显认证载荷。

## 真实与桌面验证

真实核对使用相同账号、时区和范围；活跃日期说明采样时间差，已结束日期可避免正常新增请求造成差异。管理员与普通用户分别验证个人接口，不能把管理员全站统计与个人用量比较。

桌面验收包含拖动、菜单边界、100%～200% DPI、多屏、展开收起、详情释放、托盘、正常退出与至少一小时运行。浏览器示例通过不能替代原生操作，实际限制逐项记录。

Release 运行后可观察自身及 WebView2 子进程：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/observe-resources.ps1 -ApplicationProcessId <SubGauge进程ID> -Minutes 60
```

CSV 仅包含时间、CPU、内存与进程数，默认输出至 `release/validation/`；文件存在则拒绝覆盖。`CpuOneCorePercent` 以一个逻辑核为 100%，`CpuMachinePercent` 再除以逻辑处理器数。首次采样和新进程的首个间隔不计算 CPU 增量；进程退出时结束观察。

记录必须说明被测版本、是否纯空闲、观察总时长、有效 CPU 段与内存趋势。进程树工作集可能重复计算共享页，不能直接视为独占内存。持续运行、CPU 忙循环与长期内存稳定性分别评价，未测项不记作通过。
