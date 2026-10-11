# D4B：Inbox Queue 与 Reading 真实字段映射

> 状态：**已实现；仅在本地回环固定 BFF 夹具上验证（2026-08-24）。** D4B 只重构 Human Mail
> 的 Inbox Queue 扫描层级，以及 Reading 的标题/元数据纯呈现。它不表示 D4、邮件端或任何
> Agent Surface 已完成；没有在本表列出的字段、状态和动作一律不得新增。

## 1. 目标与不变量

目标是在不扩大读取、授权或写入能力的前提下，让用户先能扫描“谁发来、主题是什么、何时收到、
是否未读/星标/有附件”，再在**明确打开**后阅读受控详情。视觉服务于这个阅读顺序，而不是用
风险色、伪优先级、统计数字或装饰性卡片制造注意力。

D4B 是纯呈现切片，必须保持以下不变量：

- 不新增 API、DTO、权限、角色判断、路由、URL/fragment/query UI 状态、浏览器存储键、缓存、
  轮询、hover/focus 预取或后台详情读取；现有请求仍使用 `cache: "no-store"`，见
  [`web/mail-app/src/api.ts`](../web/mail-app/src/api.ts)。
- 不改变精确 mailbox 绑定、会话锁定、Abort、详情代际隔离、已读/星标写入、草稿 CAS、未知
  submission 冻结或 Compose 的提交语义。
- `j`/`k` 只是队列光标移动；它们不能读取详情、标记已读或产生任何写请求。只有鼠标/触摸
  点击某邮件，或光标已有选中项时按 `Enter`，才可以进入 Reading 并读取详情。
- 不为 `kind="agent"` 推断运行状态、Owner、工具、策略、在线状态或任何 Agent 事实。D4B
  没有 Agent Runtime 区，也没有第四个 Inspector。

## 2. 已有 DTO 与 Queue 可见字段

### 2.1 现有契约

后端的权威契约位于
[`crates/copilotmail-contracts/src/lib.rs`](../crates/copilotmail-contracts/src/lib.rs) 的
`InboxMessageSummaryResponse`、`InboxPageResponse` 与 `MessageDetailResponse`；前端严格解析并
拒绝未知字段，见 [`web/mail-app/src/api.ts`](../web/mail-app/src/api.ts)。前端对应类型位于
[`web/mail-app/src/types.ts`](../web/mail-app/src/types.ts)。

| DTO / 字段 | D4B Queue 是否可见 | 允许的纯呈现用途 | 禁止的扩展 |
| --- | --- | --- | --- |
| `InboxPage.items` | 是 | 当前响应页的可扫描消息行 | 不能把当前页长度或累计页数表述为邮箱总数、未读总数或 SLA 数字 |
| `InboxPage.has_more` | 是 | 显式“加载更多”动作的条件 | 不自动无限滚动、预加载下一页或伪造剩余数量 |
| `InboxPage.next_cursor` | 否 | 仅作为现有分页请求的内部游标 | 不显示、不写入 URL 或浏览器存储 |
| `MessageSummary.message_id` | 否 | React key、当前选择和既有安全详情请求的内部标识 | 不显示 UUID，不能换成 Provider/Email/Thread ID |
| `MessageSummary.from` | 是 | 有序发件人中的可读名称或邮箱；当前 `MessageList` 采用首项名称、再回退首项邮箱 | 不推断发件人组织、风险、信任级别、联系人画像或会话状态 |
| `MessageSummary.subject` | 是 | Queue 的主扫描标题；空主题使用既有本地化文本 | 不生成摘要、优先级或业务分类 |
| `MessageSummary.received_at` | 是 | 格式化后的接收时间 | 不推断时限、响应承诺、时区外业务状态 |
| `MessageSummary.preview` | 是 | 单个辅助预览层 | 不将其扩展为正文、HTML、线程摘要或隐藏内容 |
| `MessageSummary.is_read` | 是 | 未读/已读的真实差异；必须同时保留非色彩线索 | 不显示未读总数，不能用颜色作为唯一差异 |
| `MessageSummary.is_flagged` | 是 | 真实星标图标与既有星标动作状态 | 不改称“优先级”或生成风险等级 |
| `MessageSummary.has_attachment` | 是 | 真实附件标记图标 | 不产生下载、预览、扫描结论或附件内容读取 |

现有 Queue 组件是
[`web/mail-app/src/components/MessageList.tsx`](../web/mail-app/src/components/MessageList.tsx)。
D4B 的层级固定为：

1. 发件人、接收时间和未读状态构成第一眼的定位层；时间靠末端，不能挤压主题到不可辨识。
2. 主题是唯一主标题；未读可使用字重/结构差异，但不能仅依赖颜色。
3. `preview` 是次级说明，必须在窄宽度下先于装饰压缩；星标和附件只保留为紧凑、带无障碍
   文字的真实标记。
4. 当前页消息数只能继续表述为当前响应范围；`has_more` 为真时只出现显式加载更多入口。

这不是新的收件箱模型：搜索结果仍是相同 `InboxPage`，由既有 `searchInbox` 产生；刷新、搜索、
清除搜索与加载更多仍由 `App.tsx` 现有控制器触发。D4B 不改变 30 条页面大小、游标语义或错误
重试策略。

## 3. Reading 标题与元数据的字段边界

阅读区只能消费两类已存在的数据：已选择的 `MessageSummary`，以及明确请求后返回的
`MessageDetail`。详情组件与其现有条件渲染位于
[`web/mail-app/src/components/MessageDetail.tsx`](../web/mail-app/src/components/MessageDetail.tsx)。

| 来源 | D4B 可呈现内容 | 何时允许出现 | 绝不呈现或推断 |
| --- | --- | --- | --- |
| 已选 `MessageSummary` | 主题、`from`、`received_at`、真实星标状态 | 只有已明确打开某邮件时；可在详情尚未返回时显示标题、发件人和接收时间等真实摘要 | 正文、To/Cc、附件清单、投递状态、风险、优先级、Provider ID |
| `MessageDetail.message` | 与已打开 selection、回复关系检查一致的返回摘要 | `detailMatchesSelection` 通过时 | 不显示或接受错消息 UUID，也不接受错回复关系的响应 |
| `MessageDetail.to` | 服务器返回的完整 To 元数据 | 详情成功后 | 不推断收件人身份、组织或授权 |
| `MessageDetail.cc` | 服务器返回的完整 Cc 元数据；空数组时不画 Cc 行 | 详情成功且 `cc.length > 0` | 不绘制空的占位 Cc 区 |
| `MessageDetail.text_body` | 受控纯文本正文 | 详情成功后 | HTML 正文、富文本/远程资源、隐藏附件内容、自动摘要 |
| `MessageDetail.text_body_truncated` | 服务器返回的截断提示 | 详情成功且值为 `true` | “加载全部正文”、自动续读或臆测未返回内容 |
| `MessageDetail.attachments` | 名称、MIME 类型、字节大小的元数据 | 详情成功且数组非空 | 下载、打开、预览、转发、恶意文件判断或二进制读取 |
| `MessageDetail.reply_to_message_id` | 真实、可回复关系的条件 | 详情成功；仅作为回复草稿关系，不显示该 UUID | 线程图、引用链推断、任意转发关系 |

`MessageDetail` 的解析会验证返回消息 UUID 与请求 UUID 一致，并要求非空
`reply_to_message_id` 与该消息 UUID 一致；不能因为视觉改造绕过这一检查，见
[`web/mail-app/src/api.ts`](../web/mail-app/src/api.ts) 与
[`web/mail-app/src/detail-request.ts`](../web/mail-app/src/detail-request.ts)。

### 3.1 阅读状态与错误边界

| 状态 | 允许显示 | 不允许显示 / 行为 |
| --- | --- | --- |
| 未选择 | 既有“选择一封邮件”空状态 | 不能发起详情请求或展示伪详情卡片 |
| 明确打开，详情加载中 | 已选消息的真实标题/发件人/接收时间、真实星标状态、明确的 loading 状态 | 不能显示正文、To/Cc、附件、预估内容或 skeleton 伪数据 |
| 详情读取失败 | 已选消息的真实摘要、可见错误和既有显式重试入口 | 不自动重试、不保留上一个消息的正文、不把错误归因于服务器外的猜测 |
| 详情成功 | 表中允许的纯文本与元数据 | 不插入 HTML、下载按钮、转发/归档/删除动作或风险面板 |
| 已读/星标写入失败 | 既有针对该消息/操作的错误提示 | 不乐观伪造成功，也不改变其它消息状态 |

## 4. 打开、键盘与动作条件

### 4.1 读取时机

- `MessageList` 的行点击调用既有 `onOpenMessage`；`App.tsx` 只有在 `openedMessageId` 已由此
  明确设置后才调用 `getMessage`。详情请求使用 `AbortController`，并由
  `MessageDetailRequestGate` 的 generation 与安全 UUID 做结果验收。
- 全局快捷键实现在
  [`web/mail-app/src/mail-keyboard-shortcuts.ts`](../web/mail-app/src/mail-keyboard-shortcuts.ts)：
  `j`/`k` 只调用 `onMoveMessage`；`Enter` 才调用 `onOpenSelectedMessage`。`App.tsx` 的
  `moveMessageSelection` 会关闭详情、更新选中和焦点，但不会设置 `openedMessageId`。
- 因此 D4B 禁止在 row hover、focus、可视区域进入、`j`/`k`、屏幕宽度变化、Queue 重新排序或
  Loading 展示时读取详情；也禁止选中变化导致任何 `getMessage` 预取。
- 既有的未读→已读 PATCH 仅随明确 `openMessage` 发生；D4B 不得让光标移动、样式变化或渲染
  重试触发它。

### 4.2 保留动作及其精确条件

| 动作 | 可执行条件 | D4B 呈现约束 |
| --- | --- | --- |
| 打开邮件 | 会话未锁定、当前有 mailbox settings、用户点击行或按 `Enter` | 唯一允许读取详情的入口；在窄屏进入详情栈并把焦点交给详情标题 |
| 返回 Queue | Reading 已打开 | 清除已打开详情并按既有逻辑恢复行焦点；不得重读列表或详情 |
| 星标切换 | 会话未锁定、已认证、已有选中消息与 mailbox settings | 仅保留既有真实星标动作；不新增“重要/风险”动作或批量动作 |
| 详情重试 | 会话未锁定且存在已打开消息 | 仅为失败的同一详情重新请求；仍受 generation/Abort/消息匹配保护 |
| 回复 | `reply_to_message_id` 非空 **且** `activeMailbox.can_send=true` **且** submission 未处于 unresolved 冻结 | 关系为空时不出现回复入口；关系存在但只读或 unresolved 时入口保留为禁用说明，绝不能可发送或绕过只读 |

回复由 `App.tsx` 的 `replyToSelected` 与 `replyFromKeyboard` 继续守卫；阅读组件只接收已计算的
`canReply`。D4B 不改变回复收件人、主题、草稿关系或已存在内容时的确认流程。

以下动作不属于现有 Human Mail Inbox/Reading 契约，D4B 必须不创建 UI、键盘命令、隐藏菜单或
请求：**下载、打开附件、转发、归档、删除、移动、批量处理、投递确认、风险/优先级标记、HTML
切换、线程展开、Agent 运行/策略面板。**

## 5. 自适应与视觉冻结

D4B 必须在既有 Shell 几何内工作，不能用更窄的 Reading 区换取装饰：

| 宽度 | Queue / Reading 规则 |
| --- | --- |
| ≥1084px | 224px scope rail、328–400px Queue、Reading 至少 520px；Queue 允许快速扫描，Reading 的长主题和地址优先完整可读 |
| 1008–1083px | 160px 的文字 scope rail、328px Queue、Reading 至少 520px；不退化为无文字图标导航，也不把元数据压成无意义缩写 |
| ≤1007px | 明确 Queue→Reading 栈；打开后隐藏 Queue，返回后恢复选中行焦点；不能双栏挤压或产生横向滚动 |

视觉规则：

- 使用已有设计系统 token、稳定排版和间距；颜色只服务真实的成功/警告/错误或选中状态，不能作为
  风险、优先级、Agent 身份能力或未读的唯一信息通道。
- Loading、空状态、错误和只读都必须以真实文本与语义状态表达，不能以无含义渐变、随机色块、
  空白“dashboard”或伪数据占位替代。
- 在 `≤1007px`，所有 D4B 新增或重新布局的可触控原生控件至少 `44×44px`；Queue 邮件行、返回、
  星标、回复、刷新、搜索与重试必须保持语义原生控件及可达名称。
- 长主题、地址和附件元数据可按可读性换行/截断，但不能用 CSS 内容伪造字段、删除真实安全提示或
  仅靠悬浮提示传达必要事实。

现有断点、触控尺寸和原生控件回归分别由
[`web/mail-app/src/styles.css`](../web/mail-app/src/styles.css)、
[`web/mail-app/src/detail.css`](../web/mail-app/src/detail.css)、
[`web/mail-app/src/search.css`](../web/mail-app/src/search.css) 与
[`web/mail-app/src/mobile-touch-targets.test.tsx`](../web/mail-app/src/mobile-touch-targets.test.tsx)
约束；D4B 必须继承而非复制另一套断点。

## 6. Compose 的边界：只做回归保护

Compose 不是 D4B 的设计输出。D4B 不修改
[`web/mail-app/src/components/ComposerDialog.tsx`](../web/mail-app/src/components/ComposerDialog.tsx)、
[`web/mail-app/src/useComposerSession.ts`](../web/mail-app/src/useComposerSession.ts) 的字段、状态、
CAS、提交、Evidence Inspector 或 unknown submission 冻结。

实现 Queue/Reading 时只需证明它们仍然不受破坏：打开/关闭 Reading、`r` 回复快捷键、只读邮箱、
已有未提交内容的回复确认、草稿恢复和提交状态都继续走原有路径。不得借“阅读体验”移动、复制或
重新解释 Composer footer 的提交状态。

## 7. 实施与回归门槛

实现 D4B 前后，以下证据必须保留或增强：

1. `web/mail-app/src/components/MessageList.test.tsx`：初始/刷新 loading、失败重试、搜索重试，
   并补充 Queue 只使用本表字段、当前页计数不冒充总数的断言。
2. `web/mail-app/src/components/MessageDetail.test.tsx`：From、Received、完整 To、条件 Cc 与安全
   纯文本；需继续验证空 Cc 不渲染，且不得出现 HTML 或附件下载动作。
3. `web/mail-app/src/detail-request.test.ts` 与 `web/mail-app/src/App.test.tsx`：同 UUID 的状态刷新不
   重读、过期结果不接管、`j`/`k` 不读取详情/不标记已读，`Enter` 才读取。不得删除
   `AbortController`、generation 或精确消息/回复关系校验。
4. `web/e2e/mail/mail-state-contracts.spec.ts`：桌面与 390px 都证明 `j`/`k` 保持在 Queue，
   `Enter` 后才出现 Reading、才产生一条详情读取；保留只读回复禁用和 44px 控件检查。
5. `web/e2e/mail/mail.spec.ts`：保留 1440、1280、1024、768、390px 的视觉/几何检查，确认
   1007px 前切换为显式栈、无横向滚动、初始不读详情；视觉快照必须逐组人工审图。
6. `web/e2e/mail/mail-fixtures.ts`：继续让未知 API 请求直接失败，禁止为设计方便而放宽 fixture
   路由或放行新的端点。
7. 在 `web/` 目录执行 `npm run check`、`npm test`、`npm run build` 和 `npm run e2e:run`；随后执行
   根目录的 `python scripts/check_design_governance.py --self-test` 与
   `python scripts/check_documentation.py`。只有所有门禁通过且人工审图确认无占位符/无虚构字段，
   D4B 才能标记为已实现。

### 本轮实现证据（2026-08-24）

Queue 行只消费已有 `MessageSummary`；Reading 标题和元数据只消费已明确打开的 selection 及通过
既有匹配校验的详情。`j`/`k` 仍只移动选择，点击或 `Enter` 才读取详情；未新增 API、DTO、权限、
路由、缓存、预取、轮询或邮件动作。

本轮全量 Web 门禁均通过：`npm run check`、`npm test`（408 条：设计系统 17、Admin 207、Mail
184）、`npm run build`、`npm run e2e:run`（101/101）以及
`python scripts/check_design_governance.py --self-test`、`python scripts/check_documentation.py`。
人工审图覆盖 77 张 Windows PNG（Admin 40、Mail 37）。测试仅使用本地回环 Vite 产物和内存
BFF 夹具；不证明 Docker、NixOS、数据库、IdP、域名邮件或生产环境。

## 8. 与 D4 的关系

本文细化并约束
[`DESIGN_D4_MAIL_FIELD_MAPPING.zh-CN.md`](DESIGN_D4_MAIL_FIELD_MAPPING.zh-CN.md) 中尚未实现的
Queue 与 Reading 层级，不改变其 Scope rail、目录、Sent、Draft 与 Compose 边界。若未来需要
下载、转发、归档、删除、HTML 渲染、风险事实、Agent 状态或任何新邮件字段，必须先定义独立
DTO、授权和错误契约，再建立新的映射文档；不得通过 D4B 视觉改造暗中扩大数据或动作范围。
