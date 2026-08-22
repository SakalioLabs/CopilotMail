# 显式邮件回复语义

## 目标

Human 与 Agent 都只能使用 CopilotMail 入站事件生成的 Message UUID 表达“回复这封
邮件”。API 不接受 `Message-Id`、`In-Reply-To`、`References`、JMAP Email ID 或
Thread ID，避免调用者伪造会话关系或跨邮箱引用。

## 授权与解析

保存回复草稿同时要求 `mailbox.use`、邮箱 `Send` 和 `Read` 能力。服务端按
`organization_id + mailbox_id + message_id` 查询不可变入站记录，并要求其
`source + account_ref` 与邮箱当前绑定完全一致；查不到、跨租户、跨邮箱和旧绑定统一
隐藏为不可访问。

Human 邮件详情仍使用数据平面的短期内容凭据读取。控制平面只在对应入站投影已提交
且匹配当前绑定时返回 `reply_to_message_id`；投影尚未到达时该字段为 `null`，UI 不
展示回复入口。Agent 直接复用 Feed/Claim 中的 Message UUID。

## JMAP 与数据库证据

邮件引擎适配器使用解析出的不透明 Email ID 执行一次有界 `Email/get`，只读取
`id`、`messageId`、`references`。源邮件必须精确命中一条、只有一个合法
`messageId`，引用链不超过 100 项且总长度不超过 16 KiB。适配器去重后写入新草稿的
`inReplyTo` 与 `references`；线程 ID 始终由服务端邮件引擎决定。

Migration 0046 将安全 `reply_to_message_id` 通过租户/邮箱复合外键绑定到入站事件。
它被纳入草稿内容 SHA-256，且 PostgreSQL 触发器禁止在草稿替换、冻结和发送转换中
换绑。远端 Email/Thread 标识不会通过草稿 API 或该证据列暴露。

## 当前边界

当前只支持回复已形成可信入站投影的邮件；不支持回复“已发送”列表中的邮件，也不
自动包含原文或附件。HTML、附件安全和完整会话 UI 仍属于后续 P4/P5 工作。
