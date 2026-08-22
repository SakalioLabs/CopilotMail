# Agent OAuth2 接入与凭据轮换

CopilotMail V2 将 Agent 建模为独立的 `Principal`，而不是模拟 Human
登录。当前支持 OAuth 2.0 Client Credentials、RFC 7523
`private_key_jwt` 与原生 mTLS 客户端认证；每个 Agent 可以同时保有多个凭据，以便无停机轮换。

> 当前实现支持 `private_key_jwt` 与原生 mTLS。SPIFFE/SPIRE、云工作负载
> 身份联邦等能力尚未实现，不能在部署说明中宣称已经支持。

## 安全边界

- `client_id` 是 Agent Principal UUID。
- JWT Header 的 `kid` 是 Credential UUID。
- Agent 私钥只存在于 Agent 的密钥管理系统；CopilotMail 仅存公钥。
- 授权服务器签名私钥与 Agent 客户端私钥必须是不同的密钥。
- Client Assertion 仅接受 RS256，必须包含 `iss`、`sub`、`aud`、`exp`、
  `iat` 和一次性 `jti`。
- `iss` 与 `sub` 必须都等于 `client_id`；`aud` 必须精确等于 Token Endpoint。
- Assertion 最长有效 5 分钟，`jti` 以 SHA-256 摘要原子消费，重放会被拒绝。
- Access Token 最长 15 分钟，推荐 5 分钟；实际过期时间不会晚于凭据过期时间。
- 最终 Scope 是“凭据允许 Scope”与“实时 RBAC 权限”的交集。
- 每次受保护请求都会实时检查 Agent、凭据和 Scope。撤销凭据后，已签发但未过期的
  Access Token 也会立即失效。
- mTLS 身份只接受 Rustls 已验证的 TLS 会话证书，不接受
  `X-Forwarded-Client-Cert` 等客户端可控 HTTP Header。
- mTLS 签发的 Token 带有 RFC 8705 `cnf.x5t#S256`；每次调用都必须继续出示同一证书。

当前可签发的 Agent Scope：

| Scope | 用途 |
|---|---|
| `organization.read` | 读取本组织的非敏感元数据 |
| `agent.operate` | 执行 Agent 平面操作 |
| `agent.mail.read` | 读取 Feed 精确引用消息的有界纯文本正文 |
| `mailbox.use` | 使用显式授权的邮箱能力 |

Scope 不是角色的替代品。请求同时需要 Access Token Scope 和租户内有效 RBAC
权限；任一侧缺失都会默认拒绝。

## 1. 配置授权服务器

生成一组只用于 CopilotMail 签发 Access Token 的 RSA 密钥：

```bash
openssl genpkey -algorithm RSA \
  -pkeyopt rsa_keygen_bits:3072 \
  -out agent-oauth-private.pem
openssl pkey -in agent-oauth-private.pem \
  -pubout -out agent-oauth-public.pem
chmod 600 agent-oauth-private.pem
```

私钥文件必须是 PKCS#8 PEM，公钥文件必须是 SubjectPublicKeyInfo PEM。生产环境应通过
Docker Secret、Kubernetes Secret CSI 或等价密钥设施只读挂载，不能写入镜像、Compose
文件、环境变量或 Git。

配置全部变量：

```dotenv
COPILOTMAIL_AGENT_OAUTH_ISSUER=https://mail.company.example
COPILOTMAIL_AGENT_OAUTH_AUDIENCE=https://mail.company.example/api/v2
COPILOTMAIL_AGENT_OAUTH_TOKEN_ENDPOINT=https://mail.company.example/oauth2/token
COPILOTMAIL_AGENT_OAUTH_SIGNING_KEY_ID=agent-as-2026-01
COPILOTMAIL_AGENT_OAUTH_PRIVATE_KEY_FILE=/run/secrets/agent-oauth-private.pem
COPILOTMAIL_AGENT_OAUTH_PUBLIC_KEY_FILE=/run/secrets/agent-oauth-public.pem
COPILOTMAIL_AGENT_OAUTH_ACCESS_TOKEN_TTL_SECONDS=300
COPILOTMAIL_REQUIRE_AGENT_AUTH=true
```

生产环境仅允许 HTTPS。`COPILOTMAIL_AGENT_OAUTH_ALLOW_INSECURE_HTTP=true`
只对 `localhost`、`127.0.0.1` 和 `::1` 生效，并且生产模式仍会拒绝。
如果 `COPILOTMAIL_REQUIRE_AGENT_AUTH=true` 而配置或密钥缺失，readiness 会失败。

## 2. 注册 Agent

Agent 先在自己的密钥管理边界内生成客户端密钥：

```bash
openssl genpkey -algorithm RSA \
  -pkeyopt rsa_keygen_bits:3072 \
  -out billing-agent-private.pem
openssl pkey -in billing-agent-private.pem \
  -pubout -out billing-agent-public.pem
```

将公钥交给控制平面注册：

```bash
copilotmail-control register-agent \
  --organization-id "$ORGANIZATION_ID" \
  --display-name "Billing Reconciliation Agent" \
  --purpose "Reconcile invoice mailbox and open review tasks" \
  --environment production \
  --risk-level medium \
  --public-key-file billing-agent-public.pem \
  --scope organization.read,agent.operate,mailbox.use \
  --review-in-days 90 \
  --credential-expires-in-days 90
```

命令输出 JSON。保存其中的：

- `client_id`：Agent Principal UUID；
- `credential_id`：JWT Header 使用的 `kid`；
- `allowed_scopes`、`credential_expires_at` 和 `token_endpoint`。

注册命令会创建 Agent Profile、首个凭据、Agent Operator 角色分配、审计事件和
Outbox 事件，并在同一 PostgreSQL 事务内提交。Agent 必须关联当前组织的 Active Human
Owner，并带有用途、环境、风险等级和复核到期时间。

## 3. 构造 Client Assertion

JWT Header：

```json
{
  "alg": "RS256",
  "typ": "JWT",
  "kid": "<credential_id>"
}
```

Claims 示例：

```json
{
  "iss": "<client_id>",
  "sub": "<client_id>",
  "aud": "https://mail.company.example/oauth2/token",
  "iat": 1784937600,
  "exp": 1784937900,
  "jti": "019b0a6e-5f20-7be1-91eb-7bc9db77bfab"
}
```

`jti` 每次交换必须重新生成。Agent 使用与已注册公钥对应的私钥进行 RS256
签名，不得在 Header 中提供 `jwk`、`jku` 或 `crit`。

## 4. 换取 Access Token

```bash
curl --fail-with-body \
  -X POST "https://mail.company.example/oauth2/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "grant_type=client_credentials" \
  --data-urlencode "client_id=$CLIENT_ID" \
  --data-urlencode \
    "client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer" \
  --data-urlencode "client_assertion=$CLIENT_ASSERTION" \
  --data-urlencode "scope=organization.read mailbox.use"
```

成功响应：

```json
{
  "access_token": "<signed-jwt>",
  "token_type": "Bearer",
  "expires_in": 300,
  "scope": "mailbox.use organization.read"
}
```

Token Endpoint 的响应带有 `Cache-Control: no-store` 和 `Pragma: no-cache`。
错误使用 OAuth 2.0 的 `invalid_request`、`invalid_client`、`invalid_scope`
或 `temporarily_unavailable`。

服务端 Access Token 验证公钥发布在 `/oauth2/jwks.json`。生产多密钥热加载、
新旧 `kid` 重叠、强制保留窗口和回滚流程见
[授权服务器签名密钥轮换](AGENT_SIGNING_KEY_ROTATION.zh-CN.md)。

## 5. 无停机轮换

必须按“新增、切换、验证、撤销”顺序轮换，不能先撤销旧凭据：

```bash
# 1. Agent 侧生成新密钥，然后先登记新公钥。
copilotmail-control add-agent-credential \
  --organization-id "$ORGANIZATION_ID" \
  --client-id "$CLIENT_ID" \
  --public-key-file billing-agent-public-2026-10.pem \
  --scope organization.read,agent.operate,mailbox.use \
  --credential-expires-in-days 90

# 2. 将 Agent 切换到新私钥和命令输出的新 credential_id/kid。
# 3. 确认新凭据能换取 Token 且业务探针正常。
# 4. 立即撤销旧凭据。
copilotmail-control revoke-agent-credential \
  --organization-id "$ORGANIZATION_ID" \
  --credential-id "$OLD_CREDENTIAL_ID"
```

新旧凭据在切换窗口内都可用。撤销旧凭据后，旧凭据不能再次交换 Token，绑定旧凭据的
Access Token 也会在下一次受保护请求时失败。撤销前应保存审计事件 ID、变更单号和验证证据。

## 6. 原生 mTLS 与证书绑定 Token

### 6.1 配置可信 TLS 边界

生产环境应使用公司 PKI 分别签发服务端证书和带 `clientAuth` EKU 的 Agent
客户端证书。配置三个只读文件：

```dotenv
COPILOTMAIL_NATIVE_TLS_CERT_FILE=/run/secrets/control-plane-tls-cert.pem
COPILOTMAIL_NATIVE_TLS_KEY_FILE=/run/secrets/control-plane-tls-key.pem
COPILOTMAIL_NATIVE_TLS_CLIENT_CA_FILE=/run/secrets/agent-client-ca.pem
```

三个变量必须同时存在。启用后控制平面端口直接提供 HTTPS；Rustls 会请求可选客户端
证书。没有证书的 Human OIDC 请求仍可连接，但任何已提交的客户端证书必须能够构建到
配置的 Agent Client CA。

反向代理不得在控制平面前终止 Agent mTLS 后改用普通 HTTP，因为应用将无法获得原生
TLS 证明。若需要四层负载均衡，应使用 TCP/TLS passthrough。

### 6.2 登记 Agent 客户端证书

先确认 Agent 已存在，再登记客户端证书的叶证书 PEM：

```bash
copilotmail-control add-agent-mtls-credential \
  --organization-id "$ORGANIZATION_ID" \
  --client-id "$CLIENT_ID" \
  --certificate-file billing-agent-client.pem \
  --scope organization.read,agent.operate,mailbox.use \
  --credential-expires-in-days 90
```

命令仅存储证书 DER 的 SHA-256 指纹，不存储私钥。输出包含新的
`credential_id`、`certificate_fingerprint`、Scope 和凭据到期时间。
命令要求本部署已配置原生 TLS；证书是否属于受信 Client CA 会在实际 TLS
握手时由 Rustls 验证。

### 6.3 通过 mTLS 换取 Token

mTLS 方式不发送 `client_assertion`：

```bash
curl --fail-with-body \
  --cert billing-agent-client.pem \
  --key billing-agent-client-key.pem \
  -X POST "https://mail.company.example/oauth2/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  --data-urlencode "grant_type=client_credentials" \
  --data-urlencode "client_id=$CLIENT_ID" \
  --data-urlencode "scope=organization.read mailbox.use"
```

得到的 Access Token 是证书绑定 Token。调用任何受保护 API 时必须同时提供
Token 与同一客户端证书：

```bash
curl --fail-with-body \
  --cert billing-agent-client.pem \
  --key billing-agent-client-key.pem \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  "https://mail.company.example/api/v2/organizations/$ORGANIZATION_ID"
```

缺少证书、证书不同、证书不受 Client CA 信任、凭据已撤销或 RBAC/Scope
不匹配都会失败。

### 6.4 mTLS 证书无停机轮换

1. 由企业 PKI 签发新证书和新私钥。
2. 使用 `add-agent-mtls-credential` 登记新叶证书。
3. 将 Agent 切换至新证书，验证 Token 签发和受保护 API。
4. 使用通用 `revoke-agent-credential` 撤销旧 `credential_id`。
5. 在 PKI 侧按公司策略吊销旧证书并保留审计证据。

## 反向代理要求

Token Endpoint 当前只将 `X-Forwarded-For` 作为审计观测来源，不用于授权决策。公网部署
必须让受信任反向代理删除客户端传入的同名 Header 并重新设置；应用不能直接暴露在允许伪造
该 Header 的网络边界。限流、WAF 和异常 Client Assertion 告警应配置在受信任入口层。

## Agent 纯文本发信

Active Agent 在获得 `agent.operate mailbox.use` Token Scope、实时 RBAC 和目标邮箱
Send 成员关系或限时委托后，可使用与 Human UI 分离的显式机器工作流。`production`
Agent 还必须绑定一份已激活的不可变邮件策略，并通过服务端准入判定：

1. `POST /api/v2/mailboxes/{mailbox_id}/drafts` 保存有界纯文本草稿；
2. 使用返回的 CopilotMail `draft_id` 与 `version`，向
   `POST /api/v2/mailboxes/{mailbox_id}/submissions` 提交稳定
   `Idempotency-Key`；
3. 轮询 `GET /api/v2/mail-submissions/{submission_id}`，区分 `pending`、
   `reconciling`、`accepted`、`rejected` 与 `manual_review`。

Migration 0041 已建立由 Active Human 创建的 production Agent 不可变策略候选，
可表达规范收件域白名单、单封/小时/单地址额度、首次联系人审批和自动回复上限。
应用服务与数据库已提供精确双人审批、请求人/执行人绑定、版本单调前移及原子
Audit/Outbox 的不可变激活闭环，并提供 Human-only 候选创建、精确查询和版本激活
API。Migration 0042 进一步让 Draft 持久化规范域名与租户/Agent 范围的不可逆
地址摘要；Submission 在同一 PostgreSQL 事务中按 Agent 串行校验域白名单、单封/
小时/单地址每日额度、同线程熔断和首次联系人精确审批，并写入不可变 Admission。
Worker 只租赁拥有已提交 Admission 的 production Agent Intent。任一策略、审批、
额度或证据缺失时仍保持默认拒绝。

production Agent 应按 HTTP 状态、稳定 `code`、`retryable` 和 `Retry-After` 编排，
不得解析错误消息文本，也不得通过更换 `Idempotency-Key` 绕过拒绝：

| HTTP | 稳定错误码 | 含义与动作 |
|---|---|---|
| 422 | `agent_mail_recipient_policy_denied` | 至少一个收件域不在活动策略中；响应不会披露地址。停止自动重试，由 Human 调整草稿或策略。 |
| 429 | `agent_mail_hourly_quota_exceeded` | Agent 小时额度已用尽；按 `Retry-After` 退避。 |
| 429 | `agent_mail_recipient_daily_quota_exceeded` | 某收件人日额度已用尽；响应不会披露地址，按 `Retry-After` 退避。 |
| 429 | `agent_mail_thread_circuit_breaker_open` | 同线程自动回复熔断；按 `Retry-After` 退避或转人工。 |
| 409 | `agent_first_contact_approval_required` | 首次联系人需要 `agent.first_contact` 审批。按响应中的 `resource_kind=agent.mail_draft` 与 `resource_id_format={draft_id}:{expected_draft_version}` 创建精确绑定的审批，再使用原 Idempotency Key 重放。 |

上述 `details.scope` 只标识策略维度，不包含邮箱地址、地址摘要或引擎标识。普通草稿版本/
幂等冲突也使用 409，但有不同错误码；Agent 必须按 `code` 分支。

在创建草稿或提交前，Agent 可使用同一个短期 Token 查询自身当前约束：

```bash
curl --fail-with-body \
  --cert billing-agent-client.pem \
  --key billing-agent-client-key.pem \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  "https://mail.company.example/api/v2/agent/mail-policy"
```

该接口只要求 `agent.operate`，租户和 Agent ID 完全来自认证上下文。返回：

- Token Scope 与实时 RBAC 的有效权限交集；
- `development`、`testing` 或 `production` 环境；
- `not_required`、`blocked_no_active_policy` 或 `active` 准入状态；
- 活动策略的允许域、单封/小时/单地址/线程上限与首次联系人审批要求；
- 数据库观察时刻、当前滚动一小时窗口的 Admission 汇总数和剩余额度。

production Agent 没有活动策略时返回 `blocked_no_active_policy`，而不是伪造默认额度。
单地址日额度与线程额度只返回配置上限；接口不会返回地址、地址摘要、收件人级计数、
Thread 引用、审批 ID、Message ID 或邮件引擎标识。查询也受跨副本 Agent Ingress
窗口约束，429 时按 `Retry-After` 退避。成功响应带 `Cache-Control: no-store`；
每次发送决策前需要新鲜状态时应重新查询，不能把快照当作额度预留。

Agent 不能提交 `from`、JMAP Email ID、Thread ID、Identity ID 或 Submission ID；
From 始终由授权邮箱派生。Draft 精确绑定当前 Agent，Human 与其他 Agent 不能复用。
Submission 请求再次验证 Active Agent、Token Scope、RBAC 与 Send ABAC，并把
Idempotency Key 绑定到租户、Agent、邮箱、Draft 和版本。响应丢失时重放相同请求，
不得生成新 Key 后盲目重发。

正文只进入隔离的 JMAP Compose Adapter；控制平面只保存 SHA-256 与远端不透明引用，
Audit、Outbox、日志和指标不复制正文或收件人。Agent 的草稿、提交和状态读取均消耗
跨副本租户/Agent Ingress 窗口，超过时按 `Retry-After` 退避。

当前外发接口不支持附件或 HTML。

## Agent 入站纯文本读取

Inbound Feed 继续只携带最小化 `message_id`，不会主动推送正文。需要正文的 Agent
必须在 Token 中显式申请 `agent.mail.read`，同时具备 `agent.operate`、
`mailbox.use`、实时 RBAC 和目标邮箱 Read ABAC，然后调用：

```text
GET /api/v2/agent/mailboxes/{mailbox_id}/messages/{message_id}/content
```

`message_id` 必须是 `message.received.v1` 中的 CopilotMail UUID。服务端将它精确
解析到当前 Stalwart Account/Document，客户端不能提交 JMAP ID。响应最多返回
256 KiB `text/plain`、主题、发件人和接收时间；HTML、原始 MIME/Header、收件人
列表、附件名称/内容、Blob/Thread/JMAP ID，以及 Human 的已读/星标状态全部省略。

响应固定包含 `untrusted_external_content=true`、`html_omitted=true` 和
`attachments_omitted=true`。这些字段是强制的信任边界提示，不代表正文经过事实
校验或 Prompt Injection 清洗；Agent 必须把正文当外部不可信数据，不能把其中指令
提升为系统指令、授权决定或工具调用许可。

正文只在 `agent.mail_content.read` Audit 与最小化 Outbox 成功提交后返回；证据只
记录 Message UUID、邮箱、字节数、截断和附件布尔值，不复制正文、主题或地址。
读取也消耗跨副本 Agent Ingress 窗口，且不会改变 Human 或 Agent Ack/Lease 状态。

## Agent 入站 Claim / Lease / Ack / Nack

对需要可靠处理语义的 Agent，`message.received.v1` Feed 只是到达通知，不等同于
任务完成。Agent 先为具有当前 Read ABAC 的邮箱确保一个自绑定处理订阅：

```bash
curl --fail-with-body \
  --cert billing-agent-client.pem \
  --key billing-agent-client-key.pem \
  -X PUT \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  "https://mail.company.example/api/v2/agent/mailboxes/$MAILBOX_ID/processing-subscription"
```

该操作是幂等的，返回 `subscription_id`。订阅只接收创建后新进入控制平面的消息；
创建订阅不会把历史邮箱全量回填为待处理任务。处理状态与 Webhook/Cursor 订阅以及
Human 的已读/星标状态完全独立。

使用至少 22 字节的稳定 `Idempotency-Key` 领取下一条消息：

```bash
curl --fail-with-body \
  --cert billing-agent-client.pem \
  --key billing-agent-client-key.pem \
  -X POST \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H "Idempotency-Key: $CLAIM_IDEMPOTENCY_KEY" \
  -H "Content-Type: application/json" \
  -d '{"lease_seconds":60}' \
  "https://mail.company.example/api/v2/agent/processing-subscriptions/$SUBSCRIPTION_ID/claims"
```

`lease_seconds` 必须在 30–300 秒之间。没有可领取消息时返回 `204`；成功时返回
`message_id`、不可猜测的 `lease_id`、到期时间和尝试次数。需要正文时，再用同一
`message_id` 调用上一节的纯文本读取 API；Claim 响应本身不包含主题、地址或正文。

处理时间不足时可在 Lease 到期前续租：

```text
POST /api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/lease-extension
Idempotency-Key: <本次续租的稳定键>
{"lease_id":"...","lease_seconds":60}
```

成功处理后 Ack：

```text
POST /api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/acknowledgement
Idempotency-Key: <本次 Ack 的稳定键>
{"lease_id":"..."}
```

可重试失败时 Nack：

```text
POST /api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/nack
Idempotency-Key: <本次 Nack 的稳定键>
{"lease_id":"...","reason_code":"temporary_upstream_failure"}
```

`reason_code` 只能是 1–100 字节的小写机器码，不得携带原始提供商响应、正文、地址、
Token 或 Secret。前四次失败回到 `pending`；第五次进入 `dead_lettered`。每次重新
Claim 都产生新的 Lease 并递增尝试次数。

计划停机前应暂停处理订阅：

```text
PATCH /api/v2/agent/processing-subscriptions/{subscription_id}
Idempotency-Key: <本次生命周期操作的稳定键>
{"expected_status":"active","status":"paused"}
```

暂停继续把入站消息 Fanout 为持久 `pending` backlog，但立即停止新 Claim；已经持有
的未过期 Lease 仍可续租、Ack 或 Nack，因而暂停期间不丢消息且可安全排空当前工作。恢复使用
`{"expected_status":"paused","status":"active"}`，服务端重新校验 Active Agent 和
当前邮箱 Read ABAC。退役使用目标状态 `retired`，可从 `active` 或 `paused` 进入，
但有未过期 Lease 时返回 `409`；退役是终态，不能恢复或重新创建同一默认订阅。
生命周期状态、不可变操作账本、Audit 和 Outbox 在同一事务提交，精确重放返回原操作。

Agent 可查询自己订阅中的终态失败：

```bash
curl --fail-with-body \
  --cert billing-agent-client.pem \
  --key billing-agent-client-key.pem \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  "https://mail.company.example/api/v2/agent/processing-subscriptions/$SUBSCRIPTION_ID/dead-letters?limit=50"
```

响应按失败时间倒序返回最多 100 条，只包含 `message_id`、尝试次数、有界
`reason_code` 和失败时间。非空 `next_cursor` 精确绑定当前订阅和真实死信锚点；
伪造、跨订阅或已经不再可见的锚点会拒绝，不会降级为从头查询。服务端在返回结果
前提交 `agent.message_dead_letters.queried` Audit 与最小化 Outbox。查询不披露
正文、主题、地址、邮件引擎 ID、原始错误响应、Human 状态或其他 Agent 的状态。

所有操作同时要求 Agent 身份、`agent.operate mailbox.use` Token Scope、实时 RBAC、
Active Principal、订阅所有权和当前邮箱 Read ABAC。新 Claim 只允许 `active` 订阅；
Ack、Nack 和续租允许 `active` 或 `paused` 订阅，但还必须提供同一 Agent 持有的、
尚未过期且与消息精确匹配的 Lease。`retired` 订阅不能继续处理。过期 Lease、旧 Lease、
其他 Agent 的 Lease 或跨租户 ID 均默认拒绝。

每个状态变更、Audit 和 Outbox 在一个 PostgreSQL 事务内提交；操作账本不可更新或
删除。响应丢失时必须使用相同 `Idempotency-Key` 与完全相同的请求重放；同一键改变
参数返回 `409 agent_processing_conflict`，不能通过轮换键并发推进同一 Lease。
全部接口都消耗跨副本 Agent Ingress 窗口，`429` 时遵循 `Retry-After`。

## 当前限制

- 尚未提供在线 Agent 注册/轮换管理 API 或管理 UI，当前使用受控离线 CLI。
- 尚未实现 SPIFFE/SPIRE 或云工作负载身份联邦。
- Client CA、服务端证书和服务端私钥变更当前需要滚动重启；动态 CRL/OCSP
  与在线信任包热加载尚未实现。
- 当前签名 Key Ring 使用文件 Manifest 热加载；尚未直接集成 HSM/KMS 签名 API。
- PostgreSQL 真实并发/撤销集成测试由 CI 的 PostgreSQL 17 执行；无数据库的本地测试会明确跳过。
