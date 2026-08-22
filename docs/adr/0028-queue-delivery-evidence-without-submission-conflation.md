# ADR 0028：队列投递证据与 Submission 状态分层

- 状态：Accepted
- 日期：2026-07-26

## 背景

CopilotMail 已能通过精确 `EmailSubmission.id` 读取 JMAP `deliveryStatus`，把每个
收件人的投递结果绑定到正式发送 Intent。这是 Human 和 Agent 可见投递状态的可靠
来源。

Stalwart WebHook 同时提供 `delivery.delivered`、`delivery.failed`、
`queue.rescheduled` 和 `delivery.completed`。这些事件继承队列 Span 的
`queueId`、Envelope From 等字段，适合排障、证明邮件引擎发生过什么，但没有携带
CopilotMail Submission UUID。用主题、收件人或时间窗口猜测两者关系既不唯一，
也会扩大敏感数据暴露面。

## 决策

采用双轨证据模型，不把队列事件伪装成 Submission 终态：

1. Human/Agent 可见状态只由精确 JMAP Submission 对账产生，继续写入不可变
   Submission 投递快照和外部允许的
   `message.delivery_status_changed.v1`。
2. Stalwart 队列事件写入独立的
   `outbound_mail_engine_events` 追加式账本，只用于内部运维证据。
3. 队列事件不接受调用方提供的租户。服务端规范化 Envelope From，并要求它精确
   对应唯一 `mailboxes.primary_address`，且该邮箱已有 Stalwart Account 绑定；
   无匹配或多匹配时整批 Fail Closed。
4. 当前不建立 `queueId → Submission` 关系，也不允许队列事件改写用户可见状态。
   将来只有邮件引擎通过正式协议返回不可伪造、可精确绑定到 Submission Intent 的
   标识时，才能另立 ADR 增加该关系。
5. 账本保留来源 Event ID、来源摘要、队列引用、类型、2xx SMTP Code、下一次重试
   时间和租户内邮箱。收件地址落库前转换为由 `queueId` 限定作用域的 SHA-256
   引用；Envelope From、远端响应文本、Reason、Hostname、正文和 Header 不写入
   Outbox 或日志。
6. `delivery.failed` 按当前固定 Stalwart 版本中“临时投递到期后转永久失败”的
   事件语义记录为 `permanent_failure`。版本升级必须重跑事件契约 PoC；若上游
   语义改变则 Fail Closed，而不是静默重分类。
7. 来源账本与最小化内部
   `mail_engine.delivery_trace.observed.v1` Outbox 在同一 PostgreSQL 事务提交。
   该内部事件不进入 Agent WebHook/Cursor 的外部允许列表。

## 后果

- 运维人员能够证明队列发生过投递、到期失败、重调度或完成，而不会把弱关联证据
  误报为某一用户邮件的最终状态。
- 租户归属不依赖收件人、主题或时间猜测，跨租户误归属默认拒绝。
- 同一收件人在不同队列中的摘要无法直接关联，远端诊断文本不会扩散到控制平面。
- 当前无法从队列账本直接跳转到某个 Submission；真实关联能力需等待上游提供稳定
  关联标识并单独评审。

## 不采用的方案

- 不用收件人、主题、Message-ID 或时间窗口做启发式关联。
- 不把 `queueId` 暴露给 Human/Agent 状态 API 或外部 WebHook。
- 不用队列 `delivery.completed` 推断所有收件人成功；队列完成可能同时包含失败。
- 不保存 SMTP response、Reason 或 Hostname 原文作为普通应用日志。
