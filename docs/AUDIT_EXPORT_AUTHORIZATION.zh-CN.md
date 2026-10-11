# 审计归档导出授权

本模块为 Human Principal 发起的浏览器或受控操作客户端提供安全的“导出前授权”能力。
Agent 与 Service Principal 不能使用该网络下载能力；它不会接受服务器本地路径，也不会把
现有离线 CLI 直接暴露给网络请求。

## 安全模型

一份 `AuditExportGrant` 只绑定：

- 一个租户；
- 一个不可变 `audit.archive`；
- 一个发起申请的活跃 Human Principal；
- 一个操作类型为 `audit.export` 的已批准请求；
- 最少两名独立审批人；
- 最长 15 分钟的使用窗口，且不会超过审批请求自身的失效时间。

审批请求不能转让给其他调用者，也不能用于其他归档。一个审批请求最多产生一份凭证；相同请求重放会返回同一份不可变记录，更新、删除与截断均由 PostgreSQL 拒绝。

成功授权会在同一事务内写入：

- `audit_export_grants`；
- `audit.export.authorized` 审计事件；
- `audit.export.authorized.v1` Outbox 事件。

## API 流程

先使用审批 API 创建请求，其中：

```json
{
  "policy_key": "audit.archive.export",
  "action": "audit.export",
  "resource_kind": "audit.archive",
  "resource_id": "<archive UUID>",
  "reason": "季度审计证据交付",
  "ticket_reference": "AUDIT-42",
  "expires_at": "2026-07-26T12:00:00Z"
}
```

审批完成后，由原申请人调用：

```http
POST /api/v2/audit-archives/{archive_id}/exports
Authorization: Bearer <human access token>
Content-Type: application/json

{"approval_request_id":"<approval UUID>"}
```

查询凭证：

```http
GET /api/v2/audit-export-grants/{grant_id}
Cookie: __Host-copilotmail-admin-session=<opaque>
```

查询只对凭证原申请人开放；跨租户、其他 Principal 或不存在的凭证统一表现为资源不可见。

下载归档中的精确对象版本：

```http
POST /api/v2/audit-export-grants/{grant_id}/artifacts/events
Origin: https://admin.company.example
Cookie: __Host-copilotmail-admin-session=<opaque>
```

```http
POST /api/v2/audit-export-grants/{grant_id}/artifacts/manifest
Origin: https://admin.company.example
Cookie: __Host-copilotmail-admin-session=<opaque>
```

下载使用 POST，因为它会新增不可变下载证据，并必须经过 Session 的精确 Origin
CSRF 校验；不会提供有副作用的 GET 降级路径。下载只允许凭证原申请人在有效期内
执行。服务端不会接受对象键或 Version ID
作为用户输入，而是从不可变归档记录中选择目标；读取后重新比对 Version ID、长度与
SHA-256，并在返回任何字节前，将下载证据、`audit.export.downloaded` 审计事件和
`audit.export.downloaded.v1` Outbox 事件原子提交。响应使用
`application/octet-stream`、`Cache-Control: no-store`，并通过
`X-CopilotMail-Artifact-SHA256` 返回已验证摘要。

## 当前边界

代码质量加固（2026-09-19）：精确幂等重放仍须在事务中复核当前 Human 的 `audit.export`
权限，不延长原凭证。Grant/Archive/申请人/租户/审批 ID 与预期不一致时拒绝；下载凭据还必须
与已验证对象逐字段一致。0059/0060 均需后续隔离环境迁移，当前未执行部署。

0060 将凭证创建和下载时间改为锁等待后的数据库时刻，禁止调用者倒填旧时间绕过过期。
应用先接收数据库返回证据、重新检查有效期，再用同一时刻提交 Audit/Outbox。网络对象读取仍
在数据库事务之外，任何证据提交失败都不会向调用者返回文件字节。

下载当前采用 256 MiB 单对象硬上限，逐块累计长度与 SHA-256；任何服务端声明长度与
实际流不一致、超出上限或摘要不符均在返回字节前拒绝。下载仍需在内存中保留已验证
内容，适合当前审计批次。后续如需支持更大归档，应改为有界临时文件或双遍流式读取；
不能以牺牲“验证并落账后再返回”为代价直接透传对象存储响应。凭证仍不等同于文件内容，
也不允许推导或读取任意对象键。
