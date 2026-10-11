# 企业审计事件目录

> 状态：V2 治理基线，2026-08-24。领域目录与 PostgreSQL 目录必须完全一致，由 `scripts/check_audit_governance.py` 在 CI 强制检查。

## 事件目录

| Action | Resource Type | Risk | 特权事件 |
|---|---|---:|---:|
| `organization.bootstrapped` | `organization.tenant` | Critical | 是 |
| `identity.principal.created` | `identity.principal` | High | 是 |
| `identity.oidc_owner.bound` | `identity.principal` | Critical | 是 |
| `identity.agent.registered` | `identity.agent` | High | 是 |
| `identity.agent.queried` | `identity.agent` | High | 是 |
| `identity.agent_credential.added` | `identity.agent_credential` | High | 是 |
| `identity.agent_mtls_credential.added` | `identity.agent_credential` | High | 是 |
| `identity.agent_credential.revoked` | `identity.agent_credential` | High | 是 |
| `identity.scim_token.issued` | `identity.scim_token` | High | 是 |
| `identity.scim_token.revoked` | `identity.scim_token` | High | 是 |
| `identity.scim_user.provisioned` | `identity.scim_user` | Medium | 是 |
| `identity.scim_user.adopted` | `identity.scim_user` | Medium | 是 |
| `identity.scim_user.updated` | `identity.scim_user` | High | 是 |
| `identity.scim_group.provisioned` | `identity.scim_group` | Medium | 是 |
| `identity.scim_group.updated` | `identity.scim_group` | Medium | 是 |
| `identity.scim_group.deleted` | `identity.scim_group` | High | 是 |
| `identity.human_session.created` | `identity.human_session` | High | 是 |
| `identity.human_session.revoked` | `identity.human_session` | High | 是 |
| `identity.human_sessions.revoked` | `identity.principal` | High | 是 |
| `authorization.approval_policy.created` | `authorization.approval_policy` | High | 是 |
| `authorization.approval_policy.versioned` | `authorization.approval_policy` | High | 是 |
| `authorization.approval.requested` | `authorization.approval_request` | Medium | 是 |
| `authorization.approval.decided` | `authorization.approval_request` | High | 是 |
| `authorization.approval.cancelled` | `authorization.approval_request` | High | 是 |
| `authorization.approval.queried` | `authorization.approval_request_query` | High | 是 |
| `authorization.role_assignment.granted` | `authorization.role_assignment` | Critical | 是 |
| `authorization.role_assignment.revoked` | `authorization.role_assignment` | Critical | 是 |
| `authorization.role_assignment.queried` | `authorization.role_assignment_query` | High | 是 |
| `mailbox.created` | `mailbox` | Medium | 是 |
| `mail.messages.queried` | `mailbox` | Medium | 是 |
| `mail.message.read` | `mail.message` | High | 是 |
| `mail.drafts.queried` | `mailbox` | Medium | 是 |
| `mail.draft.read` | `mail.draft` | High | 是 |
| `mail.submissions.queried` | `mailbox` | Medium | 是 |
| `agent.mail_content.read` | `mail.message` | High | 是 |
| `agent.mail_policy.activated` | `agent.mail_policy` | Critical | 是 |
| `agent.mail_policy.versioned` | `agent.mail_policy` | High | 是 |
| `agent.mail_policy.queried` | `agent.mail_policy_query` | High | 是 |
| `agent.mail.admitted` | `agent.mail_admission` | Critical | 是 |
| `agent.mail_submission_evidence.queried` | `agent.mail_submission_evidence` | High | 是 |
| `agent.mail_sandbox.captured` | `agent.mail_sandbox_submission` | High | 是 |
| `agent.processing_subscription.created` | `agent.processing_subscription` | Medium | 是 |
| `agent.processing_subscription.paused` | `agent.processing_subscription` | Medium | 是 |
| `agent.processing_subscription.resumed` | `agent.processing_subscription` | Medium | 是 |
| `agent.processing_subscription.retired` | `agent.processing_subscription` | High | 是 |
| `agent.message.claimed` | `agent.message_processing` | Low | 是 |
| `agent.message.lease_extended` | `agent.message_processing` | Low | 是 |
| `agent.message.acknowledged` | `agent.message_processing` | Low | 是 |
| `agent.message.nacked` | `agent.message_processing` | Medium | 是 |
| `agent.message.dead_lettered` | `agent.message_processing` | Medium | 是 |
| `agent.message_dead_letters.queried` | `agent.message_dead_letter_query` | Medium | 是 |
| `mail.submission.requested` | `mail.submission` | High | 是 |
| `mail.submission.accepted` | `mail.submission` | High | 是 |
| `mail.submission.rejected` | `mail.submission` | High | 是 |
| `mail.submission.review_required` | `mail.submission` | Critical | 是 |
| `mail_engine.mailbox_binding.updated` | `mail_engine.mailbox_binding` | High | 是 |
| `authorization.mailbox_delegation.granted` | `authorization.mailbox_delegation` | High | 是 |
| `authorization.mailbox_delegation.revoked` | `authorization.mailbox_delegation` | High | 是 |
| `audit.checkpoint.created` | `audit.checkpoint` | Critical | 是 |
| `audit.archive.completed` | `audit.archive` | Critical | 是 |
| `audit.events.queried` | `audit.query` | High | 是 |
| `audit.export.authorized` | `audit.export_grant` | Critical | 是 |
| `audit.export.downloaded` | `audit.export_grant` | Critical | 是 |
| `compliance.retention_policy.created` | `compliance.retention_policy` | High | 是 |
| `compliance.retention_policy.versioned` | `compliance.retention_policy` | High | 是 |
| `compliance.retention_policy.activated` | `compliance.retention_policy` | Critical | 是 |
| `compliance.retention_policy.queried` | `compliance.retention_policy_query` | High | 是 |
| `mail_domain.registered` | `mail_domain` | Medium | 是 |
| `mail_domain.challenge_rotated` | `mail_domain` | High | 是 |
| `mail_domain.dns_observed` | `mail_domain` | Low | 是 |
| `mail_domain.verified` | `mail_domain` | High | 是 |
| `mail_domain.activated` | `mail_domain` | Critical | 是 |
| `mail_domain.suspended` | `mail_domain` | Critical | 是 |
| `mail_domain.route_updated` | `mail_domain` | Critical | 是 |
| `agent.webhook_subscription.created` | `agent.webhook_subscription` | High | 是 |
| `agent.webhook_subscription.updated` | `agent.webhook_subscription` | High | 是 |
| `agent.webhook_subscription.disabled` | `agent.webhook_subscription` | High | 是 |
| `agent.webhook_delivery.replayed` | `agent.webhook_delivery` | Critical | 是 |
| `agent.event_cursor.read` | `agent.event_cursor` | Medium | 是 |
| `agent.event_stream.opened` | `agent.event_stream` | Medium | 是 |
| `compliance.case.created` | `compliance.case` | High | 是 |
| `compliance.case.closed` | `compliance.case` | High | 是 |
| `compliance.case.queried` | `compliance.case_query` | High | 是 |
| `compliance.legal_hold.applied` | `compliance.legal_hold` | Critical | 是 |
| `compliance.legal_hold.released` | `compliance.legal_hold` | Critical | 是 |
| `compliance.legal_hold.queried` | `compliance.legal_hold_query` | High | 是 |

邮件读取、删除、恢复、投递、隔离、Break-glass、DKIM
密钥和备份恢复事件会在相应业务能力实现时追加；不得提前登记一个没有真实生产者
的“纸面事件”。

## 必填字段

每条事件必须包含：

- UUIDv7 `event_id`；
- `organization_id`；
- 实际 `actor_principal_id`，以及可选 `on_behalf_of`；
- catalog 中登记的 action 与唯一 resource type；
- resource ID、outcome；
- `trace_id`、`request_id`；
- 服务端 `occurred_at` 与 `received_at`；
- 不含秘密和正文的结构化 details。

显式治理上下文支持：

- subject Principal；
- 来源 IP、客户端、Session、Token JTI；
- Policy、Policy Version；
- Approval Request、工单；
- 环境与区域。

请求级 Human 浏览器会话在 `begin_authorized` 事务的审计事件上写入该会话的
不可变 ID。这个字段不是 Cookie、OIDC Access Token 或 JTI。没有浏览器会话的
Agent 凭据、SCIM Token、旧 Bearer 和本地维护路径保持为空。若生产者已经写入
Session ID，它必须与本次认证绑定相同，否则持久化前拒绝。会话创建和撤销仍在
事件上显式记录 Session ID，并在当时保存已验证的 `token_jti`。

Actor、subject、on-behalf-of、Policy 和 Approval 均通过 PostgreSQL 复合外键绑定同一租户。

## 数据最小化

以下内容禁止写入 details，包括嵌套字段：

- 密码、Access/Refresh Token、Client Assertion；
- Authorization/Cookie、Secret、私钥、一次性明文；
- 邮件正文、原始 MIME、附件内容。

领域层递归检查，单条 details 编码上限为 64 KiB；数据库额外拒绝顶层敏感键。允许记录稳定 ID、摘要、证书指纹、Scope、数量、状态、原因和有效期。

## 覆盖率门禁

`python scripts/check_audit_governance.py` 同时验证：

1. Rust 领域目录与全部 versioned migration 累积的数据库目录完全一致；
2. Application 中真实生产的 action 与目录完全一致；
3. 每个应用事务提交前同时追加 Audit 与 Outbox；
4. 例外必须被静态门禁精确识别：没有状态变化的幂等重放，或不能虚假归因给 Human
   Actor、并且同时写入不可变来源回执与 Outbox 的机器运行事件。

Stalwart 入站邮件属于第二种：Human 修改 Account→邮箱绑定仍写受控审计；邮件到达
本身由 `inbound_mail_engine_events` 的追加式来源证据和同事务
`message.received.v1` 表示，不把绑定管理员伪装成每封邮件的 Actor。

数据库目录和审计事件均禁止 UPDATE/DELETE。CI PostgreSQL 17 集成测试继续核对目录、显式请求上下文和不可变触发器。

## 审计检索

具备 `audit.read` Token Scope 和实时 RBAC 的 Active Human 可以查询：

```text
GET /api/v2/organizations/{organization_id}/audit-events
```

默认窗口为最近 24 小时，单次窗口最长 31 天，每页最多 100 条。支持按 action、
Actor、resource type/id、outcome 和 request ID 精确过滤；`resource_id` 必须与
`resource_type` 同时使用。结果按租户哈希链 `chain_sequence` 倒序返回。

Cursor 采用 `v1.<organization-id>.<chain-sequence>`，服务端不仅校验租户，还确认
锚点在当前时间窗口和过滤条件下真实存在。伪造、跨租户或换过滤器后失效的锚点返回
`410 audit_cursor_expired_or_invalid`，不会被当成任意跳转位置。
继续分页时客户端必须复用响应中的 `occurred_from`、`occurred_to` 和原过滤器；
只传 Cursor 会重新采用默认时间窗口，长时间暂停后可能按设计失效。

返回字段包含 Actor 类型、全部显式治理上下文、脱敏 details、前序/当前 SHA-256
哈希、链序和签名批次引用，但不返回内部 `hash_payload`。查询只读权限不赋予邮件
正文权限。

每次成功查询（包括空结果）都会先原子追加 `audit.events.queried` Audit 与治理
Outbox；只记录查询条件、结果数量和首尾链序，不复制查询结果。若该证据提交失败，
API Fail Closed，不向调用方返回已经读取的结果。

## 防篡改哈希链

Migration 0011 为每个租户维护相互独立的 SHA-256 顺序链：

- `chain_sequence` 从 1 连续递增，并由 `(organization_id, chain_sequence)` 唯一约束；
- `previous_hash` 指向同租户前一事件，首事件为 `NULL`；
- `hash_payload` 保存数据库生成并实际参与摘要计算的 UTF-8 规范负载；
- `event_hash = SHA-256(previous_hash bytes || hash_payload UTF-8 bytes)`；
- `hash_version = 1`，后续算法升级必须保留版本化校验能力。

写入触发器使用租户维度的 PostgreSQL transaction advisory lock 串行分配链序。同一租户的并发写入不能产生分叉，不同租户仍可并发追加。链元数据拒绝由客户端指定，审计事件和事件目录同时拒绝 UPDATE、DELETE 与 TRUNCATE。

升级已有数据库时，事件按租户、`occurred_at`、`id` 稳定排序并回填链。`hash_payload` 覆盖事件 ID、租户、Actor/Subject、动作、资源、结果、请求与策略上下文、时间和 details，但不包含链哈希本身。

控制平面提供只读校验命令：

```shell
copilotmail-control verify-audit-chain --organization-id <uuid>
```

该命令在 PostgreSQL `REPEATABLE READ READ ONLY` 快照中按 1000 条分页读取，然后在独立 Rust 进程中重新计算完整链；成功输出事件数、首尾事件、链头和算法版本的 JSON 证据，失败精确指出断链事件及原因。空租户链也是合法结果，事件数为 0。

哈希链能检测数据库内容被改写或删除。签名 Checkpoint 可将指定链前缀交给独立信任根验证，具体格式、密钥轮换和导出流程见[审计 Checkpoint 与离线验签](AUDIT_CHECKPOINTS.zh-CN.md)。在 Manifest 尚未进入独立 WORM 存储前，拥有数据库和签名私钥的联合攻击者仍可能重建证据。

## 尚未完成

- 跨数据库调度与生产 Collector/Trace 告警演练；
- 邮件内容 Legal Hold、保留策略和 SIEM adapter；
- 真实 Object Lock 环境中的审计归档 Legal Hold 故障演练。
