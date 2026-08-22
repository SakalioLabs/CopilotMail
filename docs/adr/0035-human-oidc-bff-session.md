# ADR 0035：Human OIDC 使用服务端 BFF Session

- 状态：Accepted
- 日期：2026-07-27

## 背景

企业实施计划要求浏览器使用 `HttpOnly`、`Secure`、`SameSite` Cookie。现有两个 SPA
直接执行 Authorization Code + PKCE，并在 JavaScript 内存中持有和发送 Access
Token。虽然没有写入持久浏览器存储，这仍扩大了 XSS 后的凭据窃取面，也无法形成统一
的登录、Session 注销和吊销证据。

Human 与 Agent 的认证边界必须继续分离：Human 使用企业 OIDC 浏览器会话；Agent
继续使用短期 Scope Token 与可选 mTLS 绑定，不能借用浏览器 Cookie。

## 决策

控制平面提供按受信浏览器客户端注册的登录、回调、Session 查询和注销接口。服务端
执行 Authorization Code + PKCE：

1. 登录开始时生成独立的 OIDC `state`、浏览器绑定 nonce 和 PKCE verifier。
2. PostgreSQL 只保存 state 与 nonce 的 SHA-256 摘要；PKCE verifier 使用部署提供的
   32 字节密钥加密后保存，明文不落库。
3. 回调同时要求 state 和 Host-only 登录 Cookie，原子消费登录事务；重放、过期、
   客户端或回调绑定不一致均 Fail Closed。
4. Token Endpoint 响应只在服务端处理。Access Token 经现有严格 OIDC
   issuer/audience/JWKS 校验后只留下独立 SHA-256 摘要用于证据绑定，原文立即
   丢弃且不返回给浏览器；Refresh Token 一律拒绝。
5. 创建至少 256 bit 随机 opaque Session Token；数据库仅保存其 SHA-256 摘要。
   Session 精确绑定组织、Active Human Principal、OIDC `jti`、浏览器客户端和上游
   Token 到期时间，且最长不超过部署配置的八小时上限。
6. Session 创建、注销以及相应 Audit/Outbox 必须原子提交。每个请求仍实时解析
   Principal 状态和 RBAC；停用或会话过期立即拒绝。
7. 登录开始使用 PostgreSQL 固定窗口执行跨副本、按客户端的硬 admission；超限不
   新增事务。过期且未形成 Session 的 pre-auth 记录允许受控、有界清理，已经形成
   Session 的登录、会话和撤销证据继续追加式不可变。

生产 Cookie 按客户端使用
`__Host-copilotmail-{admin|mail}-{login|session}`，固定 `Path=/; HttpOnly;
Secure` 且不设置 `Domain`。短期登录关联 Cookie 使用 `SameSite=Lax` 以完成 IdP
顶层回调；已登录 Session Cookie 使用 `SameSite=Strict`。只有非生产 loopback
环境可以显式启用非 Secure 开发 Cookie。

Cookie 认证的非安全方法必须携带与 Session 客户端完全一致的 `Origin`。登录的
`return_to` 只能是相对站内路径；公开 Origin、Client ID、Scope 和回调路径均来自
服务端配置，不能由浏览器自由指定。Mail Session 只能进入 Human 邮件 API，
Admin Session 不能读取正文或写信，两个 Cookie 同时存在或 Cookie 与任意
`Authorization` Header 并存均按身份歧义拒绝。

审计下载等会产生业务证据的读取动作使用 POST 并进入 Origin 校验，不提供带副作用
的 GET 降级路径。所有 `/api/v2/auth/*` 响应统一 `no-store` 与
`Referrer-Policy: no-referrer`；反向代理不得把登录/回调查询串写入访问或错误日志。

当生产配置要求 BFF Session 时，Human Bearer Token 不得作为降级路径。Agent
Bearer/mTLS 鉴权保持原有独立路径。

## 结果与边界

- Access Token、Refresh Token、PKCE verifier 和原始 Session Token 不进入前端
  JavaScript、日志、审计或数据库明文字段。
- Session 撤销、Human 停用和实时 RBAC 变更可立即生效。
- 多个 SPA 使用各自注册的公开 Origin/Client ID 和不同 Cookie Host；端口不能
  隔离 Cookie，因此同 Host 的 Admin/Mail Origin 在配置阶段直接拒绝。
- 当前不保存 Refresh Token；上游 Access Token 到期后需要重新登录。
- IdP Back-Channel Logout、连续访问滑动续期、全设备 Session 管理和全局匿名失败
  登录安全账本属于后续增量，不得用它们作为绕过当前默认拒绝的理由。
