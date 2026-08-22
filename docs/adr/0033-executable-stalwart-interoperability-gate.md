# ADR 0033：可执行的 Stalwart 互操作与持久性门禁

- 状态：Accepted
- 日期：2026-07-27

## 背景

仅启动 Stalwart 容器和探测管理 JMAP，不能证明普通邮件账号能够安全使用 SMTP
Submission、IMAP 与 JMAP Mail，也不能证明邮件在进程重启后仍然存在。人工测试如果
没有唯一消息标识和机器可读结果，也无法作为可复核的选型证据。

## 决策

项目保留一个只依赖 Python 标准库的外部烟雾测试 `tests/stalwart_poc.py`，并将
测试账号初始化描述为可重复执行的 Stalwart CLI `upsert` 计划：

1. Relay、Submission 与 IMAP 必须使用三个不同的 listener。
2. Relay 与 Submission 必须支持 STARTTLS；Relay 不公布 AUTH，Submission 在
   STARTTLS 后必须公布 AUTH。
3. 未认证的外部到外部 RCPT 必须被拒绝，防止开放中继。
4. 普通账号必须能发现 JMAP Mail，并成功执行 `Mailbox/get`。
5. SMTP AUTH 提交的唯一邮件必须能通过另一个普通账号以只读 IMAP 精确读回。
6. 持久性验收分为 `send` 和 `verify` 两步，中间由操作者明确重启 Stalwart；不允许
   单进程脚本把“未发生重启”误报为持久性通过。

凭据只从环境变量读取。落盘 receipt 仅含版本、Message-ID、随机 marker、两端地址和
发送时间；读取时必须与当前配置精确绑定。JMAP 响应、IMAP 邮件和等待时间均有硬上限，
JMAP 重定向不得改变 scheme 或 origin。

## 验收证据

- 默认 CI 运行两个不访问外部服务的自测，约束计划结构、引用、Secret 所在字段、
  参数拒绝和 receipt 绑定。
- 外部 PoC 报告必须附上 `preflight`、`send`、容器重启和 `verify` 的命令结果，以及
  对应 Stalwart 版本和存储配置。
- 未实际执行外部 PoC 时，实施状态必须继续标记 P3 为“进行中”。

## 后果

本决策让协议互操作和重启持久性成为可重复、可审计的验收步骤，但不覆盖公网 MX、
250 后断电、队列重试/DSN、垃圾邮件防护、真实 TLS/DNS 或备份恢复。上述项目仍需
独立演练和签字。
