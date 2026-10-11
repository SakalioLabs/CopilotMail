# CopilotMail 工程审计报告

> 审计日期：2026-09（本轮）
> 审计基线：分支 `codex/nixos-dev-snapshot-20260822` 工作树（认证围栏本轮已接通，
> 见 §4.1）
> 审计范围：V1 根 crate、V2 全部 14 个 crate、`apps/copilotmail-control`、
> `web/` 三个 npm 包、PostgreSQL 迁移、质量与部署资产
> 审计方法：逐模块功能走查 + 静态代码质量审查 + 客观门禁实测 + 目标文档比对
>
> **重要边界**：本报告所有"通过"均为本地编译/静态/单元证据。32 个 PostgreSQL
> 集成场景、MinIO Object Lock、真实 IdP/DNS/SMTP/IMAP、Docker 与生产验收均未在
> 本轮执行，不得据此宣称运行时验收（与 `IMPLEMENTATION_STATUS.zh-CN.md` 口径一致）。

## 1. 本轮客观门禁实测

| 门禁 | 命令 | 结果 |
|---|---|---|
| 编译 | `cargo check --workspace --locked` | 通过 |
| 格式 | `cargo fmt --all --check` | 通过（本轮规范化 6 个文件 11 处漂移后） |
| Lint | `cargo clippy --workspace --all-targets --locked` | **零警告** |
| Rust 测试 | `cargo test --workspace --locked` | **381 通过 / 0 失败 / 43 ignored** |
| PG 集成套件 | `cargo test -p copilotmail-postgres --test postgres_integration -- --include-ignored`（disposable PG17 容器） | **38 通过 / 0 失败** |
| Web 类型检查 | `npm run check`（三包） | 通过 |
| Web 单测 | `npm test` | **416 通过 / 0 失败**（design-system 18 + admin 207 + mail 191） |

> 注：381/416 均高于 STATUS 文档记录的历史基线（317/411），说明测试在持续增长；
> ignored PostgreSQL 套件已在 disposable PG17（host-network 容器）完整执行通过，
> 覆盖认证围栏竞态、Agent Inspector、审计导出/归档、Human sent、Testing Agent 沙箱等。

## 2. 模块清单与职责边界

### 2.1 V1 原型（根 crate `src/`，1,024 行）

| 模块 | 职责 | 边界评估 |
|---|---|---|
| `main.rs` | 进程入口、healthcheck、SMTP+API 双任务编排 | 薄，正确 |
| `config.rs` | 环境变量配置 | 薄，正确 |
| `api.rs` | Axum HTTP API：邮箱 CRUD、Argon2 登录、JWT、发信、WebSocket | 单文件但内聚；原型级弱点见 §4.2 |
| `smtp.rs` | 手写 SMTP 状态机（EHLO/MAIL/RCPT/DATA）、本地投递 | 仅域内投递；无 TLS/DSN，符合"回归基线"定位 |
| `outbound.rs` | DNS MX 解析 + 直连远端 SMTP:25 | 无 STARTTLS/超时上限/重试；原型定位 |
| `store.rs` | SQLite WAL 存储 | 全局 `Mutex<Connection>` + `.lock().unwrap()`；单进程原型可接受 |
| `events.rs` | tokio broadcast 事件 | 薄，正确 |

定位结论：与 `ARCHITECTURE.md` 一致——**仅作端到端回归基线，不是生产邮件内核**。
GOAL.md 明确"不扩展手写 SMTP 原型作为生产内核"，当前代码未违反该原则。

### 2.2 V2 Workspace（14 crates + control app）

| Crate | 行数 | 职责 | 边界评估 |
|---|---|---|---|
| `copilotmail-domain` | 5,756 | 纯领域模型：Principal/RBAC/审批/审计事件/保留/域名/邮箱/Agent 策略状态机 | **零外部依赖**，`forbid(unsafe_code)`，15 权限 + 9 内置角色目录为权威事实源 |
| `copilotmail-application` | 29,677 | 41 个用例服务模块 + `ports.rs`/`mail_ports.rs` 端口抽象 | 每个服务持有 `S: ControlPlaneStore` 泛型，可注入内存假实现单测；默认拒绝端口（`Err(Unavailable)`）防止可选能力被静默忽略 |
| `copilotmail-postgres` | 26,784 | 33 个仓储 + 27 个事务模块 + 61 份迁移 + `schema/readiness.sql` 730 行就绪账本 | 仓储按聚合对齐；所有写路径经 `PgControlPlaneTransaction`（单一 sqlx 事务），SQL 全部参数绑定 |
| `copilotmail-control` | 18,645 | Axum 路由（~70 条）、认证编排、Outbox Worker、webhook 投递、域名投影、SSE | 双客户端 Cookie 分区、凭据歧义拒绝、Origin CSRF、per-route 权限+scope 双重检查 |
| `copilotmail-mail-engine` | 4,789 | Stalwart JMAP adapter：域名投影/草稿/提交/内容/事件解析 | 依赖倒置正确（实现 application 的 ports）；响应全部有界（1MB 上限、长度校验、opaque ref 白名单） |
| `copilotmail-oidc` | 1,835 | OIDC Discovery/JWKS/JWT 验证 + Authorization Code + S256 PKCE BFF | RS256 限定、必需 claim 校验、有界响应、不留存 refresh token |
| `copilotmail-scim` | 1,354 | SCIM 2.0 Users/Groups 生命周期 HTTP adapter | 独立凭据域（与人 OIDC token 隔离）、无凭据即 fail-closed |
| `copilotmail-agent-auth` | 1,285 | Agent OAuth2 `private_key_jwt` + mTLS 证书绑定 token + JWKS 重叠轮换 | 断言一次性、即时撤销、scope 不得超出实时 RBAC |
| `copilotmail-audit` | 817 | SHA-256 哈希链、RSA 签名 Checkpoint、便携导出离线验签 | 版本化格式常量、拒绝跨租户行与序号缺口 |
| `copilotmail-webhook` | 644 | 签名 webhook 投递 adapter | SSRF 防护（私有地址拒绝、不跟重定向）、HMAC 绑定 timestamp+event+body、有界 key set |
| `copilotmail-object-store` | 917 | S3 Object Lock/WORM 归档 + 精确 Version ID 回读校验 | 归 application 端口实现，职责单一 |
| `copilotmail-dns` | 918 | DNS 安全预检、公网地址判定、MX/TXT 解析 | 为 webhook SSRF 与域名验证共用 |
| `copilotmail-contracts` | 1,903 | 稳定传输 DTO（Health/Readiness/SystemInfo/Session/错误） | 纯 serde 类型，无逻辑 |
| `copilotmail-platform` | 1,595 | 配置、OTLP/Trace、日志、Secret 文件有界读取 | `SecretString`、文件大小上限 |
| `copilotmail-crypto` | 227 | RSA PEM/DER 有界编解码与策略校验 | 不做 RSA 运算，委托 aws-lc；错误不含密钥材料 |

### 2.3 前端（`web/` npm workspaces）

| 包 | 行数 | 职责 | 边界评估 |
|---|---|---|---|
| `design-system` | 3,114 | 语义令牌、Dialog、ContextBar、命令面板、UnknownWriteLock、EvidenceInspector | 无业务耦合；翻译走类型化词典（`check_web_i18n` 门禁） |
| `admin-app` | 23,178 | 治理端：域名/审批/角色/保留/Cases/Holds/审计/Agent Inspector/证据下载 | API 全 `cache:"no-store"`；无 `dangerouslySetInnerHTML`；localStorage 仅存 locale |
| `mail-app` | 16,038 | 人类邮件端：目录/队列/详情/写信/草稿/已发送 | 请求代次隔离、abort、草稿 CAS、提交锁、纯文本渲染、焦点恢复（均有测试） |
| `e2e` | 6,961 | Playwright 回环回归 + 像素基线 | 仅回环证据，符合声明 |

## 3. 逐模块功能审查要点

- **Application 服务统一模式**（已抽查 mail_submission、mailbox_authorization、
  audit_export、agent_claim、inbox 等验证）：`context.permits()` 前置拒绝 →
  `store.begin()` → 事务内授权/存在性重验（数据库时钟）→ 写 →
  `append_audit_event` → `enqueue_event`（Outbox）→ `commit`。
  全局 83 处 `begin()` 对 83 处 `commit()`；`mailbox_authorization` 中两处
  begin-without-commit 均为显式 `drop()` 的只读 owner 查询，无遗漏提交。
- **并发语义**：Agent claim 使用 advisory xact lock（org+principal+op+幂等键）
  + `FOR UPDATE OF state SKIP LOCKED` + 过期租约回收；Outbox 领取用
  `FOR UPDATE SKIP LOCKED` + `locked_by` 认领 + 指数退避释放；幂等重放以
  `request_sha256` 区分"精确重放"与"冲突"。设计与实现一致，质量高。
- **错误映射**：`map_store_error` 把 23503→MissingReference、23505/23514→
  Conflict、其余→Unavailable 且仅服务端记日志，SQL 细节不泄给调用方；
  HTTP 层 Forbidden→403、Unavailable→503，语义一致。
- **披露前审计**：Agent Inspector / 邮件正文 / 已发送目录等敏感读均"先写
  audit+outbox，同事务提交后才返回内容"——与 STATUS 声称一致，已验证实现。
- **防御性候选校验**：审计导出/证据读取对回读对象重验租户、ID、时间序、
  计数和，任何不一致 fail-closed 为 Unavailable——"fail closed"纪律成立。
- **PG 就绪账本**：`schema/readiness.sql` 逐表+逐约束+填充校验（含
  `pg_get_constraintdef` 断言权限目录），防止半迁移被误报为就绪。
- **控制面 HTTP**：凭据选择拒绝歧义（多 Cookie/双重 Authorization）、
  Admin/Mail Cookie 路由分区、非安全方法强制精确 Origin 匹配、敏感路由
  `no-store`/`no-referrer`、双窗口限流、SSE 租约配额、请求 ID 传播 +
  W3C traceparent 提取（恶意上下文安全忽略）。

## 4. 代码质量审查发现

### 4.1 P0 — 认证代次围栏（authentication fencing）【已实现，待运行期验收】

**机制**：`AuthorizationContext.authentication` 携带
`AuthenticationBinding`（HumanSession/HumanBearer/AgentCredential/ScimToken），
请求级应用服务经 `store.begin_authorized(&context)` 建事务，adapter 的
`bind_request_authentication` 在 BEGIN 后立即锁定/校验凭据行并在 `commit()`
前复查过期与撤销，消除"请求开始时验证、提交时已撤销"的 TOCTOU 竞态。

**实现状态（本轮接通）**：

1. 三个认证器产出真实 binding（OIDC→HumanBearer、agent-auth→AgentCredential、
   浏览器会话→HumanSession）；`authenticate_api_request` 在授权解析后把
   `identity.authentication` 合并进 `AuthorizationContext`。
2. `PgControlPlaneTransaction::bind_request_authentication` 已实现
   （`transactions/authorization.rs`）：HumanSession/ScimToken/AgentCredential
   对 `human_browser_sessions`、`scim_tokens`、`credentials`（限
   `private_key_jwt`/`mtls` 类型）对应行 `SELECT ... FOR UPDATE` 加锁并校验
   组织/主体归属、活跃状态、未撤销、未过期；AgentCredential 与 HumanBearer
   另按数据库时钟校验 token `expires_at`。
3. `tx_commit` 在提交前执行 `tx_bound_authentication_still_valid` 复查：
   会话复查追加式 `human_browser_session_revocations` 撤销事实与 `expires_at`，
   凭据/SCIM token 复查 `status='active' AND revoked_at IS NULL` 与过期，
   Bearer 复查 token 过期——均以数据库时钟为准，失败回滚并返回
   `StoreError::AuthenticationInvalid`。
4. 请求级写路径全部改用 `begin_authorized(&context)`（约 50 处，含 SCIM
   写路径从 `ScimCredential` 合成 `ScimToken` 绑定上下文）；受信本地路径
   （bootstrap、owner 绑定、SCIM token 签发/吊销 CLI、登录/登出会话生命周期、
   投递 reconcile 与归档/检查点 worker、submission 执行 lease 路径）保留
   `begin()`，与"None 仅供受信本地工作"的设计一致。
5. HTTP 层把 `AuthenticationInvalid` 独立映射为 401 `authentication_invalid`
   （不再混入 503 可重试臂）。
6. 新增 5 个 disposable-PG 集成测试（ignored 套件）覆盖：会话提交期撤销竞态、
   Agent 凭据撤销/过期绑定、SCIM token 撤销、Bearer 过期在提交期被抓、
   无 binding 的本地工作兼容 `begin_authorized`。

**证据边界**：围栏运行时语义已在 disposable PG17 上验证：会话撤销竞态
（`FOR KEY SHARE` 阻塞→commit 获胜→后续 bind 拒绝）、Agent 凭据撤销/过期、
SCIM token 撤销、Bearer 提交期过期、无 binding 本地工作兼容，全部通过。

> 套件执行还暴露并修复了两类真实缺陷（非测试问题）：
> - **审计导出 grant 跨时钟域比较**：迁移 `0060` 将 `created_at` 改写为
>   `clock_timestamp()`，而 `resolve`/`record` 用调用方时钟 `is_active(now)`
>   判定——DB 时钟领先时新 grant 被判"未生效"。已新增
>   `AuditExportQueryStore::audit_export_grant_is_active`（SQL 侧
>   `clock_timestamp()` 判定）并将下载/记录路径切换到 DB 时钟域。
> - **SCIM token 吊销审计 details 传 `Value::Null`**：`AuditEvent::new` 要求
>   object，`RevokeScimTokenService` 此前必回滚（无测试覆盖）。已改为结构化
>   details。
> 其余为夹具缺陷（陈旧 `now`、µs 精度、全局准入表跨测试污染、错误形态断言
> 过窄等），未削弱任何生产语义。
> Agent Inspector 的"未来时间戳不变量"增加有界偏差容忍（吸收 app↔DB 时钟
> 偏差），过期/缺失/结构性校验不受影响。

### 4.2 P1 — V1 原型的既有弱点（符合定位，但需防误用）

- `api.rs` CORS `Any` 全开、admin key 明文头直比（非常量时间）、JWT 无
  aud/iss 校验、登录无限流；
- `outbound.rs` 直连 :25 无 STARTTLS/超时上限/重试/退信；
- `smtp.rs` 无 TLS/大小强制只在 EHLO 声明；
- `store.rs` `Mutex` 中毒即 panic；
- `send_message` 对本地域收件人地址未做 header 注入以外的规范化。

评估：**与文档定位一致（回归基线），暂不构成缺陷**。本轮已落地收敛：
根 `docker-compose.yaml` 端口默认绑定 `127.0.0.1`（`BIND_ADDR` 显式覆盖），
README 显式禁止将 V1 二进制发布到非回环网络，避免被误当生产入口。

### 4.3 P2 — 可维护性与健壮性观察

- `apps/copilotmail-control/src/lib.rs` 6,503 行：路由表、认证编排、Worker、
  webhook 投递、投影、凭据解析全部在一个文件。功能正确但影响域过大；
  建议按 `http/` 目录既有模式把 Worker/投递/投影段拆出（文件内已分节，
  拆分为机械移动）。
- `ports.rs` ~2,600 行：Store trait 全集大但内聚；可按聚合拆为
  `ports/{mail,audit,identity,...}.rs` 子模块并保持单一 `ControlPlaneStore`
  组合 trait，降低 diff 冲突面。
- 生产代码 `unwrap/expect` 经分类：mail-engine 全部为 `expect("length
  checked")` 且紧邻显式长度校验（不变量断言，可接受）；`store.rs` 的锁
  unwrap 属原型；其余散点位于 `platform/tls/scim/agent-auth/audit/
  draft/bootstrap_organization`，多数为配置/常量不变量。建议对
  `apps/copilotmail-control/src/tls.rs` 与 `bootstrap_organization.rs` 的
  剩余 unwrap 做逐一复核，确认不会由外部输入触发。
- `unsafe`：全 workspace `#![forbid(unsafe_code)]`，无实际 unsafe 块。
- TODO/FIXME/HACK：零匹配——没有显性技术债标记，但 §4.1 的半成品重构
  说明"隐性技术债"存在；§4.1 的围栏本轮已接通，该隐性债已闭合。

## 5. 事务边界专项结论

| 检查项 | 结论 |
|---|---|
| 写路径原子性（状态+审计+Outbox 同事务） | 抽查的服务全部满足；`tx_append_audit_event`/`tx_enqueue_event`/`tx_commit` 共用同一 sqlx 事务 |
| 授权重验在事务内（防 TOCTOU） | 满足（`authorize_human_permission`、`active_principal_exists`、`mailbox_owned_by` 等均在事务内执行） |
| 幂等重放 | advisory lock + `idempotency_key_sha256` + `request_sha256` 精确匹配；冲突与重放语义区分正确 |
| 并发领取 | `FOR UPDATE SKIP LOCKED`（Outbox、Agent claim）+ `locked_by` 认领 + 过期回收 |
| CAS/版本 | draft、domain route、retention、approval、policy 均以版本谓词更新 |
| 远程 I/O 位置 | webhook 投递、JMAP 调用、对象存储回读均在事务外（先 resolve 授权、后 record 证据），未把网络往返放进 DB 事务——正确 |
| `begin_authorized` 围栏 | **已接通（见 §4.1）——bind 加锁 + commit 复查；会话撤销竞态与凭据代次均有事务级语义** |

## 6. 模块化评估（低耦合/高内聚/影响域）

**已达标的**：领域零依赖；端口归 application 所有且 adapter 可替换（mail-engine
反向依赖正确，架构门禁按层拒绝越界）；41 个应用服务均可注入假 Store 单一
复测；仓储/事务按聚合对齐；前端 design-system 与业务解耦；事件经事务 Outbox
解耦跨边界副作用。

**可单一复测单元清单**（已具备独立测试接缝）：每个 application 服务
（41）、每个 PG 仓储方法（disposable PG）、mail-engine 每个 JMAP 方法
（本地假 JMAP server）、webhook 投递（SSRF/签名/重试分类）、audit 链验签、
scim 路由、oidc 验证、agent-auth 断言、前端 hooks/组件、每条 OpenAPI
operation。`quality/function-test-matrix.yaml` 已登记 **105 个能力**且
`inventory_scope=complete`：覆盖 application 全部服务、domain 全部不变量
模块（含 ids/mailbox/webhook 值类型，本轮补最小单测）、全部 adapter crate、
control 全部辅助/CLI/17 个 HTTP 路由模块、postgres 链完整性与 web 面。
未登记项仅剩：纯类型 crate（contracts 无函数）、trait 声明文件（能力由
服务条目承载）、仓储/事务内部实现（由能力条目的 PG 测试引用覆盖）、
`main.rs`、测试模块自身与 V1 原型（按 §2.1 基线排除）。`--release` 门禁
仍 fail-closed：每项需 `verified` + 逐源哈希证据文件；两个 HTTP 集成场景
（agent_inspector_http、evidence POST）仍为 ignored/not_run。

**影响域控制**：新增功能的标准落点清晰——domain 加类型/权限目录 →
application 加服务+端口 → postgres 加迁移+仓储+事务 → control 加路由 →
contracts/web 加 DTO/界面 → `function-test-matrix` 登记。唯一薄弱点是 §4.3
的两个巨型文件会放大 diff 面。

## 7. 能力矩阵（vs GOAL.md / IMPLEMENTATION_PLAN）

状态口径：**实现**=代码存在且有本地测试证据；**部分**=代码存在但缺运行时/
集成证据或语义不完整；**未实现**=无代码或仅文档。

### 7.1 人与 Agent 双第一公民

| 能力 | 状态 | 依据/缺口 |
|---|---|---|
| 统一 Principal/Mailbox/Message/授权模型 | 实现 | domain + 61 迁移 + 仓储 |
| 人类 Web 邮件体验（目录/详情/写信/草稿/已发送） | 实现（代码级） | mail-app + 服务全通；未接真实数据平面运行 |
| Agent 机器身份（private_key_jwt/mTLS/JWKS 轮换） | 实现（代码级） | agent-auth 5 测试；真实部署未验 |
| Agent 幂等发送/草稿 CAS/提交对账 | 实现（代码级） | advisory lock + sha256 重放语义 |
| Agent Claim/Lease/续租/Ack/Nack/死信 | 实现（代码级） | SKIP LOCKED + 租约回收；PG 集成场景 ignored |
| Agent Cursor/SSE/签名 Webhook/限流配额 | 实现（代码级） | 双窗口限流+租约配额+SSRF 投递 |
| 人/Agent 状态隔离与责任追溯 | 实现 | processing 状态隔离 + actor/on_behalf_of 审计 |
| Testing Agent 沙箱（非投递冻结） | 实现（代码级） | 应用+DB 双层拒绝正式投递 |
| Production Agent 准入（策略+审批+额度+熔断） | 实现（代码级） | 原子 admission + 机器可读稳定错误码 |

### 7.2 真实域名邮件收发

| 能力 | 状态 | 依据/缺口 |
|---|---|---|
| 邮件数据平面可替换适配器 | 实现 | mail_ports + Stalwart adapter + 数据平面 ID 亲和 |
| 域名生命周期/DNS 验证/路由治理/投影 | 实现（代码级） | 域名工作台 + CAS + 投影状态机；真实 DNS 未跑 |
| 入站事件接入（Stalwart webhook→租户绑定→投影） | 实现（代码级） | 双凭据验证 + 追加回执；真实队列未演练 |
| 出站投递状态对账（JMAP Submission） | 实现（代码级） | 精确 reconcile；真实投递未验 |
| 标准 SMTP/IMAP 外部客户端 | **部分→未验证** | 依赖 Stalwart 侧能力；PoC 脚本就绪但真实互操作未做 |
| MX/SPF/DKIM/DMARC/PTR/TLS 全链路 | **部分** | DNS 预检代码在；真实域名端到端未验 |
| 可靠队列/退信/重试 | **部分** | Outbox 可靠投递在；远端 MTA 队列/DSN 依赖数据平面未验 |

### 7.3 企业管理与审计

| 能力 | 状态 | 依据/缺口 |
|---|---|---|
| 组织/人员/Agent/邮箱/域名/角色/审批管理 | 实现（代码级） | admin-app + 全套服务 |
| OIDC SSO + BFF Session + SCIM | 实现（代码级） | PKCE S256、摘要存储、双客户端分区；真实 IdP 互操作未做 |
| RBAC + 资源级 ABAC + 双人审批 | 实现（代码级） | 15 权限/9 角色目录 + 成员/委托时限 |
| 结构化审计（85 类事件目录） | 实现 | 目录与生产者一致性有 CI 门禁 |
| 哈希链 + 签名 Checkpoint + WORM 归档 + 验签 | 实现（代码级） | audit crate + object-store；MinIO 实跑未做 |
| 审计导出双人授权 + 下载账本 | 实现（代码级） | 精确对象回读 + 15 分钟授权 |
| 合规案件/Legal Hold/保留策略 | 实现（代码级） | 状态机 + 审批 + 优先语义 |
| 日志/审计不泄密 | 实现（代码级） | 正文不进审计/URL/Cursor；敏感路由 no-store |

### 7.4 工程架构与交付

| 能力 | 状态 | 依据/缺口 |
|---|---|---|
| 分层模块化 Rust workspace | 实现 | 架构门禁强制执行 |
| PostgreSQL 持久化与迁移 | 实现（代码级） | 61 迁移 + readiness 账本；完整 ignored 套件未跑 |
| Docker/Compose 交付 | **部分** | Dockerfile/Compose/静态安全门禁在；目标环境实际部署未做 |
| 可观测性（日志/Trace/指标） | 实现（代码级） | OTLP + request-id 贯穿；生产 Collector 演练未做 |
| **请求级认证代次围栏** | **实现（代码级）** | §4.1：合并→`begin_authorized`→PG bind+commit 复查；运行时撤销竞态证据待 disposable PG 执行 |
| migration rollback/升级演练 | **部分** | 备份恢复脚本与自测在；rollback 与 `0050→latest` 动态门禁未跑全 |

### 7.5 文档与治理

| 能力 | 状态 |
|---|---|
| 中英文 README/架构/API/部署/运维文档 | 实现（88 份，链接门禁通过） |
| OpenAPI/AsyncAPI 契约 | 实现（83 路径/99 operation） |
| 质量矩阵/测试清点 | 实现（代码级）（105 项登记、`inventory_scope=complete`、矩阵门禁通过；release 门禁仍 fail-closed——每项需 verified + 哈希化证据文件，且含 2 个 not_run HTTP 集成场景） |

### 7.6 总体未实现/未验证项（阻塞 Enterprise GA 的清单）

1. 全部 disposable-PG 集成场景（含本轮新增 5 个围栏测试） + 4 个 MinIO 场景实际执行；
2. 真实企业 IdP OIDC/SCIM 互操作、Back-channel Logout、停用 SLA；
3. 真实域名端到端收发（MX/SPF/DKIM/DMARC/TLS/退信）与真实 Stalwart
   PoC（重启/升级/HA/普通账号互操作）；
4. Docker/NixOS 实际部署、PostgreSQL 17 同版本原生恢复、第二机 LAN
   负向探测、HA、容量、压力、故障注入、渗透测试；
5. migration rollback 演练、`0050→latest` 动态门禁；
6. SPIFFE/SPIRE 或云工作负载身份联邦、HSM/KMS 签名评估；
7. P7 GA：SBOM、镜像签名、发布演练、放行签字——未开始。

## 8. 修复优先级队列

| 序 | 项 | 理由 | 影响域 |
|---|---|---|---|
| 1 | ~~接通认证围栏~~（§4.1，**本轮已闭合**） | 已按四步落地：合并→`begin_authorized`→PG bind+commit 复查→5 个竞态测试 | control `authenticate_api_request`、application 约 50 处写路径、postgres `bind_request_authentication`/`tx_commit`、HTTP 401 映射 |
| 2 | ~~跑通 ignored 套件~~（**本轮已闭合**，38/38 通过） | disposable PG17 实测：认证围栏竞态、Agent Inspector、审计导出/归档、Human/Agent 流程全绿；暴露并修复 2 个真实缺陷（跨时钟 is_active、SCIM revoke details），见 §4.1 证据边界 | disposable PG17 host-network 容器 |
| 3 | ~~control `lib.rs`/`ports.rs` 拆分~~（**本轮已闭合**） | `lib.rs` 6,526→246 行（拆出 state/cli_setup/cli_audit/audit_maintenance/cli_admin/serve/web_router/web_token/web_session/web_auth/web_errors/shutdown 12 个模块）；`ports.rs` 3,419→38 行 mod 聚合（records 按 identity/audit/agent/mail/org 5 域 + error/transaction/stores 共 8 个模块） | 纯机械移动，无行为变更，check/clippy/test/fmt 全绿 |
| 4 | ~~剩余生产 unwrap 逐点复核~~（**本轮已闭合**） | 70 处生产区 `unwrap`/`expect` 全量复核：全部为紧邻守卫的不变量断言（`all(Option::is_some)` 后消费迭代器、`expect("length checked")`、`from_std` 常量、字面量 `HeaderValue::parse`、写 `String`、构造器锁连接在场）——无外部输入可达 panic 面；V1 `src/store.rs` 的 Mutex unwrap 属原型基线（§2.1 已标注） | 无代码变更 |
| 5 | ~~function-test-matrix 扩到全能力面~~（**本轮已闭合**，17→105，scope=complete） | 已按真实入口函数与真实测试符号登记：application 29 项、adapter crate 7 项、control（web/CLI/17 路由模块）27 项、domain 不变量 14 项、postgres/crypto 2 项、web 4 项；为 cli_admin/cli_audit/ids.rs 补了 8 个最小单测以提供登记证据；PG 集成证据按实跑结果标 `passed`；schema 校验全部通过，`--release` 保持 fail-closed（每项需 verified + 证据文件 + 全 passed） | `quality/function-test-matrix.yaml`；release 未放行 |
| 6 | ~~V1 部署面收敛声明~~（**本轮已闭合**） | 根 `docker-compose.yaml` 两个发布端口默认绑定 `127.0.0.1`（`BIND_ADDR` 显式 opt-in）；README.zh-CN 显式禁止非回环暴露并修正控制台地址与公网 SMTP 说明 | Compose/文档，无代码变更 |

## 9. 结论

代码库整体工程质量**显著高于一般项目**：分层清晰、端口倒置正确、事务
纪律一致（写+审计+Outbox 原子）、并发原语使用准确（advisory lock、
SKIP LOCKED、CAS）、错误分类不泄密、审计先于披露、安全边界 fail-closed、
文档不夸大证据。

**本轮审计发现的唯一实质缺陷——认证代次围栏半途重构——已在同一轮闭合**：
请求绑定合并、`begin_authorized` 全写路径接通、PG 侧 bind 期加锁与 commit
期复查、401 错误映射、以及五类 disposable-PG 竞态测试均已实现。其余差距
全部是"代码已就绪、运行时证据未采集"的验收缺口，而非实现缺口。建议按
§8 队列在真实 PG/MinIO/Stalwart 环境补齐集成证据，能力矩阵中"部分"项才可
升级为"实现"。
