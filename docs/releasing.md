# 发布流程

源码检查、GitHub 仓库建立和二进制公开发布是不同步骤。本项目的 CI 检查和构建产物上传不自动创建公开 Release。

公开仓库为 [YuVerseX/SubGauge](https://github.com/YuVerseX/SubGauge)，`main` 已推送，MIT、About 描述、topics 和私密漏洞报告已设置。首次及发布候选提交的远端 CI 均已通过，证据见验证记录。

[`v0.1.6` 预览版](https://github.com/YuVerseX/SubGauge/releases/tag/v0.1.6) 已于 2026-10-02 公开发布，固定指向 `e9897615223e0066648499c8c980582ab9748030`。四个附件已上传并下载回查，内容、服务器 digest、尺寸、说明和目标提交一致；后续纯文档提交不移动这个版本。

0.1.6 的产品说明、变更、附件名称和验证范围见 [发布说明](releases/0.1.6.md)。本地构建产物保留在 Git 忽略的 `release/`，后续版本公开二进制前按下文核对对应验收和第三方声明。

## 源码维护

当前项目已重新初始化为独立的 `main` 历史，旧 Git 备份位于项目目录外；旧导出包和临时验证产物已清理。后续直接从当前项目目录连接 GitHub，不需要额外导出源码包。

上传前检查 `git status`、`git log --all` 和 `git ls-files`：只提交源码、正式文档和必要资源，不包含用户配置、凭据、分发文件、依赖缓存或构建输出。截图与问题报告使用示例数据。

首次提交使用通用项目署名。关联个人 GitHub 账号时，在仓库中设置自己的署名与 GitHub noreply 邮箱；不要直接复制内部仓库的个人邮箱或其他本地配置。

后续正常提交并推送 `main`，不重新初始化历史或强推覆盖。更换远端或仓库可见性前确认准确目标；上传后核对远端提交与 CI 结果。

## 二进制候选版本

1. 对齐 npm、Cargo、Tauri 版本与变更说明，运行 `npm ci`、`npm run verify`。
2. 运行 `npm run package`，随后 `npm run package:portable`。核对 EXE、安装器的内部版本、ZIP 内容及 SHA256。
3. 按 [验收清单](acceptance-plan.md) 验证受本次修改影响的桌面、同步和升级行为，将结果写入 [验证记录](validation.md)。生成安装包本身不代表安装或升级通过。
4. 核对许可证、所分发第三方依赖声明、使用说明及截图。公开材料仅使用示例账号；不上传原始站点报告或用户配置。

安装、卸载和缺失 WebView2 场景使用独立测试环境。升级前通过托盘正常退出所有实例；结束进程不计作正常退出验证。两种分发共享设置，卸载清除数据会影响免安装版。

同版本重复打包可能产生不同 Hash，需要标明候选产物，不能沿用旧包的安装结论。仅改变公开文档时也不应将旧二进制描述为包含未构建的源码修改。

## 实际上传与发布

首次远端 CI 成功后才记录为 CI 通过。CI 使用指定的 Windows runner 与工具链；runner 镜像、浏览器和工具链仍需在日志中核对。失败时下载示例布局报告排查，不使用真实账号日志。

发布说明列出版本变化、运行依赖、签名状态、已验证系统及重要未测试项，并附安装版、免安装 ZIP 和 SHA256。实际推送、打 Tag 或创建公开 Release 前确认目标仓库和可见性；准备工作不会自动执行这些动作。

## 软件发布方式

软件使用 GitHub Releases 分发，源码留在 `main`。首次软件版本标记为 pre-release，上传四个明确附件：安装包、免安装 ZIP，以及各自的 `.sha256`。两种包都应包含使用说明与三份法律声明，GitHub 自动生成的 Source code ZIP 不等于可直接运行的软件。

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
