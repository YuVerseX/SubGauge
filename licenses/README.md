# 分发声明来源

本目录仅保留已审查的许可证补充文本与来源索引，不包含第三方实现代码。完整分发声明由 `npm run notices` 生成到 Git 忽略的 `release/legal/`，随安装版和 ZIP 一起提供。

## 收集范围

- 前端为当前构建使用的七个运行包：Vue、四个 Vue runtime 包、Tauri API 和 Lucide Vue；完整保留 Lucide 文件中的 Feather MIT 声明。
- Rust 按锁定的 Windows x64 `normal,build` 依赖树收集，含宿主端代码生成；以排除 proc-macro 的 normal 树标记运行候选，不声称所有列出的包都编入 EXE。
- 递归保留依赖包中的 LICENSE、COPYING、COPYRIGHT 与 NOTICE；包括 ring 的嵌套声明。原始文本按内容去重，保留每个来源与版本。
- 列出准确版本的 crates.io 源码下载地址，包括 MPL-2.0 包；当前未修改这些依赖的源码。

## 固定补充文件

`overrides.json` 记录固定版本来源、上游原始 SHA256 与本地文件 SHA256。文本不翻译；部分换行归一为 LF，因此两种 SHA256 可能不同。

alloc-stdlib 和 webview2-com 系列的 crate 包未包含完整许可证文件，补充来自 `.cargo_vcs_info.json` 对应固定提交。selectors 的固定源码头声明 MPL-2.0，但该版本包和固定仓库根没有 LICENSE；本目录使用已锁定 cssparser 上游的完整 MPL-2.0 文本，明确区分许可证文本来源与 selectors 源码来源。

Microsoft WebView2 SDK 1.0.3800.47 的 LICENSE 和 NOTICE 来自官方 NuGet 包，安装包内 x64 静态 Loader 与依赖中实际使用的库已按 SHA256 核对。生成器每次重新核对 Loader；版本或内容变化时要求重新审查。crate 的 MIT 声明不替代 Microsoft SDK 声明。

Rust 标准库声明使用当前 `rustc --print sysroot` 下的 `share/doc/rust/COPYRIGHT-library.html`，生成时记录工具链和文件 SHA256，单独随包提供；不把包含 LLVM 等组件的完整编译器声明混为运行依赖。缺少该文件时停止生成，补齐相应官方工具链材料后再试。

## 维护

增加前端依赖、改用本地或非 crates.io Rust 依赖、更新缺失文本的包或预编译 Loader 时，先复核收集范围与固定来源。生成器发现未审查的依赖、缺失文本或校验不符会失败；打包流程不得静默跳过。

这些材料用于记录分发来源和保留声明，不代表完整法律认证。生成文件和输入均在打包时核对，开发与发布命令见 [开发说明](../docs/development.md)。
