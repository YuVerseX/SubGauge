# 发布流程

源码检查、GitHub 仓库建立和二进制公开发布是不同步骤。本项目的 CI 检查和构建产物上传不自动创建公开 Release。

公开仓库为 [YuVerseX/SubGauge](https://github.com/YuVerseX/SubGauge)，`main` 已推送，MIT、About 描述、topics 和私密漏洞报告已设置。首次及发布候选提交的远端 CI 均已通过，证据见验证记录。

[`v0.1.6` 预览版](https://github.com/YuVerseX/SubGauge/releases/tag/v0.1.6) 已于 2026-10-02 公开发布，固定指向 `e9897615223e0066648499c8c980582ab9748030`。四个附件已上传并下载回查，内容、服务器 digest、尺寸、说明和目标提交一致；后续纯文档提交不移动这个版本。

0.1.6 的产品说明、变更、附件名称和验证范围见 [发布说明](releases/0.1.6.md)。本地构建产物保留在 Git 忽略的 `release/`，后续版本公开二进制前按下文核对对应验收和第三方声明。

[v0.1.8 预览版](https://github.com/YuVerseX/SubGauge/releases/tag/v0.1.8) 已于 2026-10-03 公开，固定二进制源码为 `704fc14e526161eb408ee402c819f9e7e2077098`；该提交的 [Windows CI](https://github.com/YuVerseX/SubGauge/actions/runs/37123263193) 通过。六个正式附件上传并匿名下载回查后，更新清单才以 `6a4524b` 提交上线；没有覆盖 0.1.6。证据与尚未验收项见 [验证记录](validation.md)。

[v0.1.9 预览版](https://github.com/YuVerseX/SubGauge/releases/tag/v0.1.9) 已于 2026-10-04 公开，固定二进制源码为 `26d6da401aebe4143285d3aae9d7c4453ad166ae`；其 [Windows CI](https://github.com/YuVerseX/SubGauge/actions/runs/37197619149) 全部通过。六个附件公开下载回查及验签通过后，更新清单以 `2e045b8` 提交上线，公共 HTTPS 清单与本地一致。本轮公共原生插件探针执行被自动审批拒绝，明确列为未执行；未替换日常安装版。

[v0.1.10 预览版](https://github.com/YuVerseX/SubGauge/releases/tag/v0.1.10) 已于 2026-10-08 公开，固定二进制源码为 `0e3562840de15decdc47470fd46b331c129095a1`；其 [Windows CI](https://github.com/YuVerseX/SubGauge/actions/runs/37745922155) 全部通过。六个正式附件从草稿及公共地址回查一致，生产更新签名验证通过；清单以 `63e93fd` 提交上线，固定公共 HTTPS 地址完整内容与本地一致。用户现有安装程序文件与 0.1.10 发布包一致，本轮未执行安装升级或公共原生插件探针，详见 [验证记录](validation.md)。

## 源码维护

当前项目已重新初始化为独立的 `main` 历史，旧 Git 备份位于项目目录外；旧导出包和临时验证产物已清理。后续直接从当前项目目录连接 GitHub，不需要额外导出源码包。

上传前检查 `git status`、`git log --all` 和 `git ls-files`：只提交源码、正式文档和必要资源，不包含用户配置、凭据、分发文件、依赖缓存或构建输出。截图与问题报告使用示例数据。

首次提交使用通用项目署名。关联个人 GitHub 账号时，在仓库中设置自己的署名与 GitHub noreply 邮箱；不要直接复制内部仓库的个人邮箱或其他本地配置。

后续正常提交并推送 `main`，不重新初始化历史或强推覆盖。更换远端或仓库可见性前确认准确目标；上传后核对远端提交与 CI 结果。

## 二进制候选版本

1. 对齐 npm、Cargo、Tauri 版本与变更说明，运行 `npm ci`、`npm run verify`。
2. 运行 `npm run package`，随后 `npm run package:portable`、`npm run package:update-manifest`。核对 EXE、安装器的内部版本、ZIP 内容、更新签名及 SHA256。
3. 按 [验收清单](acceptance-plan.md) 验证受本次修改影响的桌面、同步和升级行为，将结果写入 [验证记录](validation.md)。生成安装包本身不代表安装或升级通过。
4. 核对许可证、所分发第三方依赖声明、使用说明及截图。公开材料仅使用示例账号；不上传原始站点报告或用户配置。

安装、卸载和缺失 WebView2 场景使用独立测试环境。升级前通过托盘正常退出所有实例；结束进程不计作正常退出验证。两种分发共享设置，卸载清除数据会影响免安装版。

同版本重复打包可能产生不同 Hash，需要标明候选产物，不能沿用旧包的安装结论。仅改变公开文档时也不应将旧二进制描述为包含未构建的源码修改。

## 实际上传与发布

首次远端 CI 成功后才记录为 CI 通过。CI 使用指定的 Windows runner 与工具链；runner 镜像、浏览器和工具链仍需在日志中核对。失败时下载示例布局报告排查，不使用真实账号日志。

发布说明列出版本变化、运行依赖、签名状态、已验证系统及重要未测试项，并附安装版、免安装 ZIP 和 SHA256。实际推送、打 Tag 或创建公开 Release 前确认目标仓库和可见性；准备工作不会自动执行这些动作。

## 软件发布方式

软件使用 GitHub Releases 分发，源码留在 `main`。0.1.6 预览版有四个附件；0.1.8 起有六个：安装包、对应 `.sig`、免安装 ZIP，以及三者的 `.sha256`。两种包都应包含使用说明与三份法律声明，GitHub 自动生成的 Source code ZIP 不等于可直接运行的软件。

先对完成本地构建的准确提交创建草稿，核对附件名称、下载后 Hash、Release 目标提交和实际 CI 结果，再公开发布。不要通过推送到 `main` 自动发布，也不要把之后变动的分支头误作为已有二进制的来源。

```powershell
# 首次创建示例；v0.1.6 已发布，不要重复执行或覆盖既有附件
# 构建、核对并推送源码后，固定对应候选版本的提交
$releaseCommit = git rev-parse HEAD
gh release create v0.1.6 --repo YuVerseX/SubGauge --target $releaseCommit --draft --prerelease --latest=false --title 'SubGauge 0.1.6 · Windows x64 预览版' --notes-file docs/releases/0.1.6.md release/SubGauge_0.1.6_x64-setup.exe release/SubGauge-0.1.6-windows-x64.zip release/SubGauge_0.1.6_x64-setup.exe.sha256 release/SubGauge-0.1.6-windows-x64.zip.sha256
gh release view v0.1.6 --repo YuVerseX/SubGauge
```

已有同名草稿时先读取并核对，不重复创建或默认覆盖附件。完成最终确认后才取消 draft；首次发布保留 pre-release 标记，不默认设为稳定版 Latest。公开发布后更新 README 的下载状态和版本说明。

草稿完成最终确认后，可在 Releases 的编辑页选择 Publish release，或按下列示例执行；这一步会让附件对所有人可见。`v0.1.6` 已完成发布，后续使用对应版本号。

```powershell
gh release edit v0.1.6 --repo YuVerseX/SubGauge --draft=false --prerelease=true --latest=false
```

操作依据：[GitHub 管理 Releases](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository)、[GitHub CLI release create](https://cli.github.com/manual/gh_release_create)、[Tauri Windows 分发](https://v2.tauri.app/distribute/windows-installer/)。

## 0.1.8 签名与更新渠道

更新采用 [Tauri updater](https://v2.tauri.app/plugin/updater/) 的 Minisign 格式，独立于 Windows Authenticode 证书签名。正式公钥固定在源码，私钥仅存仓库外；初始化、构建及备份边界见 [开发文档](development.md)。已有公钥时不得静默生成替代身份，否则已安装客户端无法验证后续更新。

普通 CI 不持有正式私钥，使用临时身份构建 `validation-only` 产物；这些产物不能上传为正式 Release，也不能用作更新渠道。正式包由维护者身份构建，`package` 和清单工具验证签名、可信版本与内部产品版本。不要将生成签名时的私钥、密码或完整命令输出上传到日志。

预览渠道固定为 `https://raw.githubusercontent.com/YuVerseX/SubGauge/main/updates/preview.json`，不依赖排除 pre-release 的 Latest。`npm run package:update-manifest` 只生成 `release/update-preview.json` 待审文件，不修改活跃渠道。

发布顺序：

1. 完成候选检查，固定确切源码提交，以正式身份构建六个附件和待发布清单。
2. 对对应版本创建 pre-release 草稿，上传六个附件；核对源码提交、下载文件、Hash、签名和公告版本后公开。`v0.1.8` 已发布，后续不得覆盖这些附件。
3. 附件已公开可下载后，再将核对过的清单复制为 `updates/preview.json`，单独提交并推送。不得先上线指向草稿、缺失或未经验证附件的清单。
4. 从客户端核对公共检查、下载与安装流程；记录真实 SubGauge 升级结果。公共渠道未测试前不写通过。

0.1.6 的首次升级仍手动安装。安装器启动后的故障不保证自动回滚，发布页保留手动安装方式。候选清单中的 URL 是预期公开地址，生成成功不代表这些地址已经可访问。关闭自动检查或 GitHub 无法连接时，仍可从 Releases 手动下载。

二进制源码的完整 CI 在固定提交通过后，清单及后续纯发布文档提交可使用 `[skip ci]`，不重复构建相同代码。清单仍须单独核对公共 HTTPS 内容与附件签名；原生插件及实际安装结果按各版本的验证记录说明，未执行项不能记为通过。这不表示发布文档是原二进制源码，也不改变已有 Tag。跳过方式依据 [GitHub 官方文档](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/skip-workflow-runs)，不能用于未经检查的代码修改。
