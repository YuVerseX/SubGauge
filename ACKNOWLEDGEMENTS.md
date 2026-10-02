# 来源与致谢

SubGauge 的产品研究参考以下项目。源码研究固定在所列版本，协议与权限结论见 [接入研究](docs/integration-notes.md)。

| 项目 | 参考内容 | 所研究版本的许可证 |
| --- | --- | --- |
| [chopperH0824/sub2api-usage-float](https://github.com/chopperH0824/sub2api-usage-float/tree/cbabadecb85955331ab9085bf243c1b8da91792c) | 邮箱登录、悬浮窗和安全存储的设计思路 | [MIT](https://github.com/chopperH0824/sub2api-usage-float/blob/cbabadecb85955331ab9085bf243c1b8da91792c/LICENSE)，Copyright © 2026 Qiangbin Hu |
| [Wei-Shaw/sub2api](https://github.com/Wei-Shaw/sub2api/tree/d6adebd22de00478cd021119ba755f37bcb94fb5) | 个人 HTTP 接口、登录流程、统计字段与时区语义 | [LGPL-3.0](https://github.com/Wei-Shaw/sub2api/blob/d6adebd22de00478cd021119ba755f37bcb94fb5/LICENSE) |

SubGauge 采用 Tauri、Vue、Lucide、Reqwest、Tokio 等开源依赖。依赖及其传递依赖由 `package-lock.json` 和 `src-tauri/Cargo.lock` 记录；其许可证由各自项目决定。本项目的 MIT 许可证不替代第三方许可证。发布二进制时应核对所分发依赖的授权与声明要求。

分发提供项目 `LICENSE`、生成的 `THIRD-PARTY-NOTICES.txt` 和 `RUST-STANDARD-LIBRARY-NOTICES.html`。生成范围包括七个前端运行包、已锁定 Windows Rust normal/build 候选依赖及实际使用的 Microsoft WebView2 Loader；声明区分运行候选与宿主构建，不将所有依赖都称为编入二进制。固定补充文件、来源及维护方式见 [分发声明来源](licenses/README.md)。
