# Stalwart 邮件数据平面 PoC

> 当前用户要求先完成代码、不部署。下方部署/联网操作仅保留为后续运行手册，不在本阶段执行；
> 可执行的离线测试与外部客户端边界见 [外部邮件客户端说明](EXTERNAL_MAIL_CLIENTS.zh-CN.md)。

## 当前边界

本 PoC 使用固定小版本镜像 `stalwartlabs/stalwart:v0.16`，验证
CopilotMail 可通过文档化 JMAP 接口连接可替换的邮件数据平面。业务层不读取或写入
Stalwart 私有表。当前仓库只有这一套 adapter；“可替换”是架构边界，不代表第二种
供应商已经实现或通过验收。

稳定 UUID `MailDataPlaneId` 表示一个远端不透明引用命名空间，而不是 vendor。管理、
Content、Compose、Submission 与 Event Ingress 都必须使用由服务端配置的同一个 ID；
前四项网络 adapter 还必须与显式 canonical origin 精确同源。

当前已完成前两道工程门：

- Stalwart Docker 启动与持久卷；
- 带 Bearer API Key 的 `/.well-known/jmap` 实时探测；
- JMAP Core 与 Stalwart 管理能力验证；
- 禁止重定向、限制响应大小、验证返回 API URL 同源；
- 控制平面 `/readyz` 可按环境决定是否强制邮件数据平面可用；
- API Key 从只读文件读取，可不重启轮换。
- 使用 Session 返回的同源 API URL 调用 `x:Domain/query/get/set`；
- 按规范化域名幂等创建 Domain，并把 `MailDataPlaneId` 与 Stalwart 不透明 ID
  同时单次绑定回控制平面；已有投影不能静默切换命名空间；
- Active 投影为 `isEnabled=true`，Suspended 投影为 `false`，任何流程都不自动删除 Domain；
- 激活/暂停通过事务 Outbox 异步投影，失败使用有上限的指数退避，重试读取最新控制平面状态而不是盲放旧事件。
- 提供双凭据认证的 Stalwart WebHook 入口：Bearer 先于 Body 读取，随后按
  v0.16 源码规则校验原始 Body 的 Base64 HMAC-SHA256 `X-Signature`；
- 接收入站 `message-ingest.ham/spam`，通过带 `MailDataPlaneId` 来源亲和的版本化
  Account→租户邮箱绑定生成最小化
  `message.received.v1`，来源回执与 Outbox 原子提交并按来源 Event/Message 双键去重；
- 接收 `delivery.delivered/failed/completed` 与 `queue.rescheduled`，以精确
  Envelope From→已绑定主邮箱映射租户，写入不可变队列证据；不使用主题、收件人或
  时间猜测 `queueId → Submission`，Human/Agent 最终状态仍来自精确 JMAP 对账。
- 提供可重复的普通账号声明式初始化计划，以及有界的 SMTP/IMAP/JMAP、非开放中继和
  显式重启持久性验证程序；默认 CI 只运行其离线自测，不把未执行的外部测试计为通过。

Stalwart API Key 只用于管理 JMAP，不能登录 IMAP、SMTP Submission 或最终用户
JMAP Mail。PoC 会记录该会话是否额外公布 Mail capability，但不会把它误当成用户
邮件认证。后续互操作测试必须使用独立的普通测试账号。

## 本地启动

复制 `deploy/.env.v2.example` 为被 Git 忽略的 `deploy/.env.v2`，至少替换
PostgreSQL、MinIO 和 Stalwart bootstrap 密码，并显式确认以下非 Secret 身份配置：

```dotenv
COPILOTMAIL_MAIL_DATA_PLANE_ID=018f305d-3d0f-7b2a-9d68-7f4a3f6e2101
COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN=http://host.docker.internal:8082
COPILOTMAIL_STALWART_PUBLIC_URL=http://host.docker.internal:8082
```

Migration 0053 将为既有单实例 Stalwart 证据登记上述 legacy-primary UUID。凭据
轮换不改变它；新建或迁移到另一远端引用命名空间时必须通过受治理的注册/升级流程
分配新 UUID，不能临时自造一个未登记值。Migration 0001→0053 与针对数据平面身份、
Origin、Domain、Outbox 的直接 SQL 不变量验证已在 disposable PostgreSQL 17 实际执行；
这不代表完整 ignored PostgreSQL 套件或目标 Pilot 升级已完成。

已有数据库若停留在 0052 或更早版本，不得直接执行本文后续的整栈
`up -d --build`。必须停止旧 API、Worker 与审计维护进程，完成备份，单独运行新版本
`migrate` 并验证 Origin 绑定后，再只启动新版本；旧、新 API/Worker 不能并跑。操作顺序见
[Ubuntu / WSL Pilot 部署手册](UBUNTU_WSL_PILOT.zh-CN.md)。

恢复管理员只允许由一次性
`compose.mail-bootstrap.yaml` 注入；首次引导只启动 Stalwart，不启动尚未具备六份
正式凭据的 API/Worker：

```powershell
docker compose --env-file deploy/.env.v2 `
  -f deploy/compose.dev.yaml `
  -f deploy/compose.mail-poc.yaml `
  -f deploy/compose.mail-bootstrap.yaml `
  up -d stalwart
```

打开 `http://127.0.0.1:8082/admin`。首次启动处于 Stalwart bootstrap mode，
使用 `COPILOTMAIL_DEV_STALWART_BOOTSTRAP_ADMIN` 登录并完成五步向导。仅用于本地
PoC 时建议：

1. hostname 使用 `mail.test`，默认域使用 `example.test`；
2. 关闭公网 ACME，保留 DKIM 生成；
3. 第一轮使用默认 RocksDB，目录使用 Internal Directory；
4. 日志选择 Console；
5. DNS 选择 Manual。

完成向导后保存永久管理员账号和一次性密码，并重启 Stalwart：

```powershell
docker compose --env-file deploy/.env.v2 `
  -f deploy/compose.dev.yaml `
  -f deploy/compose.mail-poc.yaml `
  -f deploy/compose.mail-bootstrap.yaml `
  restart stalwart
```

## 声明式创建普通测试账号

安装 Stalwart CLI 1.0.2 或更高版本后，在当前 PowerShell 会话设置永久管理员凭据
以及两个仅用于隔离 PoC 的普通账号。`render_stalwart_poc_plan.py` 输出含密码的
NDJSON，因此只能直接管道到 CLI，禁止重定向到文件、日志或剪贴板：

```powershell
$env:STALWART_URL = "http://127.0.0.1:8082"
$env:STALWART_USER = "poc-admin"
$env:STALWART_PASSWORD = Read-Host "Stalwart administrator password"
$env:COPILOTMAIL_STALWART_POC_DOMAIN = "example.test"
$env:COPILOTMAIL_STALWART_POC_SENDER_LOCAL = "poc-sender"
$env:COPILOTMAIL_STALWART_POC_RECIPIENT_LOCAL = "poc-recipient"
$env:COPILOTMAIL_STALWART_POC_SENDER_PASSWORD = Read-Host "Sender password"
$env:COPILOTMAIL_STALWART_POC_RECIPIENT_PASSWORD = Read-Host "Recipient password"

python scripts/render_stalwart_poc_plan.py --self-test
python scripts/render_stalwart_poc_plan.py | stalwart-cli apply --stdin --quiet
```

该计划先按规范化 `name` 幂等 upsert Domain，再按 `name + domainId` upsert 两个
`Account/User`，并使用前一步的 `#poc-domain` 引用。它不使用 `reconcile`，因此
不会删除服务器上任何未出现在计划中的对象。不要对含 Secret 的计划运行会打印计划的
`--dry-run`。

用永久管理员登录，在 `Account → Credentials → API Keys` 创建专供
CopilotMail 的 API Key。密钥只显示一次；将其保存为
`deploy/secrets/stalwart-jmap.token`，不要写入 `.env` 或提交 Git。就绪探测本身
只需要认证能力；域名投影使用 `Replace` 权限模式授予 `sysDomainQuery`、
`sysDomainGet`、`sysDomainCreate` 与 `sysDomainUpdate`，不授予
`sysDomainDestroy`，更不能继承完整管理员权限。

Human 收件箱不能复用上述管理 API Key。Stalwart 明确将管理 JMAP 与邮件 JMAP
凭据隔离；请由测试 OAuth/OIDC 流程签发仅具 `Email/query`、`Email/get`、
`Email/set` 权限且只覆盖所需 Account 的 Bearer Token，保存为
`deploy/secrets/stalwart-mail-content.token`。生产环境应由外部 Token Broker
原子轮换该文件，不要把 Refresh Token 交给 API 进程。适配器把 `Email/set`
限制为 `$seen` / `$flagged` PatchObject；内容凭据不得具备 Submission 或管理权限。

写信草稿再使用独立的 `deploy/secrets/stalwart-mail-compose.token`。只授予精确
Account 所需的 JMAP Session、`Mailbox/get` 与 Email create/destroy，不能授予
EmailSubmission、内容查询或管理权限。CopilotMail 用新建并销毁旧版本的方式替换
不可变正文；若远端部分成功，会保留新草稿 ID 并明确报告旧版本未销毁。

正式发送使用只挂载给 Worker 的
`deploy/secrets/stalwart-mail-submission.token`。只授予精确 Account 的
JMAP Session、`Identity/get`、`Mailbox/get`、`EmailSubmission/query`、
`EmailSubmission/get`、`EmailSubmission/set` 创建能力，以及提交成功后清除
`$draft` 并移动到 Sent 所需的 `Email/set`；不能修改 Identity、不可变正文或
管理对象，也不能读取邮件正文。Worker 启动时会验证 Mail/Submission
Capability 和至少一个合法 Identity；运行时仍要求 Identity 的邮箱地址唯一匹配
控制平面授权 From。CopilotMail 不提交 Envelope，由 Stalwart 从冻结 Email
Header 派生。

确认永久管理员可用后，删除 `deploy/.env.v2` 中
`COPILOTMAIL_DEV_STALWART_BOOTSTRAP_ADMIN` 的值，并且从此不再加载
`compose.mail-bootstrap.yaml`。使用常态 Mail 覆盖层重新创建 Stalwart，确保运行中
环境不再含 recovery/bootstrap 后门：

```powershell
docker compose --env-file deploy/.env.v2 `
  -f deploy/compose.dev.yaml `
  -f deploy/compose.mail-poc.yaml up -d --force-recreate stalwart
```

六份独立凭据全部就位后，使用不含 bootstrap 覆盖层的常态配置启动 API 与 Worker。
`compose.mail-poc.yaml` 会给 API/Worker 注入同一个 `MailDataPlaneId`，并要求
`COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN`、Mail Engine、Content、Compose 与 Submission
Base URL 全部精确匹配 `COPILOTMAIL_STALWART_PUBLIC_URL`。任何一个角色指向另一
Stalwart 集群都在启动时 Fail Closed，不能依靠相同数字 Account ID 继续运行。

该覆盖层还会把 Mail Engine、Content、Compose、Submission 和 Event
Ingress 五项依赖全部强制为就绪硬依赖；任何凭据或 Stalwart readiness 缺失都会阻止
服务进入 ready：

```powershell
docker compose --env-file deploy/.env.v2 `
  -f deploy/compose.dev.yaml `
  -f deploy/compose.mail-poc.yaml up -d --build

docker compose --env-file deploy/.env.v2 `
  -f deploy/compose.dev.yaml `
  -f deploy/compose.mail-poc.yaml exec api copilotmail-control check-mail-engine
```

访问
`http://127.0.0.1:8081/readyz`，应同时看到 `mail_data_plane: ready` 和
`mail_content: ready`、`mail_compose: ready`。

提交凭据不会挂载到 API；Worker 的启动日志出现
`durable outbox worker started` 才表示 Capability/Identity 探测已通过。API
接收 `202` 只表示 Intent 已持久化，JMAP 接受状态通过 Submission 状态资源确认。

邮件事件接入还需创建两套独立 Secret、先绑定 Stalwart Account ID，再在管理界面
配置明确的 WebHook 事件白名单。完整步骤见
[邮件数据平面事件接入](MAIL_EVENT_INGRESS.zh-CN.md)。常态 Mail 覆盖层已强制该依赖；
确认 `/readyz` 中 `mail_event_ingress: ready`。

## 本地端口

| 用途 | Host | Container |
|---|---:|---:|
| Bootstrap / HTTP / JMAP | 8082 | 8080 |
| SMTP relay 测试 | 2525 | 25 |
| SMTP Submission | 2587 | 587 |
| Implicit TLS Submission | 2465 | 465 |
| IMAP | 2143 | 143 |
| IMAPS | 2993 | 993 |

这些是冲突规避后的开发端口，不是生产映射。公网接入仍应分别开放 25、587/465、
443 和按需的 993，并配合可信 TLS、固定公网 IP、PTR、SPF、DKIM、DMARC、
MTA-STS、TLS-RPT、滥用控制和监控。

## 普通账号协议互操作

把创建账号时的密码只注入当前 Shell。以下本地模式明确允许 HTTP JMAP 与自签名
STARTTLS；公网或共享环境必须改用 HTTPS、可信 CA，并移除两个 insecure 开关：

```powershell
$env:COPILOTMAIL_STALWART_SMOKE_HOST = "127.0.0.1"
$env:COPILOTMAIL_STALWART_SMOKE_RELAY_PORT = "2525"
$env:COPILOTMAIL_STALWART_SMOKE_SUBMISSION_PORT = "2587"
$env:COPILOTMAIL_STALWART_SMOKE_IMAP_PORT = "2143"
$env:COPILOTMAIL_STALWART_SMOKE_JMAP_URL = "http://127.0.0.1:8082/.well-known/jmap"
$env:COPILOTMAIL_STALWART_SMOKE_SENDER_ADDRESS = "poc-sender@example.test"
$env:COPILOTMAIL_STALWART_SMOKE_RECIPIENT_ADDRESS = "poc-recipient@example.test"
$env:COPILOTMAIL_STALWART_SMOKE_SENDER_PASSWORD = $env:COPILOTMAIL_STALWART_POC_SENDER_PASSWORD
$env:COPILOTMAIL_STALWART_SMOKE_RECIPIENT_PASSWORD = $env:COPILOTMAIL_STALWART_POC_RECIPIENT_PASSWORD
$env:COPILOTMAIL_STALWART_SMOKE_ALLOW_INSECURE_HTTP = "true"
$env:COPILOTMAIL_STALWART_SMOKE_ALLOW_INSECURE_TLS = "true"

python tests/stalwart_poc.py self-test
python tests/stalwart_poc.py preflight
python tests/stalwart_poc.py roundtrip --receipt .local/stalwart-poc-roundtrip.json
```

`preflight` 会验证三个 listener 分离、SMTP/IMAP 所选 TLS 模式、Relay 不公布 AUTH、
Submission 公布 AUTH、外部到外部未认证 RCPT 被拒绝，以及普通账号 JMAP Mail
`Mailbox/get`。`roundtrip` 再以 SMTP AUTH 发送唯一 Message-ID，并通过另一个账号
的只读 IMAP 精确读回；JMAP 响应最多 1 MiB，IMAP 邮件最多 2 MiB。

### 隐式 TLS 与离线验证

验证程序兼容原 STARTTLS 默认值，也可分别设置
`COPILOTMAIL_STALWART_SMOKE_SUBMISSION_TLS_MODE=implicit_tls` 和
`COPILOTMAIL_STALWART_SMOKE_IMAP_TLS_MODE=implicit_tls`；未指定相应端口变量时选择开发覆盖层的
2465/2993。显式端口变量保持优先，不能仅改端口而让协议模式自动猜测。

连接逻辑独立于流程编排，隐式 TLS 使用 `SMTP_SSL` / `IMAP4_SSL`；TLS 失败不会回退到明文，
API Key 不会当作用户密码。JMAP 重定向在请求跟随后续地址之前被拒绝，命令输出不回显协议服务端
错误原文或密码。离线回归不需要真实账号，也不连接任何服务器：

```powershell
py -3.13 -m unittest discover -s tests -p test_mail_client_transport.py -v
```

普通客户端直连邮件引擎不等于经过控制面的 Agent 准入/审计；当前 Web Sent 目录也不保证列出
外部客户端直接提交的邮件。这些限制详见 [外部客户端治理边界](EXTERNAL_MAIL_CLIENTS.zh-CN.md)。

### 重启持久性边界

单次 roundtrip 不证明持久性。使用两阶段命令，在两步之间显式重启 Stalwart：

```powershell
python tests/stalwart_poc.py send --receipt .local/stalwart-poc-restart.json

docker compose --env-file deploy/.env.v2 `
  -f deploy/compose.dev.yaml `
  -f deploy/compose.mail-poc.yaml restart stalwart

python tests/stalwart_poc.py verify --receipt .local/stalwart-poc-restart.json
```

Receipt 不含密码，只包含 Schema 版本、Message-ID、随机 marker、测试地址和 UTC
发送时间，并已由 `.gitignore` 排除；`verify` 会把它与当前配置精确绑定。测试报告
必须同时保存 `send`、重启命令及 `verify` 的终端证据，因为程序不会也不能猜测操作者
是否真的执行了重启。完成后从当前 Shell 移除管理员及测试账号密码。

## 尚未通过的 P0/P3 验收

本覆盖层不能作为“已可生产收发”的证据。以下仍需逐项完成并留存测试报告：

- 使用独立数据库角色验证 Stalwart PostgreSQL 主存储；
- 验证 S3 兼容 blob 存储、重启恢复、备份、恢复和升级回滚；
- 在真实容器执行并留存上述普通账号 SMTP Submission、IMAP、JMAP Mail、非开放
  中继及重启持久性证据；自动化程序和离线门禁已具备，本开发机尚未执行；
- 入站 250 后断电不丢信、出站队列持久重试、逐收件人状态与 DSN；
- 在真实 Stalwart 容器中执行入站及队列 WebHook 签名、断线重试、临时失败、
  到期退信、并发重放、Cursor 对账与 Secret 协调轮换；代码边界和测试已完成，
  但本机尚未执行该外部集成；
- 反垃圾、反病毒、SPF/DKIM/DMARC、rDNS 与 TLS 结果；
- 公网测试子域、真实 MX、外部种子邮箱和回滚演练；
- AGPL/商业授权、支持等级和 SLA 的法务及采购签字。

## 上游依据

- [Stalwart Docker 安装与固定 minor 镜像](https://stalw.art/docs/install/platform/docker/)
- [Stalwart 环境变量与 Public URL](https://stalw.art/docs/configuration/environment-variables/)
- [JMAP discovery 与 API endpoint](https://stalw.art/docs/http/jmap/)
- [API Key 的用途和邮件协议隔离](https://stalw.art/docs/auth/authentication/api-key/)
- [Stalwart v0.16 JMAP Domain 对象与管理方法](https://stalw.art/docs/ref/object/domain/)
- [v0.16 升级说明：REST 管理接口替换为 JMAP](https://github.com/stalwartlabs/stalwart/blob/main/UPGRADING/v0_16.md)
- [Stalwart CLI 安装与认证](https://www.stalw.art/docs/management/cli/)
- [Stalwart CLI 声明式 apply](https://www.stalw.art/docs/management/cli/apply/)
- [PostgreSQL 存储后端](https://stalw.art/docs/storage/backends/postgresql/)
- [S3 blob 存储后端](https://stalw.art/docs/storage/backends/s3/)
