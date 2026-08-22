# ADR 0023：使用有界 Agent SSE 与订阅级 Cursor 恢复

- 状态：Accepted
- 日期：2026-07-26

## 背景

Agent 已可通过签名 Webhook 接收后台事件，并通过订阅级 Cursor 补拉遗漏事件。
部分交互式 Agent 还需要低延迟通道，但新增通道不能产生第三套消息状态、暴露通用
Outbox，或让长连接无限期持有已撤销的权限。

## 决策

增加：

```text
GET /api/v2/webhook-subscriptions/{subscription_id}/stream
```

该端点只接受具备 `agent.operate` Scope 和 RBAC 权限的短期 Agent Bearer Token。
它复用 Cursor 查询服务、订阅事件允许列表、Owner 隔离和稳定事件信封，不单独维护
已读或处理状态。

恢复位置可以来自 query `cursor` 或 `Last-Event-ID`。二者均使用
`v1.<subscription-id>.<event-id>`，同时提供时必须相同。服务端在返回 HTTP 200
前验证锚点真实属于当前订阅，避免伪造 UUID 被当成任意排序位置。

流具有明确资源上限：

- 每批 50 条；
- 无积压时每 2 秒轮询；
- 每 15 秒发送无数据 keepalive 注释；
- 每条连接最多 5 分钟，然后由客户端携带最后 Event ID 重新认证；
- 不缓存、不允许反向代理缓冲。

业务帧的 SSE `event` 是版本化事件类型，`id` 是完整不透明 Cursor，`data` 是与
Webhook 相同的 JSON 信封。运行期错误映射为有限的稳定错误码，通过
`stream.error` 控制帧发送后关闭，不发送数据库错误、Token 信息或邮件内容。

成功建立连接会在同一事务追加 `agent.event_stream.opened` Audit 和治理 Outbox。
实际非空批次继续产生 `agent.event_cursor.read` 证据；空轮询不写审计。

## 后果

- Webhook、Cursor 与 SSE 共享同一个事件身份、过滤和恢复语义。
- Agent 权限或凭据撤销不会被无限长 SSE 连接永久绕过；最迟在五分钟重连时完整
  重新认证，轮询时还会更早发现订阅禁用或 Agent 停用。
- 第一版每个活跃连接会产生 PostgreSQL 轮询负载。达到较大并发规模前应增加连接
  配额和指标，并可用 `LISTEN/NOTIFY` 或专用唤醒 adapter 降低空轮询；Cursor
  仍是唯一正确性来源。
- 浏览器原生 `EventSource` 无法可靠设置 Authorization Header，因此本端点面向
  Agent HTTP 客户端；未来人类 UI 的实时更新应通过安全浏览器 Session/BFF 单独
  设计。

## 不采用的方案

- 不将 SSE 作为独立消息队列：这会复制处理状态并破坏 Cursor 对账。
- 不建立无限时长连接：这会放大权限撤销和资源占用风险。
- 不通过 query 参数传 Access Token：URL 容易进入日志、历史和代理观测数据。
- 不允许 Human 管理权限读取 Agent 事件：管理权不等于消息内容读取权。
