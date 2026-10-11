# 高风险操作审批工作流

## 安全模型

审批是授权的收窄条件，不是独立放权机制。执行高风险动作时必须先通过：

1. Active Principal 与认证凭据校验；
2. 精确租户边界；
3. RBAC 权限；
4. 邮箱或其他资源级 ABAC；
5. 精确审批证明。

基础授权失败时，不查询审批记录，最终结果始终拒绝。

## 已支持动作

- Send As；
- Send On Behalf Of；
- 正文合规查看；
- 审计导出；
- 保留策略变更；
- 权限提升；
- Legal Hold；
- Break-glass；
- 批量删除；
- 撤销他人浏览器会话（`session.revoke_others`）。

## 策略版本

`policies` 保存稳定策略身份，`policy_versions` 保存不可变版本。策略编辑必须追加新版本并切换 Active Version，不得修改历史版本。审批请求固定引用提交时的版本，因此后续策略修改不会篡改历史审批依据。

每个版本至少定义动作、所需独立审批人数（1–5）和 JSON 规则快照。当前领域和持久化已支持版本边界；规则表达式执行器、Dry Run 和管理 UI 属于后续管理端切片。

## 请求与决策

审批请求固定包含租户、策略版本、动作、资源类型/ID、请求人、原因、可选工单、所需审批人数和到期时间。最长有效期为 30 天。

决策规则：

- 请求人不能自审；
- 同一审批人只能决策一次；
- 决策人当前必须是 Active Human，并具有 `authorization.manage`；
- 任意一次拒绝立即终止请求；
- 达到所需的不同审批人数后才进入 Approved；
- 到期后的 Pending 或 Approved 均不能继续授权；
- 审批步骤不可更新或删除。

请求人可以取消自己尚未到期的 Pending 请求，但不能取消他人的请求，也不能撤销
Approved、Rejected、Cancelled 或已过期请求。取消时仍会重新校验原始动作对应的
实时权限和 Token Scope，避免已失去业务权限的主体继续操作旧请求。

## 审批中心读模型

管理端提供两种严格隔离的视图：

- **待我审批**：只向具备实时 `authorization.manage` 的 Active Human 返回当前
  可决策请求；排除自己发起、自己已经决策、非 Pending 或已过期的请求。
- **我的申请**：Human 与 Agent 均可查看自己发起的请求，并可按状态筛选；请求人
  身份由认证上下文绑定，不能通过查询参数代选其他主体。

列表默认 50、最大 100 条，按 `created_at` 与 ID 倒序。版本化 Cursor 绑定租户、
视图和状态过滤器，并要求锚点仍是当前调用者真实可见的请求；跨租户、换视图、
换筛选器或伪造锚点均返回稳定的过期 Cursor 错误。

详情包含固定策略版本、资源范围、原因、工单、有效期、最终状态和按时间排序的
不可变决策历史。数据库中的 Pending 请求在读取时若已超过有效期，会以有效
`Expired` 状态返回；执行授权仍由 PostgreSQL 按服务器时间重新验证。

所有成功的列表与详情读取都必须先提交 `authorization.approval.queried` Audit +
Outbox 后才返回结果。若证据事务失败，读取 Fail Closed。

## 执行时证明

调用方构造 `ApprovalProof`，精确提供：

- `organization_id`；
- `approval_request_id`；
- action；
- resource kind；
- resource ID；
- 服务端当前时间。

PostgreSQL 会重新检查状态、通过人数、拒绝记录和到期时间。不得只信任客户端提交的 `approved` 状态，也不得只依赖缓存。

## 审计

以下动作与业务写入在同一事务产生追加式审计和 Outbox：

- `authorization.approval_policy.created`
- `authorization.approval.requested`
- `authorization.approval.decided`
- `authorization.approval.cancelled`
- `authorization.approval.queried`

记录策略版本、资源范围、理由、工单、审批步骤和最终状态；不记录 Token、Secret 或邮件正文。

## 离线策略治理

策略创建和版本追加由组织 Owner 离线执行，所有结果以 JSON 输出：

```bash
copilotmail-control create-approval-policy \
  --organization-id "$ORGANIZATION_ID" \
  --policy-key mail.send_as.external \
  --display-name "External Send As" \
  --action mail.send_as \
  --required-approvals 2 \
  --rule-json '{"recipient_scope":"external"}'

copilotmail-control append-approval-policy-version \
  --organization-id "$ORGANIZATION_ID" \
  --policy-key mail.send_as.external \
  --action mail.send_as \
  --required-approvals 2 \
  --rule-json '{"recipient_scope":"external","max_recipients":20}'
```

版本追加会锁定策略并比较当前 Active Version，避免并发更新丢失。审批请求和决策刻意不提供离线 CLI，它们必须通过已认证 Principal 的受保护 API：

- `POST /api/v2/approval-requests`
- `GET /api/v2/organizations/{organization_id}/approval-requests`
- `GET /api/v2/approval-requests/{approval_request_id}`
- `POST /api/v2/approval-requests/{approval_request_id}/decisions`
- `POST /api/v2/approval-requests/{approval_request_id}/cancellation`

创建请求时会先校验该动作对应的实时 RBAC 和 Token Scope；决策要求实时 `authorization.manage`。完整请求/响应格式见 `docs/openapi-v2.yaml`。

## 当前边界

- 策略治理 CLI、审批请求/查询/决策/取消 API 与中英双语审批中心已接入。
- 审计归档 Legal Hold 已消费精确审批：施加请求绑定 `audit.archive`，解除请求绑定
  `compliance.legal_hold`，两者不可复用；详见
  [审计归档 Legal Hold](LEGAL_HOLD.zh-CN.md)。
- 保留策略激活已消费精确双人审批：动作固定为
  `compliance.retention_change`，资源固定为待激活的
  `compliance.retention_policy_version`；草稿创建和版本追加本身不会改变生产行为，
  已批准版本不能授权同一策略的其他版本。详见
  [生产保留策略](RETENTION_POLICIES.zh-CN.md)。
- 角色授予已消费精确双人审批：动作固定为
  `authorization.privilege_elevation`，资源类型固定为
  `authorization.role_assignment`，资源 ID 固定为
  `v1:{principal_uuid}:{role_uuid}:{valid_until_epoch_millis}`。服务端要求有效期按
  毫秒对齐且不超过 365 天，并把请求人绑定到当前 Active Human 操作人；同一审批
  不能改授其他主体、角色或期限。撤销不需要新的提升审批，但仍要求实时
  `authorization.manage`，且不能撤销租户最后一个有效 Owner。
- 他人浏览器会话撤销已消费精确双人审批：动作固定为
  `session.revoke_others`，资源类型固定为 `identity.principal`，资源 ID 固定为
  目标 Principal UUID。执行权限不是 `identity.manage` 或 `security.admin`，
  默认只授予 Organization Owner；审批人不能是发起人，且 `required_approvals`
  小于 2 的策略不能执行。同一审批只使用一次。操作者当前会话不被撤销。
  目标不存在、没有有效会话，以及路径组织与调用者组织不一致，对外都是同一个
  空 `204`，响应不包含会话数量。
- 尚未实现按 SCIM Group、部门或职责动态筛选审批人。
- 尚未实现催办、通知、策略模拟和审批 SLA 指标。
- 本地没有 PostgreSQL/Docker 时，数据库与 Object Lock 集成场景只能编译；CI
  PostgreSQL 17 与 MinIO 负责实际迁移和行为验证。
