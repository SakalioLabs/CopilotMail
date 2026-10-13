# Agent MCP over WebSocket 通道

> 状态：批次 18 新增，2026-10-14。实现位于 `apps/copilotmail-control`（传输层 `src/http/agent_mcp.rs`、会话审计用例 `crates/copilotmail-application/src/agent_mcp.rs`、审计目录迁移 `crates/copilotmail-postgres/migrations/0064_agent_mcp_session.sql`）。

## 差距映射（V2 REST → MCP）

用户需求（2026-10-14）要求在既有 V2 Agent 能力之上提供完整的 MCP WebSocket 接口。映射时遵循的原则：传输层只做协议适配，所有授权、策略、审批、沙箱、限流与幂等判断复用对应 REST 端点相同的 application 层用例与错误映射，不允许出现比 REST 更宽的路径。

| V2 REST 能力 | MCP 对应 | 本批状态 |
|---|---|---|
| `GET /api/v2/agents/self/mail-policy` | 工具 `agent_mail_policy_get`；资源 `agent://self/mail-policy` | 已实现 |
| `GET /api/v2/mailboxes/{id}/messages/{id}/content` | 工具 `agent_mail_content_read`；模板 `agent://mailboxes/{mailbox_id}/messages/{message_id}/content` | 已实现 |
| `PUT /api/v2/processing-subscriptions/{mailbox_id}`（ensure） | 工具 `agent_processing_subscription_ensure` | 已实现 |
| `PATCH /api/v2/processing-subscriptions/{id}`（状态迁移） | 工具 `agent_processing_subscription_change`（`idempotency_key` 为必填参数） | 已实现 |
| `POST /api/v2/processing-subscriptions/{id}/claims` | 工具 `agent_message_claim` | 已实现 |
| `POST /api/v2/processing-subscriptions/{id}/leases` | 工具 `agent_message_extend_lease` | 已实现 |
| `POST /api/v2/processing-subscriptions/{id}/acknowledgements` | 工具 `agent_message_acknowledge` | 已实现 |
| `POST /api/v2/processing-subscriptions/{id}/nacks` | 工具 `agent_message_nack` | 已实现 |
| `GET /api/v2/processing-subscriptions/{id}/dead-letters` | 工具 `agent_dead_letters_list`；模板 `agent://processing-subscriptions/{subscription_id}/dead-letters` | 已实现 |
| `POST /api/v2/drafts` | 工具 `agent_draft_save`（`idempotency_key` 必填） | 已实现 |
| `POST /api/v2/mail-submissions` | 工具 `agent_mail_submission_request`（`idempotency_key` 必填） | 已实现 |
| `GET /api/v2/mail-submissions/{id}` | 工具 `agent_mail_submission_get`；模板 `agent://mail-submissions/{submission_id}` | 已实现 |
| `POST /api/v2/mail-sandbox/captures` | 工具 `agent_mail_sandbox_capture` | 已实现 |
| `GET /api/v2/event-cursor`（事件游标拉取） | 工具 `agent_events_pull`；模板 `agent://webhook-subscriptions/{subscription_id}/events` | 已实现 |
| SSE/Webhook 实时事件推送 | `resources/subscribe` + `notifications/resources/updated` | 本批不实现：既有事件游标可经 `agent_events_pull` 轮询获得同等语义；推送语义留待后续批次评估 |
| prompts / logging / completion / 采样（sampling） | — | 不实现：V2 Agent 通道没有对应的业务语义 |
| JSON-RPC 批处理 | — | 不实现：逐帧处理使每条消息的限流与审计归属明确；批量请求按 `-32600` 拒绝 |

## 传输与会话

- 端点：`GET /api/v2/mcp`（HTTP/1.1 WebSocket 升级）。子协议必须包含 `mcp`；服务端在 `101` 响应中回选 `mcp`。
- 协议：JSON-RPC 2.0 文本帧。协议版本协商支持 `2025-06-18`（首选）与 `2025-03-26`；未知版本回退到 `2025-06-18` 并在 `initialize` 结果中声明。
- 生命周期：`initialize` 之前的一切请求以 `-32602` 拒绝（notification 静默忽略）；`notifications/initialized`、`notifications/cancelled` 与未知 notification 被接受；重复 `initialize` 报错；`notifications/cancelled` 中止对应在途调用。
- 限制：单帧 ≤ 128 KiB（`MCP_MAX_MESSAGE_BYTES`）、每连接并发在途请求 ≤ 4、心跳 ping 间隔 25 s、空闲超时 75 s、会话寿命上限 900 s、协议违规累计 8 次后关闭连接。连接关闭时在途任务全部取消，不产生半提交写入。
- 错误：JSON-RPC 标准码（-32700/-32600/-32601/-32602/-32603）用于协议层错误；并发超限使用 `-32000`；所有领域/授权/幂等失败以 `isError: true` 的工具结果返回，负载为 `{"http_status", "error"}`，其中 `error` 与 REST `ErrorResponse` 完全一致（相同的 `code`/`message`/`retryable`/`details`）。
- `tools/call` 成功时返回 `content[text]`（JSON 文本）；协商为 `2025-06-18` 时附带 `structuredContent`。

## 认证与授权

- 复用 V2 Agent 短期 Scope Token：凭证走 `Authorization: Bearer` 头；不能设置头的浏览器客户端可使用唯一一条 `bearer.<token>` 形式的 `Sec-WebSocket-Protocol` 条目（与 `mcp` 同时提供）。两个通道同时出现凭证按 `ambiguous_authentication`（400）拒绝；查询串 Token 一律不接受。
- Origin：携带 `Origin` 头的请求必须与 `Host` 同源，否则 403 `forbidden_origin`；无 `Origin` 的非浏览器客户端不受影响。
- 升级前依次执行：子协议校验 → Origin 校验 → 认证 → Agent 主体身份 → `agent.operate` 权限/Scope → 入口限流 → 会话审计写入，然后才接受升级。所有拒绝返回与 REST 相同的 JSON 错误信封。
- 每次 `tools/call` 重新解析实时授权上下文并执行与对应 REST 端点完全一致的权限、Scope、审批、沙箱与限流检查。

## 审计

- 会话建立在升级前通过 `AgentMcpSessionService::open` 写入：域审计事件 `agent.mcp_session.opened`（资源 `agent.mcp_session`，风险 Medium，特权）与 outbox 事件 `agent.mcp_session.opened.v1` 在同一事务提交；存在已验证 Human 会话上下文时按治理规则关联。
- 会话建立失败（认证/授权/限流/审计写入失败）不产生审计行——失败在写证据之前被拒绝。
- 工具调用的领域审计由被复用的各 application 用例照常产生，与 REST 完全一致。
- 审计目录行由迁移 `0064_agent_mcp_session.sql` 注册，并由 `readiness.sql` 治理检查强制存在。

## REST 等价性

测试约定：离线协议/端点测试直接运行（`tests::agent_mcp_tests`），等价性用例 `mcp_tool_calls_match_the_rest_contract` 沿用仓库 ignored + `COPILOTMAIL_DATABASE_URL` + `COPILOTMAIL_ALLOW_AGENT_INSPECTOR_HTTP_TEST_WRITES=1` 的弃置数据库约定，覆盖：ensure 幂等返回同一订阅、自身策略读取与服务层一致、收窄 Scope 得到与 REST 相同的 `permission_denied`、未授权邮箱与不存在邮箱同样得到 `resource_not_found`（租户不可探测）、缺 `idempotency_key` 得到 `idempotency_key_required`、会话审计/出站事件持久化。
