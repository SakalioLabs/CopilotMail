# CopilotMail

语言：[语言入口](README.md) · [简体中文](README.zh-CN.md) · [English](README.en.md)

CopilotMail 正在从面向 AI Agent 的自托管邮件 MVP 演进为人与 Agent 均为第一公民的企业邮件系统。仓库当前同时保留可回归的 V1 原型与正在建设的 V2 企业控制平面；V1 的手写 SMTP 不会直接扩展为生产邮件数据平面。

项目的统一目标入口见 [项目目标](GOAL.md)，V2 的完整范围、架构决策和阶段验收标准见 [企业级实施计划](docs/IMPLEMENTATION_PLAN.zh-CN.md)，当前完成度和下一步见 [实施状态](docs/IMPLEMENTATION_STATUS.zh-CN.md)，系统信任边界、威胁与剩余风险见 [企业威胁模型](docs/THREAT_MODEL.zh-CN.md)，需要公司确认的容量、IdP、域名、合规及恢复参数见 [P0 输入与签字清单](docs/ENTERPRISE_INPUT_CHECKLIST.zh-CN.md)，同步接口见 [V2 OpenAPI](docs/openapi-v2.yaml)，Agent 事件信封见 [V2 AsyncAPI](docs/asyncapi-v2.yaml)，Stalwart 入站事件配置见 [邮件事件接入指南](docs/MAIL_EVENT_INGRESS.zh-CN.md)，订阅、签名、SSE、死信与重放运维见 [Agent 事件投递指南](docs/WEBHOOK_DELIVERY.zh-CN.md)。

## V2 开发预览

V2 已建立 Rust Cargo Workspace、统一 `Principal` 领域模型、人与 Agent 独立消息状态、PostgreSQL migration、事务型 Outbox、结构化审计骨架，以及可分别运行的 API、Worker 和迁移命令。邮件数据平面的不透明引用由稳定 UUID `MailDataPlaneId` 划分命名空间，该值不等于供应商；邮箱绑定调用方只能提交 `account_ref` 与期望版本，服务端注入已治理的数据平面身份。管理、内容、写信与提交 adapter 必须共享显式配置的 canonical origin。当前仅实现 Stalwart v0.16 adapter，不能据此宣称已支持多供应商；详见 [ADR 0037](docs/adr/0037-stable-mail-data-plane-identity.md)。

> 从 Migration 0052 或更早版本升级到 0053 时，不得直接执行下面的整栈
> `up --build`。0053 是停写升级：先停止旧 API、Worker 与审计维护进程，完成备份，
> 单独运行新版本 `migrate` 并核对数据平面 Origin 绑定，再只启动新版本进程。完整顺序见
> [Ubuntu / WSL Pilot 部署手册](docs/UBUNTU_WSL_PILOT.zh-CN.md)。

> 当前仓库最新迁移为 `0056_human_mail_sent_directory_index.sql`。0053 的停写规则仍只适用于
> 跨越 0053 的升级；0054、0055、0056 及完整动态迁移链必须在干净、隔离的 PostgreSQL 环境重新验收。
> 0056 使用 `CREATE INDEX CONCURRENTLY`，必须由单一迁移进程完成后再通过只读 `check`；
> 不得在生产提交表上手工改用阻塞式建索引。
> NixOS 目前仅可作为受限开发/隔离测试节点，不能将源码同步视作已获部署授权；详见
> [NixOS 开发与隔离测试节点边界](docs/NIXOS_DEVELOPMENT_NODE.zh-CN.md)。

```bash
cp deploy/.env.v2.example deploy/.env.v2
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml up --build
curl http://127.0.0.1:8081/healthz
curl http://127.0.0.1:8081/readyz

# 配置两套 OIDC 公共客户端并生成 32 字节会话密钥后，启用 Human BFF：
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.human-session.yaml \
  -f deploy/compose.pilot.yaml up --build

# 首次初始化企业；重复执行同一 slug 会返回相同 ID
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml run --rm \
  api bootstrap-organization --slug example-corp --display-name "Example Corporation"

# 配置 deploy/.env.v2 中的 OIDC issuer/audience 后，将首位企业人员设为 Owner。
# ORGANIZATION_ID 来自上一条命令；SUBJECT 是 IdP 令牌中稳定的 sub，不是邮箱。
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml run --rm \
  api bind-oidc-owner --organization-id "$ORGANIZATION_ID" \
  --subject "$SUBJECT" --display-name "Initial Organization Owner"

# 签发只显示一次明文的租户 SCIM Token；数据库仅保留摘要。
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml run --rm \
  api issue-scim-token --organization-id "$ORGANIZATION_ID" \
  --label "Enterprise IdP provisioning" --expires-in-days 90
```

Ubuntu / WSL 或兼容原生 Linux 的隔离 Pilot 在启动前必须遵循
[Ubuntu / WSL Pilot 部署手册](docs/UBUNTU_WSL_PILOT.zh-CN.md)，并先运行其中的
只读、Fail-Closed 前置检查。NixOS 共享宿主机还必须先满足
[NixOS 开发与隔离测试节点边界](docs/NIXOS_DEVELOPMENT_NODE.zh-CN.md)。
V2 PostgreSQL schema 升级前还必须完成
[逻辑备份、隔离恢复与一致性验收](docs/BACKUP_RESTORE.zh-CN.md)；该流程使用原生
PostgreSQL 客户端，不依赖 Docker 执行备份。

企业管理端默认位于 `http://127.0.0.1:8083`。配置企业 OIDC 和管理租户后，可在
同一界面处理“待我审批”、查看和取消“我的申请”、检索不可变审计证据，并管理
九个内置角色及限时授权分配、合规案件与审计归档 Legal Hold；角色授予消费精确
双人审批，撤销保护最后一个有效 Owner。登录由 Rust BFF 执行 Authorization
Code + S256 PKCE，浏览器只持有 Host-only HttpOnly Session Cookie，Provider
Token 不进入前端 JavaScript。配置、CSRF、会话撤销和剩余 Pilot 边界见
[Human OIDC BFF 会话](docs/HUMAN_OIDC_SESSION.zh-CN.md)与
[企业管理端指南](docs/ADMIN_APP.zh-CN.md)。

Human 邮件端已具备授权邮箱目录与 Shared Mailbox 切换、真实 `role=inbox` 收件箱、
隐私优先的有界搜索、纯文本安全详情、已读/星标、自动保存草稿、本人私有草稿目录、
完整性校验后的恢复编辑、本人已发送／投递状态目录及可恢复正式发送。
Mail Session-only 的 `GET /api/v2/mailboxes` 要求 Active Human 与 `mailbox.use`，且只列出
具有 current data-plane binding 和资源级 Read 的邮箱；最小响应只含邮箱 UUID、kind、
主地址和由 `Send(None)` 推导的 `can_send`，canonical Cursor 不能跨授权或绑定漂移复用。
`SendAs`-only 邮箱保持只读；写信框明确显示不可编辑 From，切换邮箱会作废旧异步请求并
清空搜索、详情、选择与写信草稿。部署 `mailboxId` 只是初始偏好，不能扩展目录授权。
Shared Mailbox 的成员关系不会授予其他人的草稿：目录只列出调用人创建的 active Draft，
且不含主题、地址或正文；目录页只有在 `mail.drafts.queried` Audit/Outbox 原子提交成功后
才披露。恢复只按 PostgreSQL 已证明的单个 Draft 执行 JMAP
`Email/get`，精确核对 Drafts Mailbox、`$draft`、From、Thread 和内容摘要，并在最终
权限/版本复核与 `mail.draft.read` Audit/Outbox 原子提交成功后才向浏览器返回正文。
已发送目录只列出调用人自己的 durable `accepted` Submission，且必须证明对应冻结 Draft
已经成为 `sent`；它只显示接受时间、收件人数与可选的聚合投递计数，绝不显示主题、正文、
地址、收件人摘要、SMTP reply、Draft/Provider/data-plane 引用或远端错误。目录不执行 JMAP
调用，合法 data-plane 重绑后仍保留本地历史；Active Human、`mailbox.use`、Read/Send、本人
所有权和当页最新快照会在 `mail.submissions.queried` Audit/Outbox 原子提交前再次复核。
列表和搜索只返回既有入站投影形成的安全
Message UUID；原始 JMAP Account/Email/Thread/Query State 不进入浏览器、Cursor 或
持久证据。信封和正文分别只有在最小化 Audit/Outbox 原子提交后才披露，所有 Human
邮件响应默认 `no-store`，快速切换详情也会中止并丢弃过期响应。Human-only 身份门禁、
邮箱资源授权和数据平面命名空间内的 Account 永久租户归属共同保护读取边界，内容/草稿/提交
凭据相互隔离；冻结 Draft、持久化 Intent、查询优先的 EmailSubmission 对账、租约
接管以及终态审计/Outbox 共同避免响应丢失后的盲目重复投递。JMAP 接受后由 Worker
读取精确 Submission 的收件人级 `deliveryStatus`，追加不可变快照并明确区分排队、
对端接受和永久失败。Stalwart 队列级 Webhook 已进入独立不可变运维账本，且不会
用弱关联改写用户状态；真实公网退信与 DSN 演练仍待完成。配置、返回数据最小化和
未完成边界见 [Human 邮件端安全边界](docs/HUMAN_MAIL_APP.zh-CN.md)。
[显式回复语义](docs/EXPLICIT_REPLY_SEMANTICS.zh-CN.md)进一步说明 Human/Agent
如何只用安全 Message UUID 回复，并由服务端生成标准回复链。

企业 IdP 的 SCIM Base URL 为 `https://<control-plane>/scim/v2`。用户
`externalId` 必须映射到 OIDC 的稳定 `sub`；完整配置、轮换和停用验证见
[SCIM 2.0 接入指南](docs/SCIM.zh-CN.md)。

Agent 使用独立 Principal 和公钥凭据，不复用 Human 会话或静态 API Key。V2
已支持 OAuth2 Client Credentials + `private_key_jwt`、短期 Scope Token、
一次性 Assertion、凭据重叠轮换和立即撤销；密钥生成、注册、Token 交换与轮换步骤见
[Agent OAuth2 接入指南](docs/AGENT_AUTH.zh-CN.md)。
仓库内可分发的 [`copilotmail` Agent Skill](skills/copilotmail/SKILL.md) 已按
V2 安全模型重写，配套无第三方依赖 CLI 覆盖策略查询、处理订阅、
Claim/Lease/Ack/Nack、死信/Cursor 恢复、纯文本安全读取及 Draft/Submission
幂等发送与 Testing Agent 非投递 Sandbox；CLI 只消费短期 Token 文件或临时
环境变量，不负责生成或输出凭据。
production Agent 外发进一步要求 Active Human 创建并经双人审批激活的不可变策略；
每次 Submission 在 PostgreSQL 中原子校验收件域、小时/单地址额度、同线程熔断和
首次联系人审批，并将 Admission、审计与 Outbox 一起提交。控制平面只保存规范域名
与主体范围的地址摘要，Worker 不会租赁缺失 Admission 的 production Intent。Agent
可通过主体绑定的 `GET /api/v2/agent/mail-policy` 查询自身有效权限、活动策略及
当前小时汇总用量；响应不会返回收件人或线程级账本。
所有 Agent 作者的草稿由服务端增加 RFC 3834 `Auto-Submitted` 标记；回复来源也由
JMAP Adapter 可信分类，Agent 对自动来源的回复默认拒绝。相同主体在滚动 24 小时内
第 11 次提交完全相同的正文会触发独立熔断。Testing Agent 不能创建可投递 Intent，
只能冻结精确 Draft 版本并记录 `delivery_suppressed=true` 的不可变 Sandbox 证据；
应用和数据库均执行该边界。详见
[Agent 邮件 Sandbox 与循环保护](docs/AGENT_MAIL_SANDBOX_AND_LOOP_GUARDS.zh-CN.md)。
入站消息可通过独立于 Human 已读/星标状态的处理订阅消费：Agent 为具有 Read ABAC
的邮箱建立自绑定订阅后，使用有界 Claim/Lease/续租/Ack/Nack API 工作；精确
`Idempotency-Key` 重放不会重复推进状态，错误 Lease、跨主体调用和过期 Lease
默认拒绝，连续失败最多五次后进入死信；Agent 可使用订阅绑定的有界 Cursor 查询
自己的终态失败，而不会获得原始错误或其他主体状态。订阅支持幂等暂停、恢复和
终态退役：暂停保留持久入站 backlog、停止新 Claim 并允许现有 Lease 排空，退役前
拒绝未过期 Lease。
完整接口与编排规则见
[Agent OAuth2 接入指南](docs/AGENT_AUTH.zh-CN.md)。
需要更强工作负载证明时，可启用原生 Rustls mTLS：证书身份不依赖代理 Header，
签发的 Access Token 通过 RFC 8705 `cnf.x5t#S256` 绑定到已验证客户端证书。
授权服务器支持热加载多密钥 Key Ring 与公开 JWKS；生产轮换流程见
[Agent 签名密钥轮换](docs/AGENT_SIGNING_KEY_ROTATION.zh-CN.md)。

邮箱资源和高风险审批策略可通过离线治理命令初始化；审批请求、查询、决策与取消
必须走受保护 API，不能由离线命令伪造调用人。

```bash
copilotmail-control create-mailbox \
  --organization-id "$ORGANIZATION_ID" \
  --kind shared --primary-address support@example.com

copilotmail-control grant-mailbox-delegation \
  --organization-id "$ORGANIZATION_ID" --mailbox-id "$MAILBOX_ID" \
  --grantee-principal-id "$PRINCIPAL_ID" \
  --capability mailbox.read,mailbox.send_as \
  --reason "Support rotation" --valid-in-hours 24

copilotmail-control create-approval-policy \
  --organization-id "$ORGANIZATION_ID" \
  --policy-key mail.send_as.external --display-name "External Send As" \
  --action mail.send_as --required-approvals 2 \
  --rule-json '{"recipient_scope":"external"}'

# 在只读一致性快照中独立重算该租户的完整审计哈希链
copilotmail-control verify-audit-chain \
  --organization-id "$ORGANIZATION_ID"

# 创建签名链头并导出便携证据包；私钥只从挂载文件读取
copilotmail-control create-audit-checkpoint \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --checkpoint-id "$CHECKPOINT_ID" --key-id audit-2026-01 \
  --private-key-file /run/secrets/audit-private.pem \
  --public-key-file /run/secrets/audit-public.pem \
  --archive-object-key "audit/$ORGANIZATION_ID/$CHECKPOINT_ID/events.jsonl"

copilotmail-control export-audit-checkpoint \
  --organization-id "$ORGANIZATION_ID" \
  --checkpoint-id "$CHECKPOINT_ID" \
  --output-directory ./audit-export

# 此命令不读取数据库配置，可复制到隔离主机执行
copilotmail-control verify-audit-export \
  --export-directory ./audit-export \
  --public-key-file ./trusted-audit-public.pem

# 先离线验真，再上传并回读验证两个 S3 Object Lock Version
copilotmail-control archive-audit-export \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --checkpoint-id "$CHECKPOINT_ID" --archive-id "$ARCHIVE_ID" \
  --export-directory ./audit-export \
  --public-key-file ./trusted-audit-public.pem

# 配置 COPILOTMAIL_AUDIT_MAINTENANCE_* 后，执行一次或启动独立定时 Worker
copilotmail-control audit-maintenance
copilotmail-control audit-maintenance-worker
```

常驻审计维护进程默认在 `:9090` 提供 `/metrics`、`/healthz` 和 `/readyz`，用于监控连续失败、周期超时与未归档 Checkpoint 超龄。
可选的 OpenTelemetry 覆盖层通过 OTLP/HTTP 把受控 Span 发送到
Collector/Tempo，并提供 Prometheus 与 Grafana 开发环境；生产 HTTPS 边界、W3C
Trace Context、数据最小化与演练步骤见
[可观测性与分布式 Trace](docs/OBSERVABILITY.zh-CN.md)。

权限语义见[授权矩阵](docs/AUTHORIZATION_MATRIX.zh-CN.md)，策略版本、职责分离和执行时证明见[审批工作流](docs/APPROVAL_WORKFLOW.zh-CN.md)，事件分级、上下文和敏感数据规则见[企业审计事件目录](docs/AUDIT_EVENT_CATALOG.zh-CN.md)，签名、导出和隔离环境验证见[审计 Checkpoint 与离线验签](docs/AUDIT_CHECKPOINTS.zh-CN.md)，不可变对象存储配置与故障恢复见[审计 WORM 归档](docs/AUDIT_WORM_ARCHIVE.zh-CN.md)，案件绑定、精确双版本保全与失败恢复见[审计归档 Legal Hold](docs/LEGAL_HOLD.zh-CN.md)，浏览器安全的双人审批、短期凭证、精确版本下载和不可变下载审计见[审计归档导出授权](docs/AUDIT_EXPORT_AUTHORIZATION.zh-CN.md)，不可变版本、审批激活与 Legal Hold 优先级见[生产保留策略](docs/RETENTION_POLICIES.zh-CN.md)，OTLP、Trace Context 与数据最小化见[可观测性指南](docs/OBSERVABILITY.zh-CN.md)。

该开发栈用于 V2 控制平面开发。代码已具备经认证的 Stalwart SMTP 入站及队列事件边界，
并提供普通测试账号声明式初始化、SMTP/IMAP/JMAP 互操作、非开放中继与两阶段重启
持久性验证程序，操作步骤见 [Stalwart 邮件数据平面 PoC](docs/STALWART_POC.zh-CN.md)。
Migration 0001→0053 与针对数据平面身份、Origin、Domain、Outbox 的直接 SQL 不变量
验证已在 disposable PostgreSQL 17 实际执行；这里不据此宣称完整 ignored PostgreSQL
套件、目标 Pilot 升级或真实 Stalwart 互操作已经完成。真实域名邮件收发仍需完成公网
PoC、域名认证、反垃圾、队列持久性、真实退信与故障恢复测试。

当前仓库已新增 0054、0055、0056；上述 0001→0053 动态证据不覆盖它们，故不能作为最新迁移链或
NixOS 目标机的运行态验收结论。

## V1 原型

## 能力

- 创建独立 Agent 邮箱并通过密码换取短期 Token
- 自研 Tokio SMTP 服务接收 RFC 5322 邮件
- API 发送本地域邮件；外域邮件查询 MX 后直接投递到目标 SMTP
- 收件箱、消息详情、已读、删除与邮箱销毁 API
- `/v1/events` WebSocket 按邮箱隔离推送新邮件事件
- 同仓库、同镜像发布的解耦 Web 前端
- Dockerfile、Compose、单元测试和全链路 E2E 测试

## Docker 启动

> 以下为保留 V1 原型的历史回归说明，不得在共享 NixOS/生产宿主机执行；V2 不使用
> 根目录 `docker-compose.yaml`。

```bash
cp .env.example .env
# 将 JWT 和管理员密钥替换为随机值
docker compose up -d --build
curl http://127.0.0.1:8080/healthz
```

打开 `http://服务器:8080` 使用控制台。SMTP 默认映射到主机 `2525`；正式收取公网邮件时，将 `.env` 的 `SMTP_PORT` 设置为 `25`。

## 第一个邮箱

```bash
curl -X POST http://127.0.0.1:8080/v1/mailboxes \
  -H 'Content-Type: application/json' \
  -H 'X-Admin-Key: your-admin-key' \
  -d '{"address":"agent@mail.local","password":"change-me-now"}'

curl -X POST http://127.0.0.1:8080/v1/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"address":"agent@mail.local","password":"change-me-now"}'
```

完整接口见 [OpenAPI](docs/openapi.yaml)，设计见 [架构说明](docs/ARCHITECTURE.md)，生产部署见 [部署指南](docs/DEPLOYMENT.md)。

## 验证

```bash
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
python tests/e2e.py
python scripts/check_architecture.py --self-test
npm --prefix web ci
python scripts/check_web_i18n.py --self-test
npm --prefix web audit --audit-level=moderate
npm --prefix web run check
npm --prefix web test
npm --prefix web run build
python scripts/check_audit_governance.py
python scripts/check_agent_skill.py
python scripts/check_compose_security.py --self-test
python scripts/check_documentation.py
docker compose config
docker compose build
docker compose --env-file deploy/.env.v2 -f deploy/compose.dev.yaml config
docker compose --env-file deploy/.env.v2 -f deploy/compose.dev.yaml build
```

`check_architecture.py` 同时是 CI 门禁：它从 Domain、Audit、Application 三个权威包根
解析完整依赖图，按分层允许集拒绝直接、传递、别名与目标特定的向外依赖；同时扫描
Mail Engine 的全部 Rust 源码，确保邮件无关 ports 只由 Application 拥有且 adapter 仅私有导入。

`check_web_i18n.py` 同样由 CI 强制执行：Human Mail 生产组件中的可见文案必须来自
类型安全的简体中文/英语词典，避免新功能绕过本地化与安全回退约束。

E2E 测试会启动临时服务，并覆盖邮箱创建、登录、SMTP 投递、WebSocket 事件、消息 CRUD、API 本地发信和前端加载。

## 公网邮件要求

为邮件域配置 MX 记录，将 TCP/25 转发到 CopilotMail，并确保云厂商/运营商允许入站与出站 25。API 应放在 HTTPS 反向代理后。直接 MX 投递不等于高送达率；生产发信还需要正确的 PTR、SPF、DKIM、DMARC、队列与退避策略。

## 许可证

[Apache License 2.0](LICENSE)。
