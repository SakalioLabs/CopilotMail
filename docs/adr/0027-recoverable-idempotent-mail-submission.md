# ADR 0027：可恢复且幂等的正式邮件提交

- 状态：Accepted
- 日期：2026-07-26

## 背景

正式发信跨越 PostgreSQL 控制平面与 JMAP EmailSubmission 数据平面，不存在分布式
事务。若 JMAP 已接受提交，而 API 在保存本地结果前断电，直接重试
`EmailSubmission/set` 可能重复投递。仅在远端成功后写入通用
`idempotency_keys` 不能封闭这个窗口。

JMAP 草稿正文不可变。发送请求还必须绑定一个已经停止编辑的精确草稿版本、授权邮箱
派生的 From、服务端选择的 Identity，以及调用者和租户；任何调用者提交的引擎内部
Email ID 都不能直接成为删除或发送依据。

## 决策

正式发送采用持久化 `Intent → Reconciling → Accepted/Rejected` 状态机；租约是
非终态上的正交并发控制字段，不是会掩盖“远端是否已经尝试”的业务状态。
PostgreSQL 中的 Intent 是唯一业务真相，JMAP Submission 是需要对账的远端效果。

### 身份与幂等

- 客户端必须提供 128 位以上随机 `Idempotency-Key`；数据库只保存 Key 的
  SHA-256，不保存明文。
- 唯一键为 Organization、Principal、`mail.submit` 和 Key Hash。
- Intent 同时保存规范化请求 Hash。相同 Key 与相同请求只返回同一 Intent；相同
  Key 与不同请求永久冲突。
- 请求只引用 CopilotMail Draft ID 和版本，不接受 JMAP Email ID、Identity ID 或
  From。
- Intent 创建时重新校验 Active Human、RBAC、Token Scope、Mailbox Send ABAC、
  租户和草稿所有权，并冻结该草稿版本。共享邮箱的 SendAs/OnBehalfOf 继续经过显式
  能力与审批边界。

### 状态与租约

- `pending`：Intent 已持久化，尚无远端尝试；可由一个 Worker 持有有界租约。
- `reconciling`：发生过远端调用或响应不确定，后续只能先查询，不可盲目再次创建。
  该状态同样可被租约保护，接管租约不会把状态降回 `pending`。
- `accepted`：保存精确 JMAP Submission ID、接受时间和最终状态；终态不可逆。
- `rejected`：只用于确定性的永久拒绝，保存稳定非敏感错误码；终态不可逆。

租约 ID、到期时间和尝试次数一起写入。租约过期后可由另一 Worker 接管。旧租约的
完成写入必须使用租约 ID 比较并交换，
不能覆盖接管者状态。

### 远端执行与恢复

1. Worker 取得租约后，先用冻结的 JMAP Email ID 查询 `EmailSubmission/query`，
   再以 `EmailSubmission/get` 读取候选。
2. 找到唯一且 Identity、Email 与 Intent 精确一致的 Submission 时，
   直接把 Intent 对账为 `accepted`。
3. 找到零条时才调用 `EmailSubmission/set`；每次租约重试仍必须重新从步骤 1
   开始，不能直接重放上一次 create。
4. 调用前先把 Intent 置为 `reconciling` 并提交数据库事务。网络超时、连接中断或
   进程退出后，接管者只能回到步骤 1。
5. 找到多个候选或候选字段不一致时 Fail Closed，进入人工调查状态，不能自动再发。

JMAP 查询必须使用精确 Account。Identity 由服务端 `Identity/get` 中唯一匹配授权
邮箱主地址的记录选择；找不到或不唯一都拒绝。发送凭据与内容、草稿及管理凭据分别
配置，只授予 Identity/特殊邮箱读取、EmailSubmission 查询/读取/创建，以及
`onSuccessUpdateEmail` 清除 `$draft`、从 Drafts 移到 Sent 所需的 Email 更新
权限。客户端不构造 Envelope，由服务器从冻结 Email 的 Header 派生。隐式
`Email/set` 失败时 Submission 可能已经成功，因此只能记为结果不确定并重新对账，
不能写 `rejected`。

### 事务与证据

- Intent 创建、草稿冻结、`mail.submission.requested` 审计事件和 Outbox 必须在
  同一 PostgreSQL 事务提交。
- `accepted`/`rejected` 状态、对应审计事件和版本化 Outbox 也必须原子提交。
- 审计只保存 Intent、Mailbox、Draft、Actor、策略/审批与稳定结果，不保存主题、
  地址、正文、附件、Envelope、Token 或引擎响应。
- HTTP 首次请求返回 `202 Accepted` 与稳定 Intent ID；客户端通过状态资源查询，
  不把 API 请求时长绑定到公网投递。

## 后果

- 远端成功、本地响应丢失后可以查询并收敛，不会因普通重试再次投递。
- 发送延迟包含一次持久化和可能的远端对账，但用户可立即关闭写信窗口。
- CopilotMail 草稿元数据映射、内容摘要、版本 CAS 与冻结表已经由 Migration 0030
  落地；发送应用服务只使用该映射，不接受 JMAP 草稿 ID。
- Migration 0031、独立 JMAP Submission adapter、租约 CAS、Outbox Worker、
  Human API 与写信界面已经实现本决策；SMTP 最终投递/退信是后续独立状态。
- JMAP 服务若不能按 Email ID 稳定查询 Submission，PoC 必须判定该实现不满足
  正式发送要求，并在 adapter 边界切换方案。

## 不采用的方案

- 不在 JMAP 成功后才写 `idempotency_keys`：崩溃窗口会产生重复邮件。
- 不在一个 PostgreSQL 事务中等待 JMAP：长事务仍不能提供跨系统原子性。
- 不用主题、收件人或时间窗口猜测是否已发送：这些字段不唯一且会泄露内容。
- 不允许客户端提交 From、Identity ID 或引擎 Email ID：这会绕过邮箱授权与租户
  绑定。
- 不把网络超时当作失败后立即重发：超时表示结果未知，必须进入对账。
