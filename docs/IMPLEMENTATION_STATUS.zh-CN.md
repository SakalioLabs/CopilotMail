# CopilotMail V2 实施状态

> 更新时间：2026-08-22
> 状态口径：只记录已有代码、测试或文档证据的结果；未验证项不视为完成。
> 依据：[企业级实施计划](IMPLEMENTATION_PLAN.zh-CN.md)
> 当前权威计数：83 类审计事件；Application 102、Domain 43、Mail Engine 35、
> Contracts 16、Control Plane 54；全工作区 `cargo test --workspace --locked` 实跑
> 304 个非忽略 Rust 测试通过、0 个失败，另有 33 个 ignored 场景未在该命令中执行；
> OpenAPI 82 条路径、98 个唯一 Operation ID；Web 共 125 个 Vitest
> （Admin 18、Mail 107）。
> 当前 Windows 工作区全量门禁已通过；上述测试计数为本轮最终已完整执行的基线。
> 已向受限 NixOS 开发目录同步经提交校验的源码快照；尚未在该宿主机启动容器、写入
> Secret、执行 migration 或变更端口/服务。WSL 不再作为后续验证依赖。目标运行环境仍
> 未应用当前 0056，因此本地门禁不能替代部署验收。

## 当前结论

项目已从单包 V1 原型进入 V2 企业控制平面与纵向业务切片阶段。V1 继续作为邮件概念和端到端回归基线；V2 已形成独立的模块化 Rust workspace、Human/Agent 双认证边界、邮件端和管理审计端。此前 Ubuntu WSL 的仅回环 Base Pilot 运行结果仅作为历史基础栈证据；因持续 I/O 风险，后续不再依赖该 WSL。当前 NixOS 仅保存了经提交校验的开发快照，尚无任何运行态部署证据。无论何种宿主机，上述结果都不等同于内部 Alpha 或生产验收；真实企业 IdP、企业域名邮件收发、PostgreSQL 17 同版本原生恢复、HA、容量与安全验收仍未完成。

> **当前宿主机阻断（2026-07-27）**：部署验收完成后，WSL 内核报告承载
> `Ubuntu-24.04` 的 ext4 VHD 出现目录块校验错误并中止 journal，随后将文件系统
> 重新挂载为只读；同机另一个 WSL VHD 也出现 inode 校验错误。现有容器仍可从内存
> 响应健康检查，但该状态不具备可靠写入和重启恢复能力，因此 Base Pilot 当前不得
> 视为稳定可用。必须在获准的维护窗口停止同机全部 WSL 工作负载，对相关 VHD 做离线
> 备份与 `e2fsck` 修复，并在修复后重新执行全部运行态验收。
>
> **2026-08-12 只读复查**：`/dev/sdc` 当前以 `rw` 挂载，本次启动的 `dmesg`
> 未再检出既有 ext4/journal 错误，Docker Socket 可响应；从 WSL ext4 仓库根运行
> 默认 Base Pilot 前置检查时，架构、空间、Docker、Compose 渲染与 8081/9000/9001
> 均通过。该结果不能替代缺失的离线 VHD 备份/`e2fsck` 记录。既有
> `copilotmail-v2-dev` PostgreSQL 与 MinIO 容器仍自 2026-07-28 同时以 255 退出，
> API/Worker 处于重启循环，Admin/Mail 占用 8083/8084；目标副本只有 Migration
> 0051。因此未停止其他工作负载、完成离线修复和受控切换前，仍不执行部署写入。

## 阶段进度

| 阶段 | 状态 | 已有证据 | 主要未完成项 |
|---|---|---|---|
| P0 产品、架构与邮件 PoC | 进行中 | 总体实施计划；控制平面 ADR；邮件数据平面边界 ADR；企业威胁模型及可追踪威胁/控制/验证矩阵；可签字的公司输入、容量、IdP、域名、合规、SLO/RPO/RTO 与运维依赖清单；邮箱资源授权矩阵；可机检审计事件目录 | 公司完成输入与风险签字、Stalwart 对比 PoC 与数据平面选型签字 |
| P1 工程底座 | 进行中 | Cargo Workspace；领域/应用/契约/平台/PG 与 S3 Object Lock adapter；邮件引擎无关的 DTO、错误和 port traits 归 `copilotmail-application` 所有，`copilotmail-mail-engine` 仅作为 Stalwart adapter 实现这些 ports；稳定 UUID `MailDataPlaneId` 区分独立远端引用命名空间且不等同 vendor，binding/ingress 由服务端注入 ID，Application 在网络调用前校验 adapter 来源亲和；从 Domain、Audit、Application 三个权威包根解析全特性依赖图、按层拒绝直接/传递/别名/目标特定向外依赖并扫描 Mail Engine 全 Rust 源码的 CI 架构门禁；migration 及 CI 重复执行无操作验证；API/Worker/Migrate/Check；Docker 开发栈；Ubuntu WSL Base Pilot 容器实跑、健康检查和持久化重启；结构化 JSON 日志；可选 OTLP/HTTP Span exporter、W3C Trace Context 与 Collector/Tempo/Prometheus/Grafana 覆盖层；CI 门禁 | migration rollback、PostgreSQL 17 同版本原生恢复、生产 Collector/Trace 告警演练、统一错误与请求 ID 全链路 |
| P2 身份与组织 | 进行中 | `Organization`/`Principal`；Bootstrap；默认拒绝 RBAC；九内置角色权威目录和既有租户回填；Active Human 限时角色授予/撤销、自审计有界读模型、精确双人权限提升审批、最后有效 Owner 保护和中英双语角色治理 UI；严格 OIDC Discovery/JWKS/JWT；服务端 Authorization Code + S256 PKCE BFF、只保存摘要/密文的 PostgreSQL Human Session、Admin/Mail Host-only HttpOnly Secure Cookie 与路由隔离、精确 Origin CSRF、跨副本登录限流和不可变创建/撤销证据；可审计的首位 Human Owner 接管；SCIM Users/Groups 生命周期与令牌轮换；Agent OAuth2 `private_key_jwt` 与原生 mTLS 证书绑定 Token、重叠轮换与立即撤销；邮箱成员关系与显式时限委托 ABAC；不可变策略版本和双人审批上下文；请求人/待办隔离的审批读模型、Active Human 决策、请求人取消与中英双语审批中心；受保护 Organization 查询；合规案件与审计归档 Legal Hold 状态机 | 真实企业 IdP/SCIM 互操作与停用 SLA、上游会话撤销/Back-channel Logout、Step-up/AAL、Agent 工作负载身份联邦、审批人组/规则执行器、自定义角色与权限编辑 |
| P3 邮件数据平面 | 进行中 | 独立 `copilotmail-mail-engine` 与 `copilotmail-dns` 边界；显式数据平面 UUID + canonical origin 配置，管理/Content/Compose/Submission 跨源启动拒绝；带 ID 亲和的 Stalwart JMAP 管理面安全探测与可靠域名投影；真实 DNS 安全预检；域名生命周期、投影状态与路由治理应用事务；双凭据 Stalwart 入站/队列事件验证、租户邮箱绑定、追加式来源回执、最小化 `message.received.v1` 与独立队列运维证据；精确 JMAP Submission 收件人投递状态对账；可选 readiness 硬依赖；固定版本 Docker PoC 覆盖层与 bootstrap runbook；普通账号声明式 upsert、SMTP/IMAP/JMAP/非开放中继及两阶段重启持久性自动验收程序；V1 SMTP 继续用于回归 | 在真实 Stalwart 执行 PG/S3/重启/升级和普通账号互操作证据；真实队列 WebHook/DSN 演练、TLS、反垃圾、可靠投递队列 |
| P4A 人类邮件体验 | 进行中 | 独立 React/TypeScript 邮件端与 Human-only 收件箱：浏览器只使用同源 BFF Session，不执行 Discovery/PKCE/Token 交换、不保存或发送 Provider Token；Admin/Mail 客户端与 Cookie 完全分区；Mail Session-only 的 `GET /api/v2/mailboxes` 在查询前拒绝 Admin Cookie 与 Bearer，并要求 Active Human、`mailbox.use`、current data-plane binding 和资源级 `Read(None)`；目录仅返回邮箱 UUID、kind、主地址与由 `Send(None)` 推导的 `can_send`，canonical Cursor 绑定仍可见锚点，`SendAs`-only 邮箱保持只读；可选 `mailboxId` 仅作初始偏好，桌面/移动端可切换授权 Human/Shared 邮箱，切换时作废异步请求并清空搜索、列表、详情、选择与写信草稿；写信框明确显示不可编辑 From，只读邮箱禁用写信、回复、草稿和发送；实时 RBAC/Scope、邮箱 Read/Send ABAC、精确租户 JMAP Account 绑定及永久 Account 所有权；列表/搜索必须先解析唯一可读 `role=inbox`，每项只使用既有入站投影的安全 UUID，原始 JMAP Account/Email/Thread/Query State 不进入响应或 Cursor；POST 请求体内的隐私优先有界搜索，查询词不进入 URL、Cursor、审计或 Outbox，搜索 Cursor 绑定邮箱与查询摘要；信封及详情正文分别只有在最小化 Audit/Outbox 原子提交后才披露；详情只取有界 `text/plain`，不渲染 HTML、不暴露 Blob ID；Human 邮件响应统一 `no-store`，前端快速切换会中止并丢弃过期详情响应且状态更新不重复 GET 正文；应用固定文案使用类型安全的简中/英文词典，按持久化值、文档和浏览器语言确定 Locale，无刷新切换并同步 `<html lang>`，日期、数量和附件大小按 Locale 格式化；不超过 900 px 时使用单列列表—详情，打开后聚焦详情标题、返回恢复原邮件焦点，移动工具栏保留写信、语言和退出；写信对话框具备 Modal 语义、背景 `inert`、初始焦点、Tab 环绕、发送中禁止关闭和触发器焦点恢复；一次操作打开并标记已读、切换星标；内容、草稿与提交 OAuth 凭据物理隔离；写信请求不能选择 From，纯文本草稿严格串行自动保存；控制平面 Draft UUID、摘要和 CAS 隐藏远端 ID；本人 active 草稿目录只返回最小元数据，Shared Mailbox 不共享其他人的草稿；恢复仅按 PG 证明的单 ID 读取，复核 Drafts/$draft/From/Thread 与内容证据，并在最终授权/版本复核及 Audit+Outbox 提交后披露正文；本人已发送目录只列 durable accepted/sent 记录及可选投递聚合，并在最终 Read/Send/所有权/快照复核与 `mail.submissions.queried` Audit/Outbox 原子提交后披露；已提交入站投影提供安全回复 UUID，回复按钮不接受原始 Header/JMAP ID，服务端验证并生成 `In-Reply-To`/`References`；点击发送在首个异步等待前同步锁定快照，保存最新版本后以 Header-only Idempotency Key 冻结 Draft 并创建 durable Intent，可能出网后持续锁定且不静默复制 Draft；Worker 以已提交租约执行 Identity 唯一解析、EmailSubmission 查询优先对账、歧义人工复核及终态原子审计/Outbox；接受后继续读取精确 Submission 的收件人级状态，UI 区分服务端接受、排队、对端接收和永久失败；响应未知不会盲目重发；Human JMAP 关键词与 Agent Ack/Lease 状态严格隔离 | 真实 IdP 与公网延迟/退信/DSN 演练、附件、线程、快捷键、孤儿草稿检测/受治理清理、共享协作编辑、真实浏览器移动端/键盘/屏幕阅读器 E2E 与 axe/WCAG 2.2 AA 验收 |
| P4B Agent Mail Platform | 进行中 | 自绑定邮箱处理订阅；入站事务 Fanout；所有权与有效期约束的 Claim/Lease/续租/Ack/Nack/五次死信工作流；幂等暂停/恢复/终态退役、暂停排空与活动 Lease 退役保护；订阅/真实锚点绑定、先审计后披露且不含原始错误的 Agent 自助死信查询；Header-only Idempotency 精确重放、冲突拒绝和不可变操作证据；Human 已读/星标、Webhook/Cursor 与 Agent processing 状态隔离；事务型 Outbox Worker；签名 Webhook 安全 adapter、Agent 所有权订阅治理、追加式投递账本、独立重试/死信检索、受控人工重放、订阅级 Cursor、有界 SSE、跨副本租户/Agent 一分钟 Ingress 请求窗口与 SSE 租约配额、稳定 `Retry-After`、独立无主体标签 Prometheus 指标与统一 AsyncAPI 信封；development Agent 可通过主体隔离 Draft、CAS、Send ABAC 和 durable Idempotency Submission 发送有界纯文本邮件；Testing Agent 正式投递在应用/数据库双层拒绝，只能原子冻结精确 Draft 并写入不可变非投递 Sandbox、Audit 与 Outbox；所有 Agent 外发由服务端写 RFC 3834 标记，自动来源回复默认拒绝，同主体精确重复正文具备独立 24 小时熔断；production Agent 通过不可变策略激活、最小收件证据、首次联系人审批、跨副本小时/单地址额度、同线程熔断、原子 Admission 和 Worker 租赁证据受控放行，并为策略、额度、熔断和审批拒绝提供不泄露地址的稳定机器错误码；主体绑定的自助 API 返回 Token/RBAC 有效权限、活动策略和数据库时钟下的小时汇总用量，不披露收件人/线程账本；真实 SMTP 入站 `message.received.v1` 生产边界；Feed Message UUID 精确映射、三 Scope 与 Read ABAC 约束、审计先于披露且不污染 Human 状态的有界纯文本正文读取；安全 UUID 绑定、服务端推导 RFC/JMAP 头且不可换绑的显式回复语义；精确 JMAP 投递状态外部事件与隔离的队列运维证据；按 V2 安全模型重写且含 Sandbox 的可分发 Agent Skill 与无依赖 CLI | 多语言 SDK、附件/HTML、公司实际额度签字、真实 Stalwart Header/双 Agent 回环及公网投递/退信验收 |
| P5 管理、审计与治理 | 原型中 | 追加式审计模型；迁移管理的 83 类事件目录；Human-only `audit.read` 有界检索与查询自身 Fail Closed 审计；显式请求/策略/审批/Session 上下文；敏感数据拒绝；事务审计覆盖率门禁；租户级并发安全 SHA-256 哈希链；RSA 签名 Checkpoint、便携 Manifest、完全离线验签与经回读确认的 S3 Object Lock/WORM 双对象归档；可恢复且由 PostgreSQL 租户锁协调跨主机单活的定时维护 Worker；Prometheus 失败/时效指标与阈值就绪检查；稳定业务 Trace 贯穿审计、日志与对象 metadata，OTLP Span 进一步关联 HTTP Request ID、维护周期和 Checkpoint；案件绑定、双人审批、精确双版本回读和跨主机会话锁保护的审计归档 Legal Hold CLI、受认证管理 API、自审计有界分页读模型及 React 中英双语审计/合规管理工作流；审批请求人与执行人强绑定、双人审批且最长 15 分钟的不可变审计导出授权凭证；精确 S3 Version ID、长度和 SHA-256 回读校验，先落不可变下载账本再返回内容的审计证据下载 API 与 UI；不可变保留策略版本、Active Human 双人审批激活、单调版本前移、Legal Hold 优先语义及中英双语管理 UI；请求人/待办隔离、绑定筛选 Cursor、自审计读取、Active Human 决策和请求人取消的审批中心；角色目录、授权查询、精确双人审批授予、Owner 保护撤销与不可变证据账本；正式发送请求/接受/拒绝/人工复核、Human 邮箱信封/正文与本人草稿目录/正文读取/已发送目录、production Agent Admission、Testing Agent Sandbox、Human Session 与 Agent processing 全生命周期均进入受治理目录，状态与 Outbox 原子提交；禁止非法状态变更/更新/删除/截断触发器 | 邮件内容 Legal Hold、保留策略自动执行与精确处置证明、跨数据库选主、生产 Collector/Trace 告警与真实 IdP/Object Lock 故障演练及其余管理信息架构 |
| P6 硬化、迁移与企业 Pilot | 进行中 | V1 E2E 回归继续通过；PostgreSQL 原生 custom-format 逻辑备份、自动生成隔离数据库恢复、迁移/审计/触发器/租户证据一致性比较与精确清理门禁；真实 `0050 → latest` Account 历史归属、direct-INSERT、触发器破坏与歧义回滚 CI 场景；Ubuntu/WSL 只读前置检查与部署手册；NixOS 开发/隔离测试节点边界；Ubuntu WSL Base Pilot 曾在 PostgreSQL 17 应用当时全部 51 份迁移并核对 80 类审计目录，API/Admin/Mail/MinIO 健康、端口仅回环、应用进程非 root，PostgreSQL/MinIO 持久化重启和 Worker 同容器自恢复通过；单主机 Pilot 资源/CPU/PID 上限、持久 SIGTERM 优雅关停、Worker 健康检查与有界 CI 重启验收已编码 | 在专用或明确隔离的 Linux Docker 环境实际应用至最新 Migration 0056、第二台机器 LAN 负向探测、PostgreSQL 17 同版本原生恢复、完整 ignored PostgreSQL/MinIO Object Lock 测试、企业 IdP/域名/邮件验收、migration rollback、负载/模糊/故障注入/渗透测试、旧邮件导入与 MX 灰度、容量和完整企业 Pilot 运维演练 |
| P7 GA 发布 | 未开始 | 根 README 语言入口、独立中英文完整 README、贡献/安全/变更文档、Apache-2.0 LICENSE 与 CI 文档治理门禁（仅为 GA 文档基础，不代表 GA 放行） | HA 部署验证、版本升级/回滚、SBOM、镜像签名与来源证明、正式发布说明、培训、文档站、安装/故障/回滚演练和放行签字 |

## 本轮已落地

- 完成根级多语言与协作治理文档：`README.md` 只承担简洁语言/项目入口，
  `README.zh-CN.md` 与 `README.en.md` 分别保留完整中文和英文说明；新增
  `CONTRIBUTING.md`、`SECURITY.md`、`CHANGELOG.md` 以及与 Cargo 元数据一致的完整
  Apache-2.0 `LICENSE`。新增 `scripts/check_documentation.py` 并接入 CI，Fail
  Closed 检查 13 个必需文件、UTF-8/BOM、代码围栏、仓库内相对链接、核心契约引用
  和三处语言导航，防止语言入口再次漂移或文档链接静默失效。
- 完成 P1 邮件边界依赖反转：邮件引擎无关的共享 DTO、错误和 port traits 移入
  `copilotmail-application::mail_ports`，Application 用例只依赖自有抽象；
  `copilotmail-mail-engine` 反向依赖并实现这些 ports，Stalwart wire DTO 保持私有。
  新增 `scripts/check_architecture.py` 并接入 CI，结合 Cargo manifest、解析后的
  all-features 完整依赖图与源码边界检查，从 Domain、Audit、Application 三个权威
  package-id 根按层拒绝直接、传递、workspace 别名和目标特定的向外依赖；把 Mail
  Engine 全部 `src/**/*.rs` 视为一个 corpus，拒绝额外导入、重导出、crate 别名、
  名称遮蔽或重复定义共享契约。
- 完成 P1B 稳定邮件数据平面身份切片并新增 ADR 0037：领域 UUID
  `MailDataPlaneId` 表示远端引用命名空间而非 vendor；Application-owned ports 由
  adapter 自报 ID，邮箱绑定请求移除调用方 `source`，服务端从受控配置注入 ID 并在
  Content、Draft、Submission、Delivery 与 Ingress 的远端调用前复核来源亲和。平台要求
  ID 与 canonical origin 成对配置，并拒绝四类 Stalwart role URL 跨源。Migration 0053
  登记 legacy-primary ID 与不可变 origin，把绑定、所有权、Domain 投影、入出站证据、
  草稿、Intent、投递快照和 Outbox 路由切换为 ID 作用域；域名在远端 I/O 前由激活事务
  预分配数据平面，旧 `source` 仅作历史/回退兼容证据。固定 legacy-primary ID 保持
  历史 v1 指纹字节与重放结果，其他 ID 使用带数据平面 UUID 的 v2 指纹；邮箱绑定原子
  双发 routed v1 兼容事件与 typed v2 事件。0053 同时修复 `agent.mail.read` 未进入
  `role_permissions_permission_check` 及两类 Agent 凭据 scope CHECK 的既有 schema drift，
  将凭据 scope 数量上限有界扩为 4，避免后续租户 Bootstrap 或 Agent 注册失败。
  0053 明确采用停写升级：停止旧 API/Worker、备份、迁移后原子切换全部新二进制，不支持
  旧新版本并跑或滚动升级。当前仍只有 Stalwart v0.16 adapter；Migration 0001→0053
  与针对数据平面身份、Origin、Domain、Outbox 的直接 SQL 不变量验证已在 disposable
  PostgreSQL 17 实际执行，
  完整 ignored PostgreSQL 套件和目标 Pilot 升级不由该证据代替。
- 完成 P4A-01 双语、响应式和可访问性基础切片：应用固定文案收口到类型安全且插值
  参数一致的简体中文/英语词典，Locale 按持久化值、文档语言、浏览器语言顺序选择，
  支持不刷新切换并同步 `<html lang>`，日期、数量和附件大小按 Locale 格式化。
  不超过 900 px 时切换为焦点可往返的单列列表—详情，移动工具栏保留写信、语言和退出；
  写信窗口具备 Modal 语义、背景精确 `inert`/`aria-hidden` 隔离、初始焦点、Tab 环绕、
  发送中禁止关闭和触发器焦点恢复。当前证据为 Vitest/jsdom 组件测试，尚未执行真实
  浏览器、触屏、屏幕阅读器、axe 或 WCAG 2.2 AA 验收。
- 完成 P4A 授权邮箱目录与 Shared Mailbox 切换切片：新增 Mail Session-only
  `GET /api/v2/mailboxes`，在访问目录存储前拒绝 Admin Cookie、Bearer 与缺少 Mail
  Session 的请求，并在用例和 PostgreSQL 一致性快照内复核 Active Human、实时
  `mailbox.use`、current data-plane binding 与资源级 `Read(None)`。UUID 顺序分页只接受
  canonical `v1.<lowercase UUID>` Cursor，锚点授权或绑定漂移时 Fail Closed；响应只披露
  `mailbox_id`、`kind`、`primary_address` 和由 `Send(None)` 精确推导的 `can_send`，
  `SendAs`-only 不会获得写入能力。部署 `mailboxId` 降为不可扩权的初始偏好；桌面与移动
  邮件端可切换授权邮箱，并同步作废收件箱/详情请求、清空搜索、列表、详情、选择及写信
  草稿。写信窗口显示只读 From；`can_send=false` 时禁用写信、回复、自动保存和正式发送。
  当前自动化证据仍为 Rust/契约与 Vitest/jsdom 测试，不代表真实浏览器或 WCAG 验收。
- 完成 P4A 本人私有草稿目录与精确恢复切片：目录只查询 PostgreSQL 中调用人创建的
  active Draft，Shared Mailbox 成员不能读取彼此草稿，列表只披露 UUID、版本、回复
  UUID、收件人数和时间，并使用 active/owned/current-binding 的 canonical Cursor
  锚点。恢复在事务外只做单 ID JMAP `Email/get`，要求唯一可读 Drafts Mailbox、
  `$draft`、精确 Account/Email/Thread/From、单一有界纯文本且无 HTML/附件；返回前用
  保存草稿时相同算法重算正文、内容和收件人证据，再在新事务中复核 Active Human、
  `mailbox.use`、`Read(None)` + `Send(None)`、当前 data-plane binding、本人所有权、
  active 版本与全部远端/摘要证据。只有 `mail.drafts.queried` / `mail.draft.read` Audit
  与最小 Outbox 原子提交成功后才披露目录或正文；证据不含正文、地址、主题、Cursor
  或 Provider 引用。点击发送也改为首个异步等待前同步锁定快照；请求可能出网后持续
  锁定并复用 Idempotency Key，不把未知提交状态静默复制成新 Draft。孤儿草稿清理和
  Shared Draft 协作仍明确未实现。
- 新增单主机 `compose.pilot.yaml` 运行策略：为全部基础服务固定 CPU、内存和 PID
  上限、停止宽限期与重启策略，并给 Worker 增加数据库 readiness 健康检查。Rust
  API、Outbox Worker 和审计维护 Worker 现在同时响应 Ctrl+C 与容器常用的
  `SIGTERM`；两个常驻循环在循环外只注册并 pin 一次信号 future，工作或退避分支先
  完成也不会吞掉单次终止信号。CI 增加有硬超时、保留原退出码和有限故障日志的
  启停/退出码为零/重启恢复 smoke；当前仅完成代码与 CI 配置，动态执行仍受未完成的
  WSL VHD 离线修复、既有异常 dev 栈和端口切换阻断。
- Stalwart 常态 Mail 覆盖层移除恢复管理员，五项 Mail adapter/Event Ingress
  readiness 固定 Fail Closed，API/Worker 等待 Stalwart `/healthz/ready`；恢复
  管理员只存在于显式、一次性的 `compose.mail-bootstrap.yaml`。Compose 安全门禁
  同时拒绝特权容器、Host namespace、额外 capability、设备与 Docker Socket。
  Ubuntu/WSL 前置检查会拒绝 ambient Compose 控制/插值变量，静默核对最终项目名和
  回环端口，并验证仓库与 Docker data-root 所在挂载均为 `rw`。
- Ubuntu WSL Base Pilot 已在 WSL ext4 源码快照上完成真实 Compose 构建与启动。
  PostgreSQL 17 成功应用 51/51 份迁移，审计事件目录核对为 80 类；API、Admin、
  Mail 与 MinIO 健康检查通过。所有发布端口保持 `127.0.0.1` 回环绑定，应用进程
  非 root 运行；PostgreSQL 与 MinIO 定向重启后迁移、审计目录和对象存储证据保持，
  Worker 故障注入后在同一容器身份下自恢复。该验收仅覆盖 Base Pilot：尚未从第二台
  LAN 机器完成端口不可达负向探测，尚未执行 PostgreSQL 17 同版本原生备份—恢复、
  完整 ignored PostgreSQL/MinIO Object Lock 测试，也未启用企业 IdP、真实域名邮件、
  HA、容量、安全或 GA 验收。验收后宿主 ext4 VHD 出现 checksum/journal 错误并被
  重挂为只读，现存健康响应不能证明可靠写入；修复与完整复验前该部署不可使用。
- 新增 Migration 0050、0051、0052 与 ADR 0036：Human 列表、搜索和正文只接受当前绑定下
  已提交入站事件的安全 Message UUID，信封/正文分别在 Audit/Outbox 原子提交后披露；
  Mail Engine Account 首次绑定后永久归属同一租户邮箱。`0051` 升级在同一事务中先
  阻断旧 binding 写入，再从当前绑定与不可变审计历史恢复退休 Account，歧义历史
  整体回滚；`0052` 严格复核历史证据、追加修复缺失 owner，并以提交期约束拒绝没有
  精确 current binding 的直接 INSERT。readiness 核对 owner 主键、四个触发器的启用
  状态、事件/时机、UPDATE OF 列、延迟属性、函数和精确 owner 覆盖。PostgreSQL CI
  门禁扩展为真实 `0050 → latest` 历史回填、跨租户/跨邮箱拒绝、孤立 INSERT 回滚、
  正常首次绑定/轮换、触发器破坏、coverage 缺口及冲突回滚。新增 0052 与 0053 已随
  0001→0053 完整迁移链在 disposable PostgreSQL 17 执行，并完成数据平面直接 SQL
  不变量验证；完整 ignored PostgreSQL 套件仍需单独验收。此前还修正
  0040/0043/0044/0045 的审计目录列名，避免全新安装在迁移阶段失败。
- Human Mail API 与前端移除 JMAP Thread/Query State 及原始 Provider 标识，Cursor
  只绑定邮箱、位置和可选查询摘要；全部 Human 邮件响应使用 `no-store` 与
  `no-referrer`。详情选择代次、AbortController、同步清空和身份复核共同阻止 A→B
  快速切换时显示迟到正文；状态更新不再二次 GET 正文。严格响应重建拒绝未知字段、
  越界值和回复 UUID 不一致。
- 新增 PostgreSQL 逻辑备份—隔离恢复门禁与运行手册：使用原生匹配主版本的
  `pg_dump` custom archive，拒绝覆盖既有制品，恢复目标仅能是脚本生成的独立数据库；
  对 migration ledger、审计目录/哈希头、不可变触发器与指定租户证据做源/恢复一致性
  摘要比较，并在成功或失败时精确清理生成数据库。PostgreSQL 17 CI 执行真实备份和
  恢复，数据库无关安全自测可在开发机运行。
- 新增 Ubuntu/WSL Pilot 独立运行手册和只读 Fail-Closed 前置检查。脚本要求显式
  选择 SSH 隧道或 HTTPS 入口，拒绝 Windows 挂载文件系统、旧 Docker/Compose、
  ext4 非 `rw` 挂载、非 Linux 架构、不足磁盘/inode、非回环开发端口、占用端口、不安全 Origin、
  非普通/符号链接/权限或 UID 10001 不匹配的 Secret，并按 Human、Stalwart
  bootstrap、Mail、审计维护和观测 profile 分阶段校验；不会读取 Secret 内容、
  拉取镜像或启动/修改服务。仓库新增 `.gitattributes` 固定 shell 脚本 LF，
  避免 Windows checkout 破坏 WSL 执行。
- 新增 Migration 0049、ADR 0035 与 Human OIDC BFF Session。浏览器端已移除
  Discovery、PKCE、Token Endpoint 和 `Authorization` Token 状态；Rust 服务端
  使用公共客户端 Authorization Code + S256 PKCE，严格限制 issuer、同源端点、
  JWKS/RS256、JWT audience/subject/expiry、响应大小与重定向。PostgreSQL 只持久化
  state/nonce/Session/Provider Token 摘要和加密 verifier；登录事务单次消费，
  Session 创建/撤销与 `identity.human_session.*` Audit/Outbox 原子提交。Admin 与
  Mail 使用独立 Host-only HttpOnly Secure Cookie，登录关联为 SameSite=Lax、
  Session 为 SameSite=Strict；显式客户端路由、业务路由、Cookie 与精确 Origin
  任一不一致均 Fail Closed。数据库按客户端执行跨副本每分钟硬限额，并只做有界的
  过期、未形成 Session 的 pre-auth 清理。OpenAPI、Compose 安全 overlay、反向代理
  敏感认证日志抑制、前端与中英文文档同步更新。
- 新增 ADR 0034、Migration 0048 与 Human 邮箱隐私优先搜索。Human-only
  `POST /api/v2/mailboxes/{mailbox_id}/messages/search` 在 JMAP 调用前校验
  `mailbox.use`、Read ABAC 和精确租户 Account 绑定；查询词只在 JSON 请求体和
  数据平面内存在，不进入 URL、Cursor 或持久证据。查询绑定 Cursor 拒绝跨邮箱/
  跨查询复用，JMAP 只返回有界信封字段。结果必须在
  `mail.messages.queried` Audit 与最小化 Outbox 原子提交后披露，提交失败
  Fail Closed。React 邮件端提供搜索、清除、刷新与继续加载，OpenAPI、双语
  README、安全边界和测试同步更新。
- 新增 Migration 0047、ADR 0032 与 Testing Agent 非投递 Sandbox。正常 Submission
  对 Testing Agent 在应用与数据库双层返回 `agent_mail_sandbox_required`；专用
  Agent-only API 在双 Scope、Ingress、实时 Send ABAC 和环境校验后，使用租户/Agent
  事务锁与版本 CAS 冻结精确草稿，原子写入不可变 Sandbox、High Audit 和 Outbox，
  不创建 Intent 且不调用 JMAP Submission。Header-only Idempotency Key 精确回放
  返回相同证据，变参复用拒绝。所有 Agent 作者的草稿由服务端写 RFC 3834
  `auto-generated`/`auto-replied`；回复源的 `Auto-Submitted` 由精确 JMAP Email
  可信读取并固化，Agent 回复自动来源在数据库拒绝。新增租户/主体范围的正文摘要，
  滚动 24 小时第 11 次完全相同正文触发独立 429 熔断；原有 production 线程熔断
  继续并行生效。OpenAPI、Agent Skill/CLI、威胁模型、双语 README、未认证 Fail
  Closed 测试及 PostgreSQL 集成场景同步更新。
- 新增 Migration 0046 与 Human/Agent 共用的显式回复语义：调用者只提交同租户、
  同邮箱的入站 Message UUID，且必须同时持有 Send 与 Read ABAC；控制平面解析当前
  邮箱绑定下的精确 JMAP Email，数据平面有界读取唯一 `messageId` 和 `references`
  并生成 `inReplyTo/references`，不接受原始 Header、Thread ID 或 JMAP Email ID。
  回复 UUID 纳入内容摘要和草稿证据，并在草稿替换、冻结、发送过程中不可换绑；
  Human 详情仅在入站投影提交后披露安全回复 UUID，邮件端据此显示回复入口，
  Agent CLI 提供 `--reply-to-message-id`。OpenAPI、中文边界文档和适配器测试同步更新。
- 新增 Migration 0045 与 Agent processing subscription 生命周期 API：自有 Active
  Agent 可使用 Header-only Idempotency Key 执行 `active→paused`、`paused→active`
  和 `active/paused→retired`。暂停期间入站 Fanout 继续形成持久 pending backlog，
  新 Claim 立即停止但已持有的有效 Lease 仍可续租、Ack/Nack 以安全排空；恢复再次
  校验实时 Read ABAC；退役是终态，
  数据库拒绝仍有未过期 Lease 的请求。身份、状态、时间单调性、合法迁移和 ABAC
  在应用与 PostgreSQL 触发器双重校验，状态、不可变幂等操作账本、三类 Audit 与
  Outbox 原子提交；OpenAPI、Agent Skill/CLI、中文指南和认证前 Fail Closed 测试同步更新。
- 按实施计划 6.3 重写仓库内 `skills/copilotmail`：删除 V1 密码登录、静态 Admin Key、
  `/v1` 邮箱 CRUD 和查询串 WebSocket Token 指引，改为 V2 短期 Scope Token、
  `private_key_jwt`/原生 mTLS 信任边界。主 Skill 明确 Claim/Lease/Ack/Nack 的提交
  顺序、邮件内容不可信、Human 状态隔离、Cursor/SSE 至少一次语义、生产 Agent 策略
  与首次联系人审批；单层 API reference 记录精确 Scope、状态码和幂等/恢复语义。
  无第三方依赖 CLI 覆盖策略、订阅、Claim、正文、续租、Ack/Nack、死信、事件补拉、
  Draft、Submission 与状态查询，只从短期 Token 文件或临时环境取凭据，拒绝向非
  Loopback 明文 HTTP 发送 Bearer，并只输出稳定错误字段。
- 完成实施计划 3.5 的 Agent 入站可靠处理闭环：Migration 0043 将处理订阅绑定到
  同租户 Active Agent 与当前邮箱 Read ABAC；认证入站事件在来源回执事务内为每个
  活跃订阅 Fanout 独立 `pending` 状态。五条 Agent-only API 支持订阅确保、按序
  `FOR UPDATE SKIP LOCKED` Claim、30–300 秒 Lease、续租、Ack 和带有界机器原因码的
  Nack；第五次 Claim 失败后进入死信。所有迁移再次校验租户、主体、订阅所有权、
  Active Agent、实时 Read 授权与未过期精确 Lease。Header-only Idempotency Key
  绑定全部请求参数，精确重放不重复审计，变参重用返回稳定 409。状态、不可变操作
  账本、六类 Audit 与最小化 Outbox 原子提交；Human 已读/星标、Webhook/Cursor 和
  Agent processing 状态互不污染。OpenAPI、中文编排文档、契约未知字段拒绝、未认证
  Fail Closed 路由测试及 PostgreSQL 全流程集成场景同步落地。
- 新增 Agent processing 死信自助查询：Migration 0044 为同租户/主体/订阅倒序
  查询建立局部索引；`GET /api/v2/agent/processing-subscriptions/{subscription_id}/dead-letters`
  重新验证 Agent 身份、双 Scope、实时 RBAC、订阅所有权、Active Principal 和邮箱
  Read ABAC。Cursor 同时绑定订阅与数据库中真实可见的死信锚点，伪造或跨订阅 Cursor
  默认拒绝。每页最多 100 条，只返回 Message UUID、尝试次数、有界机器原因和失败时间；
  Audit/Outbox 提交失败时不披露结果，响应不含正文、地址、引擎 ID、原始错误或其他主体状态。
- 新增 `GET /api/v2/agent/mail-policy`，完成实施计划 3.5 的 Agent 自助策略查询：
  只允许带 `agent.operate` Scope 与实时 RBAC 的 Active Agent，并消费跨副本 Ingress
  配额。PostgreSQL 以单语句数据库时钟快照读取 Agent 环境、最新活动策略和当前滚动
  一小时 Admission 总数；租户/主体只取认证上下文。响应返回有效 Agent Token 权限交集、
  准入状态、域白名单与额度上限，但刻意省略地址/摘要、收件人级计数、Thread、审批、
  Message 和引擎引用。production Agent 缺少策略时显式返回
  `blocked_no_active_policy`，不提供不安全的默认值。
- 完成 production Agent 发信策略到执行面的闭环：Migration 0041 的不可变候选与
  双人激活继续作为策略根；Migration 0042 在 Draft 中只保存规范收件域和租户/
  Agent 范围的地址 SHA-256，不复制原始地址。Submission 事务按 Agent advisory
  lock 串行校验域白名单、单封/小时/单地址每日额度、同一远端 Thread 的保守熔断，
  新联系人还必须携带精确绑定 `draft_id:version` 的实时 `agent.first_contact`
  审批。Intent、不可变 Admission、Critical Audit 与 Outbox 共同提交，失败回滚
  Draft freeze 和所有计数；Worker 对 production Agent 只租赁已有 Admission 的
  Intent。应用层也在 JMAP Draft 调用前执行激活策略的域名/单封上限，数据库触发器
  再次验证，构成默认拒绝的纵深防线。
- 将 Migration 0042 的五类准入拒绝通过 PostgreSQL Constraint 元数据映射为稳定
  领域错误：收件策略使用 422；小时额度、单地址日额度与线程熔断使用带有界
  `Retry-After` 的 429；首次联系人审批使用 409。响应只返回策略维度或审批资源格式，
  不暴露收件地址、摘要和引擎引用，Agent 可按 `code` 与 `retryable` 确定重试或转人工。
- 新增 production Agent 发信策略候选边界：纯领域模型与 Migration 0041 强制
  Active Human 只能为同租户 Active production Agent 创建不可变版本；规范 ASCII
  收件域白名单、单封收件人数、小时额度、单地址每日频率和每线程自动回复次数同时
  受领域/数据库硬上限约束，首次联系人审批不可关闭。新增
  `agent.production_mail` 审批动作和按 Agent 串行化的不可变激活账本：精确绑定
  策略版本、请求人/执行人、Active Human、至少双人通过与有效期，并只允许版本单调
  前移。应用激活服务进一步验证 `authorization.manage`、Active Human 和精确审批
  Evidence，并将激活、`agent.mail_policy.activated` Critical Audit 与 Outbox 原子
  提交。Human-only 管理 API 只接受路径中的 Agent/版本与正文中的审批 ID，实时验证
  `authorization.manage` Scope/RBAC，并返回不含策略秘密的不可变激活证据。候选
  创建 API 在 Agent 事务锁下分配单调版本，将不可变版本、High Audit 与 Outbox 原子
  提交；精确 Agent/版本查询在返回策略前提交自身 High Audit/Outbox。创建、查询或
  激活本身不会单独解除 production Agent 默认拒绝；只有 Migration 0042 的精确
  Admission 才能使对应 Intent 被 Worker 租赁。
- 新增 Agent 入站安全正文读取：Agent 只能使用 Feed 返回的 Message UUID 读取精确
  绑定消息，并同时通过 `agent.operate`、`agent.mail.read`、`mailbox.use` 的实时
  RBAC/Token Scope 与邮箱 Read ABAC。响应最多返回 256 KiB `text/plain`、主题、
  发件人与附件存在性，固定标记外部内容不可信，省略 HTML、MIME、Header、收件人、
  附件元数据、JMAP 标识和 Human 状态；正文只在最小化 Audit/Outbox 成功提交后披露，
  且不会改变 Human 已读/星标或 Agent Ack/Lease 状态。
- 新增企业威胁模型：明确 Browser、Agent、IdP/SCIM、邮件数据平面、PostgreSQL、
  Webhook、Object Lock 和 OTel/SIEM 的信任边界；以 25 个威胁 ID 记录初始风险、
  当前控制、代码/文档证据及剩余工作，并给出滥用案例、Pilot 放行条件和风险接受
  必填字段。未完成的真实 IdP、公网邮件、附件安全、容量、供应链和恢复演练均保持
  显式未验证，不能因已有单元测试而误记为生产受控。
- 新增 P0 公司输入与签字清单：把人员/Agent 数、邮件峰值、附件上限、IdP/SCIM、
  域名/PTR/TCP 25、客户端兼容、驻留/保留、SLO、RPO/RTO、KMS、SIEM、备份恢复和
  运维责任转为具名决策、证据、复核日期与签字门禁；未确认值不会被代码默认值替代。
- 新增集群级 Agent SSE 配额：Migration 0037 以 PostgreSQL 租约和租户事务锁在
  所有 API 副本间强制租户/Agent 两级并发上限；租约、建连 Audit 与 Outbox 原子
  提交，正常断连最佳努力释放，崩溃后最长 5 分钟自动过期。超额返回稳定可重试
  `429`；独立 Prometheus 监听器记录 Active、建连、分 Scope 拒绝、流错误与清理
  失败，且不使用租户或 Principal 标签。
- 新增 Agent Cursor/SSE Ingress 双层限流：Migration 0038 与 ADR 0030 使用数据库时钟的一分钟
  租户/Active Agent 窗口和租户事务锁，使所有 API 副本共享同一配额。Agent 窗口
  拒绝会回滚试探性的租户计数；活跃窗口不可删除或截断，计数只能单调递增。
  超额在解析 Cursor 或读取事件前返回稳定 `429` 与 1–60 秒 `Retry-After`，存储
  异常默认拒绝为 `503`；指标只有四种固定 Outcome，不含业务主体标签。
- 新增受控 Agent 纯文本外发：复用经过验证的 Draft 远端版本替换、控制平面 CAS、
  服务端 From 派生、邮箱 Send ABAC、Header-only Idempotency、durable Intent、
  Query-before-create Worker 与收件人级投递状态，不向 Agent 暴露 JMAP 标识。
  Migration 0039 允许 development/testing Agent 精确拥有自己的 Draft/Intent；
  Agent 还必须具备 `agent.operate` Scope/RBAC，并消耗跨副本 Ingress 窗口。
  未激活 production Agent 在远端 Draft 调用前保持默认拒绝；已激活主体仍必须通过
  Migration 0042 的逐消息原子 Admission，Worker Lease 才能取得 Intent。
- 新增 Outbox 恢复性 PostgreSQL 集成场景：证明未提交应用事务不会留下事件、
  被另一事务行锁占用的最老事件会由 `FOR UPDATE SKIP LOCKED` 跳过、失败释放只允许
  当前 Worker 且进入退避、过期租约可由新 Worker 领取、旧 Worker 不能提交完成。
  PostgreSQL CI 现在连续执行两次完整 migration，第二次必须保持无操作成功。
- 新增审批中心完整读写闭环：Human 与 Agent 均可查看精确绑定自己的申请，只有
  具备 `authorization.manage` 的 Active Human 能看到“待我审批”并决策；列表
  Cursor 绑定租户、视图、状态和真实可见锚点。原请求人可取消仍有效的 Pending
  请求；详情保留固定策略/资源与不可变步骤历史。查询和取消分别进入
  `authorization.approval.queried`、`authorization.approval.cancelled`，状态、
  Audit 与 Outbox 原子提交，管理端提供中英文低交互工作流。
- 内置 RBAC 从仅初始化 Owner/Agent Operator 扩展为九角色权威目录；领域层统一
  定义稳定 key、展示名和最小权限，Bootstrap 在同一事务为新租户完整创建，
  Migration 0035 为既有租户幂等回填。Compliance Auditor 保持只读，
  Content Reviewer 与 Break Glass 均不因角色目录存在而绕过案件/审批/资源授权。
- 新增角色治理闭环：受保护 API 和中英双语管理页读取权威角色目录，按 Principal
  筛选有效或历史分配；Active Human 只有在实时 `authorization.manage` 与精确双人
  `authorization.privilege_elevation` 审批同时成立时才能授予最长 365 天的角色。
  审批资源绑定目标、角色和毫秒级期限；非 Human 只能获得 Agent Operator，撤销
  不能移除最后一个有效 Owner。授权账本、状态、Audit 与 Outbox 原子提交且不可改写。
- 新增 `web/admin-app` 与 `web/design-system`：React/TypeScript 管理端通过
  OIDC Authorization Code + PKCE 登录，Access Token 仅驻留页面内存；提供合规
  案件和 Legal Hold 的筛选、有界分页、详情、建立、结案、施加与解除工作流，
  覆盖中英文、移动端、键盘焦点和稳定 Request ID 错误反馈。
- 管理端新增不可变审计检索：默认 24 小时、最长 31 天，支持 Action、Actor、
  资源、Outcome 和 Request ID 的受控筛选；编辑条件与已执行查询分离，续页复用
  服务端精确窗口及全部筛选。详情呈现脱敏治理上下文、链序号、前后 Hash 与签名
  批次；审计 `BIGINT` 链序号改为十进制字符串契约，避免浏览器超过安全整数后
  破坏证据精度。
- 新增生产保留策略治理：租户策略身份和版本不可变，草稿不改变生产行为；只有具备
  `retention.manage` 的 Active Human 使用与待激活版本精确绑定、至少两人通过的
  `compliance.retention_change` 审批，才能单调激活更高版本。管理端明确展示活动
  版本与最新草稿，Legal Hold 始终覆盖后续处置。
- 新增可选 OTLP/HTTP Span 导出：生产 Endpoint 强制 HTTPS 且拒绝 URL 凭据、
  Query 与 Fragment；W3C `traceparent` 进入 HTTP Span，审计维护 Span 关联稳定
  业务 Trace ID 和 Checkpoint。远程只发送有界 Span 字段，不发送任意日志 Event、
  错误字符串、Header、Query 或正文；新增 Collector/Tempo/Prometheus/Grafana
  开发覆盖层与独立运维边界文档。
- 新增独立管理端多阶段 Docker 镜像与同源 Nginx API 代理：运行时注入非秘密
  OIDC/租户配置，设置 CSP 等安全响应头，保持只读根文件系统、临时运行时配置、
  零 capabilities 与 `no-new-privileges`；CI 增加 npm 安全审计、类型、测试、
  构建和镜像门禁。

- 根目录升级为兼容 V1 的 Cargo Workspace，V2 crates 使用单向依赖边界。
- `Principal` 统一支持 Human、Agent、Service，并保留明确生命周期。
- 人类已读状态与 Agent Lease/Ack 状态分离，避免互相污染。
- 邮箱地址值对象拒绝 CRLF 和 SMTP 命令注入。
- 注册 Principal 的应用服务保证业务写入、审计事件和 Outbox 在同一事务提交。
- PostgreSQL migration 建立组织、身份、邮箱、委托、凭据、Agent 订阅/处理、幂等、Webhook、Outbox 和审计基础表。
- V2 控制进程提供 `serve`、`worker`、`migrate`、`check` 四种运行模式，以及 liveness/readiness 接口。
- Docker 开发栈包含 PostgreSQL、MinIO、迁移任务、API 和 Worker；运行进程使用非 root、只读根文件系统和最小 Linux capabilities。
- CI 增加 workspace 格式、测试、Clippy、构建、V1 E2E、PostgreSQL migration/check 和双镜像构建门禁。
- Organization Bootstrap 在单事务中创建租户、激活的根 Service Principal、审计和 Outbox；同一 slug 并发执行返回同一组 ID。
- HTTP 控制平面传播 `x-request-id`，错误契约包含稳定错误码、request/trace ID、可重试标记和安全详情。
- PostgreSQL 集成测试覆盖并发 Bootstrap 幂等性、业务/审计/Outbox 原子结果，以及审计 UPDATE/DELETE 拒绝。
- Permission、系统 Role 和有效期 Role Assignment 已进入领域模型及 PostgreSQL；授权只接受从已认证 Principal 解析出的租户。
- Org Owner 不自动拥有正文合规查看或 Break-glass，跨租户访问默认隐藏资源，未配置可信认证 adapter 时受保护 API Fail Closed。
- 新增 V2 OpenAPI 契约和默认拒绝授权 ADR。
- 新增独立 OIDC adapter：严格校验 Discovery issuer、HTTPS、RS256、`kid`、签名及 `exp/nbf/iss/aud/sub`，并带 JWKS 轮换刷新和未知 key 限流。
- Human OIDC 身份仅在精确 `(issuer, subject)` 映射到 Active Human Principal 后认证成功；认证配置可纳入 readiness。
- `bind-oidc-owner` 在单事务中创建首位 Human Owner、写入 Profile/审计/Outbox，并撤销 Bootstrap 的全部角色、永久退休 Bootstrap Service Principal。
- OIDC Owner 接管以组织级 advisory lock 保证并发幂等；不同人员不能通过离线命令覆盖首位 Owner。
- 新增独立 `copilotmail-scim` adapter，提供 SCIM 2.0 ServiceProviderConfig、Users、Groups、分页、受限过滤、PUT/PATCH/DELETE 和弱 ETag。
- SCIM User `externalId` 与 OIDC `sub` 共用 HumanProfile；首位 Owner 可被目录采用而不会创建重复 Principal，停用会使 OIDC 和授权解析立即失败。
- SCIM Group 成员通过租户复合外键仅能引用同组织的 SCIM User；成员整体替换使用版本 CAS，并支持常用 add/remove PATCH。
- SCIM Token 仅存储 SHA-256 摘要，绑定租户、有效期和 Active Human Owner；提供一次性签发输出与立即撤销命令，所有目录变更均原子写入审计和 Outbox。
- 新增独立 `copilotmail-agent-auth` adapter：Agent 使用 Principal UUID 作为 OAuth2 `client_id`，Credential UUID 作为 JWT `kid`，以 RS256 `private_key_jwt` 完成 Client Credentials 认证。
- Client Assertion 强制精确 issuer/subject/audience、最长五分钟和一次性 `jti`；PostgreSQL 原子消费 `jti` 摘要，重复 Assertion 失败。
- Agent Access Token 最长十五分钟，实际 Scope 为凭据 Scope 与实时 RBAC 的交集；每次受保护请求实时校验 Agent 和凭据状态，凭据撤销会使存量 Token 立即失效。
- Agent 注册、增加重叠凭据和撤销凭据均提供离线 CLI，并在同一事务写入业务状态、审计和 Outbox；新增接入指南与 ADR 0007。
- 新增原生 Rustls HTTPS listener：客户端证书由专用 CA 在 TLS 握手期间验证，应用只接受进程内连接证明，不接受代理证书 Header。
- Agent mTLS 凭据只存储叶证书 DER 的 SHA-256 指纹；mTLS Token 使用 RFC 8705 `cnf.x5t#S256`，所有受保护请求必须继续出示同一证书。
- mTLS 凭据同样支持重叠轮换、实时 Scope/RBAC 校验和立即撤销；新增 `add-agent-mtls-credential` CLI、migration 0007、握手测试与 ADR 0008。
- Agent 授权服务器签名由单 Key 升级为最多 10 Key 的共享 Key Ring；签发和验证按不可复用 `kid` 精确选 Key，支持活动 Key 原子切换。
- 新增 `/oauth2/jwks.json`，发布 RFC 7517 RSA 验证 Key，并支持 ETag 与 60 秒缓存；私钥不会进入响应、日志或数据库。
- Key Set Manifest 与 Key 文件支持周期热加载；候选集合整体校验后一次替换，旧 Token 保留窗口未结束时拒绝删除旧 Key，重启后同样采用保守窗口。
- 新增签名密钥轮换与回滚 Runbook，以及 ADR 0009。
- `mailbox.use` 现在仅作为 RBAC 上限，不能再隐式访问任意邮箱；资源判定还必须满足同租户、同邮箱及当前有效的成员关系或显式委托。
- 新增邮箱创建、委托签发与撤销应用服务，业务状态、追加式审计和 Outbox 在同一事务提交。
- PostgreSQL migration 0008 为邮箱成员关系补充租户列与复合外键，为 Delegation 限定可委托 scope、有效记录唯一性和授权解析索引。
- Mailbox Owner、Member、Delegate 的能力矩阵已固化；旧 Delegate 标签本身不授予任何能力，`send_as` 必须精确携带委托人 `on_behalf_of`。
- 新增邮箱授权矩阵与 ADR 0010；审批上下文被明确为后续收窄层，不能绕过 RBAC 或资源授权。
- PostgreSQL 集成场景覆盖跨邮箱拒绝、跨租户隐藏、Send As 身份绑定、撤销立即生效和过期失效。
- 新增不可变 `policy_versions`、资源范围固定的 `approval_requests` 和追加式 `approval_steps`；数据库与领域层同时拒绝请求人自审和同一人重复审批。
- 审批决策按请求行锁串行化，拒绝立即终止，达到策略要求的不同审批人数后才通过；状态写入、审计和 Outbox 原子提交。
- `ApprovalGateService` 明确执行“基础身份/RBAC/资源授权 AND 精确审批证明”，审批无法创造或扩大权限；证明不能跨租户、动作或资源重用，到期后立即失效。
- 新增审批工作流说明和 ADR 0011；数据库集成场景覆盖双人审批、自审拒绝、资源范围、到期和审计/Outbox 证据。
- 新增 `create-mailbox`、`grant-mailbox-delegation`、`revoke-mailbox-delegation`、`create-approval-policy` 和 `append-approval-policy-version` 离线治理命令；策略追加使用 Active Version 比较避免并发丢失。
- 新增受保护审批 API：请求创建按动作检查实时 RBAC 与 Token Scope，审批决策要求 `authorization.manage`；两者都从认证身份绑定 Principal/租户，不能由请求正文伪造。
- OpenAPI 3.1 增加审批请求/决策契约、稳定错误响应和动作枚举；未配置可信认证时两个端点均在访问数据库前 Fail Closed。
- 新增 `GET /api/v2/mailboxes/{mailbox_id}`：Human 与 Agent 共用认证内核，依次校验 Token Scope、`mailbox.use` 和邮箱资源授权；不存在、跨租户、跨邮箱、委托过期/撤销统一隐藏为 404。
- 新增 Human 收件箱信封读取 API：身份适配器以不可伪造类别区分 Human/Agent，
  仅 Human 在通过实时 RBAC/Scope、资源 Read ABAC 后，才可用同租户邮箱精确绑定
  的 JMAP Account 查询 `Email/query` + `Email/get`；管理 API Key 与邮件内容
  OAuth Token 物理分离，单页最多 50 条且不返回正文、原始 Header 或 Blob ID。
- 新增 Human 纯文本草稿 API 与写信界面：实时 RBAC/Scope 和邮箱 Send ABAC 通过后，
  服务端从邮箱投影派生 From，调用者无法伪造；独立 Compose Token 只能解析 Drafts
  Mailbox 并创建/销毁 Email，不能读取收件箱或提交邮件；客户端只持有控制平面 Draft
  UUID 与 CAS 版本，远端 Email/Thread ID 由租户、Actor、Mailbox 绑定的 PostgreSQL
  溯源记录解析；旧 ID 销毁前必须再次证明
  属于唯一 Drafts Mailbox、带 `$draft` 且 From 匹配授权邮箱。前端 1.2 秒防抖且严格串行
  替换不可变 JMAP 草稿；部分成功时保留新 Draft ID 并提示旧版清理失败。
- 新增 Migration 0030 与 ADR 0027：草稿远端版本、内容 SHA-256、创建 Human、租户、
  邮箱和 Account 形成可冻结的单调版本记录；发送 Intent 以 Idempotency/Request
  SHA-256 固定请求，使用有界租约和 `pending → reconciling → terminal` 状态，禁止
  删除、截断、终态修改及跳过对账直接接受。CI PostgreSQL 场景覆盖 CAS、冻结精确
  绑定、跨 Actor 隐藏、非法终态跳转与不可变触发器。
- 新增 Migration 0031、独立 JMAP Submission adapter、租约执行器、正式发送 API
  与写信交互：API 只冻结 Draft 并写 durable Intent，不持有提交 Token；Worker
  启动时验证专用凭据，每次租约均先按冻结 Email 查询远端，唯一既有结果直接收敛，
  零条才创建。网络/响应不确定保持 `reconciling`；重复 Identity/Submission 或绑定
  不一致进入人工复核；接受/拒绝/复核终态、Draft sent 状态、审计与 Outbox 使用
  Lease ID CAS 原子提交。前端点击发送先等待最新自动保存，随后复用同一
  Idempotency Key 并轮询稳定 Intent。
- 新增 Migration 0032 与投递对账服务：Worker 通过精确
  `EmailSubmission.id` 获取每个收件人的 JMAP `deliveryStatus`，地址转换为
  Submission 范围摘要；不可变快照、收件人明细和最小化
  `message.delivery_status_changed.v1` 原子提交，相同快照幂等去重，未终态沿原
  Outbox 指数退避。状态 API 与 Human UI 不再把 JMAP `accepted` 表述成最终投递。
- 新增 Migration 0033、ADR 0028 与 Stalwart 队列证据接入：白名单解析
  `delivery.delivered/failed/completed` 和 `queue.rescheduled`，以规范化
  Envelope From 精确映射唯一已绑定主邮箱；不可变来源账本与最小化内部 Outbox
  原子提交，收件人只保存队列作用域摘要，远端响应文本不落库。队列事件不猜测
  `queueId → Submission`，Human/Agent 可见状态仍只来自精确 JMAP 对账。
- 新增受保护邮箱管理 API：创建要求实时 `mail.admin`；委托签发/撤销要求实时 `authorization.manage` 和该邮箱有效 Owner；租户、Actor 与 grantor 均来自认证上下文，所有写入继续原子记录审计与 Outbox。
- 领域级审计事件目录已随归档生产者扩展为 24 类；未登记 action、错误 resource type、超过 64 KiB 的 details，或嵌套密码、Token、私钥、正文、MIME、附件内容会在持久化前拒绝。
- migration 0010 建立基础目录，后续 migration 只随真实生产者增量扩展；`(action, resource_type)` 受外键约束，目录与事件表禁止 UPDATE/DELETE/TRUNCATE。
- 审计表新增 request/received time、subject、来源、客户端/Session、Token JTI、Policy/Version、Approval、工单、环境与区域列；Principal、Policy、Approval 引用均使用租户复合外键。
- `scripts/check_audit_governance.py` 已进入 CI：校验 Rust/SQL/生产者目录完全一致，并拒绝没有 Audit + Outbox 的应用事务提交；仅允许明确标记的无状态幂等重放。
- 新增企业审计事件目录说明和 ADR 0012；PostgreSQL 集成测试扩展为核对目录一致性、显式请求上下文和目录不可变性。
- Migration 0011 为历史和新增审计事件建立租户隔离的连续哈希链；并发写入通过租户级事务锁避免分叉，链字段由数据库独占生成。
- 新增 `verify-audit-chain` 只读 CLI，在一致性快照中分页读取并由 Rust 独立重算 SHA-256；单元测试覆盖负载篡改、序号缺口和跨租户错配，PostgreSQL 集成场景覆盖并发追加与真实断链检测。
- 新增独立 `copilotmail-audit` 加密模块、Migration 0012 和 ADR 0014：Checkpoint 使用 2048–8192 位 RSA/SHA-256 签名，数据库复合外键把签名链头绑定到同租户真实事件，并禁止修改或截断。
- `create-audit-checkpoint` 要求实时 `audit.export` 权限，Checkpoint、关键审计事件和 Outbox 原子提交；稳定 Checkpoint ID 支持精确幂等重放。
- `export-audit-checkpoint` 分页生成 `events.jsonl` 与签名 `manifest.json`；`verify-audit-export` 不读取环境配置或数据库，使用独立提供的受信公钥逐行重算链和文件摘要。
- 新增独立 `copilotmail-object-store` adapter 和 Migration 0013：归档前确认 Bucket Versioning/Object Lock，上传携带 SHA-256、条件创建与逐对象 Retention，随后按精确 Version ID 回读校验。
- `archive-audit-export` 在任何上传前完成离线验真；两个远程对象均验证后，归档证据、`audit.archive.completed` 与 Outbox 才在 PostgreSQL 原子提交。稳定 Archive ID 支持远程成功、数据库失败后的精确重试。
- Docker 开发栈通过 MinIO Client 创建启用 Object Lock 的 Bucket；CI 增加真实 MinIO 带保留上传及幂等版本重放测试。生产强制 HTTPS 与 Compliance 模式，凭据使用 AWS 标准 Provider Chain。
- 新增独立审计维护进程与 ADR 0025：每租户绑定最小权限 Service Principal、签名 Key 和持久工作卷；优先恢复最早未归档 Checkpoint，使用稳定 UUIDv8 Checkpoint/Archive 身份，复用完整签名包并隔离半成品；远程回读和 PostgreSQL 证据均完成后才清理本地包，旧 Key 遗留任务不一致时 Fail Closed。Docker 提供可选 `audit-maintenance` profile。
- 审计维护单活从部署约定升级为 PostgreSQL 会话级租户 advisory lock：跨主机实例不持有长事务即可协调，锁竞争会产生明确跳过日志；正常路径显式解锁，失败、取消或 panic 路径淘汰持锁连接，避免会话锁泄漏回连接池。跨数据库选主仍由外部调度器负责。
- 审计维护常驻进程新增独立 Prometheus/健康监听：指标覆盖成功、失败、锁冲突、连续失败、周期执行时长、最后成功和最早未归档 Checkpoint 年龄；`/readyz` 对连续失败、漏归档超龄及周期超时返回稳定 `503` 原因，Compose 暴露内部 9090 端口并在 WORM 运维文档给出告警表达式。
- 审计维护新增双层追踪关联：每次调度使用临时 UUIDv7 `cycle_id`，选定 Checkpoint 后由租户/Checkpoint 稳定派生 UUIDv8 `trace_id`；后者贯穿 Checkpoint 与归档审计、导出/上传日志、S3 双对象 metadata、命令结果及 OTLP 维护 Span，重试保持不变以兼容 WORM 精确幂等恢复。
- 新增 Migration 0026、ADR 0026 与审计归档 Legal Hold：只有 Active Human 具备实时
  `legal_hold.manage`、Open 合规案件和资源精确的独立审批时，才可对归档的
  events/manifest 两个精确 S3 Version ID 施加或解除 Hold。远端逐版本回读后才提交
  状态、Critical Audit 与 Outbox；PostgreSQL 会话锁按租户/归档跨主机串行化，审批
  ID 单次消费，部分远端失败保持本地原状态并支持原参数重试。数据库再次验证 Human、
  审批人数/期限/范围、版本证据和单向状态迁移，并拒绝删除或截断。
- 新增合规案件与 Legal Hold 受保护管理 API：建案、读取、结案、施加、读取和解除六个
  入口全部从认证上下文绑定 Actor/租户，要求专用 `legal_hold.manage` 与 Active Human；
  施加/解除只接受审批 ID，由服务端验证精确审批并直接操作 S3，客户端不能伪造 Object
  Lock 证据。未认证请求在数据库或对象存储访问前 Fail Closed；CLI 同样把 Human 证明
  前置到任何远端变更之前。
- 新增合规案件与 Legal Hold 管理列表：默认 50、最大 100 条，按创建时间与 ID 倒序；
  支持案件状态、Hold 状态及案件 ID 过滤。版本化 Cursor 绑定资源类型、租户和真实
  锚点，服务端验证锚点仍匹配当前过滤条件；伪造、跨租户或换过滤器返回稳定 410。
  PostgreSQL 增加覆盖列表排序的复合索引与一个需外部数据库执行的分页集成场景。
  单项/列表读取在返回结果前提交 `compliance.case.queried` 或
  `compliance.legal_hold.queried` Audit + Outbox；证据失败时查询 Fail Closed。
- Rust、CI 与控制平面 Docker Builder 统一固定到 1.97.1；AWS SDK 精确锁定版本并关闭未使用的 SSO 默认特性，保留 HTTPS、Tokio、凭据进程和标准工作负载凭据链。

- 新增独立 `copilotmail-mail-engine` adapter：使用凭据文件执行 JMAP discovery，验证 Core 与 Stalwart 管理能力，禁止 HTTP 重定向与跨源 API URL，限制会话响应和凭据大小；Stalwart API Key 不会被误用为最终用户邮件凭据。
- 控制平面新增 `check-mail-engine`，并把实时 `mail_data_plane` 探测纳入 `/readyz`；生产默认要求邮件引擎就绪，开发环境可在 bootstrap 完成前保持软依赖。
- 新增固定 `stalwartlabs/stalwart:v0.16` 的 Docker PoC 覆盖层、只读密钥挂载和中文运行手册；新增 ADR 0033、普通账号声明式非删除 upsert 计划，以及有界 SMTP/IMAP/JMAP、STARTTLS、非开放中继和两阶段重启持久性验收程序。默认 CI 只执行计划/程序离线自测，当前仍未把未运行的外部互操作、公网收发或 PG/S3 恢复误记为完成。
- 新增邮件域名领域状态机与 Migration 0014：域名全局唯一并做 IDNA/Punycode 规范化；DNS 观测绑定 ownership challenge 摘要且不可修改；DNS 完整验证与显式启用分离；暂停恢复强制重新验证；Direct MX 与 Smart Host 路由配置独立版本化。
- 新增独立 `copilotmail-dns` adapter 与 `check-domain-dns`：禁用 hosts 文件并使用绝对 DNS 名；限制总超时、记录/TXT/MX 数量和证据 TTL；逐 MX 解析并拒绝非公网 IPv4/IPv6；识别 Null MX；验证唯一 SPF/DMARC 及至少 2048 位 RSA DKIM；输出不包含 ownership token 明文。
- 新增 `MailDomainGovernanceService` 与 PostgreSQL 原子适配：注册、challenge 轮换、DNS 证据、显式激活、暂停和路由更新均执行实时 `mail.admin` 校验，并把业务状态、31 类受控审计目录中的对应事件与 Outbox 放入同一事务；激活只接受当前 challenge 的最新合格证据，路由使用版本 CAS，暂停与双向断路原子提交。
- 新增 Human/Agent 共用的域名治理 HTTP API、租户隔离读模型与 OpenAPI 契约：服务端一次性签发及轮换 ownership token；轮换使用单调版本、使旧证据失效且禁止 Active/Retired 域名静默换权；DNS 网络查询在认证授权后执行，并把规范化查询域名绑定到持久化域名；激活、暂停和路由更新使用稳定 JSON/错误码与请求 ID；详情与规范化域名游标列表返回 challenge/路由版本和当前 challenge 最新 DNS 状态，所有入口均实时校验 Token Scope、`mail.admin` 和租户。
- 新增 Migration 0018 与 ADR 0018：持久化公开 challenge 版本并登记 `mail_domain.challenge_rotated` 审计事件；轮换事务、旧证据拒绝、Active 禁止和 API Fail Closed 已覆盖测试。请求使用 `expected_version`，PostgreSQL 并发场景验证同一版本只有一个轮换可提交。
- 新增 `MailDomainProjector` 与 Stalwart v0.16 JMAP Domain 适配：通过文档化 `x:Domain/query/get/set` 精确匹配规范化域名，幂等创建并单次绑定不透明 Engine ID；Active 启用、Suspended 禁用且永不自动删除。Worker 从事务 Outbox 投影当前控制平面状态，失败保持未发布并通过 Migration 0016 指数退避重试。
- 新增 Migration 0017 与域名投影可观测状态：`not_projected/pending/ready/failed`、引擎实际启停值、稳定错误码及 attempt/success 时间进入租户读模型和 OpenAPI；Worker 对生命周期状态使用条件更新，网络调用期间发生的新管理动作不会被旧投影结果覆盖。
- 新增独立 `copilotmail-webhook` 出站 adapter、Migration 0019 与 ADR 0019：稳定事件信封、HMAC-SHA256 版本化签名、文件 Key-set 重叠轮换、1 MiB Body/超时上限、HTTPS 强制、无重定向、禁 URL 凭据、host-file-disabled DNS、公网地址校验及连接地址固定。Worker 按租户/事件 fan-out，每次尝试追加落库，成功订阅不重复投递，可重试失败保留 Outbox，永久错误或达到上限进入死信；日志不再输出 Payload。
- 新增 Webhook 订阅领域模型、Migration 0020 与管理 API：只允许同租户 Active Agent 作为 Owner；对外事件固定为三类版本化允许列表并移除 `*`；Agent 仅能管理自己，Human 代管同时检查 `agent.operate`、`identity.manage` 的 RBAC 与 Token Scope；Endpoint、Key Reference、版本 CAS、禁用审计均在领域与数据库双重约束。
- 新增 Migration 0021 与 ADR 0020：`security.admin` 可查询无响应正文的租户死信，并对精确的最新死信执行受控重放。重放追加 `manual_replay_requested` 记录、重新调度原 Outbox、开启独立重试周期，并与关键审计及治理事件同事务提交；投递账本继续拒绝 UPDATE/DELETE/TRUNCATE。
- 新增订阅级 Agent Event Cursor、Migration 0022 与 ADR 0021：启用中的订阅只能由精确 Owner Agent 以实时 `agent.operate` RBAC + Token Scope 补拉；Feed 只查询三类外部允许事件并复用 Webhook 稳定信封，不暴露通用 Outbox。版本化 Cursor 同时绑定订阅和 Event，锚点必须是该订阅真实可见事件；非空页面仅审计数量与首尾 ID，不复制消息 Payload。
- 新增 Stalwart v0.16 认证事件入口、Migration 0023 与 ADR 0022：HTTP Bearer 与原始 Body 的 Base64 HMAC-SHA256 必须同时通过；只接收 `message-ingest.ham/spam`，批次/时间/资源引用有界，未知事件和未绑定 Account 整批 Fail Closed。Human MailAdmin 以版本 CAS 维护 Account→租户邮箱绑定；来源 Event ID 与 Account/Document 双键去重，追加式来源回执和最小化 `message.received.v1` Outbox 原子提交，Agent Payload 不含引擎引用、地址、Header、Blob、大小或正文。
- 新增有界 Agent SSE 与 ADR 0023：只允许订阅精确 Owner Agent 通过 Authorization Header 建连，复用 Cursor 锚点验证、事件允许列表和稳定信封；支持 query Cursor/`Last-Event-ID` 一致性恢复，固定 50 条批次、2 秒轮询、15 秒心跳及 5 分钟重认证上限。运行期错误只发送稳定无敏感数据的 `stream.error` 后关闭；成功建连原子写入 `agent.event_stream.opened` Audit/Outbox，非空批次继续复用 Cursor 审计。
- 新增租户审计检索、Migration 0025 与 ADR 0024：仅 Active Human 通过 `audit.read` Scope/RBAC 查询，默认最近 24 小时、最长 31 天、每页最多 100 条；支持受控精确过滤，按哈希链序倒排，Cursor 同时绑定租户并验证当前过滤窗口内的真实锚点。返回显式治理上下文及前后哈希但不返回内部 hash payload；每次成功查询（含空结果）必须先原子写入 `audit.events.queried` Audit/Outbox，证据提交失败则不返回已读结果。

## 已验证

| 验证项 | 结果 |
|---|---|
| `cargo fmt --all --check` | 通过 |
| `cargo check` | V1 根包、V2 workspace 与最终控制平面依赖组合分拆检查通过 |
| `python scripts/check_architecture.py --self-test` | 通过；Domain、Audit、Application 三个权威 package-id 根均按分层允许集检查解析后的 all-features 完整依赖图，覆盖根 dev/全部 target、workspace 别名、非 workspace 中转和后续 build 边；Mail Engine 全 Rust 源码 corpus 只能恰好一次私有导入 Application ports，并拒绝子模块重导出、名称遮蔽、raw identifier 与 `extern crate` alias 反例 |
| `python scripts/check_web_i18n.py --self-test` | 通过；保留全生产 TS/TSX Han 字符所有权检查，并用仓库锁定的 TypeScript AST 拒绝 JSX 文本、字符串表达式及常见可见/无障碍属性绕过简中/英文类型化词典；英文、条件、模板与属性反例自测通过 |
| Rust 单元/协议测试 | `cargo test --workspace --locked` 实际运行 304 个非忽略测试并全部通过，0 个失败；其中 Application 102 个、Control Plane 54 个、Domain 43 个、Mail Engine 35 个、Contracts 16 个。新增覆盖本人已发送目录的私有所有权、canonical Cursor、投递聚合、最终授权/快照重验与最小 Audit/Outbox 证据，并继续覆盖本人私有草稿目录、当页 UUID/active 版本一致性、精确单 ID Draft 恢复、Provider 内容完整性、稳定数据平面身份、legacy/v2 指纹兼容、来源亲和、routed Outbox、唯一可读 Inbox、信封/正文审计先于披露、UUID 回复、OIDC BFF、Agent 邮件、角色审批、邮件投递、域名、审计归档/导出/Legal Hold、保留策略、OTLP、SCIM 与 Webhook 安全边界；Windows 使用 `CARGO_PROFILE_TEST_DEBUG=0` 生成并运行测试二进制 |
| 需外部服务的集成测试 | 全工作区命令报告 33 个 ignored 场景：29 个 PostgreSQL 场景（包括授权邮箱目录、本人私有草稿和本人已发送目录的绑定、Read/Send、SendAs-only、所有权、active/version、Cursor、投递聚合、最终授权数据库时钟及 Audit/Outbox 原子披露；以及 Human Message UUID 当前绑定/租户/顺序投影、Account 跨租户重绑、所有权账本、Human Session、Testing Agent Sandbox、Agent 入站/Processing/生产准入、Outbox、草稿/Submission/投递、邮件事件、角色治理、审计导出及保留策略）与 4 个 MinIO Object Lock 场景均成功编译但未由该命令自动执行。当前版本另在每次重建的独立 disposable PostgreSQL 15 数据库中精确执行 7 个既有关键场景：schema readiness、Domain 生命周期、Human 当前绑定/所有权、Outbox 租约恢复、入站重放、草稿/Submission/投递及时钟偏差，以及生产 Agent 准入；均通过。本轮 Human 邮箱、草稿及已发送目录场景因本机无可用 PostgreSQL，均只完成编译，仍待 CI 或干净 disposable 数据库实跑。Migration 0001→0053 与数据平面直接 SQL 不变量另有 disposable PostgreSQL 17 实跑证据；不能据此宣称其余 ignored 场景、0054—0056 或 MinIO Object Lock 已执行 |
| `cargo clippy -- -D warnings` | 平台、应用、PostgreSQL、Object Store 与控制平面目标通过 |
| `python scripts/check_audit_governance.py` | 通过；83 个领域/数据库/生产者事件一致，事务覆盖检查通过 |
| `cargo test -p copilotmail-control` | 54 个测试本机全部通过；新增 `/submissions` 的 GET/HEAD/POST 在认证前拒绝畸形路径、GET 目录不允许隐式 HEAD 审计副作用的回归，以及仅对固定 legacy-primary UUID 选择兼容指纹、其他数据平面选择 v2 的回归、Ctrl+C/SIGTERM 选择、常驻循环持久监听与工作分支先完成后的取消窗口覆盖；Human Session 未配置时存储前 Fail Closed、双客户端 Cookie/路由分区、Cookie 与 Authorization 歧义拒绝、Cookie 属性、PKCE 密文状态绑定、登录 429、回调 Cookie 自愈和认证响应禁止缓存/Referrer 泄漏；既有 Agent、角色、审批、草稿/发送、审计、合规、原生 TLS、邮件事件、OAuth/JWKS、请求 ID、签名 Key 与 Webhook 边界继续通过 |
| Web `npm run check/test/build` 与依赖审计 | 管理端和邮件端 TypeScript 严格检查通过；Vitest 实际重跑共 125 个测试（Admin 18、Mail 107）通过。Mail App 当前 11 个测试文件覆盖类型化双语词典、Locale、授权邮箱目录、本人私有草稿列表/分页/精确恢复、本人已发送列表/分页/投递聚合、恢复覆盖确认与焦点、只读 Shared Mailbox、明确 From、写信对话框可访问性、严格 Draft/Submission/Delivery 响应解析，以及邮箱/视图/写信会话切换时全链路迟到响应隔离；未知提交状态不能由新建、回复、恢复或邮箱切换清除冻结快照和幂等上下文。既有 Human 安全 UUID 收件箱、Provider 字段拒绝、`no-store`、搜索与提交契约继续通过。两个 Vite 生产构建通过；显式 `npm audit --audit-level=moderate` 为 0。企业 IdP 登录、邮件正式发送、移动端/键盘/屏幕阅读器和 WCAG 2.2 AA 仍待真实浏览器 E2E/验收 |
| OpenAPI / AsyncAPI / Compose | V2 契约、CI、Compose 与观测配置 YAML 均可解析；OpenAPI v2 包含 82 条路径、98 个唯一 Operation ID；Admin/Mail Session 与 Logout 使用四条字面量路径和各自 Cookie Scheme，登录声明数据库准入 `429 + Retry-After`，审计制品下载为写证据的 POST；邮箱绑定请求不再接受 vendor/source、响应返回 UUID `data_plane_id`；新增 Mail Session-only 授权邮箱目录、本人私有草稿列表/精确恢复与本人已发送/投递状态目录（`HEAD` 显式认证后拒绝）；Human 邮箱搜索、Testing Agent Sandbox、Agent 自助策略/Processing/正文读取、角色/授权、Human 正式提交/审批/保留/合规/审计及邮件事件契约继续保持租户、客户端和内容权限分离 |
| `docs/openapi-v2.yaml` / `docs/asyncapi-v2.yaml` YAML 解析、必填字段与路径/操作断言 | 通过；Webhook 契约采用 AsyncAPI 3.1 |
| `python scripts/check_agent_skill.py` / `skills/copilotmail` | CI 治理脚本通过：拒绝 V1 鉴权/路径回退，核对关键 V2 OpenAPI 路径，执行 11 条 CLI 模拟请求编排，并断言非 Loopback 明文 HTTP 不得携带 Bearer；Skill Creator 结构校验和 CLI Python 编译另行通过 |
| `python scripts/check_documentation.py` | 通过；13 个必需文件、当前 78 份根级/架构/运维/Skill 文档的仓库内链接及 3 个语言入口一致，中文链接不再错误指向根 README；完整 Apache-2.0 LICENSE 与 Cargo 元数据一致 |
| `python tests/e2e.py` | 通过；HTTP、鉴权、SMTP、WebSocket、发送、CRUD、前端 |
| Stalwart PoC 自动化离线门禁 | `python scripts/render_stalwart_poc_plan.py --self-test` 与 `python tests/stalwart_poc.py self-test` 通过；覆盖声明式对象顺序/引用/Secret 字段/非法输入，以及 listener 配置、明文 JMAP 显式选择和 receipt 防篡改绑定；真实 Stalwart 协议与重启步骤仍待有 Docker 的环境执行 |
| PostgreSQL 集成测试实际执行 | WSL disposable PostgreSQL 16 已实际执行当时全部 51 份迁移、`0050 → 0051` 专项升级及完整备份—恢复演练；Ubuntu WSL Base Pilot 的 PostgreSQL 17 也已实际应用当时全部 51 份迁移并核对 80 类审计事件目录。当前代码的 Migration 0001→0053 已在独立 disposable PostgreSQL 17 完整执行，并通过数据平面注册/Origin 不可变、Domain 单次亲和及 Outbox 精确路由的直接 SQL 不变量验证；另在 PostgreSQL 15 逐测试重建数据库后通过上述 7 个关键 ignored 场景。该证据不宣称完整 ignored PostgreSQL 套件或目标 Pilot 的 0053 停写升级已经完成 |
| MinIO Object Lock 实际执行 | Ubuntu WSL Base Pilot 的 MinIO 健康检查与持久化重启通过，对象存储证据在重启后保持；完整 ignored MinIO Object Lock 带保留上传、精确 Version ID 回读与重放测试尚未在该 Pilot 执行 |
| PostgreSQL 升级与恢复门禁 | `check_postgres_backup_restore.sh --self-test` 与 Shell/CI 检查通过；WSL PostgreSQL 16 已实际完成 51 份迁移后的 custom dump、自动生成隔离数据库 restore、迁移/审计链/触发器/租户证据摘要比对及零泄漏清理。Account 所有权门禁现已扩为 `0050 → latest`、孤立 INSERT、正常绑定/轮换、四触发器与 coverage 破坏、跨租户/跨邮箱拒绝及歧义回滚；其脚本自测与静态检查通过。0052/0053 的干净迁移链和数据平面直接 SQL 不变量已在 disposable PostgreSQL 17 执行；完整 `0050 → latest` 动态门禁、ignored 套件与目标 Pilot 升级仍以最终 CI/Pilot 结果为准。Ubuntu WSL Base Pilot 曾证明 PostgreSQL 17 卷重启持久性，但 PostgreSQL 17 同版本原生 custom dump/隔离 restore 演练仍未完成 |
| Compose/CI 与 Ubuntu/WSL Pilot | YAML 与 Compose 安全策略解析通过；13 个可选发布端口保持回环绑定，认证敏感路径日志抑制、Human Session Key 隔离和构建上下文 Secret 排除通过；常态 Mail 五项依赖硬失败、恢复管理员一次性覆盖层隔离、危险容器权限拒绝及 Pilot 资源/停止策略门禁通过；`bash -n scripts/check_ubuntu_wsl_pilot.sh` 与 `--self-test` 通过，前置检查拒绝 ambient Compose 控制/插值变量，静默核对最终项目名/端口，并验证仓库与 Docker data-root 所在挂载均为 `rw`。CI 已编码有界 SIGTERM/退出码/重启 smoke，但尚未由本机 Linux Docker 动态执行。Base Pilot 曾实际构建并启动 PostgreSQL 17、MinIO、Migrate、API、Worker、Admin 与 Mail；当时 51/51 迁移、80 类审计目录、健康检查、回环发布、应用非 root、持久化重启及 Worker 同容器自恢复均有实测证据。随后目标 VHD 发生 ext4 checksum/journal I/O 错误并被重挂只读，故当前部署状态降级为不可靠，必须离线修复并应用至最新 Migration 0056 后重新验收；0056 为必须由单一迁移进程执行的在线并发索引步骤，完成后还须只读 `check`；第二台 LAN 负向探测、企业 IdP/域名/邮件 profile、HA、容量、安全与 GA 验收仍未完成；2026-08-12 只读复查再次证明当前挂载为 `rw`、Docker 可达且 Base Compose 可渲染，但异常 dev 栈占用 8083/8084、PostgreSQL/MinIO 仍以 255 退出、目标副本只到 0051，且无离线修复记录，因此没有执行任何部署写入 |

## 下一实施队列

1. 不再将存在 I/O 风险的 WSL 作为验证前提。先由责任人确认专用或明确隔离的 Linux
   Docker 环境、至少 40 GiB 可用磁盘和无端口/Compose 资源冲突；以提交校验源码重新
   实际应用至 Migration 0056（由单一迁移进程完成在线索引后执行只读 `check`），并重新执行 Base Pilot 的写入、重启与持久性验收。随后
   从第二台 LAN 机器执行发布端口不可达负向探测；使用 PostgreSQL 17 同版本原生客户端
   完成 custom dump、自动生成隔离库 restore 和证据比对；运行完整 ignored
   PostgreSQL/MinIO Object Lock 测试及最终 Linux 全量门禁。企业 IdP、域名和邮件输入
   到位前，Base Pilot 成功不得记为生产邮件、SSO、HA、容量、安全或 GA 验收。
2. 由公司责任人填写并签署 P0 输入清单中的容量、IdP、域名、合规、SLO/RPO/RTO
   与运维决定；完成邮件数据平面对比 PoC 与选型签字。威胁模型已建立，后续每个
   新外部边界必须持续更新。
3. 使用目标企业 IdP 完成 SCIM 2.0 Users/Groups 互操作测试、同步游标观测和停用 SLA 演练。
4. 为 Agent 增加 SPIFFE/SPIRE 或云工作负载身份联邦 PoC，并评估 HSM/KMS 签名 adapter。
5. 在真实 HTTPS Receiver 上演练订阅治理、Cursor/SSE 对账、Key 重叠轮换、死信
   重放、429/5xx 退避和 Worker 崩溃恢复，并执行 SSE 配额、断连清理、Ingress
   双窗口边界突发、数据库锁竞争与容量压力测试。
6. 在现有 PostgreSQL 17 CI 的事务回滚、`SKIP LOCKED`、失败释放、租约恢复和
   migration 重复执行证据之上，继续增加真实连接中断/进程崩溃恢复、migration
   升级与回滚演练，并扩大邮箱授权和双人审批的并发负向用例。
7. 为审计 Checkpoint/归档维护 Worker 增加跨数据库选主；在真实 Collector/Tempo、
   Object Lock/Prometheus 环境演练 Legal Hold 双版本部分失败恢复、告警、锁竞争、
   Trace 检索与旧 Key 轮换，并设计邮件内容 Legal Hold。
8. 开展 Stalwart JMAP/Webhook/队列/存储/升级/HA PoC，通过 adapter 验证后再确定生产邮件引擎。
