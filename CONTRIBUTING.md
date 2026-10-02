# 参与开发

先阅读 [产品规则](docs/product-design.md)、[架构](docs/implementation-plan.md) 和 [开发说明](docs/development.md)。开发目标为 Windows x64；依赖安装使用 `npm ci`，Rust 命令使用锁文件。

## 修改与验证

提交应说明具体触发条件、修改后的行为及验证方法。保持账号、范围和请求代次隔离，不让真实连接失败回退成示例数字。

```powershell
npm ci
npm run verify
```

影响原生窗口、登录或分发时，还需完成对应的 [验收项目](docs/acceptance-plan.md)。只执行过浏览器示例时，不将结果描述为实际 Windows 鼠标、DPI 或站点兼容通过。GitHub Actions 的构建产物不自动公开发布。

优先复用现有回归；统计、同步、会话及窗口状态的修复应保留有价值的测试。临时脚本和生成文件不要提交。不要混入无关依赖升级或大范围格式修改。

## 报告问题

提供版本、Windows 版本、实际显示缩放、安装方式、复现步骤、预期与实际结果。截图和日志先脱敏；不要上传账号配置、真实接口响应、邮箱、私有站点地址、密码、验证码、令牌或 Cookie。

涉及凭据或跨账号数据暴露的问题，按 [安全说明](SECURITY.md) 处理，避免公开敏感复现材料。

贡献采用本项目的 [MIT 许可证](LICENSE)。发布流程见 [发布准备](docs/releasing.md)。
