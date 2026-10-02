# 源码研究与接入约束

源码研究日期：2026-10-01；整理日期：2026-10-02。以下结论对应固定检出的源码版本；实际部署结果单独记录在 [validation.md](validation.md)，不与源码推断混写。

## 研究版本

- 参考浮窗：[chopperH0824/sub2api-usage-float](https://github.com/chopperH0824/sub2api-usage-float/tree/cbabadecb85955331ab9085bf243c1b8da91792c)
- Sub2API：[Wei-Shaw/sub2api](https://github.com/Wei-Shaw/sub2api/tree/d6adebd22de00478cd021119ba755f37bcb94fb5)

## 已验证事实

参考浮窗在 `src/main/sub2api-client.ts` 使用 `/auth/login`、`/auth/login/2fa`、`/auth/refresh`，并在 `acceptTokens` 中拒绝非管理员。这个限制是参考应用的产品选择，不能推导为普通用户无法通过邮箱登录查看个人用量。

参考浮窗的 `src/main/store.ts` 使用 Electron `safeStorage` 加解密秘密数据。SubGauge 保留安全存储会话的思路，实际采用 Tauri 原生核心与用户范围 DPAPI。

Sub2API 个人接口见 `frontend/src/api/user.ts`、`frontend/src/api/usage.ts`：

| 相对 API 路径 | 用途 |
| --- | --- |
| `/user/profile` | 当前用户资料、余额 |
| `/usage` | 当前用户用量明细及分页 |
| `/usage/:id` | 单笔详情 |
| `/usage/stats` | 用量汇总 |
| `/usage/dashboard/stats` | 个人面板统计 |
| `/usage/dashboard/trend` | 趋势 |
| `/usage/dashboard/models` | 模型分布 |

SubGauge 接受站点根地址或以 `/api/v1` 结尾的地址，去掉该后缀后统一添加 `/api/v1`。子路径部署保留子路径；禁止在地址中携带凭据、查询参数或片段。实际接口仍以部署权限为准。

`backend/internal/handler/usage_handler.go` 的日期参数按 `YYYY-MM-DD` 解析，带 `timezone`；结束日期转换为次日边界。其 `period=month` 的起点使用 `now.AddDate(0, -1, 0)`，所以不能拿它实现产品中的“本月”。本月必须显式使用自然月月初日期，并统一截止时间。

分钟级窗口不能仅依赖日期筛选；需要覆盖时间窗口的记录分页并按时间戳筛选，或者在部署确有支持时使用等价的精确范围能力。必须验证完整覆盖，不能只取最新一页。滚动 168 小时同样需检查服务端的时间和时区语义。

`frontend/src/components/charts/TokenUsageTrend.vue` 的缓存命中率分母为普通输入、缓存读取和缓存写入之和。`backend/internal/repository/usage_log_repo_dashboard.go` 的个人汇总总 Token 包含输入、输出、缓存写入和缓存读取四项。

部分面板组件采用的 Token 简算与完整汇总不同，不能照搬所有组件。原始供应商输入量的缓存处理需看服务端归一化逻辑；不要再对已归一化字段重复扣除缓存。

## 设计推断与待验证项

基于个人接口，多账号切换足以支撑“自建站自己的用量 + 上游普通账号用量与余额”的主流程；目前没有必要为套餐账期增加专属模型。

管理员和普通用户在两个部署上的登录、个人汇总、分页及余额已有实测证据；已结束日期的费用和四分项 Token 分别与完整明细核对一致。此样本不证明所有 Sub2API 版本或定制站点兼容。以下项目仍需在相应部署核对：

- 两个站点的版本、二次验证、验证码或登录策略差异。
- 上游定制余额字段、结转行为及更新延迟；以返回值展示，不自行解释账务规则。
- 新部署的 `actual_cost`、Token 四分项与时间戳语义。
- 各汇总接口能否返回所需的缓存四分项和过滤结果，以及分页上限、排序稳定性。
- 普通用户所有调用的授权范围、历史记录可用性、请求完成到用量可见的延迟。
- 不同站点数据时区、API 前缀和错误格式。

不将参考项目的管理员上游账号池接口作为 SubGauge 个人用量页面的默认数据源。也不将某个部署暂不支持的指标显示为零。

参考项目许可证与研究来源见 [来源与致谢](../ACKNOWLEDGEMENTS.md)。本项目通过 HTTP 对接个人接口，不将研究项目的许可证标记为 SubGauge 的许可证。
