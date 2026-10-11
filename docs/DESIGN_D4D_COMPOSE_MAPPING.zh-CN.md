# D4D：Compose / Submission Evidence 真实字段映射

> 状态：**已实现（2026-08-24）。** D4D 是 Human Mail 写信窗口的纯呈现收敛：把“以什么
> 已授权身份写信、正在编辑什么、当前一次提交的真实结果与证据”排成低干扰的顺序。它不重写
> Draft、Submission 或 Delivery 状态机，也不表示邮件发送、真实后端、Docker、NixOS 或任何
> 生产依赖已经验收。
>
> 实现仅将既有 `can_send` 状态由第三个 ContextBar fact 收入标题旁的 `status` 槽，并将既有
> 最小 Submission Evidence 拆为 display-only 组件；未增加字段、请求、动作或素材。当前回环
> 证据为 411 条 Web Vitest、104 条 Chromium 场景与 88 张 Windows PNG（Admin 40、Mail 48）。

## 1. 目标与范围

Compose 是一次受当前 Human Mail Session 与当前授权邮箱精确约束的写入工作，而不是一个
通用 Agent 控制台。D4D 只允许改善以下已有信息的层级：

1. 标题及已有的回复绑定说明；
2. 不可编辑的发件范围、身份种类和服务端可发送性；
3. To / Cc / Bcc、主题、纯文本正文与已有自动保存状态；
4. 当前一次提交的真实状态，以及默认收起的最小证据；
5. 唯一已经存在的发送或“确认同一提交”动作。

实现采用一条紧凑的 Context Strip：`primary_address` 与 `kind` 保留为 definition-list
事实，现有 `can_send` 的 `StatusIndicator` 放入同一 `ContextBar` 的 status 槽，避免把
服务器状态扩张成第三个同权表单区。下方始终是信封字段、正文、异步状态与唯一动作；不以
色块、伪时间线、卡片墙或空白 Inspector 填充空间。

## 2. 不变量

- 不新增 API、DTO、权限、路由、URL/query/fragment 状态、浏览器存储、缓存、轮询、预取，
  也不因 hover、focus、折叠/展开而读取邮件或状态。
- 不修改 `useComposerSession.ts`、`api.ts` 或 `App.tsx` 的请求顺序、自动保存、CAS、
  幂等、代际隔离、会话锁、邮箱绑定、关闭确认或焦点返回语义。
- From 始终从当前已授权 `MailboxDirectoryItem` 派生且不可编辑。`kind="agent"` 仅是
  邮箱目录给出的身份类别；绝不据此推断 Agent Owner、委托、运行、在线、策略、权限或
  投递结果。
- `can_send=false` 是服务器返回的当前只读事实；它不能推导为其他权限，也不能被视觉样式
  绕过发送按钮的既有禁用条件。
- 静态上下文状态不使用 `aria-live`。只有已有、紧邻唯一写入动作的 footer 异步状态继续
  使用 `role=status` / `aria-live="polite"`。
- D4D 不新增手动保存、撤销、再次发送、取消发送、附件、HTML/富文本、联系人、日程发送、
  转发、下载、导出、审批、Agent Token 或 Agent 自动写信。

## 3. 可见字段与披露边界

| 来源 | D4D 可显示 | 允许用途 | 必须隐藏 / 不得推断 |
| --- | --- | --- | --- |
| 当前授权邮箱目录 | `primary_address`、`kind`、`can_send` | 不可编辑发件范围、带文字与原创 SVG 的身份、服务端可发送/只读事实 | `mailbox_id`、Owner、委托、权限集合、Agent 运行/在线/策略 |
| 当前本地草稿 | `to`、`cc`、`bcc`、`subject`、`text_body` 与已有自动保存文案 | 已有信封字段、纯文本正文和 footer 草稿状态 | `draft_id`、`version`、JMAP/Provider/Thread ID、CAS 或同步健康推断 |
| 回复关系 | 已有“由服务器验证并绑定到原邮件”的说明 | 解释回复不是浏览器推断的线程关系 | `reply_to_message_id`、原邮件 Header、引用链、线程图 |
| 严格解析的 `MailSubmission` | 默认收起的 Inspector 中的 `submission_id`、`status`、`state_version`、`updated_at` | 对当前同一提交的最小、可复制事实证据 | `mailbox_id`、`draft_id`、`draft_version`、`created_at` 和任何“DTO 有就可见”的扩大披露 |
| 可选 `delivery` 聚合 | `observed_at`、`terminal`、收件总数与 delivered/failed/queued/unknown 聚合 | 已有的投递快照事实 | 地址、`recipients`、`recipient_ref`、单收件人轨迹、SMTP reply、Provider/JMAP、正文 |
| Image 2 人工复核素材 | 已登记的 `agent-human-review-v1.webp` 与其 alt | **仅** `manual_review` 的展开证据说明 | unknown、rejected、普通失败、空态或任何装饰性背景；D4D 不新增位图素材 |

`ComposerSubmissionEvidence` 必须继续是当前服务器返回 `MailSubmission` 的窄投影，不能
从浏览器推断身份、授权、委托、策略或投递状态。Evidence Inspector 默认收起，且不扩成
邮件端常驻的第四栏。

## 4. 状态语义：服务端枚举与本地派生状态

服务端 `MailSubmission.status` 仅有 `pending`、`reconciling`、`accepted`、`rejected` 和
`manual_review`。草稿、保存失败及“未知请求”不是该枚举，不能被压缩为一个泛化的
`failed` 或 `unknown` 标签。

| 条件 | 允许的真实语义 | 唯一允许的 UI 行为 | 禁止的文案或动作 |
| --- | --- | --- | --- |
| 本地草稿 / 自动保存 | 等待自动保存、保存中、已保存、恢复、自动保存失败 | 编辑、既有自动保存 | 不暗示已提交、已接受或已投递；不新增手动保存 |
| `pending` | 已进入发送队列 | 继续当前受控对账 | 不写成成功或已接受 |
| `reconciling` | 正在确认发送结果；超过现有轮询窗口时仍在后台处理 | 保留锁与同一提交上下文 | 无限 spinner、盲目重试或新提交 |
| `accepted` + `delivery=null` | 邮件服务器已接受；正在取得最终投递状态 | 显示已有真实进度/等待文案 | 说成已送达 |
| `accepted` + 非终态聚合 | 已接受，以及实际 `delivered / total` 聚合 | 继续现有对账 | 说成最终完成或预测 ETA |
| `accepted` + 终态且无失败 | 已投递至全部收件方服务器 | 既有终态关闭行为 | 推断已阅读、业务完成或其他结果 |
| `accepted` + 终态且含失败 | 投递完成：成功与永久失败的真实聚合 | 保留终态证据 | 隐藏失败或以绿色成功覆盖 |
| `rejected` | 邮件服务器拒绝了发送 | 现有锁定；联系管理员 | resend、undo、自动重发 |
| `manual_review` | 需要人工复核；请勿重复发送 | 展开同一证据、复制 Submission ID、联系管理员 | 再次提交、派生草稿、解除冻结 |
| 客户端请求结果未知 | 发送请求状态未知；再次确认**同一提交**不会创建第二封邮件 | 使用现有同一幂等上下文精确确认 | 把它称为服务端 `unknown`、普通 Retry 或新 Send |

`accepted`、Agent/jade 标识和 positive 绝不能彼此替代。无投递快照、对账中、请求未知与
人工复核均不得获得成功暗示。

## 5. 视觉与响应式冻结

| 区域 | D4D 布局规则 |
| --- | --- |
| 标题 | 使用已有 New / Reply / Edit draft 真实模式标题；回复模式保留服务器绑定说明。 |
| Context Strip | 标题旁仅放现有 `can_send` 的 `StatusIndicator`；facts 只为发件范围和身份。地址可截断但需保留原有 title，状态文字必须自然换行。 |
| 信封与正文 | 保持 To、按需披露的 Cc/Bcc、主题和纯文本正文的既有控件、最大长度与禁用条件。字段以细分隔和类型层级组织，不新增卡片或虚构摘要。 |
| Evidence | 已有 Evidence Inspector 默认收起；手动复核插画只在真实状态且用户明确展开后显示。ID 可复制和换行，不显示安全边界外的技术事实。 |
| Footer | 唯一异步状态紧邻唯一发送/同一提交确认动作。长 unknown/manual-review 文案必须完整换行，不改变按钮文本、优先级或禁用条件。 |
| `>1007px` | 右下固定紧凑 Compose，Context Strip 不压过正文或 footer；不制造全屏无意义留白。 |
| `≤1007px` | 沿用当前邮件端显式列表—详情口径；不创建第二套 Compose 状态。 |
| `≤560px` | Compose 占满可用视口；关闭、发送与同一提交确认保持至少 44×44px，长地址与长状态无横向滚动。 |

只使用共享的纸面、墨黑、结构线与既有语义 token。Jade 是已授权主动作、焦点或真实 Agent
关联的紧凑信号；不能成为大面积背景、成功色或身份唯一载体。不得引入渐变、玻璃拟态、彩虹
芯片、营销插画、虚构头像、空卡或占位数据。

## 6. 行为安全边界

以下控制语义不属于 D4D 的可改范围，必须保持逐字节等价的行为回归：

1. 有收件人且认证、精确邮箱、可发送、会话未锁、提交未锁时，才按既有 1200ms 规则自动保存；
   保存链串行，使用 draft ID/version 的 CAS。
2. 提交前先保存最终快照；随机 32-byte 幂等键、同一请求的 exact binding 检查与最多 40 次
   的对账窗口不改变。
3. 请求可能已存在时先冻结编辑内容。首次请求结果未知只能以同一幂等键确认；取得
   `submission_id` 后只能 GET 该精确提交，不能新建邮件。
4. `manual_review` 虽为服务器终态，仍是当前浏览器会话的证据锁：关闭只隐藏，重新打开
   Compose/命令仍须显示相同冻结内容和证据；邮箱切换、回复、草稿恢复与新提交继续受阻。
5. Session 重验期间不写入或轮询；所有代际隔离、Abort、Discard 确认、Dialog busy/Escape
   规则和焦点恢复保持。

## 7. 实施与验收门槛

实现前先完成最小且纯呈现的代码改动：把已有服务器可发送性从第三个 ContextBar fact 收到
现有 `status` 槽；不添加任何字段、请求或可点击动作。若需要拆分展示组件，其 props 只能
接收现有窄投影和既有回调，不能形成新的 controller 或状态源。

1. 单元测试保留并补强 Context Strip 的两项 facts、status 槽、只读文案、Agent 不推断、
   Evidence 默认收起、聚合证据最小披露和 `manual_review` 素材独占性。
2. `useComposerSession`、`App` 的自动保存、CAS、会话锁、邮箱绑定、冻结、同一幂等键、
   精确对账和焦点回归测试必须原样通过。
3. Chromium 在 1440、1008、390px 至少覆盖正常新邮件、Reply、恢复草稿、manual review
   （折叠及展开证据）和 unknown frozen；新增 Compose 1008 基线，并断言无横向溢出、长文本
   未裁剪、未知状态不增加 POST、390px 关键控件达到 44px。
4. 对正常、manual review（折叠/展开）和 unknown Compose 执行 `.composer` 的 Axe A/AA 扫描。
5. 执行 `npm run check`、`npm test`、`npm run build`、`npm run e2e:run`、
   `python scripts/check_design_governance.py --self-test`、
   `python scripts/check_documentation.py`、
   `python scripts/check_web_i18n.py --self-test`；再人工审查所有受影响 PNG。

这些浏览器证据只能来自回环 Vite 产物与内存 BFF 夹具。它们不证明真实 RBAC、后端 CAS/幂等、
邮件发送、数据库、Docker、NixOS 或生产环境。

## 8. 与 D4 其他切片的关系

本文补充 [`DESIGN_D4_MAIL_FIELD_MAPPING.zh-CN.md`](DESIGN_D4_MAIL_FIELD_MAPPING.zh-CN.md)、
[`DESIGN_D4B_QUEUE_READING_MAPPING.zh-CN.md`](DESIGN_D4B_QUEUE_READING_MAPPING.zh-CN.md) 与
[`DESIGN_D4C_DRAFTS_SENT_MAPPING.zh-CN.md`](DESIGN_D4C_DRAFTS_SENT_MAPPING.zh-CN.md)。它既不
扩大 Draft/Sent 的 metadata-only 边界，也不把 Agent Inspector 的 Owner、Policy 或 Trace
字段带入 Human Compose。未来添加任何新写信能力或证据字段，必须先建立独立 DTO、授权、错误、
状态语义与字段映射，不能借 D4D 的视觉改造暗中扩大范围。
