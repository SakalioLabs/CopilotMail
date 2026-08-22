# Agent 签名 Webhook 投递

> 当前状态：安全 HTTP adapter、PostgreSQL 追加式投递账本、Worker fan-out、
> 订阅治理 API、死信检索、受控人工重放、订阅级 Cursor 补拉和有界 SSE
> 实时流已接入。

## 订阅治理

订阅只能归属于同租户的 Active Agent。Agent 使用 `agent.operate` 管理自己的订阅；
Human 管理员若要代管其他 Agent，还必须同时具备 `identity.manage`。角色权限和
Access Token Scope 会同时检查。

对外事件采用显式允许列表，不支持 `*`：

- `message.received.v1`
- `message.delivery_status_changed.v1`
- `agent.message.dead_lettered.v1`

管理接口：

- `POST/GET /api/v2/organizations/{organization_id}/webhook-subscriptions`
- `GET/PUT /api/v2/webhook-subscriptions/{subscription_id}`

更新和禁用使用 `expected_version` 做乐观并发控制。Endpoint 必须为无 userinfo、
query、fragment 的 HTTPS URL。API 只接受 `signing_key_reference`，不接受或返回
签名 Secret。

## 事件信封

机器可读契约见 [`docs/asyncapi-v2.yaml`](asyncapi-v2.yaml)，采用 AsyncAPI 3.1。

接收方获得稳定 JSON：

```json
{
  "id": "018f4f6d-7b42-7cc0-9f7e-9d3fbe0db123",
  "organization_id": "018f4f6d-7b42-7cc0-9f7e-9d3fbe0db456",
  "type": "message.received.v1",
  "schema_version": 1,
  "occurred_at": "2026-07-26T00:00:00Z",
  "aggregate": {
    "type": "message",
    "id": "018f4f6d-7b42-7cc0-9f7e-9d3fbe0db789"
  },
  "data": {}
}
```

`id` 在所有重试中不变，接收方必须以它做幂等去重。`occurred_at` 是业务事件时间，
Webhook Header 中的 Timestamp 才是本次投递时间。

## 签名验证

请求 Header：

- `CopilotMail-Webhook-Id`
- `CopilotMail-Webhook-Timestamp`
- `CopilotMail-Webhook-Key-Id`
- `CopilotMail-Webhook-Signature: v1=<base64url>`

签名输入为以下原始字节，不得先解析再序列化 JSON：

```text
timestamp + "." + event_id + "." + exact_request_body
```

算法为 HMAC-SHA256。接收方应：

1. 根据 Key ID 选择仍在验证窗口内的 secret；
2. 拒绝超出允许时钟窗口的 Timestamp；
3. 对原始 Body 计算签名并使用常量时间比较；
4. 检查 Header Event ID 与 Body ID 完全一致；
5. 对首次出现的 Event ID 执行业务操作并原子记录去重结果；
6. 重放事件返回任意 `2xx`，不要重复业务副作用。

## 出站安全边界

- 生产只接受 HTTPS。
- Endpoint 禁止 userinfo、query 和 fragment，凭据不得放在 URL。
- 禁止重定向。
- DNS 查询不读取 hosts 文件；每次投递最多接受 16 个地址。
- 任意 private、loopback、link-local、unspecified、文档或转换地址都会拒绝整个
  Endpoint；通过校验的地址会固定到该次 HTTP Client，防止 DNS rebinding。
- 总超时限制在 1–30 秒，事件 Body 最大 1 MiB。
- 不读取、不记录响应 Body。

本地自动测试可以同时开启 HTTP 和私网地址，但生产配置必须拒绝这两个开关。

## 重试分类

| 结果 | 分类 |
|---|---|
| 任意 `2xx` | 成功 |
| `408`、`425`、`429`、`5xx` | 可重试 |
| 其他非 `2xx` | 永久拒绝 |
| DNS/连接/超时 | 可重试 |
| 非公网 Endpoint、无效事件或 Key | 永久配置错误 |

Worker 把每次尝试追加到 `webhook_deliveries`，已成功的订阅不会因其他订阅失败而
重复投递；超过最大次数或遇到永久错误会进入死信，并保留人工重放所需的 Event ID
与订阅上下文。新建订阅不会自动回放订阅创建时间之前的事件。只有所有匹配订阅均
达到 `delivered` 或 `dead_lettered`，Outbox 事件才进入发布终态。

## 死信检索与人工重放

死信接口仅允许具备 `security.admin` 的 Human 管理令牌调用：

- `GET /api/v2/organizations/{organization_id}/webhook-dead-letters`
- `POST /api/v2/webhook-subscriptions/{subscription_id}/dead-letters/{event_id}/replay`

列表只返回当前仍处于终态的最新死信，字段限于订阅、Agent、Event ID、稳定错误码、
HTTP 状态和时间；不会返回响应 Body、异常文本、邮件正文或签名材料。

人工重放必须精确匹配“该订阅 + 该事件”的最新记录，且订阅仍启用。事务会：

1. 锁定订阅和原始 Outbox 事件；
2. 在旧 `dead_lettered` 记录之后追加一条
   `retrying/manual_replay_requested` 标记；
3. 清除原事件的发布/租约状态，使 Worker 可重新领取；
4. 原子追加 `agent.webhook_delivery.replayed` 审计记录和治理 Outbox 事件。

历史投递记录禁止 UPDATE、DELETE、TRUNCATE。新的重放周期从零计算尝试次数，但旧
周期的全部尝试序号和结果继续保留；并发或重复重放会因最新状态不再是死信而拒绝。

## Cursor 补拉

Agent 可使用以下接口恢复 Webhook 中断期间的事件：

```text
GET /api/v2/webhook-subscriptions/{subscription_id}/events?cursor=...&limit=50
```

- 只允许订阅的精确 Owner Agent 调用；`identity.manage` 不会赋予 Human 消息事件
  读取权。
- 订阅必须仍为启用状态，并同时实时检查 Active Agent、`agent.operate` RBAC 和
  Access Token Scope。
- Feed 不是通用 Outbox 查询，只返回该订阅选择的三类外部允许事件。
- 事件使用与签名 Webhook 完全相同的稳定信封，Event ID 可直接用于幂等去重。
- Cursor 格式版本化并绑定 Subscription ID。服务端还会验证锚点 Event 确实属于
  该订阅；跨订阅、伪造或已清理的锚点不会被当成任意排序位置。
- `next_cursor` 指向本页最后一个 Event；空页保持传入 Cursor，首次空页返回
  `null`。`has_more=true` 时应立即继续分页，否则进入正常轮询间隔。
- 无效或不可用的锚点返回 `410 event_cursor_expired_or_invalid`，客户端应停止自动
  前进并触发运维对账，不能静默跳过区间。

每个非空页面会记录 `agent.event_cursor.read` 审计事件，只保存订阅、事件数量和
首尾 Event ID，不复制消息 Payload。空轮询不写审计，避免高频 Agent 产生无业务
价值的审计噪音。

## SSE 实时流

需要低延迟收信的 Agent 可在 Cursor 基础上建立 SSE：

```text
GET /api/v2/webhook-subscriptions/{subscription_id}/stream?cursor=...
Authorization: Bearer <short-lived-agent-token>
Accept: text/event-stream
```

- 服务端只接受 Agent Bearer Token，不使用 Cookie，也不为浏览器原生
  `EventSource` 放宽认证。无法设置 Authorization Header 的客户端应使用支持
  Header 的 SSE/HTTP 客户端。
- 首次连接可传 `cursor`；自动重连可发送 `Last-Event-ID`。二者同时存在时必须
  完全相同，否则返回 `400 conflicting_event_cursor`。
- 每个业务帧的 `event` 是版本化业务事件类型，`id` 是完整的订阅级不透明 Cursor，
  `data` 是与 Webhook/Cursor API 相同的 JSON 信封。
- 服务端固定每批最多读取 50 条；积压存在时连续读取，否则每 2 秒检查一次。
  15 秒无业务帧时发送 `keepalive` 注释，代理不得缓冲响应。
- Agent 调用 Cursor 补拉、建立 SSE 或使用草稿/提交 API 前，必须先在 PostgreSQL 原子消耗租户与当前
  Active Agent 的一分钟请求窗口。默认分别为每分钟 6000 与 600 次，通过
  `COPILOTMAIL_AGENT_INGRESS_TENANT_REQUESTS_PER_MINUTE` 和
  `COPILOTMAIL_AGENT_INGRESS_PRINCIPAL_REQUESTS_PER_MINUTE` 收窄；Agent 上限不得
  超过租户上限。窗口由数据库时钟决定并跨所有 API 副本生效。
- 超过请求窗口返回 `429 agent_ingress_tenant_rate_limited` 或
  `429 agent_ingress_principal_rate_limited`，同时携带数据库时钟推导且介于
  1–60 秒的 `Retry-After`。限流存储不可用时默认拒绝并返回可重试
  `503 agent_ingress_rate_limit_unavailable`。Human 管理调用不消耗 Agent 请求窗口。
- 单条连接最长 5 分钟，之后正常关闭。客户端使用最后一个 `id` 重连并重新认证，
  从而把 Token、Scope 或角色撤销的最长陈旧窗口限制在连接上限内。
- 每次建连会在 PostgreSQL 中获取最长 5 分钟的短期租约，因此租户和单个 Agent 的
  并发上限在所有 API 副本间共同生效。默认分别为 100 和 5，可通过
  `COPILOTMAIL_AGENT_EVENT_STREAM_TENANT_LIMIT` 与
  `COPILOTMAIL_AGENT_EVENT_STREAM_PRINCIPAL_LIMIT` 收窄；Agent 上限不能超过租户
  上限。超额在发送 SSE `200` 前返回可重试 `429` 和稳定容量错误码。
- 正常断连会最佳努力删除租约；进程崩溃或 PostgreSQL 暂不可用时由到期时间回收，
  因此泄漏槽位最长保留 5 分钟。租约不保存 Token、IP、事件或邮件内容。
- 每轮查询仍检查订阅启用状态、精确 Owner 和 Active Agent。连接期间发现撤销、
  存储不可用或 Cursor 异常时，服务端只发送不含内部错误的 `stream.error`
  控制帧，然后关闭；客户端应退避并通过 Cursor API 对账。
- 成功建连原子记录 `agent.event_stream.opened` 审计与治理 Outbox，只包含订阅、
  Agent 和连接上限；非空事件批次沿用 `agent.event_cursor.read` 审计，均不复制
  邮件 Payload。

SSE 是低延迟通知通道，不是新的消息处理状态，也不替代 Webhook 的后台投递或
Cursor 的恢复能力。当前实现使用 PostgreSQL 有界轮询；规模扩大后可以在保持同一
Cursor 语义的前提下增加 `LISTEN/NOTIFY` 或专用唤醒层。

独立指标监听器由 `COPILOTMAIL_AGENT_EVENT_STREAM_METRICS_ADDR` 配置，默认只绑定
`127.0.0.1:9092`；Compose 内部使用 `api:9091`。Prometheus 指标只包含当前进程
Active 数、累计建连、租户/Agent 配额拒绝、流错误、租约清理失败，以及 Agent
Ingress 的 Allowed、租户拒绝、Agent 拒绝和存储失败，不使用租户或 Principal
标签。PostgreSQL 租约和请求窗口是配额权威，指标不能参与授权判断。

当前 Cursor/SSE 协议、权限、索引和受控 Outbox 场景已经实现。Stalwart
`message-ingest.ham/spam` 现已通过双凭据入口、租户邮箱绑定和追加式来源回执生成
真实 `message.received.v1`；配置见
[邮件数据平面事件接入](MAIL_EVENT_INGRESS.zh-CN.md)。投递状态采集仍未接通，
真实 Stalwart/PostgreSQL 外部集成也尚未在本机执行，因此仍不能把整个生产 Feed
视为完整邮件数据面。

## Secret 文件

Key-set Manifest 示例：

```json
{
  "keys": [
    {
      "key_id": "agent-hook-2026-01",
      "secret_file": "agent-hook-2026-01.secret"
    },
    {
      "key_id": "agent-hook-2026-02",
      "secret_file": "agent-hook-2026-02.secret"
    }
  ]
}
```

相对路径以 Manifest 所在目录解析。每个 secret 必须是 32–128 字节的无空白可打印
值，Manifest 和 secret 均应只读挂载。轮换时先发布新 Key、更新订阅引用，等待旧
事件重试窗口结束后再移除旧 Key。

Worker 配置：

```text
COPILOTMAIL_WEBHOOK_SIGNING_KEY_SET_FILE=/run/secrets/webhook-keyset.json
COPILOTMAIL_WEBHOOK_TIMEOUT_SECONDS=10
COPILOTMAIL_WEBHOOK_MAX_ATTEMPTS=12
COPILOTMAIL_REQUIRE_WEBHOOK_DELIVERY=true
```

生产环境默认要求 Webhook 配置，并强制
`COPILOTMAIL_WEBHOOK_ALLOW_INSECURE_HTTP=false` 与
`COPILOTMAIL_WEBHOOK_ALLOW_PRIVATE_NETWORK=false`。开发环境未配置 Key-set 时仍可
使用元数据日志 sink，但日志不再输出事件 Payload。
