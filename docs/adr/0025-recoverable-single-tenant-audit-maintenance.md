# ADR 0025：可恢复的单租户审计维护进程

- 状态：Accepted
- 日期：2026-07-26

## 背景

审计 Checkpoint、离线导出与 S3 Object Lock/WORM 归档已经具备单次执行能力，但生产系统不能依赖人工定期拼接多个命令。远程对象上传与 PostgreSQL 证据提交之间也不存在分布式原子事务；进程可能在任意一步退出，密钥还可能在遗留任务完成前轮换。

通用 Outbox Worker 负责短周期业务事件投递，不应持有审计签名私钥，也不应承担大文件导出与长期归档职责。

## 决策

提供独立的 `audit-maintenance` 与 `audit-maintenance-worker` 运行模式。每个实例只绑定一个 Organization、一个专用 Active Service Principal、一组审计签名 Key 和一个持久工作目录。Service Principal 必须在每个周期实时通过 `audit.export` Scope/RBAC 校验。

每个周期优先恢复链序最早、已签名但尚无 `audit_archives` 证据的 Checkpoint。只有不存在遗留任务时，才验证当前完整哈希链并创建新 Checkpoint。Checkpoint ID 由租户、链头和 Key ID 稳定派生为 UUIDv8；Archive ID 由租户和 Checkpoint 稳定派生。因此同一逻辑任务跨进程重启仍使用相同身份。

每次 Worker 调度生成临时 UUIDv7 `cycle_id`，用于区分锁竞争和本次执行日志；持久化 `trace_id` 则由 Organization/Checkpoint 稳定派生为 UUIDv8。Checkpoint 审计、导出日志、S3 两对象 metadata、归档审计和最终结果使用同一个稳定 `trace_id`。重试继续使用相同值，避免条件创建对象因 metadata 改变而失去幂等性。

完整 JSONL 与 Manifest 在持久目录中生成，并在上传前离线验签。损坏或不完整的包先隔离再重建；已经完整且与数据库 Checkpoint 一致的包直接复用。对象存储按稳定 Key 条件创建并精确回读，PostgreSQL 归档证据提交成功后才清理对应本地包。

若最早遗留 Checkpoint 的 Key ID 或公钥指纹与当前挂载 Key 不一致，Worker 必须 Fail Closed。运维人员应先恢复旧 Key 完成该任务，不能跳过它为更新链头签名。

每次周期开始前，进程使用由 Organization ID 稳定派生的 PostgreSQL 会话级 advisory lock 竞争该租户的单活权。锁跨越导出和对象存储 I/O，但不持有数据库事务或 MVCC 快照。竞争失败的常驻 Worker 跳过本周期，单次命令明确失败。

正常完成后显式解锁；任务失败、取消、panic 或解锁失败时，Guard 将连接标记为不可回池并关闭会话，由 PostgreSQL 释放锁。因此连接池不会把仍持锁的 Session 借给其他业务。锁只协调连接到同一个 PostgreSQL 集群的实例，跨数据库部署仍必须由外部调度器隔离。

## 后果

- 远程上传成功、数据库提交失败等分裂故障可以精确重放，不会以新身份生成重复归档。
- 审计私钥与通用 API/Outbox Worker 隔离，运行身份可执行最小权限配置。
- 持久工作卷属于恢复状态，不能用临时目录替代，也不能被多个租户共享。
- 旧 Key 遗留任务会阻止新链头归档，这是有意的证据连续性保护。
- 结构化日志可区分锁冲突、连续失败与周期结果；独立 HTTP 监听提供 Prometheus 周期/失败/锁冲突/时效指标及健康、就绪端点，连续失败、未归档超龄和周期超时均使用明确阈值 Fail Closed。
- 本 ADR 落地后已追加 OTLP exporter、W3C HTTP Span Context 与稳定业务 Trace
  关联；跨数据库选主、HSM/KMS 和邮件内容 Legal Hold 仍需后续实现。

## 不采用的方案

- 不把维护逻辑塞入通用 Outbox Worker：会扩大私钥暴露面，并让长任务干扰业务事件投递。
- 不在失败后创建新 Checkpoint 或 Archive ID：会破坏精确幂等恢复并产生含义重复的对象。
- 不把对象上传成功直接视为归档完成：只有远程回读证据和 PostgreSQL 证据都完成才算成功。
- 不自动跳过使用旧 Key 的遗留 Checkpoint：这会在证据链中制造无法解释的归档缺口。
