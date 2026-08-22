# 生产保留策略

CopilotMail 的保留策略采用“不可变身份、追加式版本、审批后激活”的治理模型。
创建或编辑策略只会产生草稿，不会立即改变生产行为；激活必须由原申请人持有
`retention.manage`，并提交与待激活版本精确绑定、至少两名独立审批人通过且尚未过期的
`compliance.retention_change` 审批。

## 数据模型

- `retention_policies`：不可变的租户、策略键、显示名称和资源类型。
- `retention_policy_versions`：追加式版本，包含最低保留期、归档时间、可选处置时间和
  有界 JSON 范围规则。
- `retention_policy_activations`：不可变激活证据，绑定策略、版本、原申请人和审批请求。
- `active_version_id`：只能在同一事务中存在有效激活证据时前移到更高版本，不能回滚。

当前资源类型：

- `audit_evidence`：审计事件、Checkpoint、Manifest 和 WORM 归档。
- `mail_content`：邮件正文、原始 MIME 和附件；执行器尚未开放。

时间约束：

- `minimum_retention_days`：1–36500 天。
- `archive_after_days`：可选，不能超过最低保留期。
- `delete_after_days`：可选；为空表示无限期保留，存在时不能早于最低保留期。
- Legal Hold 始终覆盖处置，不是可配置开关。

## API

建立策略和首个草稿：

```http
POST /api/v2/organizations/{organization_id}/retention-policies
Authorization: Bearer <human access token>
Content-Type: application/json

{
  "policy_key": "audit.standard",
  "display_name": "标准审计证据保留",
  "resource_kind": "audit_evidence",
  "minimum_retention_days": 2555,
  "archive_after_days": 30,
  "delete_after_days": 3650,
  "rule": {"classification": ["standard"]}
}
```

追加草稿版本：

```http
POST /api/v2/retention-policies/{policy_id}/versions
```

读取当前与最新版本：

```http
GET /api/v2/retention-policies/{policy_id}
```

读取是特权操作；控制平面必须先提交
`compliance.retention_policy.queried` Audit/Outbox，证据提交失败则不返回结果。

激活前使用审批 API 创建请求：

```json
{
  "policy_key": "compliance.retention.policy_change",
  "action": "compliance.retention_change",
  "resource_kind": "compliance.retention_policy_version",
  "resource_id": "<待激活 version UUID>",
  "reason": "启用经法务与安全复核的生产保留策略",
  "ticket_reference": "RETENTION-42",
  "expires_at": "2026-07-26T12:00:00Z"
}
```

审批完成后由原申请人激活精确版本：

```http
POST /api/v2/retention-policies/{policy_id}/versions/{version_id}/activation
Authorization: Bearer <human access token>
Content-Type: application/json

{"approval_request_id":"<approval UUID>"}
```

成功激活会在同一事务写入激活证据、更新活动版本、追加
`compliance.retention_policy.activated` 审计事件和 Outbox。

## 当前执行边界

本阶段完成策略治理、数据库不变量、管理 API 和中英双语管理 UI。它尚未启动自动删除
任务，也不会因为配置了 `delete_after_days` 就删除任何对象。后续处置执行器必须同时
证明：

1. 处理对象匹配当时生效的策略版本和范围规则；
2. 已经过最低保留期和处置时间；
3. 不存在活动 Legal Hold；
4. 邮件数据平面与对象存储均返回精确删除证据；
5. 状态变更、审计和 Outbox 原子提交。

任一证据缺失、存储不可用或策略冲突时必须 Fail Closed。
