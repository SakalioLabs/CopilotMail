# ADR 0031：Production Agent 发信策略边界

- 状态：Accepted（候选、激活与原子 Admission 已实现）
- 日期：2026-07-27

## 背景

Production Agent 外发可能造成批量误发、首次联系人骚扰、单地址轰炸和
Agent-to-Agent 自动回复循环。现有 development/testing Agent 发信链路具备
Draft、CAS、幂等 Submission 和邮箱 Send ABAC，但这些能力本身不能替代业务额度
与审批。

## 决策

1. Production Agent 只有在精确策略激活、草稿收件证据和同事务 Admission 均成立
   时才可进入 Worker Lease；任一证据缺失均默认拒绝。
2. 策略按 Agent 和单调版本追加，禁止 UPDATE、DELETE 与 TRUNCATE。
3. 每个候选版本必须由同租户 Active Human 为 Active production Agent 创建。
4. 收件域使用规范化 ASCII DNS 名白名单；空白名单、重复项、通配符和非规范域名
   均拒绝。
5. 单封收件人数、每小时发送量、单收件地址每日频率和每线程每日自动回复次数同时
   受可配置值与代码/数据库硬上限约束。
6. 首次联系人审批在当前安全基线中不可关闭。
7. 激活账本必须绑定精确版本和双人审批，并按 Agent 串行化、仅允许版本单调前移；
   应用服务将激活、Critical Audit 与 Outbox 原子提交。每次发送必须由数据库原子 Admission 消耗额度并写入不可变
   账本。只有这些证据齐备后才能修改三层默认拒绝。

## 当前非目标

- 本 ADR 不定义公司的实际额度数值。
- 附件、HTML 与显式自动回复 API 仍不开放；同一远端 Thread 的重复发送按更保守的
  自动回复熔断规则计数。
- 不以 API 进程内存计数替代跨副本 PostgreSQL Admission。
- 不把正文、完整地址或附件内容写入策略与额度账本。

## 后续验收

- 创建 API 由服务端在 Agent 事务锁下分配单调版本；查询 API 在返回前提交自身
  Audit/Outbox；激活 API 只接受精确版本和审批 ID。
- Draft 保存时持久化最小化的规范收件域及不可逆地址摘要。
- Submission 创建时原子执行域白名单、首次联系人、小时额度、单地址频率和自动回复
  熔断检查。
- Worker 只租赁持有已提交 Admission 的 production Agent Intent。
- 并发 PostgreSQL 测试证明超额时只有限额内请求可提交，拒绝不会消耗部分计数。
