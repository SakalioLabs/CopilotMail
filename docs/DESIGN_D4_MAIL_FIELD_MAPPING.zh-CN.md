# D4：Human Mail Shell 真实字段映射

> 状态：**已冻结 D4 首个实现切片的可见性与安全边界。** 本文只覆盖 Human Mail 的
> scope rail、Inbox queue 与阅读区层级；不声明邮件端、Agent Client Surface 或所有 D4
> 子项已完成。没有列出的字段默认不渲染。

## 1. 切片目标

将现有邮件端收敛为清晰的“授权范围 → 邮件队列 → 阅读 / 写信”工作台：桌面端优先让
阅读区获得可读宽度，小屏继续采用显式列表—详情栈。它是布局与信息层级切片，不新增 API、
权限、路由、缓存、浏览器存储、轮询或预取。

## 2. 已确认字段与禁止推断

| 区域 | 现有来源 | 本切片可显示 | 绝不显示或推断 |
| --- | --- | --- | --- |
| Scope rail | `listMailboxes` 的 `mailbox_id`、`kind`、`primary_address`、`can_send` | 当前已授权邮箱地址、Human/Agent/Shared/Group 身份文字与图标、服务器给出的可发送/只读事实、切换器 | Owner、委托、权限集合、Agent 在线/运行状态、组织名称、邮箱统计、邮箱 UUID |
| Inbox queue | `listInbox` / `searchInbox` 当前页的 `items` 与 `has_more` | 当前页数量（不是总量）、发件人、主题、接收时间、预览、未读/星标/附件的真实标记、显式加载更多 | 邮件正文、风险、优先级、投递状态、未读总数、跨页总数、消息 UUID |
| Reading | 只在显式选中后 `getMessage` 返回的受控 `MessageDetail` | 主题、参与者、接收时间、安全纯文本、截断提示、附件名称/类型/大小、服务端返回的可回复关系 | HTML、Provider/JMAP ID、Blob ID、附件内容、线程推断、已接受即已投递 |
| Drafts | `listHumanDrafts` 的 metadata-only 项 | draft ID 的内部引用、版本、收件人数、创建/更新时间、恢复动作 | 主题、正文、地址、Provider 标识、任何未显式恢复的内容 |
| Sent | `listHumanSentSubmissions` 的 metadata-only 项 | `accepted_at`、收件人数、可选聚合 delivery 事实 | 收件人地址/标识、正文、单收件人投递、`accepted` 被写成已送达 |
| Compose / submission | 已有 Draft、Submission、Delivery 状态机与精确 Evidence Inspector | 不可编辑发件身份、收件人/正文、草稿状态、accepted/reconciling/manual_review/failed/unknown 的真实文案、服务端返回证据 | 自动写信、推测投递进度、无限重试、在未明确提交时展开敏感证据 |

`kind="agent"` 只说明目录返回的邮箱身份种类，绝不等同 Agent 的执行、授权、策略、
委托或健康状态。`can_send=false` 只说明当前邮箱只读，不能由前端推导其他权限。

## 3. 读写与异步边界

- `listMailboxes`、`listInbox`、`searchInbox` 与 `getMessage` 都保持既有调用时机；Shell
  改造不得触发 hover 预取、初始详情读取、后台刷新、自动重试或额外的目录分页。
- 邮件切换、视图切换与详情请求的代际隔离、Abort、精确 mailbox 绑定、CAS 草稿和 unknown
  submission 冻结都是现有安全契约，D4 不能降低它们。
- Inbox 与 Search 的当前页数量只能表述为当前响应范围。`has_more=true` 时不得显示伪总数。
- Drafts/Sent 进入宽列表时不保留一个空详情栏；这不是“没有选择”，而是对应 API 没有可安全
  展示的详情模型。

## 4. Shell 版式冻结

| 宽度 | 目标 | D4 首切片约束 |
| --- | --- | --- |
| ≥1084px | 完整 Scope rail 224px；Queue 328–400px；Reading 至少 520px | 不新增 Inspector；证据继续只在已有 Composer 状态下按需展开 |
| 1008–1083px | 仍有文字与完整事实的紧凑 Scope rail 160px；Queue 328px；Reading 至少 520px | 不使用无标签图标 rail；长主题和参与者优先保留在 Reading 区 |
| ≤1007px | 显式列表—详情栈 | 保留 mobile toolbar、返回焦点与 44×44px 关键控件；范围事实可按需展开，但地址、身份、服务端可发送性在未展开时仍可见；不因视觉改造新增读请求 |

## 5. 首个实现切片

1. 将完整桌面 grid 收敛为 224px rail + 328–400px queue + 至少 520px Reading；1008–1083px 用 160px 的紧凑文字 rail 守住同一阅读下限，≤1007px 直接转栈。
2. 将 rail 的 Inbox / Sent / Drafts 呈现为同一组可扫描的、具文字和原创 SVG 的真实视图入口。
3. 将移动端低频范围/会话控件收进原生按需披露，但 summary 仍显示地址、身份和服务器可发送性；保留现有 Composer、Command Palette、所有状态机和 API 调用。
4. 将唯一真实的草稿/提交状态放在 Composer footer、紧邻发送或确认同一提交的动作；不复制状态。
5. 不创建“邮件健康度”“待处理数量”“Agent 工作状态”或第四 Inspector 占位栏。

## 6. 必过的验证

1. 现有 Mail 单元测试：邮箱范围、只读发送拒绝、详情 Abort、草稿 CAS、unknown submission、
   keyboard shortcuts 与 session revalidation 保持通过。
2. 浏览器：1440、1280、1024、768、390px；桌面长主题、mobile stack、Compose、语言切换、
   44px 控件、键盘和无横向溢出保持回归。
3. 字段契约：Shell 不新增 API request；Draft/Sent 不因布局显示正文/地址；Inbox 不虚构总数。
4. 视觉快照逐组人工审图，确认 rail、queue 与 Reading 的层级来自真实数据，而不是色块或
   占位符设计。

## 7. 与 V2 规格的关系

本文是 [`DESIGN_DEVELOPMENT_SPEC_V2.zh-CN.md`](DESIGN_DEVELOPMENT_SPEC_V2.zh-CN.md) 中
**D4 Mail Shell 的首批可实现交付物**。后续若要新增 Inspector、投递视图、协作/Agent UI 或
新的邮件事实，必须先有独立 DTO、权限边界、错误状态和字段映射；不能借本表扩大数据披露。
