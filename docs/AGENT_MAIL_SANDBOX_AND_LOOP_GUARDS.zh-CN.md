# Agent 邮件 Sandbox 与循环保护

## 1. 目标与边界

本边界落实实施计划 3.6，解决两类不同风险：

1. Testing Agent 的测试邮件不得因配置或调用错误进入真实投递链路；
2. Agent 自动回复、重复正文或线程级高频回复不得形成邮件循环或放大流量。

Sandbox 不是模拟 SMTP 服务器，也不返回伪造的“投递成功”。它只冻结一个真实、
已授权的草稿版本，记录最小化不可变证据，并明确返回
`delivery_suppressed: true`。Development Agent 仍可走受控投递路径；Production
Agent 仍必须通过活动策略、额度和审批 Admission；Testing Agent 只能走 Sandbox。

## 2. 服务端可信自动邮件标记

调用方不能提交 `Auto-Submitted` Header，也不能声明一封邮件是否自动生成。
控制平面根据持久化 Principal 类型判断作者：

- Human 草稿不增加 `Auto-Submitted`；
- Agent 新邮件由 JMAP Adapter 写入 `Auto-Submitted: auto-generated`；
- Agent 显式回复由 JMAP Adapter 写入 `Auto-Submitted: auto-replied`。

显式回复只接受安全的 CopilotMail 入站 Message UUID。Adapter 在同一邮箱绑定下读取
精确 JMAP Email，并有界读取 `header:Auto-Submitted:asText`。缺失或值为 `no` 视为
非自动来源；其他有界、无控制字符的值均保守视为自动来源。该分类保存为草稿不可变
证据，不能在替换、冻结或发送时换绑。

任何 Agent 尝试提交一个回复目标被分类为自动来源的草稿，都会在 PostgreSQL
Intent 插入触发器中以 `agent_mail_automatic_reply_loop` 约束拒绝，并映射为：

```text
409 agent_mail_automatic_reply_blocked
retryable=false
scope=automatic_reply_loop
```

该策略遵循 [RFC 3834](https://www.rfc-editor.org/rfc/rfc3834.html)：
自动响应程序不应对 `Auto-Submitted` 值不是 `no` 的邮件继续自动响应。

## 3. 重复正文与线程熔断

草稿正文不会复制到控制平面策略账本。应用层计算：

```text
SHA-256(
  fixed-version-prefix
  || organization UUID
  || principal UUID
  || UTF-8 byte length
  || exact UTF-8 body
)
```

结果作为 32 字节 `body_ref` 保存。它只用于同一租户、同一 Agent 的精确重复判断，
不能跨租户或跨主体关联正文，也不是语义相似度或 DLP 分类。

所有 Agent 的正式 Submission 在事务内按租户和 Agent 串行；同一 `body_ref` 在滚动
24 小时内已有 10 个 Intent 时，第 11 个请求被拒绝：

```text
429 agent_mail_duplicate_content_circuit_breaker_open
retryable=true
Retry-After: 3600
scope=identical_body_day
```

Production Agent 还继续受活动策略中的每线程自动回复次数限制。线程熔断、精确正文
熔断和 `Auto-Submitted` 循环拒绝是三条独立防线，任一失败均不提交 Intent、
Admission、Audit 或 Outbox，也不会留下冻结草稿。

## 4. Testing Agent Sandbox

接口：

```http
POST /api/v2/agent/mailboxes/{mailbox_id}/sandbox-submissions
Authorization: Bearer <short-lived-agent-token>
Idempotency-Key: <stable-22-to-200-character-key>
Content-Type: application/json

{
  "draft_id": "<uuid>",
  "expected_draft_version": 1
}
```

必须同时满足：

- 认证主体是同租户 Active Agent，且数据库中的环境精确为 `testing`；
- Token Scope 与实时 RBAC 同时包含 `mailbox.use`、`agent.operate`；
- 当前主体对目标邮箱具有实时 Send ABAC；
- 请求通过跨副本租户/Agent Ingress 窗口；
- 草稿属于当前 Agent、目标邮箱且版本仍为 Active；
- `Idempotency-Key` 只含允许字符并绑定租户、主体、邮箱、草稿和预期版本。

事务顺序：

1. 以租户和 Agent advisory lock 串行同主体请求；
2. 先查询精确幂等回放；
3. 使用版本 CAS 将草稿从 `active` 冻结为 `frozen`；
4. 写入 `agent_mail_sandbox_submissions`，绑定冻结版本、正文摘要和收件人数；
5. 原子追加 `agent.mail_sandbox.captured` Audit 与
   `agent.mail_sandbox.captured.v1` Outbox；
6. 提交后返回证据。

新建返回 `201`；精确回放返回 `200` 和 `replayed: true`。变参复用返回 409。Sandbox
表拒绝 UPDATE、DELETE、TRUNCATE。触发器再次验证 Testing Agent、同租户、草稿所有权、
冻结版本、正文摘要和收件人数。

正常投递接口对 Testing Agent 固定返回：

```text
409 agent_mail_sandbox_required
retryable=false
scope=testing_environment
```

数据库触发器也拒绝 Testing Agent 创建 `mail_submission_intents`。因此即使应用检查
回归，Testing Agent 仍不能进入 Worker 或 JMAP EmailSubmission 路径。

## 5. 数据最小化与审计

Sandbox 响应和事件只包含稳定 UUID、冻结版本、收件人数、时间及
`delivery_suppressed=true`，不包含地址、主题、正文、JMAP ID 或 Header。
`body_ref` 只保存在租户隔离的数据库证据中。

审计事件：

| Action | Resource | Risk | Privileged |
|---|---|---:|---:|
| `agent.mail_sandbox.captured` | `agent.mail_sandbox_submission` | High | 是 |

精确幂等回放没有新状态变化，因此不重复追加 Audit/Outbox。

## 6. 已验证与剩余验收

已有自动化证据覆盖：

- Agent/Human 自动 Header 选择与入站自动来源分类；
- 显式回复 Header 的服务端推导；
- 正文摘要的租户/主体隔离；
- Sandbox 请求契约未知字段拒绝；
- 未认证路由在访问数据库前 Fail Closed；
- PostgreSQL 集成场景可编译，覆盖 Testing Agent 正常投递拒绝、Sandbox 精确回放、
  无 Intent、Audit/Outbox 原子性和证据不可变。

本地环境没有可认证的 PostgreSQL，带 `#[ignore]` 的迁移执行测试由 CI PostgreSQL
作最终裁决。进入企业 Pilot 前还必须完成真实 Stalwart Header 回读、公网投递回环、
Agent-to-Agent 双端场景、并发阈值、24 小时窗口边界和故障注入演练。
