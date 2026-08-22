# 邮件数据平面事件接入

## 当前能力

CopilotMail 现在可以通过当前唯一实现的 Stalwart v0.16 adapter，把真实 SMTP
入站事件规范化为 `message.received.v1`，并把出站队列事件保存为租户隔离、
追加式的运维证据。`message.received.v1` 可由现有的 Agent 签名 Webhook 和
订阅级 Cursor 消费；队列证据不直接作为 Agent 外部事件，也不替代 JMAP
Submission 投递状态。adapter 名称描述协议实现，事件中的不透明引用另由稳定 UUID
`MailDataPlaneId` 约束到一个远端命名空间。

当前白名单只接收：

- `message-ingest.ham`
- `message-ingest.spam`
- `delivery.delivered`
- `delivery.failed`
- `queue.rescheduled`
- `delivery.completed`

两类证据严格分层：

- Human/Agent 可见的发送结果仍由精确 `EmailSubmission.id` 查询 JMAP
  `deliveryStatus`，它可以安全绑定到 CopilotMail Submission；
- 队列事件只按规范化 Envelope From 精确映射到当前 `MailDataPlaneId` 下一个已绑定
  Account 的 CopilotMail 主邮箱，保存为运维 Trace。当前不会用主题、收件人或时间窗口猜测
  `queueId → Submission`，因此也不会用这类事件改写用户可见投递状态。

## 安全边界

入口为：

```text
POST /internal/v2/mail-engine/stalwart/events
```

每个请求必须同时满足：

1. `Authorization: Bearer ...` 与只读文件中的随机 Token 一致；
2. `X-Signature` 是使用另一把密钥对原始请求体计算的 HMAC-SHA256，并以标准
   Base64 编码；
3. 请求体不超过 1 MiB，每批 1 至 100 个事件；
4. 事件时间不得早于接收时间 7 天，也不得超前 5 分钟；
5. 入站事件的 `accountId` 已绑定到当前服务端配置的 `MailDataPlaneId` 与一个
   CopilotMail 租户邮箱；出站事件的 Envelope From 必须精确对应唯一主邮箱，且该
   邮箱在同一数据平面命名空间中已有 Account 绑定；
6. `queue.rescheduled` 的 `nextRetry` 必须晚于事件时间，投递成功的 SMTP Code
   必须在 2xx 范围。

认证先于请求体读取，签名先于 JSON 解析。任何一项失败都不会产生部分写入。

## 准备 Secret

为 Bearer 和 HMAC 分别生成至少 32 字节的随机可打印 ASCII 值，不得复用：

```powershell
openssl rand -base64 48 > deploy/secrets/stalwart-events.bearer
openssl rand -base64 48 > deploy/secrets/stalwart-events.hmac
```

开发覆盖层已经把 `deploy/secrets` 只读挂载到 API。生产环境应由 Secret Manager
或编排器挂载同名用途的文件，不要把值写入环境变量、镜像或 Git。

配置 CopilotMail。数据平面 UUID 与 canonical origin 必须成对出现；当前 Stalwart
adapter 的管理、内容、写信和提交 Base URL 必须与该 origin 精确同源。Migration
0053 注册的 legacy-primary UUID 可用于既有单实例 PoC；另一个实例必须通过受治理的
注册/迁移流程取得不同 ID，不能临时复用或改写：

```text
COPILOTMAIL_MAIL_DATA_PLANE_ID=018f305d-3d0f-7b2a-9d68-7f4a3f6e2101
COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN=https://mail.company.example
COPILOTMAIL_MAIL_EVENT_INGRESS_BEARER_TOKEN_FILE=/run/secrets/copilotmail/stalwart-events.bearer
COPILOTMAIL_MAIL_EVENT_INGRESS_SIGNATURE_KEY_FILE=/run/secrets/copilotmail/stalwart-events.hmac
COPILOTMAIL_REQUIRE_MAIL_EVENT_INGRESS=true
```

`/readyz` 中的 `mail_event_ingress` 只有在两个文件均可读且符合长度/字符约束时才是
`ready`。文件在每次请求时重新读取，因此原子替换后无需重启 CopilotMail。

## 先绑定邮箱

启用 Stalwart WebHook 前，Human MailAdmin 必须把已有邮箱绑定到当前已配置数据平面
命名空间中的数字 Account ID。首次绑定的 `expected_version` 为 `0`，后续替换使用
上次响应的版本。请求不能选择 vendor/source 或 `data_plane_id`；`data_plane_id` 由认证后的
服务端配置注入：

```http
PUT /api/v2/mailboxes/{mailbox_id}/mail-engine-binding
Authorization: Bearer <Human OIDC token>
Content-Type: application/json

{
  "account_ref": "42",
  "expected_version": 0
}
```

同一个 `(mail_data_plane_id, account_ref)` 全局只能绑定一次；响应以 UUID
`data_plane_id` 明示服务端实际使用的命名空间。请求会原子写入版本化绑定、
`mail_engine.mailbox_binding.updated` 审计事件和治理 Outbox 事件。Agent 运行时
Token 没有 `mail.admin`，不能修改绑定。

出站事件不会信任请求提供租户 ID。服务端规范化 `from` 后，只允许它精确匹配一个
当前 `MailDataPlaneId` 下已有绑定的 `mailboxes.primary_address`；无匹配、跨数据
平面或多匹配均整批拒绝。

## 配置 Stalwart WebHook

在 Stalwart 管理界面创建专用 WebHook：

- URL（Docker 开发栈）：
  `http://api:8081/internal/v2/mail-engine/stalwart/events`
- 生产 URL：内部可达的 HTTPS URL；
- Events：只包含 `message-ingest.ham`、`message-ingest.spam`、
  `delivery.delivered`、`delivery.failed`、`queue.rescheduled`、
  `delivery.completed`；
- Events policy：Include；
- HTTP auth：Bearer，值来自 `stalwart-events.bearer`；
- Signature key：来自 `stalwart-events.hmac`；
- Allow invalid certificates：关闭；
- Lossy：关闭；
- Timeout：建议 10 秒；
- Throttle：按容量测试结果设置，起步可使用 500 ms；
- Discard after：不得超过 CopilotMail 的 7 天接收窗口，建议 1 小时。

不要配置 `*` 事件。Stalwart 可能在 `data` 中发送额外字段：

- 入站只读取 `accountId`、`documentId` 并验证 `mailboxId`；
- 出站只读取 `queueId`、`from` 及事件所需的 `to`、2xx `code` 或
  `nextRetry`；
- 地址只在事务内用于归属；收件人落库前转换为由 `queueId` 限定作用域的
  SHA-256 引用。外发地址、远端响应文本、Reason、Hostname、Message-ID、
  Blob ID、正文和附件均不进入 Outbox 或日志。

Stalwart 的队列 Span 会把 `queueId`、Envelope From 等父 Span 字段继承到子事件。
若实测版本没有提供上述完整字段，CopilotMail 会 Fail Closed，不能通过放宽校验来
“兼容”。

## 返回与重试

成功返回 `202`：

```json
{"accepted":1,"inserted":1,"duplicates":0}
```

精确重放仍返回 `202`，但计入 `duplicates`。常见失败：

| HTTP | 含义 | 处理 |
|---:|---|---|
| 400 | 事件类型、结构或时间不接受 | 修正 Stalwart 事件白名单/时钟 |
| 401 | Bearer 或 HMAC 不一致 | 核对两套独立 Secret |
| 409 | 同一来源 Event ID 的内容发生变化 | 停止自动跳过，调查来源完整性 |
| 413 | 批次超过 1 MiB | 降低 Stalwart throttle 批量聚合 |
| 503 | Secret、数据库/数据平面配置不可用，或 Account 未绑定/来源不符 | 修复配置或绑定，让 Stalwart 重试 |

入口不会在错误响应、审计或日志中回显请求体。

## 验收口径

代码和本地单元测试已覆盖签名篡改、未知事件、超限字段、出站类型形状、重试时间、
整批回滚、精确重放、来源 ID 内容冲突、租户映射和最小化 Payload。Migration 0033
把队列事件保存为不可 UPDATE/DELETE/TRUNCATE 的追加式账本，并以数据库约束复核
事件类型与字段组合；来源证据与内部最小化 Outbox 在同一事务提交。

PostgreSQL 并发重放与追加式触发器场景已编译。Migration 0053 进一步把绑定、
入站/出站事件和后续读取都纳入 `MailDataPlaneId` 来源亲和，但截至 2026-08-13
尚未在真实 PostgreSQL 上执行；既有 51 份迁移的成功记录不能替代该验证。实际执行
仍以 CI PostgreSQL 17 和后续 Stalwart PoC 报告为准。
仍须在真实 Stalwart 容器和公网测试域执行签名、重试、临时失败、永久退信和 DSN
演练，不能据此宣称公网生产收发已经完成。
