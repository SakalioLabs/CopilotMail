# 审计归档 Legal Hold

> 状态：审计归档首版已实现；真实 PostgreSQL 与 S3 Object Lock 环境演练待完成。

## 适用边界

当前 Legal Hold 只保护已完成 WORM 归档的审计证据：

- `events.jsonl` 的精确 S3 Version ID；
- `manifest.json` 的精确 S3 Version ID。

它不等同于邮件正文、原始 MIME 或附件的 Legal Hold。邮件内容必须先具备稳定、
版本化且可独立验证的证据对象，之后才能复用本状态机。

## 安全条件

施加或解除 Legal Hold 必须同时满足：

1. Actor 是同租户 Active Human；
2. Actor 实时具备 `legal_hold.manage`；
3. 已存在 Open 合规案件；
4. 精确双人审批已通过且未过期；
5. S3 Bucket 与数据库归档记录匹配；
6. 两个精确对象版本均经过 Legal Hold 状态回读；
7. 业务状态、关键审计和 Outbox 在一个 PostgreSQL 事务提交。

施加审批资源为 `audit.archive/<archive-id>`；解除审批必须是另一个请求，资源为
`compliance.legal_hold/<hold-id>`。审批请求 ID 不能被另一个 Hold 重用。

## 操作流程

管理 API 与控制命令使用相同的应用服务和数据库约束。所有 API 都要求 Active Human、
实时 `legal_hold.manage` 和对应 Token Scope：

```text
POST /api/v2/organizations/{organization_id}/compliance-cases
GET  /api/v2/organizations/{organization_id}/compliance-cases
GET  /api/v2/compliance-cases/{case_id}
POST /api/v2/compliance-cases/{case_id}/closure
GET  /api/v2/organizations/{organization_id}/legal-holds
POST /api/v2/audit-archives/{archive_id}/legal-holds
GET  /api/v2/legal-holds/{hold_id}
POST /api/v2/legal-holds/{hold_id}/release
```

施加和解除请求只接受 Approval Request ID，不接受客户端提交的 `approved` 状态或
对象存储证据。服务端重新验证审批并直接对精确 S3 Version ID 操作。未认证请求在访问
PostgreSQL 或对象存储前失败。

两个列表默认返回 50 条、最大 100 条，按 `created_at, id` 倒序稳定分页。案件支持
`open/closed` 过滤；Hold 支持 `active/released` 和 `case_id` 过滤。Cursor 同时绑定
版本、资源类型、租户和锚点；服务端确认锚点仍匹配当前过滤条件。跨租户、换过滤器或
伪造 Cursor 返回 `410 compliance_cursor_expired_or_invalid`，不会被解释成任意偏移。
每次成功的单项读取或列表查询（包括空列表）都会先提交对应 Query Audit 与 Outbox；
若证据提交失败，API 不返回已经读取的合规数据。

离线恢复环境也可使用控制命令。先通过受保护审批 API 创建并完成
`compliance.legal_hold` 审批：

```bash
copilotmail-control create-compliance-case \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --external-reference "CASE-2026-001" \
  --title "审计证据保全" \
  --purpose "监管调查"

copilotmail-control apply-audit-legal-hold \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --case-id "$CASE_ID" \
  --archive-id "$ARCHIVE_ID" \
  --approval-request-id "$APPLY_APPROVAL_REQUEST_ID" \
  --reason "CASE-2026-001 证据保全"

copilotmail-control release-audit-legal-hold \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --hold-id "$LEGAL_HOLD_ID" \
  --approval-request-id "$RELEASE_APPROVAL_REQUEST_ID"

copilotmail-control close-compliance-case \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --case-id "$CASE_ID"
```

案件存在 Active Hold 时不能关闭。命令只输出稳定 ID、状态和非敏感治理字段。

## 并发、失败与重试

控制平面使用“租户 + Archive ID”的 PostgreSQL 会话锁，同一归档在所有实例间最多
执行一个远端 Hold 变更，不同归档可以并行。

S3 不支持跨两个对象的原子 Legal Hold：

- 施加时，两个版本全部回读为 `On` 后才写入本地 Active 状态；
- 解除时，本地状态保持 Active，直到两个版本全部回读为 `Off`；
- 中途失败不会写入虚假完成状态；
- 使用相同参数和审批请求重试是幂等的；
- 参数冲突、审批复用或已解除后重新使用旧施加请求会 Fail Closed。

若进程取消或崩溃，持锁数据库连接会被淘汰，锁由 PostgreSQL 自动释放。运维系统应
对命令失败告警并使用原参数重试，不能绕过数据库直接修改状态。

## 审计与数据库约束

事件目录新增：

- `compliance.case.created`
- `compliance.case.closed`
- `compliance.case.queried`
- `compliance.legal_hold.applied`
- `compliance.legal_hold.released`
- `compliance.legal_hold.queried`

施加/解除事件的审计上下文包含精确 Approval Request ID。数据库再次验证案件状态、
Human Actor、审批状态/范围/人数/有效期、归档 Version ID 和回读时间，并禁止非法
状态跳转、更新历史字段、删除或截断合规证据。

## 待演练

- 在 CI PostgreSQL 17 实际执行 migration 0026 与并发锁场景；
- 在真实启用 Object Lock 的 MinIO/S3 上执行双对象施加、幂等重试、部分失败和解除；
- 增加操作失败指标、告警和管理后台；
- 邮件数据平面完成版本化证据后扩展到邮件正文与附件。
