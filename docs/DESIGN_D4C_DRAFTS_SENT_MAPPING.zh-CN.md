# D4C：Drafts / Sent 真实目录字段映射

> 状态：**已实现（2026-08-24）。** D4C 只收敛 Human Mail 中 Drafts 与 Sent 的 metadata-only
> 宽列表、真实空/错误/恢复状态，以及 Compose footer 已有状态文案的可读性。它不表示 D4、
> 邮件端或 Agent Surface 已完成；本表以外的字段、状态和动作一律不得新增。
>
> 已在本地回环 Vite 产物与内存 BFF 夹具中验证：411 条 Web Vitest、103 条 Chromium
> 场景和 79 张 Windows PNG 基线（Admin 40、Mail 39）。该证据不证明真实后端、部署、
> RBAC、数据库、Docker、NixOS 或邮件依赖。

## 1. 目标与不变量

Drafts 和 Sent 没有可安全显示的详情模型，因而必须是**宽列表**，而不是一个空的 Reading
栏或填充用的指标/卡片。用户应能先看清“这是什么、何时更新/接受、收件人数、下一步或已知
投递聚合”，再只执行已经存在的恢复、刷新或加载更多动作。

D4C 必须保持以下不变量：

- 不新增 API、DTO、权限、路由、URL/fragment/query 状态、浏览器存储、缓存、轮询、预取或
  hover/focus 读取；现有目录请求、分页、错误与 Abort/generation 仍由既有 hooks 控制。
- 不修改 `useDraftDirectory.ts` 的精确 mailbox 绑定、Active Human/可发送性/会话锁定守卫、
  恢复确认、CAS、冲突恢复、焦点返还或 `restoreBusyId` 的全目录冻结。
- 不修改 `useSentDirectory.ts` 的目录读时机、scope 绑定、Abort/generation、分页或停用语义；
  Sent 项仍无详情、无行点击、无键盘假动作。
- 不改变 `useComposerSession.ts` 的自动保存、提交、幂等键、未知提交冻结、对账或 Evidence
  Inspector 语义。仅保证 footer 的既有真实状态文案不会因截断而隐藏关键承诺。
- 不显示主题、正文、地址、Provider/JMAP/SMTP 标识、UUID、草稿版本、单收件人投递轨迹、
  Agent Owner/运行/策略/在线状态、统计总数或风险等级。

## 2. Drafts：可恢复草稿的真实字段

权威契约为 `HumanDraftPage` 与 `HumanDraftListItem`，前端严格解析见
[`web/mail-app/src/api.ts`](../web/mail-app/src/api.ts)，类型见
[`web/mail-app/src/types.ts`](../web/mail-app/src/types.ts)。

| DTO / 字段 | D4C 是否可见 | 允许用途 | 禁止项 |
| --- | --- | --- | --- |
| `HumanDraftPage.items` | 是 | 当前响应页的可恢复草稿行 | 不把当前页长度写成草稿总数或待办数 |
| `HumanDraftPage.has_more` | 是 | 既有“加载更多草稿”动作的条件 | 不自动加载、无限滚动或伪造剩余数量 |
| `HumanDraftPage.next_cursor` | 否 | 现有请求内部游标 | 不显示、不写 URL/存储 |
| `draft_id` | 否 | React key、既有精确恢复目标与焦点回归 | 不显示 UUID，不以它生成标题或链接 |
| `version` | 否 | 既有精确 CAS 恢复的内部版本 | 不显示版本号或“同步健康”推断 |
| `reply_to_message_id` | 间接 | 仅决定“新邮件草稿”或“回复草稿”的真实类型文字 | 不显示目标 UUID、主题、线程或引用链 |
| `recipient_count` | 是 | 收件人数事实 | 不显示地址、名单、群组或投递推断 |
| `updated_at` | 是 | 最近编辑的真实时间 | 不推断 SLA、保留期限或操作者 |
| `created_at` | 否 | 保留为恢复/CAS 逻辑的现有安全元数据 | 不需要为丰富视觉而显示第二个时间轴 |

Draft 行保持**一个原生恢复按钮**：它的主层级是草稿类型，次级事实是收件人数与更新时间，
唯一行动文字是“继续编辑 / 恢复中”。点击以外的 hover、focus、箭头移动或尺寸变化不能恢复
草稿、读取正文或产生请求。恢复中所有行、刷新和分页继续禁用；冲突后仍沿用已有确认与焦点
契约。

## 3. Sent：已接受提交与投递聚合

权威契约为 `HumanSentSubmissionPage`、`HumanSentSubmissionListItem` 与可选
`HumanSentDeliverySummary`。它只提供 metadata-only 目录，不提供提交详情。

| DTO / 字段 | D4C 是否可见 | 允许用途 | 禁止项 |
| --- | --- | --- | --- |
| `HumanSentSubmissionPage.items` | 是 | 当前响应页的非交互提交行 | 不表述为邮箱总投递量或 SLA |
| `HumanSentSubmissionPage.has_more` | 是 | 既有“加载更多已发送邮件”动作的条件 | 不自动加载或伪造剩余数量 |
| `HumanSentSubmissionPage.next_cursor` | 否 | 现有请求内部游标 | 不显示、不写 URL/存储 |
| `submission_id` | 否 | React key、排序/游标验证内部标识 | 不显示 UUID、链接或详情入口 |
| `accepted_at` | 是 | “已由邮件服务器接受”的时间事实 | 不把接受表述成最终投递成功 |
| `recipient_count` | 是 | 聚合收件人数 | 不列出地址、身份或单收件人结果 |
| `delivery` 缺失 | 是 | 明确“等待投递状态快照”的事实 | 不能推断失败、成功、排队或 ETA |
| `delivery.observed_at` | 是 | 聚合快照的观察时间 | 不推断实时性或后台刷新 |
| `delivery.terminal` 与五个聚合计数 | 是 | 已有的等待、进行中、全部投递或含失败的聚合文字与紧凑事实状态 | 不展示 `recipients`、`recipient_ref`、SMTP reply、Provider 或正文 |

Sent 行的主事实始终是“已接受发送 / Accepted for delivery”。只有服务端返回的聚合完整支持时，
才可给出成功或失败的视觉状态；无快照和非终态均保持中性/等待语义，不能使用“已送达”的绿色
成功暗示。Sent 必须继续是 `article`，没有 hover、pointer、点击、Enter、详情、下载、重试发送或
隐式 action。

## 4. 目录、加载、空与错误边界

| 状态 | 允许显示 | 不允许显示 / 行为 |
| --- | --- | --- |
| 初始/刷新加载 | 已有本地化同步状态与 `aria-busy` | 伪行 skeleton、假计数、自动重试 |
| Drafts 正常 | 当前页类型、人数、更新时间、唯一恢复动作 | 正文、主题、地址、版本、UUID |
| Draft 恢复中 | 已有“恢复中”文字与全目录禁用 | 并发恢复、重复 POST、把内容提前显示出来 |
| Sent 正常 | 接受时间、人数、可选真实聚合 | row action、详情面板、收件人/正文/投递细节 |
| 空目录 | 已有真实空文案和刷新动作 | 用插画、示例草稿/邮件或虚构建议填充 |
| 列表/恢复错误 | 已有独立 `role="alert"` 和对应 retry | 将 Draft 列表错误与恢复错误混为一条，或把错误改写成零 |

静态的 delivery 事实不应被重复 `aria-live` 播报；只有既有异步目录/恢复状态继续使用当前的
状态播报。D4C 不改变语言、日期、数量格式化或已存在的中英文词典所有权。

## 5. 自适应与视觉冻结

| 宽度 | Drafts / Sent 规则 |
| --- | --- |
| ≥1008px | 保持 rail + 单一宽列表，不补空 Reading 栏；每行按“主对象 → 两项元数据 → 真实动作/聚合”组织，时间与事实 cluster 对齐而不漂到无关的整屏边缘 |
| ≤1007px | 保持单列目录；时间、人数、聚合与 Draft 恢复动作按自然两/三行换行，不产生横向滚动 |
| ≤390px | 所有可触控 Draft 恢复、刷新、分页、错误重试至少 44×44px；Sent 不因触控尺寸变成可点击行 |

视觉只能使用既有设计 token、纸面/表面/墨黑/边线和真实状态 token。恢复和当前焦点可以使用既有
agent/jade 信号；全部投递仅在真实终态聚合支持时使用紧凑 positive，含失败的终态必须同时有
文字和图标/结构，等待/无快照不使用彩虹色块或成功暗示。不得用渐变、卡片墙、伪表格、头像、
营销插画或无意义色彩拼接填满宽列表。

Compose footer 是本切片的回归边界：已有的长 `StatusIndicator` 标签必须完整可读、可换行且
`overflow-wrap: anywhere`；按钮可在空间不足时换行，但不改变其文本、禁用条件或提交顺序。

## 6. 实施与回归门槛

1. `DraftList.test.tsx`：验证类型、人数、更新时间、唯一恢复入口和无主题/正文/地址/UUID；
   恢复中继续冻结刷新、行和分页。
2. `SentList.test.tsx`：验证无快照、排队、全部投递和部分失败的真实聚合；不得出现 UUID、
   SMTP、收件人、正文或 row action。
3. 保留 `useDraftDirectory.test.tsx`、`useSentDirectory.test.tsx`、`App.test.tsx` 中的 exact
   mailbox、Abort/generation、CAS、恢复确认、未知提交和会话锁定覆盖。
4. `web/e2e/mail/mail-directory-state-contracts.spec.ts`：新增/更新 1440、1008、390px 的目录
   快照与无横向溢出断言；目录切换不产生详情读取或邮件状态写入；Draft 与 Sent 各执行 Axe
   A/AA 扫描。Compose 的 unknown/manual-review 状态继续确认关键 footer 文案未裁剪。
5. 执行 `npm run check`、`npm test`、`npm run build`、`npm run e2e:run`、
   `python scripts/check_design_governance.py --self-test`、
   `python scripts/check_documentation.py`，并逐张人工审图。只有所有门禁通过且无占位/虚构
   字段时，D4C 才可标记为已实现。

## 7. 与 D4 的关系

本文补充 D4A 的 Shell 映射和 D4B 的 Inbox Queue / Reading 映射。它不改变 Scope rail、Inbox、
Search、Reading、Composer 状态机或 Agent API Surface。未来若要显示草稿内容、Sent 详情、
附件、投递明细、转发、删除、筛选、线程或任何 Agent 事实，必须先建立独立 DTO、授权、错误
契约和字段映射，不能通过 D4C 的目录视觉改造暗中扩大可见性或动作范围。
