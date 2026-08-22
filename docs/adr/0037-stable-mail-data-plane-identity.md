# ADR 0037：稳定邮件数据平面身份与来源亲和

- 状态：Accepted
- 日期：2026-08-13

## 背景

控制平面会长期保存邮件数据平面返回的不透明 Account、Domain、Email、Submission
与队列引用。原有实现以 `stalwart` 这类供应商字符串作为来源标识，无法区分同一供应商
的两个独立实例；如果管理、内容、写信和提交凭据实际指向不同集群，数值相同的远端引用
还可能发生碰撞或被发送到错误实例。

供应商名称描述实现类型，不足以证明一个引用属于哪个持久命名空间。调用方也不能通过
请求字段选择数据平面，否则租户内的已授权操作可能被重定向到未经治理的实例。

## 决策

1. 领域使用不可为空、不可为 Nil 的 UUID `MailDataPlaneId` 标识一个稳定远端引用
   命名空间。该 ID 不等于供应商名称，也不因凭据轮换、URL 路径调整或进程重启而变化；
   重建或迁移为新的引用命名空间必须分配新 ID。
2. 供应商类型只存在于 adapter 与 composition 边界。Application 定义的 Mail ports
   由 adapter 自报 `MailDataPlaneId`，业务用例不以 `stalwart` 字面量选择实现。
3. 启用任一 Mail 能力时，部署必须同时显式配置
   `COPILOTMAIL_MAIL_DATA_PLANE_ID` 与
   `COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN`。管理、内容、写信和提交 adapter 的 Base URL
   必须与该 canonical origin 精确同源；缺失、Nil UUID、跨源或带歧义的配置均在启动
   阶段 Fail Closed。
4. 邮箱绑定 HTTP 请求只接受 `account_ref` 与 `expected_version`，不能提交 vendor、
   `source` 或 `data_plane_id`。服务端在认证授权后注入当前部署身份；响应返回
   `data_plane_id`，让操作者能够审计绑定属于哪个命名空间。
5. 所有持久远端引用都与 `MailDataPlaneId` 一起保存。读取、草稿、Submission、投递
   对账和事件接入在任何远端网络调用前同时按 ID 查询并再次检查来源亲和；不匹配时
   Fail Closed，不能把一个实例的引用提交给另一个实例。
6. Domain 在激活事务内、任何远端 I/O 之前预分配 `MailDataPlaneId`；远端引用可以暂时
   为空，但一旦写入，数据平面 ID 与远端 Domain 引用均不可原地清空或换绑。Outbox
   envelope 保存强类型路由，Worker 只能领取全局事件及与自身 ID 精确相同的邮件事件。
7. Migration 0053 以追加式注册表登记数据平面 UUID 与不可变 canonical origin，为既有
   Stalwart 证据回填固定 legacy-primary ID，并把新的唯一性与路由键扩展为
   `(mail_data_plane_id, remote_ref)`。旧 `source` 列仅作为历史/回退兼容证据，不再承担
   路由身份。0053 是停写升级：必须停止旧 API/Worker、完成备份和迁移，再原子切换全部
   新二进制；不得让旧、新二进制并跑，也不宣称滚动升级。
8. 固定 legacy-primary UUID 继续使用历史 `source=stalwart` 的 v1 指纹字节布局，覆盖
   入站/出站来源回执、队列投递收件人引用和 JMAP 投递收件人/快照摘要，以保持升级前后
   的幂等重放与去重结果不变。只有该精确 UUID 可以选择 legacy scheme；其他数据平面
   必须使用带版本化 domain prefix 与 `MailDataPlaneId` 的 v2 指纹，使相同远端引用在
   不同平面不会碰撞。scheme 不从请求或可选 payload 字段推断。
9. 邮箱绑定在同一事务发布 routed `mail_engine.mailbox_binding.updated.v1` 兼容事件与
   `mail_engine.mailbox_binding.updated.v2` 强类型事件。v1 保持历史 `source` payload，
   v2 在 payload 中携带 `data_plane_id`，两条 Outbox envelope 都使用同一个强类型
   `mail_data_plane_id` 路由。双发只给下游消费者迁移契约，不放宽 0053 禁止旧、新
   API/Worker 混跑的要求。
10. 当前仓库仍只有 Stalwart v0.16 adapter。接入另一供应商或第二个独立实例必须新增
   adapter、通过受治理的注册/迁移流程分配 ID，并完成相同的隔离、协议和恢复验收；
   本决策不代表多供应商已经可用。

## 结果

- 同一供应商的不同实例拥有不同引用命名空间，Account 或 Document 值相同也不会因此
  合并；ID 进入绑定、所有权、消息投影、草稿、Submission 和投递证据的亲和判断。
- 管理、Content、Compose 与 Submission 配置不能再分别漂移到不同 origin；凭据仍按
  最小权限分离，但必须属于同一个已声明数据平面实例。
- `MailDataPlaneId` 不是 Secret，但属于持久身份。原地改值会让既有引用失去亲和，必须
  走显式迁移，而不能作为普通配置轮换。
- HTTP 客户端不再了解 `source=stalwart`；供应商替换不会要求公开业务契约增加新的
  vendor 枚举。
- Migration 0001→0053 与针对数据平面身份、Origin、Domain、Outbox 的直接 SQL
  不变量验证已在 disposable PostgreSQL 17 实际执行。该证据不等同于完整 ignored
  PostgreSQL 套件、目标 Pilot 升级、回滚或故障恢复验收。
