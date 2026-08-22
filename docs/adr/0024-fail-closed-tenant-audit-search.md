# ADR 0024：租户隔离且查询自身 Fail Closed 的审计检索

- 状态：Accepted
- 日期：2026-07-26

## 背景

审计事件已经追加写入租户哈希链，并支持 Checkpoint、离线验签和 WORM 归档，但
合规人员缺少在线检索接口。查询接口本身是高敏感操作：它不能跨租户、不能赋予正文
访问权、不能允许无界扫描，也必须留下不可抵赖证据。

## 决策

提供：

```text
GET /api/v2/organizations/{organization_id}/audit-events
```

只允许 Active Human 调用，并同时检查 `audit.read` Access Token Scope 与实时
RBAC。Agent Token 的 Scope 目录不包含该权限；管理其他资源的权限也不隐式授予
审计读取。

查询默认最近 24 小时，最长 31 天，每页 1–100 条。过滤器只支持 action、Actor、
resource type/id、outcome 和 request ID 的精确匹配。action/resource type 必须
来自迁移管理的审计目录，resource ID 只能和 resource type 一起使用。

分页按每租户单调 `chain_sequence DESC` 执行。Cursor 包含版本、Organization ID
和锚点链序；继续分页前，PostgreSQL 必须证明锚点在相同租户、时间窗口和当前过滤器
下真实存在。Cursor 不是任意 SQL offset。

结果包括计划要求的身份、资源、请求、策略、审批、环境和哈希链字段，但不返回
数据库内部 `hash_payload`，也不会扩大 details 的数据范围。details 在领域层和
数据库层均强制为不超过 64 KiB 的对象，并继续递归拒绝秘密、正文、MIME 与附件
内容。

应用先读取有界结果，再开启事务验证 Actor 仍为 Active Human，并原子写入
`audit.events.queried` Audit 与 `audit.events.queried.v1` Outbox。事务提交成功后
才向 HTTP 层返回结果；写证据失败即 Fail Closed。

## 后果

- 合规人员可以通过稳定契约在线检索并看到链序和哈希证据。
- 每次成功查询，包括空查询，都会使审计链继续增长；分页时新产生的查询事件位于
  Cursor 之前，不会插入后续旧事件页面。
- 第一版是精确过滤，不支持全文检索、任意 JSONPath 或跨租户查询，避免索引失控和
  数据边界模糊。
- 管理 UI、异步大规模导出、合规案件和 SIEM adapter 仍需后续实现；大范围取证应走
  经审批、可签名的导出流程，而不是扩大在线查询窗口。

## 不采用的方案

- 不允许 Organization Owner 绕过 `audit.read`：能力必须显式存在于实时授权结果。
- 不使用 offset 分页：并发追加会导致重复或跳项，且大偏移性能不可控。
- 不把邮件正文放进审计详情或查询结果：正文审查属于独立合规案件权限。
- 不在审计写入失败时仍返回查询结果：这会产生无法追溯的审计访问。
