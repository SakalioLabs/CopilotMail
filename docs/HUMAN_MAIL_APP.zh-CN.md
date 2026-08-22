# Human 邮件端安全边界

> 状态：授权邮箱目录、Shared Mailbox 切换、收件箱、纯文本安全详情、显式回复、
> 自动保存草稿、本人私有草稿列表、完整性校验后的恢复编辑、已发送／投递状态目录与可恢复正式发送纵向切片已实现；
> 简体中文/英语、响应式移动端列表—详情与可访问写信对话框已完成组件级实现和 jsdom 回归。

浏览器登录统一经过 Rust OIDC BFF：Mail App 只调用
`GET /api/v2/auth/mail/session`、`GET /api/v2/auth/mail/login` 与
`POST /api/v2/auth/mail/logout`，Provider Access Token、
Refresh Token、PKCE verifier 与 `Authorization` Header 均不进入前端。生产使用
Host-only HttpOnly Secure SameSite=Strict Session Cookie；Mail Session 只能调用
本页列出的 Human 邮件路由，不能复用到管理或合规 API。完整配置与威胁边界见
[Human OIDC BFF 会话](HUMAN_OIDC_SESSION.zh-CN.md)。

## 当前能力

`GET /api/v2/mailboxes` 为 Human Mail App 提供最小化的授权邮箱目录：

- 仅接受仍有效的 Human Mail Session；Admin Session Cookie、Bearer Credential 和缺少
  Mail Session 的请求都会在查询邮箱存储前被拒绝。进入用例后仍须同时通过 Active
  Human 与实时 RBAC `mailbox.use`；
- 目录只包含当前租户内已有 current data-plane binding 且调用人具备资源级
  `Read(None)` 能力的邮箱。邮箱解除当前绑定、Read 授权过期/撤销或主体失活后，不再可见；
- 响应项只有 `mailbox_id`、`kind`、`primary_address` 与 `can_send`，不返回租户、
  Provider、Account、binding、成员角色、授权人或委托明细；
- `can_send=true` 精确表示调用人具备该邮箱的 `Send(None)` 能力。只有 `SendAs`
  委托而没有 `Send(None)` 的邮箱仍为只读，前端禁用写信、回复、草稿保存与正式发送；
- 结果按邮箱 UUID 稳定排序，单页默认 25、最大 50 条；续页只接受 canonical
  `v1.<lowercase UUID>` Cursor。Cursor 锚点必须仍属于同一租户、保持当前绑定并对同一
  Human 可读，否则整个续页请求返回 400；
- 部署配置中的 `mailboxId` 只是初始 UI 偏好，不是授权输入，也不会绕过服务端目录；
  未配置、不可见或不存在时使用授权结果中的 Human 邮箱或首个邮箱作为安全回退。

桌面与移动端均可在已加载目录中切换 Human、Shared 或其他可读邮箱。切换时客户端会
同步推进收件箱请求代次、作废并中止详情请求，清空搜索条件、列表、当前详情、已选邮件与
写信/回复草稿状态；旧邮箱的迟到响应、自动保存或提交轮询不能污染新邮箱。可发送邮箱的
写信窗口明确显示目录投影的 `primary_address` 作为只读 From，API 请求仍不接受调用人
选择或覆盖 `from`；只读邮箱持续显示只读状态并禁用所有邮件创作与发送入口。

`GET /api/v2/mailboxes/{mailbox_id}/messages` 返回最新优先的有界邮件信封页：

- 仅接受由企业 OIDC 建立且仍有效的 Human Mail Session；
- 同时校验 Active Human、实时 RBAC 的 `mailbox.use` 与邮箱资源级 Read 权限；
- 只使用数据库中与当前租户、邮箱精确绑定的 Stalwart JMAP Account ID；
- Account 第一次绑定后永久归属同一租户邮箱；升级会在阻断旧写入的同一事务中从当前
  绑定和不可变审计历史恢复已退休 Account，歧义历史整体回滚。因而一次已授权 JMAP
  请求期间发生绑定轮换，也不能把旧 Account 重新分配成其他租户资源，详见
  [ADR 0036](adr/0036-immutable-mail-engine-account-ownership.md)；
- 每次先解析恰好一个 `role=inbox` 且具备 `mayReadItems` 的 JMAP Mailbox，再以
  `inMailbox` 过滤列表；缺失、重复或不可读时不会继续查询；
- 单页默认 30、最大 50 条，Cursor 与邮箱绑定；
- 每封邮件只返回 CopilotMail `message_id` UUID、主题、发件人、时间、预览、
  已读/星标及附件存在标记；
- 每个结果必须精确匹配当前绑定下已提交的入站事件 UUID；任一项目缺失或重复时整页
  Fail Closed，不临时补造身份；
- 不返回 JMAP Email/Thread ID、Query State、正文、原始 Header、Blob ID、附件内容
  或引擎内部绑定；列表也必须先提交不含信封内容的 `mail.messages.queried` Audit 与
  Outbox 才能披露。

`POST /api/v2/mailboxes/{mailbox_id}/messages/search` 提供隐私优先的 Human 邮箱搜索：

- 查询词仅放在 JSON 请求体中，不进入 URL、访问日志或 Cursor；
- 查询词去除歧义后限制为 2–128 个 Unicode 字符、最多 512 字节，并拒绝控制字符；
- 服务端在通过 Human Session、`mailbox.use`、Read ABAC 与精确租户 JMAP Account
  绑定后，才发起有界 JMAP `Email/query` 文本过滤；
- 搜索结果仍只读取并返回有界信封字段，不读取正文、HTML、Blob 或附件内容；
- Cursor 只携带查询词的单向摘要并同时绑定邮箱和有界位置，不携带 JMAP Query
  State，不能跨查询或跨邮箱复用；
- 搜索成功必须先原子提交 `mail.messages.queried` 审计与
  `mail.messages.queried.v1` Outbox 才能返回；证据仅记录页位置、结果数和是否还有下一页，
  不持久化查询原文、摘要、主题、地址或预览。证据提交失败时 Fail Closed。

`GET /api/v2/mailboxes/{mailbox_id}/messages/{message_id}` 在相同授权边界下
返回消息详情：

- 只请求 JMAP `textBody`，不请求或渲染 HTML；
- 单个 Body Value 最多 128 KiB，组合正文最多 256 KiB；
- 编码异常、超限或响应引用不一致会 Fail Closed；
- 附件仅返回名称、媒体类型和大小，不返回 Blob ID 或下载地址；
- 保留纯文本换行，前端不使用 `innerHTML`。
- 路径与响应只往返同一个 CopilotMail `message_id` UUID；非 UUID 输入、非 UUID
  响应或响应身份与请求不一致时，浏览器在使用数据前 Fail Closed。
- 服务端先用该 UUID 反解当前租户、邮箱和 Account 的精确 JMAP Email，再核对响应
  身份；无法精确反解时不访问数据平面。
- 正文只有在 `mail.message.read` Audit 与最小化 Outbox 原子提交成功后才返回；
  证据仅包含安全 UUID、正文长度/截断状态、附件数量/存在标记和“不可信外部内容”
  标记，不含正文、主题、地址、预览、附件名或 Provider 引用。
- 已通过上述投影校验的详情返回同一个安全 UUID 作为 `reply_to_message_id`，回复链路
  不接受原始 Header 或 JMAP ID。
- 所有 Human 邮件路由（包括错误响应）统一返回 `Cache-Control: no-store` 与
  `Referrer-Policy: no-referrer`；前端请求也禁用缓存。
- 用户快速从邮件 A 切换到 B 时，前端同步清空旧详情、中止 A 的请求并以选择代次
  丢弃迟到响应；已读/星标的本地摘要更新不会再次 GET 正文或重复产生读取审计。

`PATCH /api/v2/mailboxes/{mailbox_id}/messages/{message_id}` 使用 JMAP
`Email/set` PatchObject 更新 `$seen` / `$flagged`，随后读取权威关键词状态返回。
打开未读邮件只需一次点击并自动标记已读，详情标题旁可一次点击切换星标。

Human 状态与 Agent 状态严格分离：

- Human 已读/星标属于邮件数据平面的展示与整理状态；
- Agent Claim、Lease、Ack/Nack、重试和死信属于 CopilotMail 处理状态机；
- `$seen` 不会确认 Agent 投递，Agent Ack 也不会替 Human 标记已读；
- 状态修改接口不接受 Agent OAuth/mTLS 身份。

不存在、跨租户、无资源权限和未绑定邮箱统一返回 404。内容凭据或 JMAP
不可用统一返回可重试的 503，不回退到管理凭据或其他 Account。

`POST /api/v2/mailboxes/{mailbox_id}/drafts` 保存有界纯文本草稿：

- Human 界面仅使用 Human Mail Session；底层契约也允许具备 `agent.operate`、
  `mailbox.use` 及邮箱资源级 Send 能力的 Active Agent 使用，但 Agent 不共享
  Human UI 的自动保存状态；
- 请求不接受 `from`，发件地址只能取自授权邮箱的控制平面投影；
- 收件人总数为 1–100，地址、主题和正文均在网络访问前验证并设置上限；
- JMAP 邮件正文不可变，因此编辑会创建新草稿并销毁数据库解析出的上一远端版本；
- 浏览器只持有 CopilotMail Draft UUID 与单调版本，不接收也不能提交 JMAP Email ID
  或 Thread ID；更新使用版本 CAS，跨标签页的过期写入返回 409；
- 销毁前再次 `Email/get`，并要求该 ID 精确属于当前 Drafts Mailbox、带 `$draft`
  关键词且 From 等于授权邮箱；任一条件不符都不会执行 `Email/set`；
- 前端以 1.2 秒防抖、严格串行的方式自动保存，每次只用上一成功的 Draft UUID 与
  版本继续替换；
- JMAP 部分成功时返回新草稿，并通过 `previous_draft_destroyed=false` 明确提示旧版本
  未清理，避免把已成功创建误判为失败后盲目重试。

`GET /api/v2/mailboxes/{mailbox_id}/drafts` 与
`POST /api/v2/mailboxes/{mailbox_id}/drafts/{draft_id}/restore` 提供本人私有的草稿目录与
精确恢复编辑：

- 两个入口只接受 Human Mail Session，并同时要求 Active Human、`mailbox.use`、邮箱
  `Read(None)` 与 `Send(None)`；`SendAs`-only 邮箱不能列出或恢复草稿；
- Shared Mailbox 的成员或受托人只能看到自己创建且仍为 `active` 的草稿，邮箱成员关系
  不会扩大到其他 Principal 的草稿，也不实现共享协作编辑；
- 草稿目录只从 PostgreSQL 权威投影分页，不执行 JMAP Drafts 全量查询。每项只返回
  CopilotMail Draft UUID、版本、可空的安全回复 UUID、收件人数与时间，不返回主题、
  地址、正文或 Provider Account/Email/Thread 引用；
- 目录元数据只有在新的事务按数据库当前时间复核主体、RBAC、Read/Send、当前 binding，
  并确认当页每个 Draft UUID 仍属调用人、仍为 active 且版本精确一致后，才与
  `mail.drafts.queried` Audit/Outbox 原子提交并披露；
- 目录按 Draft UUID 倒序稳定分页，单页默认 25、最大 50；只接受 canonical
  `v1.<lowercase UUID>` Cursor，锚点失权、冻结、换绑或不再属于调用人时整页拒绝；
- 恢复先从控制平面解析调用人当前绑定下的精确 active Draft，再在数据库事务外执行
  单 ID `Email/get`。适配器要求唯一可读 Drafts Mailbox、`$draft`、精确 Account、
  Email、Thread 与 From；只接受单一有界 `text/plain` 正文，拒绝 HTML、附件、截断、
  异常编码或正文长度不一致；
- 远端 To/CC/BCC、主题与正文会使用和保存草稿相同的规范化算法重新计算正文、内容和
  收件人证据；任一证据与 PostgreSQL 冻结投影不一致均返回 409，不把远端内容交给
  浏览器；
- 网络读取后会在新的 PostgreSQL 事务中再次证明主体状态、RBAC、Read/Send、当前
  data-plane binding、本人所有权、active 状态、版本、Account/Email/Thread、From 与
  全部摘要；`mail.draft.read` Audit 和最小 Outbox 原子提交成功后才返回正文；
- `mail.drafts.queried` 只记录邮箱 resource、页大小、结果数和 `has_more`；
  `mail.draft.read` 只记录邮箱/草稿安全 UUID、版本、正文 UTF-8 字节数、收件人数和
  是否为回复。两者均不记录正文、主题、地址、Cursor 或 Provider 引用；
- 草稿恢复不会枚举、采用或删除 PostgreSQL 未知的远端草稿。孤儿检测与受治理清理仍是
  后续独立能力。

显式回复在上述草稿契约中使用必填可空的 `reply_to_message_id`。非空时还必须通过
邮箱 Read ABAC；服务端按安全 UUID 解析精确入站 Email，并从其唯一 `messageId`
与有界 `references` 生成 JMAP `inReplyTo/references`。调用者不能提交原始 Header、
JMAP Email ID 或 Thread ID。回复 UUID 通过复合外键绑定同租户/同邮箱，纳入草稿
摘要，并在替换、冻结、发送生命周期中不可换绑。完整边界见
[显式邮件回复语义](EXPLICIT_REPLY_SEMANTICS.zh-CN.md)。

`GET /api/v2/mailboxes/{mailbox_id}/submissions` 提供本人已接受发送的最小投递状态目录：

- 只接受 Human Mail Session，并在查询前后都复核 Active Human、`mailbox.use`、
  `Read(None)` + `Send(None)`；Admin Cookie、Bearer、仅 `SendAs` 的委托与失效主体均
  不能进入目录；
- Shared Mailbox 的成员或受托人只会看到 `requested_by` 等于自己的记录，绝不会因拥有
  邮箱读写能力而看到其他 Human 或 Agent 的发送历史；
- 条目必须同时证明 Submission 已为 `accepted`、有 `accepted_at`，且其冻结 Draft 已
  单调进入 `sent`（版本为 Submission 冻结版本加一）。因此 pending、reconciling、
  rejected 与 manual_review 请求不被称作“已发送”；`accepted` 也仍不等于所有收件方
  最终投递成功；
- 目录只从 PostgreSQL Durable Intent 与最新不可变投递快照读取 `submission_id`、
  接受时间、收件人数及可选聚合计数。绝不读取 JMAP Sent、不执行远端调用，也不返回
  主题、正文、地址、收件人摘要、SMTP reply、Draft ID/版本、Provider 或 data-plane
  引用、错误码；合法 data-plane 重绑不会让这份无内容的本地历史消失；
- 结果按 `accepted_at DESC, submission_id DESC` 稳定分页。Cursor 保持
  canonical `v1.<lowercase UUID>` 外形，但服务端在同一可见性范围内解析锚点接受时间；
  跨租户、跨邮箱、非本人、非已接受、非 sent 或失效锚点均拒绝整页；
- 返回前会在一个新事务以数据库当前时间复核主体、RBAC、Read/Send、发送人、
  accepted/sent 关系与当页最新快照身份。只有 `mail.submissions.queried` Audit 与
  `mail.submissions.queried.v1` Outbox 原子提交后才披露；两份证据只记录页大小、结果数和
  是否还有下一页，不保存 Cursor、Submission/Draft UUID、地址、内容或投递明细。
- 路径与 query 只在 Mail Session 认证后才解析；`HEAD` 同样先经过 Mail Session 分区，
  之后明确返回 `405`（`Allow: GET, POST`），不会借由框架的 GET→HEAD 回退生成任何
  `mail.submissions.queried` Audit 或 Outbox 证据。
- 该目录不会调用或改变单项 `GET /api/v2/mail-submissions/{submission_id}` 的既有语义：
  后者仍是独立、按原请求人和 Send 权限授权的状态接口，前端目录不调用它。目录的最小
  DTO 不能被理解为收紧该独立接口的既有本人状态可见性。

`POST /api/v2/mailboxes/{mailbox_id}/submissions` 使用 `Idempotency-Key`
冻结一个精确 Draft 版本并返回 `202 Accepted` 与稳定 Submission Intent UUID：

- 同一路径的 `GET`、`HEAD` 与 `POST` 都先完成各自客户端边界的认证；`POST` 的邮箱 UUID
  也只在 API 身份、Scope 与入口限流验证后才解析，畸形路径不能成为未认证路由预言机；
- 请求仍重新校验 Active Human/Agent、RBAC、Agent Token Scope（如适用）与邮箱 Send ABAC；
- Idempotency Key 只存 SHA-256，相同 Key/相同请求返回同一 Intent，相同 Key/不同
  请求永久冲突；
- Draft 冻结、Intent、`mail.submission.requested` 审计与 Outbox 原子提交；
- Worker 在远端调用前提交 60 秒租约，每次都先按冻结 Email ID 查询
  `EmailSubmission/query/get`；唯一既有结果直接收敛，零条才创建；
- Identity 必须唯一匹配控制平面的邮箱主地址。多个 Identity、多个 Submission 或
  Email/Identity 绑定不一致都会进入 `manual_review`，不会自动重发；
- 网络超时、无效响应或进程退出只保留 `reconciling`，租约过期后继续先查询再创建；
- `accepted` 会同时把 Draft 置为 `sent`，终态、审计和版本化 Outbox 原子提交；
- `GET /api/v2/mail-submissions/{submission_id}` 只允许原请求 Principal 在仍具备该
  邮箱 Send 权限时读取，且不返回任何 JMAP 内部 ID。

写信界面在点击发送时会在任何异步等待前同步锁定当前快照，再串行等待最新草稿保存并
冻结该版本；浏览器保持同一个 Idempotency Key 处理响应丢失重试，并轮询稳定 Intent。
一旦提交请求可能离开浏览器，本轮内容持续只读且不会静默创建新 Draft；只有确定在出网前
的最终草稿保存失败才允许解锁，避免界面内容与已经冻结或状态未知的邮件分叉。

## 客户端交互、国际化与可访问性

- 应用固定文案统一由类型安全的 `zh-CN` / `en` 词典提供；Locale 依次从已持久化
  选择、文档语言和浏览器语言推导，均不支持时确定性回退到简体中文。切换语言无需
  刷新，会同步更新 `<html lang>` 与本地持久化值；日期、消息数量和附件大小按
  当前 Locale 格式化。未知词条、缺失或无效的插值参数只返回非敏感占位符；
- 视口不超过 900 px 时使用单列列表—详情层级：打开邮件后隐藏列表并聚焦详情标题，
  “返回收件箱”会清空详情并把焦点恢复到原邮件按钮；移动工具栏持续提供写信、语言
  和退出入口；不超过 400 px 时只视觉隐藏操作文字并保留本地化 `aria-label`。视口
  不超过 560 px 时，写信窗口占满可用视口；
- 写信窗口使用 `role="dialog"`、`aria-modal="true"` 和标题关联。打开时精确保留并
  隔离所有标记背景的 `inert` / `aria-hidden` 状态，初始聚焦收件人，Tab 与
  Shift+Tab 在窗口内环绕；收件人已因提交而锁定时聚焦首个仍可用控件；发送进行中拒绝
  关闭，安全关闭后恢复背景属性和原触发器焦点；
- 收件箱初始化、搜索、刷新、清空搜索和加载更多共享单调请求代次，迟到响应不能覆盖
  当前结果；写信自动保存、提交轮询和终态关闭定时器绑定写信会话代次，不能污染随后
  打开的新邮件或回复。详情加载失败会结束加载状态、在当前可见详情中提示并允许重试；
  并发已读与星标响应只合并各自字段；
- 草稿目录与恢复请求同时绑定邮箱、视图和会话代次，并使用 `AbortSignal` 中止旧请求；
  只读邮箱不请求草稿目录。恢复其他草稿前若已有编辑内容，会先显示可访问确认对话框；
  恢复快照首次灌入不触发自动保存，后续编辑继续用原 Draft UUID/版本做 CAS，409 不会
  派生副本。Submission 已出网且仍在途或结果未知时，新建、回复、恢复和邮箱切换均不能
  清除冻结快照或幂等上下文，只能重开该窗口并继续精确对账；
- 已发送目录同样绑定邮箱与视图代次；仅在可发送邮箱中启用，支持显式刷新与“加载更多”，
  不自动轮询、不提供正文/收件人展开或搜索。切换邮箱、切换视图或迟到响应都会中止或
  丢弃旧目录请求；列表仅显示“服务器已接受”与聚合投递进度，避免把 `accepted` 误写成
  最终投递成功；
- 上述能力由 Vitest/jsdom 组件测试覆盖。它们不能替代真实浏览器、触屏、屏幕阅读器、
  axe 或 WCAG 2.2 AA 验收。

## 最终投递状态

`accepted` 仅表示 Stalwart 接受了 JMAP Submission，不等于所有对端 SMTP
服务器已经接收。Worker 随后通过同一精确 `EmailSubmission.id` 读取 JMAP
`deliveryStatus`：

- 每个收件人独立归一化为 `queued`、`delivered`、`permanent_failure` 或
  `unknown`；
- 地址不写入控制平面，使用由 Submission UUID 限定作用域的 SHA-256 引用，无法
  跨邮件关联同一收件人；
- 完整快照、收件人证据和最小化 `message.delivery_status_changed.v1` Outbox
  在同一事务提交；完全相同的快照幂等去重；
- PostgreSQL 延迟约束触发器在提交时复核聚合计数与每条收件人证据，并拒绝对快照
  和明细执行 UPDATE、DELETE 或 TRUNCATE；
- 只要存在 `queued` 或 `unknown`，原对账任务就按 Outbox 指数退避继续执行；
  全部收件人进入已投递或永久失败后才成为最终状态；
- 状态 API 不返回 JMAP Account、Email、Identity 或 Submission ID。写信界面明确
  区分“邮件服务器已接受”“正在投递”和“全部成功/部分失败”。

Stalwart 队列级 WebHook 另行写入不可变运维证据，但当前不会猜测
`queueId → Submission`，因此不用于改写本节的用户状态。这样既保留队列排障证据，
也避免把只有弱关联的事件错误归到某封用户邮件。

## 凭据隔离

管理域名投影使用 `COPILOTMAIL_MAIL_ENGINE_API_TOKEN_FILE`。Stalwart 管理 API
Key 本身不具备邮件协议访问能力，Human 收件箱必须另外配置：

```text
COPILOTMAIL_MAIL_CONTENT_BASE_URL=https://mail.example.com
COPILOTMAIL_MAIL_CONTENT_TOKEN_FILE=/run/secrets/copilotmail/mail-content.token
COPILOTMAIL_MAIL_CONTENT_TIMEOUT_SECONDS=10
COPILOTMAIL_MAIL_CONTENT_ALLOW_INSECURE_HTTP=false
COPILOTMAIL_REQUIRE_MAIL_CONTENT=true
```

内容文件保存外部 Token Broker 签发并原子轮换的 Account 范围 OAuth Bearer
Token。API 进程不保存 Refresh Token。生产环境拒绝明文 HTTP。

草稿写入使用第三套独立凭据，不能复用内容或管理凭据：

```text
COPILOTMAIL_MAIL_COMPOSE_BASE_URL=https://mail.example.com
COPILOTMAIL_MAIL_COMPOSE_TOKEN_FILE=/run/secrets/copilotmail/mail-compose.token
COPILOTMAIL_MAIL_COMPOSE_TIMEOUT_SECONDS=10
COPILOTMAIL_MAIL_COMPOSE_ALLOW_INSECURE_HTTP=false
COPILOTMAIL_REQUIRE_MAIL_COMPOSE=true
```

该 Token 只需精确 Account 范围的 JMAP Session、`Mailbox/get` 与 Email
get/create/destroy 权限，不授予 EmailSubmission 或管理权限。虽然 JMAP 的
`Email/get` 能力位于 Account 范围，CopilotMail 的恢复适配器只接受 PostgreSQL 已证明的
单个本人 active Draft ID，并执行上文的 Drafts/From/Thread/摘要复核；它不会枚举收件箱、
请求 Blob ID 或返回未知远端草稿。生产 readiness 会分别检查 `mail_content` 与
`mail_compose`。

正式发送使用第四套独立凭据，且只挂载到 Durable Outbox Worker：

```text
COPILOTMAIL_MAIL_SUBMISSION_BASE_URL=https://mail.example.com
COPILOTMAIL_MAIL_SUBMISSION_TOKEN_FILE=/run/secrets/copilotmail/mail-submission.token
COPILOTMAIL_MAIL_SUBMISSION_TIMEOUT_SECONDS=10
COPILOTMAIL_MAIL_SUBMISSION_ALLOW_INSECURE_HTTP=false
COPILOTMAIL_REQUIRE_MAIL_SUBMISSION=true
```

Worker 启动时验证 JMAP Mail/Submission 能力和可用 Identity。Token 授予
`Identity/get`、`Mailbox/get`、`EmailSubmission` query/get/create，以及仅用于
`onSuccessUpdateEmail` 清除 `$draft` 并从 Drafts 移到 Sent 的 `Email/set`；
适配器不会读取正文、修改不可变正文、修改 Identity 或管理对象。API 进程只持久化
Intent，不需要也不读取该 Token。提交请求不自造 Envelope，由邮件引擎从冻结
Email Header 派生。

## 尚未完成

- 会话视图；
- 附件上传、已发送邮件正文/详情和已发送目录搜索；
- 真实公网延迟、临时失败、永久退信与 DSN 演练（队列级 WebHook 追加式证据代码
  已完成，外部 Stalwart/公网验收尚未执行）；
- 快捷键；
- 真实浏览器中的移动端、键盘和屏幕阅读器 E2E，以及 axe/WCAG 2.2 AA 审计
  （当前仅有 Vitest/jsdom 组件回归）；
- 孤儿草稿检测、受治理后台清理与共享协作编辑。

这些能力完成并通过计划中的交互与安全验收前，不将 P4A 标记为完成。
