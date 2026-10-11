# Agent Inspector API 实施计划

> 状态：阶段 A 与管理端显式检视面已实施；认证态 HTTP 夹具已接入 PostgreSQL CI，待该
> CI 环境给出最终数据库/路由验收。P3 的精确 Agent 发件证据读取已接入 Application、
> PostgreSQL、HTTP、契约与 OpenAPI，仍须完成本轮完整门禁与数据库场景验收。本文定义
> Human 管理员观察单个 Agent 及其一条明确 production 发件的最小、安全契约；它不把
> 控制面记录误称为 Agent 运行时监控。

## 实施状态（2026-08-24）

阶段 A 已落地为 `GET /api/v2/agents/{agent_id}/inspector`：

- `copilotmail-control` 在认证后先拒绝非 Human、缺少 `identity.manage` 权限或受限
  bearer 缺少同名 scope 的调用；成功响应固定 `Cache-Control: no-store` 与
  `Referrer-Policy: no-referrer`。
- `copilotmail-application` 在同一控制面事务中先后两次复核 Active Human 与
  `identity.manage`，在两次复核之间构建快照，并将成功读取的
  `identity.agent.queried` Audit 和 `identity.agent.queried.v1` Outbox 原子提交。
- PostgreSQL 适配器只投影本计划中的受控字段；它对 Owner、凭据、RBAC、邮件策略、
  订阅状态与时间不变量做严格校验。缺失目标/跨租户/非 Agent/缺 profile 统一为
  `404`，完整性或持久化异常统一为 `503`。
- `web/admin-app` 已提供双语的“Agent 检视”入口：管理员必须主动输入规范 UUID，
  前端以同源 Cookie 发送一次 `cache: no-store` 的 GET，不保存浏览器侧 Bearer，且不
  预取、轮询、列举或缓存快照。切换查询会丢弃旧结果并以请求代次拒绝迟到响应；离开
  视图即释放内存状态。

该阶段没有新增 Agent 运行时遥测、全量枚举、邮箱/委托摘要、凭据材料或邮件内容。

## 1. 目标与已证实的数据边界

V1 只提供同一租户内**单个 Agent**的只读快照，供 Human/Admin 判断其身份、受托
Owner、当前可取得的 Agent Token 能力，以及邮件控制面的有限状态。

现有控制面可以可靠读取：

- `AgentProfile`：环境、风险级别、用途、复核到期时间和 Owner principal ID；
- Agent/Human principal 的显示名与生命周期状态；
- Agent 凭据的活动数、最早到期时间及最近认证时间；凭据的许可 scope；
- 当前 RBAC 与活动凭据相交后，某个 Agent 在观测时刻可取得的 Token scope；
- Agent 邮件策略是否满足生产环境准入条件，以及订阅状态计数和最近一条控制面处理
  操作的时间。

下列内容没有可证明的数据模型，V1 不得展示或推断：

- Agent 进程是否在线、心跳、部署位置、当前任务、execution ID、队列积压或运行时
  健康；
- Owner 转移历史、业务责任链，或 mailbox 的所有权；
- 任何凭据指纹、公钥、证书、密钥、原始 token、认证来源/IP；
- mailbox ID/地址、授权/委托明细、邮件主题、正文、收件人、策略中的收件人证据；
- “最近认证”或“最近处理”以外的在线性结论。

当前 `agent_profiles.owner_principal_id` 未由数据库约束为同租户 Human。因此实现必须
在查询中验证 Owner 的 tenant 与 kind；不满足时不得返回未经验证的 Owner 数据。

## 2. 最小 HTTP 契约

已新增：

```http
GET /api/v2/agents/{agent_id}/inspector
```

该端点仅返回一个聚合快照，故不需要分页、游标或列表筛选。以后如需 Agent 列表，必须
另设端点、另行定义稳定排序和租户绑定游标，不能把 V1 扩展成全量枚举接口。

响应建议保持为以下语义（字段名称可在 contracts/OpenAPI 阶段最终固定）：

| 区块 | 最小字段 | 语义与限制 |
| --- | --- | --- |
| Agent | `agent_id`、`display_name`、`principal_status` | 目标必须是同租户 Agent；`principal_status` 是身份生命周期而非运行健康。 |
| Owner | `principal_id`、`display_name`、`principal_status` | 仅在 Owner 被验证为同租户 Human 时返回。Owner 已停用可如实显示状态；kind/tenant 不一致是完整性错误，不降级泄露原始 ID。 |
| Profile | `environment`、`risk_level`、`purpose`、`review_due_at`、服务端派生的 `review_state` | `review_state` 只说明是否到期/待复核，不能表示“已通过人工复核”。 |
| Token access | `state`、`effective_scopes`、`active_credential_count`、`next_credential_expiry_at`、`last_authenticated_at` | `effective_scopes` 是在 `observed_at` 下，至少一份活动且未过期凭据与当前 RBAC 相交后可签发的 scope 集合；不是某一张 token 的声明，也不是 mailbox 授权。`state` 应能区分无活动凭据、无有效 scope、身份非 active 和有效。 |
| Control plane | `mail_admission_state`、`active_processing_subscription_count`、`paused_processing_subscription_count`、`retired_processing_subscription_count`、`last_processing_activity_at` | 生产环境没有活动邮件策略时应明确为阻断；开发/测试环境为不要求策略。最后处理时间只代表控制面记录，不能声称 Agent 正在运行。 |
| 观测 | `observed_at` | 由服务端在同一查询/事务边界取得；客户端不得提供。 |

`effective_scopes` 只能来自既有 `Permission::AGENT_TOKEN` 白名单；当前白名单为
`organization.read`、`agent.operate`、`agent.mail.read`、`mailbox.use`。响应不得按凭据
分别列出 scope，以免让管理员把“凭据允许范围”误读为实际、持续有效的 Agent 权限。

成功响应应设置 `Cache-Control: no-store`；浏览器响应另设置 `Referrer-Policy: no-referrer`。

## 3. 默认拒绝的授权、租户与审计规则

1. 先通过既有 `authenticate_api_request()` 完成认证，再访问任何 Inspector 存储或对象；
   未认证为 `401`。
2. 调用方必须显式是 `Human`，拥有本租户 `identity.manage`，并且 bearer scope（如有）也
   包含 `identity.manage`。`agent.operate` 不能替代此条件，因为 Agent 本身可取得该权限。
   不满足为 `403`。

当前 `AuthenticatedPrincipalKind` 的认证线上枚举只有 `Human` 与 `Agent`，尽管领域
`PrincipalKind` 已有 `Service`。因此本阶段的 HTTP 回归夹具**不会**把 Service 凭据混称为
Agent 来伪造覆盖；Service wire kind、对应认证适配器及所有 machine 路由的最小权限审计是
后续独立的身份模型工作。在该模型完成前，Inspector 对已认证 Agent machine 的 `403` 已有
回归覆盖，但不能声称 Service 调用方已完成端到端验收。

3. 在写审计事件前的最终事务检查中，调用方仍须是目标租户的 active Human；避免会话解析
   与提交之间的撤权竞态。
4. 所有目标、Owner、凭据、RBAC、策略和订阅查询都以认证上下文的 `organization_id` 过滤；
   缺失、跨租户、非 Agent 或无 Agent profile 一律为不可见的 `404`，不得区分原因。
5. Owner 的 tenant/kind、scope 枚举或聚合数据若违反领域不变量，返回通用 `503`/不可用，
   不返回部分快照或脏数据。
6. 仅成功读取生成审计证据：目录动作为 `identity.agent.queried`、资源种类
   `identity.agent`、风险 `high`、`privileged=true`，并原子写入相应
   `identity.agent.queried.v1` outbox 事件。选择 `queried` 是为复用现有审计治理动词。
   审计 detail 只可含目标 ID、响应版本、调用上下文和无敏感聚合状态；不得含 purpose、
   scope、凭据、邮箱或邮件数据。
7. 审计目录必须同时更新 domain catalog、PostgreSQL migration catalog 和应用字符串字面量；
   业务读取、审计事件和 outbox 必须在同一事务提交。失败、拒绝和 `404` 不新增读取审计，
   与现有查询接口惯例保持一致。

该只读 Inspector 不额外要求 `audit.read`；查看审计日志本身仍使用既有 `audit.read` 边界。
这不适用于下面独立的 P3 发件证据读取；它显式要求两个权限。

## 4. 建议的实现分期

### 阶段 A：最小、可审计的单 Agent 快照（已实施）

- 已在 `copilotmail-application` 定义 `AgentInspectorSnapshot` 及其组成 read model、
  `GetAgentInspectorCommand`、服务错误类型和 `AgentInspectorService`；将读取口放入
  `ControlPlaneTransaction`，以便最终 Active Human 检查、读取、审计和 outbox 原子化。
- 已在 `copilotmail-postgres` 用同租户 join 与聚合一次取得 principal/profile/Owner、有效凭据、
  当前 RBAC 交集、策略和订阅统计；禁止 N+1。任何不认识的枚举或 Owner 不变量错误映射为
  `StoreError::Unavailable`。
- 已在 domain 审计目录和新的 migration 中登记 `identity.agent.queried`；不借此迁移改变
  Agent Owner 数据模型。
- 已在 `copilotmail-contracts` 增加严格反序列化的响应契约（`deny_unknown_fields`），在
  `copilotmail-control` 增加路由、Human-kind 检查、`identity.manage` scope 检查及安全响应头，
  并同步 `docs/openapi-v2.yaml`。
- 已在 `web/admin-app` 增加严格 TypeScript 枚举、同源 `cache: no-store` 客户端、原创
  导航图标与响应式事实面板。它只渲染此计划列出的字段；null 时间以明确的“无记录”
  文案显示，绝不以运行健康或虚拟仪表盘补全。错误处理统一保护 `404` 目标原因，且不
  渲染服务端 `details` 或未知 JSON。

### 阶段 A.1：生产 Agent 发件的精确证据（P3，后端已接入）

新增的端点为：

```http
POST /api/v2/agents/{agent_id}/mail-submissions/{submission_id}/evidence
```

它保持“单对 UUID 的精确读取”语义，不是 Inspector 的隐式扩字段、Agent 发件列表或邮件
数据写入；HTTP 方法却必须为 `POST`。原因是每次**成功**观测都要原子追加 High 风险的
`agent.mail_submission_evidence.queried` Audit 和同名版本化 Outbox，而不是无副作用的
缓存读。Admin Browser Session 的此类 `POST` 走控制面既有 Origin/CSRF 状态变更保护；
因此前端不得预取、轮询或自动重放它。

认证和 Human/RBAC 检查完成后，路径中的两个字符串才会按规范 UUID 解析；调用者必须是
Active Human、拥有本租户 `identity.manage` 与 `audit.read`，受限 bearer 也必须同时含有
两个 scope。Application 在同一控制面事务中先后复核读取资格和最终 Active Human 资格；
读取投影、Audit 与 Outbox 先原子提交，才返回该次证据。

响应严格限制为：

| 区块 | 允许字段 | 不能据此作出的结论 |
| --- | --- | --- |
| 提交 | 精确 submission UUID、生命周期状态、创建/更新时间 | 已接受不等于已投递；不是发件目录。 |
| 准入 | admission ID/时间、策略 activation/version 及批准请求 ID、最小收件计数、可选首次联系人批准请求 ID | 不泄露收件人、地址、规则正文或完整决策理由。 |
| 投递 | 可选的最新不可变聚合：观测时间、terminal、收件人/已投递/失败/排队/未知计数 | `null` 仅表示本响应无快照；聚合不代表投递成功，也不公开 snapshot ID、提供方或 SMTP。 |
| 审计引用 | 请求、准入以及终态时的终态动作的 event ID/action/time | 不是完整审计事件详情或执行轨迹。 |

#### PostgreSQL 权威时间与因果绑定

浏览器不提供、也不得修正任何时间。`submission.created_at`、`submission.updated_at`、
`admission.admitted_at`、策略 `activated_at`、投递快照 `observed_at`、审计引用
`occurred_at` 和响应 `observed_at` 都由 PostgreSQL 的持久记录或本次查询内的服务端时钟
产生。审计引用不是“某个早于本次读取的模糊日志”：请求审计时间必须等于
`submission.created_at`，准入审计时间必须等于 `admission.admitted_at`，终态审计时间必须
与该提交的精确终态动作绑定，并受准入、提交状态和本次观测时间约束。

批准时间只用于历史校验，不额外暴露批准步骤详情。策略激活批准的请求创建/完成/批准步骤
必须不晚于策略激活，且在当时尚未过期；首次联系人批准同样必须在 `admitted_at` 前已经
完成并有效。较晚创建、完成或决定的批准不能倒灌为早先策略激活或准入的证据，任何时间
矛盾都以 `503` fail closed，而不是返回“现在看起来已批准”的部分结果。

`agent_mail_admissions` 是在初始 intent 之后追加的不可变记录，因此 pending intent 的
初始 `updated_at` **不是** `admitted_at` 的上界：准入可以晚于那次初始更新。`accepted_at`
保留在内部而不进入 Human 响应；它只用于证明投递快照满足
`admitted_at ≤ accepted_at ≤ delivery.observed_at ≤ response.observed_at`。终态写入中的
`updated_at` 与 `accepted_at` 可由独立服务端时钟赋值，界面不得从两者的显示顺序推导额外
因果关系。

本次返回的 `observed_at` 同时绑定读取审计和 Outbox：前者记录精确 Agent/submission、
提交状态、准入/策略 ID、是否有投递快照和审计引用数量，后者记录同一目标、版本、状态及
快照存在性。两者均不携带正文、收件人、SMTP/提供方或批准内容；任一写入不能提交时，
响应不得泄露读取结果。

缺失、跨租户、非 Agent、Agent/submission 不匹配和非 production 准入目标统一映射为
`404`；调用方不能从响应区分原因。完整性异常统一 `503`，不降级返回部分证据。成功响应
必须带 `Cache-Control: no-store` 与 `Referrer-Policy: no-referrer`。拒绝、失败或 `404`
不会伪造“读取成功”审计。

管理端只能在当前单 Agent snapshot 成功后显示第二个、显式的 submission UUID 输入；不
预取、轮询、枚举、缓存或把目标写入 URL/历史/持久化存储。页面以提交 → 准入/批准 →
可选投递聚合排列，审计引用放入默认收起的 `EvidenceInspector`。任何运行状态、心跳、
执行/委托、邮箱、收件人、SMTP/提供方或正文模块都不属于此契约，不能以占位卡、图标或
推测文案出现。

### 阶段 B：数据完整性与可扩展观察面

- 单独设计并迁移 Owner 同租户 Human 约束、Owner 变更流程和审计历史；在此之前 Inspector
  只验证、绝不修复或猜测坏数据。
- 如产品确实需要，另行设计有 `authorization.manage` 边界的 mailbox/委托摘要；先定义安全
  投影，再决定是否可显示邮箱标识，不能复用 V1 的聚合响应。
- 如需 Agent 列表，加入独立、分页的目录 API 和明确的最小投影，避免凭 `identity.manage`
  获得无界枚举能力。

### 阶段 C：真实运行观测（独立项目）

只有在 Agent runtime 明确上报、签名并保留心跳/执行状态/失败原因后，才能增加在线状态、
任务与 execution 视图。该模型需要自己的留存、访问控制、审计和隐私评审，不能从现有
订阅或处理操作表推导。

## 5. 验证清单

当前本地证据：contracts 17 项单测、application 108 项单测、PostgreSQL adapter 检查及
控制层 HTTP 测试二进制编译均已通过；但本机没有可丢弃的 PostgreSQL 数据库。因此这些
`#[ignore]` 场景仍须由 CI 的 PostgreSQL 环境执行，不能把本地编译替代为数据库验收。

P3 读取是在上述 P2 基线之后新增的。其 contracts、Application、PostgreSQL adapter 与
控制层的针对性单元测试可在本机验证；完整 workspace 门禁、管理端交互回归以及依赖真实
PostgreSQL 的精确双 UUID/历史批准语义场景仍必须在本轮收口后重跑。不得把旧的 P2 全量
计数或仅编译的 ignored 测试当作这条新接口已经完成数据库/部署验收的证据。

当前控制层已覆盖未认证（含 malformed `agent_id`）的 `401`、安全响应映射与
响应最小化。另有一个仅在可丢弃 PostgreSQL 中运行的 `#[ignore]` HTTP 夹具：它以真实
`PgControlPlaneStore`、真实 Router 和由公开 `HumanSessionService` 创建的持久化 Admin
`HumanSession` Cookie，且显式开启 `human_session_required=true`。成功、缺少
`identity.manage` 的 active Human `403`、跨租户已存在目标与缺失目标的同构 `404` 都走该
BFF Cookie，不以 Bearer 冒充 Human 会话；同一夹具还断言 verified Human Bearer 返回
`401 human_session_required`，以及 Agent machine 返回 `403`。为避免伪造 OIDC 提供方，
夹具只构造已建立会话后的服务端证据，不调用登录或回调路由；会话解析、client-key 校验、
RBAC、租户隔离和响应头仍全部经过真实 Router。成功 `200` 断言
`Cache-Control: no-store` 与 `Referrer-Policy: no-referrer`。该夹具由 PostgreSQL CI 显式
执行；本机没有可用数据库时只能编译而不能替代该验收。由于上一段所述 Service wire-kind
缺口，它不声称 Service 调用方已得到端到端覆盖。

该 HTTP 夹具会运行迁移并插入隔离组织、principal、profile、role assignment、审计与
outbox 记录，因此它要求 `COPILOTMAIL_ALLOW_AGENT_INSPECTOR_HTTP_TEST_WRITES=1` 的显式
一次性确认；CI 仅向临时 PostgreSQL service 设置该变量。**不得**对生产 NixOS 的数据库
设置此变量或运行该忽略式测试，哪怕该测试的记录使用随机 UUID 也会污染生产审计账本。

- 应用层单测：未通过 Human/`identity.manage`/scope 预检查时 Store 不得被调用；成功路径必须
  断言审计、outbox 与 commit 同时发生。
- HTTP 测试：未认证在触达数据库前返回 `401`；开启 Human BFF session policy 后，Human
  Bearer 返回 `401 human_session_required`，Agent machine Bearer 返回 `403`，持久化真实
  Admin Cookie 的缺权 Human 返回 `403`；跨租户、非 Agent、无 profile 返回同一 `404`；
  成功响应带 `no-store` 与 `no-referrer`。Service wire kind 在身份模型完成前单列为未验收项，
  不能借 Agent 覆盖替代。
- PostgreSQL 集成测试：Owner 异租户或非 Human、过期/撤销凭据、RBAC 收敛、生产无策略、
  三种订阅计数和审计原子性均 fail closed；测试不读取或断言敏感字段。
- 契约/OpenAPI 测试：未知响应字段拒绝、枚举与 JSON 名称一致；文档明确时间字段的非运行时
  语义。
- 管理端测试：首次进入不读、畸形 UUID 本地拒绝、精确同源 no-store 路径、空时间语义、
  统一 `404`、以及连续请求迟到响应隔离；不使用示例 Agent 或静态状态卡。
- P3 Application/HTTP 测试：缺少 Human、`identity.manage`、`audit.read` 或任一 bearer
  scope 时不得访问存储；认证前的畸形双 UUID 必须仍是 `401`，认证后才是规范 UUID 的
  `400`；缺失、跨租户、非 Agent、双 ID 不匹配和非 production 准入统一为 `404`，成功
  `POST` 读取带 `no-store`/`no-referrer`，Admin Cookie 请求经过 Origin/CSRF guard，并原子
  写入读取审计和 Outbox。
- P3 PostgreSQL/契约测试：策略激活批准与首次联系人批准按准入时刻重新验证；投递仅为
  可选聚合且只允许对应 accepted 提交；审计引用必须精确匹配提交请求、Agent 准入和终态
  动作。测试还必须覆盖 admission 晚于初始 pending intent `updated_at` 仍合法、内部
  `accepted_at` 对投递因果的约束、以及迟到批准不能倒灌历史。未知/敏感字段、snapshot ID、
  邮箱、收件人、SMTP/提供方、正文、运行/执行/委托字段一律被响应契约与浏览器解析拒绝。
- P3 管理端测试：仅在当前 Agent Inspector 快照成功后显示 submission 输入；不预取、
  轮询、自动重放或持久化双 UUID；显式 `POST` 使用同源 Cookie 的 Origin/CSRF 语义；
  `delivery=null` 不被描述为失败；审计引用默认收起且展开按钮具有正确焦点和
  `aria-expanded` 语义。
- 合并门禁：`cargo fmt --all --check`、
  `cargo clippy --workspace --all-targets --locked -- -D warnings`、
  `cargo test --workspace --locked`、`python scripts/check_audit_governance.py`、
  `python scripts/check_documentation.py`、管理端/邮件端 TypeScript 检查、测试与构建。
