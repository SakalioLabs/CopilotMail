# 企业管理端

CopilotMail 企业管理端位于 `web/admin-app`，当前提供审批中心、角色授权治理、
不可变审计检索、合规案件、审计归档 Legal Hold、受控证据导出、邮件域名治理与单 Agent 控制面检视
工作流。它使用
React、TypeScript 和 Vite，并从 `web/design-system` 复用基础组件和视觉变量。

## 已有能力

- Rust BFF 执行 OIDC Authorization Code + S256 PKCE，浏览器 JavaScript
  不接收 Provider Access Token、Refresh Token 或 PKCE verifier。
- 浏览器只持有 Host-only、HttpOnly、Secure、SameSite=Strict 的 opaque
  Session Cookie；Session DTO 必须携带可解析的 `expires_at`，已过期即按登出处理。客户端在
  到期时、窗口重新聚焦时和受保护请求返回 `401` 时重新校验，合并并发校验；刷新期间锁定
  当前界面。身份或失效时间上下文变化会重建整个已认证工作区并清除上一操作者的内存证据，
  注销会追加不可变撤销证据。
- 审批中心分为“待我审批”和“我的申请”：前者只显示当前 Active Human 可决策的
  请求，后者让 Human 与 Agent 查看自己的申请；支持状态筛选、绑定查询条件的
  有界翻页、固定策略/资源详情和不可变决策历史。
- 审批详情采用“决策影响 → 资源与决策边界 → 不可变证据”的顺序。Context Bar 只显示
  当前工作视图、服务端筛选，以及已精确读取详情中的资源类型/ID、申请主体与状态；
  “发起审批申请”是其中唯一主动作，刷新保持次级。影响摘要只读取动作、当前审批进度和
  申请理由；边界区只读取资源、申请主体、工单和失效时间，并按当前服务端工作视图给出
  通过/拒绝或撤回这一种允许的动作。请求 ID、策略版本、时间和已记录步骤位于默认收起的
  Evidence Inspector。界面不虚构 SLA、优先级、审批人、执行结果或深链。
- 审批人可在详情内以最少交互批准或拒绝并填写理由；原请求人可确认后取消仍有效的
  Pending 请求。服务端禁止自审、重复决策、他人取消和过期请求状态变更。
- 从服务端读取九个不可变内置角色和租户授权分配，支持按 Principal 精确筛选历史；
  界面直接生成由目标 Principal、角色和失效时间绑定的审批资源 ID，并在审批完成后
  授予最长 365 天的限时角色或撤销现有授权。最后一个有效 Owner 与机器主体的角色
  边界由服务端强制保护。
- 角色分配与审计 Actor 复用共享 `IdentityBadge`，由原创 SVG、身份形状和当前语言下
  的可见文字共同表达。持久化 `service` Actor 使用独立的 `service` 身份种类；它可采用
  受边界系统核心图形作为视觉语法，但文本始终保持 `Service`/`服务`，不改称
  `System`/`系统`，也不因此声明新的 Service wire 身份。
- 合规案件按状态筛选、有界翻页、建立、查看和结案；刷新后若选中案件仍在新列表中，
  详情重新绑定最新快照，否则清除已失效的选择与详情。
- Legal Hold 按状态或案件筛选、有界翻页、查看、施加和解除；刷新同样重新绑定仍可见的
  选中 Hold，或在目标不再出现时清除详情。
- 已选中的 Cases 与 Legal Holds 详情统一按“Context Bar → 资源事实 → 当前决策 → 默认收起
  Evidence Inspector”阅读：Context Bar 只使用当前 DTO 的资源 ID、外部编号/归档/案件、建立
  时间和状态；案件提供其确有契约支持的“查看该案件的 Legal Hold”筛选深链，Legal Hold 仅以
  DTO 中的 `case_id` 打开“查看关联案件”，并且 Cases 工作区必须先执行该 ID 的精确详情 GET，
  绝不以列表结果推断关联案件；不伪造 Case→审批、Hold→审批或审计关联。证据区只保留精确
  ID、建立/解除主体与时间、施加/解除审批等已返回字段。结案和解除确认会在提交前列出实际
  对象、影响以及已有的施加审批事实；解除仍要求操作者输入与该 Hold 精确绑定的双人审批请求 ID。
- 邮件域名工作台以同租户列表—详情工作流呈现域名注册、DNS 证据、显式启用、暂停、
  路由版本与数据平面投影事实；它不把 DNS 已验证、控制平面 Active 或邮件引擎投影 Ready
  混写为同一个状态。
- Cases、Legal Holds、Approvals 与 Audit 在不超过 1080px 时使用真实列表—详情栈：
  未选择时只显示列表，选择后只显示详情并聚焦稳定标题；本地化 Back 与 Escape 恢复
  精确行按钮，行已消失时回退到列表区域。桌面继续双栏，1081px 边界不得产生页面级
  横向溢出。
- 审计调查按“查询上下文 → 结果队列 → 证据详情”组织。查询 Context Bar 只显示本次
  服务端确认的 Query ID、时间窗口、浏览器实际提交的可选筛选条件与当前页返回边界；它不
  推测总命中数、聚合指标或服务器未返回的 facet。检索仍限制在最长 31 天的精确窗口，支持
  Action、Actor、资源、结果和 Request ID；续页锁定服务端返回的窗口和全部条件。
- 结果队列只保留服务端返回的事件、目标、主体、结果和发生时间。选择某一行后，证据详情先
  呈现该事件的完整 ID、主体、动作、资源、结果和时间，再默认收起哈希链、最小治理上下文和
  脱敏详情；没有事实关联时不生成“相关资源”或深链。PostgreSQL BIGINT 链序号继续以十进制
  字符串传输，避免浏览器精度损失。
- 导出授权与调查结果是独立、默认收起的高风险安全门，而非普通下载按钮。只有输入与精确
  归档绑定的已完成双人审批后才能创建最长 15 分钟的 Grant；随后事件或 Manifest 下载会按
  Version ID、长度和 SHA-256 重新校验，并在返回字节前写入不可变审计账本。授权或下载结果
  未知时，界面冻结相应写入，分别只允许按精确归档/审批关系或 Grant ID 读取对账，绝不重放
  POST 或下载请求。
- 创建和读取审计/邮件保留策略、追加不可变草稿版本，并通过与待激活版本精确绑定的双人审批
  激活最新版本；界面明确区分“最新草稿”和“当前生效版本”。创建表单只在操作者发起时打开
  共享 `Dialog`，可空字段由明确控件和持久说明表达，不使用示例 UUID 或 placeholder 暗示语义。
- 审批决策/取消、角色授予/撤销、保留策略追加/激活、案件结案与 Hold 解除都以写响应作为
  已提交事实。迟到、复制延迟或失败的后续读取只能标记快照陈旧，不能让计数/终态回退、
  把已完成动作重新开放，或把“已创建版本”误称为“最新版本”。Cases/Holds 的终态账本与
  已提交证据在同一 Session 内跨页面保留，并在身份上下文变化时清除；列表、详情和命令响应
  严格绑定租户、目标 ID、筛选条件与分页。网络中断、`5xx` 或无效 `2xx` 造成的未知写入先
  冻结精确请求；仅在已有目标 ID 时，才允许以精确详情 GET 对账。创建响应未返回资源 ID（案件、
  Hold、审批或域名注册）时，不以宽泛列表猜测候选对象，也不展示伪对账动作，命令会保持冻结。
  只有后端已经提供主体、操作、请求摘要与响应精确绑定的幂等账本时，才允许使用同一键重放原请求，
  不能编辑后当作同一次提交。审批决策和取消可读取其原始请求 ID；审批创建没有该 ID 时同样只能
  保持冻结，绝不再次 POST。完整恢复仍需后端强制 `Idempotency-Key`、保存规范请求
  摘要与原始响应，并保证审批写入、Audit、Outbox 和幂等账本同事务提交。
- Agent 检视只接受管理员显式输入的单个 Agent UUID；进入页面不预取、不轮询、不建立
  目录。读取使用同源 Cookie、`cache: no-store`，结果只留在当前内存视图；它只显示后端
  已授权的身份、Owner、Profile、Token/RBAC 交集和邮件控制面记录，不显示凭据材料、
  邮箱/委托、邮件、心跳、运行、投递或执行状态。
- 成功快照首先进入共享 Context Bar：标题只使用 Agent 名称与其生命周期，事实区只使用
  Agent ID、Human Owner、环境和服务端观测时间；显式刷新是唯一操作。下方只保留边界与
  复核、Token 交集和邮件控制面三组增量事实，不重复拼接身份卡或假造证据区。刷新失败时
  保留的内容明确标注为“上一次已确认的观测”，绝不称作实时状态。
- Agent UUID 必须是 canonical 且不能为保留的全零值；畸形或保留值在浏览器本地拒绝，
  不发 Inspector 请求。浏览器只显示稳定、面向操作者的错误文案和允许披露的 Request ID，
  不渲染响应中的内部错误码、内部主机、Trace、调试字段或敏感材料。
- Inspector 成功响应也在浏览器按精确 DTO 解析：未知键、额外凭据/运行字段、非法 UUID/
  时间/枚举、越界计数，以及与观测时间、复核、Token 交集或邮件准入矛盾的组合都会在
  渲染前失败关闭。
- Agent Inspector 的 `404` 故意合并缺失、跨租户、非 Agent 与 Profile 不完整；界面只给
  出统一文案和 Request ID，`403` 也不会暗示目标存在。只有 API 明确标记为可重试的故障
  才显示重试；刷新是显式且会审计的读取操作，会保留上一次确认的快照与键盘焦点，并以
  简短的屏幕阅读器状态通知完成。身份和状态分别复用 `IdentityBadge` 与仅接收 API
  事实的 `StatusIndicator`，以图形轮廓/形状和文字共同表达，不以颜色单独传达含义；
  `scheduled` 只表示复核未到期。
- 施加和解除操作要求输入与服务端资源精确绑定的双人审批请求 ID。
- 共享错误提示只呈现本地化安全文案及契约明确允许披露的稳定错误码/Request ID，不渲染
  服务端 `message`、`trace_id` 或 `details`；Inspector 错误进一步只保留操作者文案与
  Request ID。
- 简体中文和英文切换、键盘焦点约束、减少动画支持、移动端布局。Locale 只接受
  `zh-CN`/`en`，以 `copilotmail.admin.locale.v1` 严格持久化；切换会同步更新
  `<html lang>`，整页刷新后仍恢复同一语言。
- 所有确认流程使用共享 `Dialog`/`ConfirmDialog`：Portal、`aria-modal`、背景 `inert`、
  Tab 环绕、Escape/遮罩关闭、busy 期间禁止重复提交或退出，以及触发器/安全回退焦点恢复
  均由同一实现负责。遮罩关闭还要求 pointer down、pointer up 和 click 完整发生在遮罩上；
  从内容区拖向遮罩不会误关。生产代码禁止原生 `alert`、`confirm` 与 `prompt`。
- Cases、Holds、Approvals、Audit 与 Retention 等重型路由按需加载，并位于可重建的安全恢复
  边界内；Chunk 加载或渲染失败只呈现本地化恢复动作，可重试或整页重载，不泄露原始异常。

管理端不会自行判断用户权限。每次读取和变更仍由控制平面实时验证 Session、
RBAC、Active Principal/Human、租户边界和审批证据。“待我审批”与决策要求
Active Human 和 `authorization.manage`；“我的申请”精确绑定当前请求人。特权读取由
服务端先提交 Audit + Outbox，证据失败时不会返回结果。

## 邮件域名工作台

该工作台是 Human 管理员的低交互操作面；Agent 作为一等主体时使用同一受控域名 API/SDK，
而不是模拟浏览器点击。无论入口来自 Human 还是 Agent，控制平面都实时校验租户、主体、
`mail.admin`、域名生命周期和请求中的版本证据。前端不以本地状态替代这些判断。

域名操作按以下事实顺序组织：

管理端详情遵循“事实 → 当前决策 → 证据与辅助配置”的阅读顺序，而不是把所有表单并列成
一张配置表。顶部 Context Bar 只读取当前详情 DTO 中的规范化域名、路由模式/版本、最近 DNS
观测及控制平面 `status`；随后并列显示四个**相互独立**的事实段：已登记（当前资源与
challenge 版本）、DNS 证据（`latest_dns_*`）、控制平面显式启用（`status`）和邮件引擎
投影（`mail_engine_projection_*`）。这四段不是推测出的进度条：`verified`、`active` 与
`ready` 始终保持不同文本与证据来源。

当前详情只将一个与持久化生命周期相符的操作作为主要决策：待证据（包括 Active 但当前 DNS
证据不再可用于启用）时验证 DNS，已验证且最新证据可启用时显式 Activate，Active 且当前
DNS 证据仍可用时编辑携带 CAS 的路由。默认收起的 Evidence Inspector 只呈现 ownership、
DNS、路由与投影的核对事实；轮换 token 是令牌丢失时的受控辅助恢复，关闭 Inspector 不会
丢失一次性 token 规则，也不会隐藏不存在的后端动作。

1. 管理员注册规范化域名并选择 `direct_mx` 或 `smart_host` 路由模式。服务端成功提交后才返回
   ownership token；该 token 仅在**当前**一次性对话框中显示，供操作者写入 DNS。关闭对话框、
   切换目标或重建认证工作区都会从内存清除它；不得写入 URL、浏览器历史、`localStorage`、
   Toast、静态卡片或审计展示。
2. 操作者在 DNS 生效后提交 ownership token 与 DKIM selector 进行验证。详情仅呈现服务端返回的
   challenge/route 版本、观测时间、失效时间、失败摘要和激活就绪事实；原始 token 不会被读取或
   再次显示。challenge 轮换同样只在本次成功响应中展示新 token，并使旧 DNS 证据失效。
3. 只有服务端确认最新 challenge 的 DNS 证据合格，域名才会进入 `verified`。`verified` 不是
   自动启用：管理员必须在明确确认后执行独立的 Activate，才会进入控制平面 `active`。暂停是另一
   个明确操作，并以原因码留下受控记录；恢复或后续启用仍受服务端最新 DNS 证据约束。
4. 入站/出站开关、DKIM key reference 与 Smart Host reference 在独立路由面板中编辑。写入必须
   携带详情中的 `route_version` 作为 CAS `expected_version`；冲突后重新读取详情并由操作者核对，
   不会静默覆盖或盲目重放旧配置。
5. Active 是控制平面生命周期，不等于邮件引擎已经完成配置。工作台分别显示
   `not_projected`、`pending`、`ready`、`failed`、引擎实际启停值、最近尝试/成功时间和可安全披露的
   错误事实。投影由 Outbox Worker 异步处理，失败不会被界面伪装成已接收邮件或已可投递。

网络中断、`5xx` 或无法验证的 `2xx` 会使写入结果未知。此时工作台冻结该域名的注册、轮换、
DNS 验证、激活、暂停与路由写操作。轮换、验证、激活、暂停和路由已有精确 domain ID 时，只允许
该 ID 的只读详情 GET 对账；注册没有返回 ID 时保持冻结，不刷新列表猜测结果。在服务端状态证实前，
界面不会自动再次提交，也不会用修改后的内容冒充原请求。该 API 当前不以浏览器端 token 代替
服务端幂等账本，因此“刷新并核对”是未知结果的唯一恢复路径。

## 本地开发

```bash
cd web
npm ci
npm run check
npm test
npm run e2e:install
npm run e2e
npm run dev --workspace @copilotmail/admin-app
```

Vite 开发服务器默认监听 `http://127.0.0.1:4173`，并把 `/api` 转发到
`http://127.0.0.1:8081`。编辑
`web/admin-app/public/runtime-config.js` 只适合本地开发，不能提交真实租户或
凭据。

## Docker 运行时配置

开发 Compose 在 `http://127.0.0.1:8083` 暴露管理端。配置以下变量后重建或
重启 `admin` 服务：

| 变量 | 用途 |
|---|---|
| `COPILOTMAIL_ADMIN_ORGANIZATION_ID` | 管理端绑定的唯一企业租户 UUID |
| `COPILOTMAIL_ADMIN_OIDC_CLIENT_ID` | BFF 使用的 IdP 公共客户端 ID |
| `COPILOTMAIL_ADMIN_PUBLIC_ORIGIN` | 管理端精确 HTTPS Origin |
| `COPILOTMAIL_OIDC_ISSUER` | 与控制平面相同的 OIDC Issuer |
| `COPILOTMAIL_OIDC_AUDIENCE` | 与控制平面验证值相同的 Access Token Audience |
| `COPILOTMAIL_ADMIN_OIDC_SCOPE` | 默认 `openid profile email` |
| `COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE` | API 容器中只读的 32 字节密钥路径 |
| `COPILOTMAIL_REQUIRE_HUMAN_SESSION` | 生产必须为 `true` |
| `COPILOTMAIL_ADMIN_HTTP_PORT` | 主机端口，默认 `8083` |

IdP 应启用 Authorization Code + S256 PKCE，不得配置 Client Secret，并须把
精确回调地址（开发环境通常为
`http://127.0.0.1:8083/api/v2/auth/admin/callback`）加入允许列表。生产环境的
页面、API 与 IdP 必须使用 HTTPS。完整 Cookie、CSRF、反向代理日志和验收要求见
[Human OIDC BFF 会话](HUMAN_OIDC_SESSION.zh-CN.md)。

```bash
docker compose --env-file deploy/.env.v2 -f deploy/compose.dev.yaml up \
  --build admin
```

容器通过同源 Nginx 代理访问 `api:8081`，运行时配置写入只读根文件系统之外的
临时目录。容器删除全部 Linux capabilities、启用 `no-new-privileges`，并设置
CSP、禁止嵌入、禁止 MIME 嗅探和 `no-store` 运行时配置缓存。Docker builder 在复制产物前
执行 Admin 单应用 Bundle 门禁：限制首屏/总 JavaScript、CSS 与 gzip 体积，拒绝 `.map`
和 JS/CSS `sourceMappingURL`，并执行已登记 Image 2 交付位图预算。Nginx 只给成功的
`/assets/` 响应一年 immutable 缓存；资产缺失精确返回 `404` 且 `no-store`，不得回落到
SPA 页面。上述内容是仓库配置契约，不表示镜像已经在 Docker 或 NixOS 生产环境实际部署。

## 验收

```bash
cd web
npm ci
npm audit --audit-level=moderate
npm run check
npm test
npm run e2e:install
npm run e2e

docker build -f deploy/docker/Dockerfile.admin-app \
  -t copilotmail-admin:local .
```

当前 Web Vitest 共 390 条（共享设计系统 16、Admin 195、Mail 179），其中 Admin 覆盖运行时
配置安全限制、BFF Session/Login/Logout 调用与到期/聚焦/`401` 重校验、查询参数编码、审计
窗口边界、审批创建/筛选/决策/取消、角色与保留策略写后单调收敛、Cases/Holds 危险操作、
共享 Dialog 手势、按需路由恢复、Admin 列表—详情、导出下载、租户路径绑定、Cookie 凭证
隔离、严格 Locale 与安全错误边界。域名工作台覆盖一次性 token 的内存边界、DNS/显式激活、
路由 CAS、异步投影事实和未知写入冻结/只读对账；写成功后读取失败、迟到响应、筛选切换和复制
延迟均有精确竞态回归。
当前 82 条 Playwright（Admin 53、Mail 29）使用本地 BFF 夹具；Admin 切片实际运行 Agent
Inspector 及治理列表—详情工作流：
canonical UUID 查询、畸形/全零 UUID 本地零请求拒绝、`403`、统一 `404`、
`503 retryable=false/true`、键盘刷新，以及首次成功后刷新失败仍保留旧快照与刷新按钮焦点；
故意带入的内部主机、Trace、错误码和敏感响应均不得显示。真实 Language 选择器进入
`zh-CN` 后会核对 localStorage、`<html lang>` 与 reload 持久化。多个代表性 Admin 页面与
关键状态执行严格的 Axe A/AA 自动扫描。当前 66 张 Windows PNG 中 Admin 占 34 张、Mail
占 32 张；Admin 包含英文/简体中文各 1440、1280、1024、768、390px 工作区以及治理详情
状态。语义断言同时覆盖 1440 双栏、1081 无溢出、Back/Escape 焦点往返、Approval 迟到
详情不能重开选择，以及 Session/终态证据收敛。截图前将 UUID 输入框失焦并把
selection/scroll 归零，避免基线只显示值尾部。
浏览器禁用 Service Worker 且只允许当前回环 Origin，未声明 API、响应矩阵之外的额外读取
或外部请求都会使测试失败，夹具不会连接真实控制平面、IdP 或 CDN。

上述浏览器证据只证明同源固定夹具，不证明真实 RBAC、租户隔离、后端审计/CAS、完整
WCAG 2.2 AA 或企业环境验收。其余 Admin 信息架构仍需按同一契约扩展；真实 IdP 登录、真实
触屏、屏幕阅读器、双人审批和 S3 Object Lock 状态变更须在真实依赖环境执行端到端验收。
