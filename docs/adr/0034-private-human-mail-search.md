# ADR 0034：Human 邮箱采用隐私优先的服务端搜索

- 状态：Accepted
- 日期：2026-07-27

## 背景

Human 邮件端需要搜索企业邮箱，但搜索词、主题、地址和预览都可能包含敏感业务信息。
搜索同时必须服从租户隔离、邮箱资源授权、审计治理和最低披露原则。浏览器不能直接接触
JMAP Account ID，也不能把查询词放进容易被代理和访问日志记录的 URL。

## 决策

新增 `POST /api/v2/mailboxes/{mailbox_id}/messages/search`。请求只接受有界文本、
可选的不透明 Cursor 和页大小。应用层在访问邮件数据平面前校验 Human 身份、
`mailbox.use`、邮箱 Read ABAC，以及数据库解析出的精确租户 JMAP Account 绑定。

适配器使用 JMAP `Email/query` 的 `text` 过滤，按最新优先查询；随后只用有界
`Email/get` 读取与普通收件箱相同的信封字段。Cursor 使用服务端签名，并绑定邮箱、
位置、查询状态和查询词 SHA-256 摘要；Cursor 不包含查询原文，且不能跨查询或邮箱复用。

成功结果只在 `mail.messages.queried` 审计事件和
`mail.messages.queried.v1` Outbox 原子提交后返回。持久证据仅包含邮箱、页位置、
页大小、结果数与是否有下一页，不保存查询词、查询摘要或任何邮件信封内容。

## 结果

- 查询词不会进入 URL、Cursor 或持久审计证据。
- 浏览器不能选择或伪造 JMAP Account。
- 搜索结果与普通收件箱共用最小化响应模型。
- 审计存储不可用时搜索 Fail Closed；JMAP 已执行但结果不会披露。
- 搜索目前不是全文索引服务，也不提供线程、附件内容或跨邮箱搜索。
