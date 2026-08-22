# ADR 0036：Mail Engine Account 永久归属首个租户邮箱

- 状态：Accepted
- 日期：2026-07-27

## 背景

Human 和 Agent 内容 API 会先在 PostgreSQL 中把安全 Message UUID 解析为当前
Mail Engine Account 与 Document，再通过有界 JMAP 请求读取正文或修改展示状态。
邮箱绑定允许使用版本 CAS 从 Account A 轮换到 Account B。

如果 A 在一次已授权 JMAP 请求完成前被重新分配给另一邮箱或租户，仅靠“查询时匹配
当前绑定”会形成 TOCTOU：请求仍可能访问 A，但 A 已代表另一资源。跨网络持有数据库
行锁可以阻止换绑，却会让每次正文访问长时间占用事务与连接，并把数据平面延迟传递给
管理面。

## 决策

新增追加式 `mail_engine_account_owners`：

1. `(source, account_ref)` 第一次绑定时原子认领唯一
   `(organization_id, mailbox_id)`；
2. 当前邮箱可以从 A 轮换到 B，A 的历史归属永久保留；
3. A 此后只能再次绑定到原租户、原邮箱，跨邮箱或跨租户重分配以 `23514`
   Fail Closed；
4. 升级迁移在同一事务中先取得绑定表写锁，再同时从当前绑定和不可变成功审计恢复
   现有及已退休 Account；若同一 Account 的历史归属不唯一，迁移整体回滚并要求人工
   调查，不会猜测所有者；
5. 锁一直持有到绑定 INSERT/UPDATE 触发器在提交时可见，避免旧实例写入落在回填与
   触发器安装之间；触发器负责此后的并发安全认领和复核；
6. 归属表拒绝 UPDATE、DELETE 与 TRUNCATE，错误码为 `55000`；后续迁移再次校验
   历史证据并追加修复缺失 owner，同时用延迟约束触发器要求每个新 owner 在事务提交
   时具有精确 current binding，孤立直接 INSERT 和失败 CAS 均以 `23514` 回滚；
7. readiness 精确核对 owner 表主键、四个触发器的启用状态、时机/事件类型、
   UPDATE OF 列集合、延迟属性、函数绑定及 current binding 到 owner 的完整覆盖；
8. JMAP 凭据撤销、Account 停用和数据清理由 Mail Engine 运维流程负责，但不得通过
   将旧 Account 重新分配给其他租户来复用身份。

## 结果

- 已解析的 Account 即使与一次并发绑定轮换交错，也不会变成另一租户或邮箱的资源，
  因而 Human GET/PATCH、Agent 内容读取、草稿回复和提交执行共享同一永久隔离边界。
- 当前绑定查询仍会在正常情况下拒绝旧 Account；永久归属是竞态期间的纵深保护，
  不是继续使用退役绑定的功能。
- Account 标识不能跨邮箱回收。容量规划与 Mail Engine 生命周期管理必须把 Account
  视为租户邮箱作用域的永久身份。
- `0050 -> latest` 真实升级门禁覆盖退休 Account 回填、歧义历史整体回滚、
  跨租户/跨邮箱拒绝、孤立 INSERT 提交回滚、正常首次绑定与轮换，以及破坏任一
  关键触发器或 current-binding coverage 后的 readiness Fail Closed。
- 未选择在 JMAP 网络调用期间持有 PostgreSQL 事务，避免连接池耗尽、绑定管理长时间
  阻塞及外部超时扩大数据库故障域。
- 数据库运行时凭据仍属于受信任基础设施；生产 GA 需继续按最小权限分离 migration
  owner 与 application role，避免把任意 SQL 执行能力误当作普通应用权限。
