# 审计导出 S3 Object Lock / WORM 归档

> 状态：V2 可执行基线，2026-07-26。生产启用前仍需由合规、法务和对象存储管理员确认保留期、权限策略与 Legal Hold 流程。

## 完成语义

单次 `PUT` 成功不等于 WORM 成功。CopilotMail 只有同时取得以下证据才记录归档完成：

1. Bucket 明确返回 Versioning `Enabled`；
2. Bucket 明确返回 Object Lock `Enabled`；
3. 上传使用 `If-None-Match: *`，避免同一 Key 被并发静默覆盖；
4. 请求携带 SHA-256 Checksum、逐对象 Retention Mode 和 Retain Until；
5. 上传后按精确 Version ID 执行 `HEAD`；
6. 回读的 Version ID、Checksum、长度、业务 Metadata、Retention Mode 和 Retain Until 全部吻合；
7. 事件对象和 Manifest 都通过后，PostgreSQL 才追加归档证据、关键审计事件与 Outbox。

数据库记录不是远程存储的替代品，也不宣称跨 PostgreSQL/S3 原子提交。远程成功而数据库失败时，使用同一 `archive-id` 重试；条件写与精确回读会复用原 Version，随后补录数据库证据。

## 配置

必填：

```text
COPILOTMAIL_AUDIT_ARCHIVE_REGION=us-east-1
COPILOTMAIL_AUDIT_ARCHIVE_BUCKET=copilotmail-audit
COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_DAYS=2555
COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_MODE=compliance
```

S3 兼容服务可设置：

```text
COPILOTMAIL_AUDIT_ARCHIVE_ENDPOINT=https://s3.company.example
COPILOTMAIL_AUDIT_ARCHIVE_FORCE_PATH_STYLE=false
COPILOTMAIL_AUDIT_ARCHIVE_ALLOW_INSECURE_HTTP=false
```

生产环境强制 `compliance`，拒绝 HTTP。Retention Days 范围为 1–36500。凭据不使用 CopilotMail 专有变量，直接采用 AWS SDK 标准凭据链；生产优先使用实例角色、工作负载身份或短期 STS 凭据，不把长期密钥加入镜像、Git、命令行或日志。

开发 Compose 自动创建启用 Object Lock 的 MinIO Bucket，使用 Governance 和 30 天保留，便于清理测试数据。Governance 不是生产合规默认值。

## 执行

先创建、导出并独立验证 Checkpoint，然后执行：

```shell
copilotmail-control archive-audit-export \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --checkpoint-id "$CHECKPOINT_ID" \
  --archive-id "$ARCHIVE_ID" \
  --export-directory ./audit-export \
  --public-key-file ./trusted-audit-public.pem
```

调用 Principal 必须实时拥有目标租户的 `audit.export`。事件对象使用 Checkpoint 中已签约的稳定 Key；Manifest 使用该 Key 加 `.manifest.json` 后缀。保留截止时间由 Manifest 的 `exported_at + retention_days` 确定，因此重试不会延长或缩短请求语义。

## 定时维护 Worker

生产建议为每个租户部署一个隔离的维护实例，绑定专用 Active Service Principal，
且该 Principal 只获得目标租户的 `audit.export`：

```text
COPILOTMAIL_AUDIT_MAINTENANCE_ORGANIZATION_ID=<organization-uuid>
COPILOTMAIL_AUDIT_MAINTENANCE_ACTOR_PRINCIPAL_ID=<service-principal-uuid>
COPILOTMAIL_AUDIT_MAINTENANCE_KEY_ID=audit-2026-01
COPILOTMAIL_AUDIT_MAINTENANCE_PRIVATE_KEY_FILE=/run/secrets/audit-private.pem
COPILOTMAIL_AUDIT_MAINTENANCE_PUBLIC_KEY_FILE=/run/secrets/audit-public.pem
COPILOTMAIL_AUDIT_MAINTENANCE_OBJECT_PREFIX=audit
COPILOTMAIL_AUDIT_MAINTENANCE_WORK_DIRECTORY=/var/lib/copilotmail/audit-maintenance
COPILOTMAIL_AUDIT_MAINTENANCE_INTERVAL_SECONDS=86400
COPILOTMAIL_AUDIT_MAINTENANCE_METRICS_ADDR=0.0.0.0:9090
COPILOTMAIL_AUDIT_MAINTENANCE_FAILURE_THRESHOLD=3
COPILOTMAIL_AUDIT_MAINTENANCE_MAX_PENDING_AGE_SECONDS=172800
COPILOTMAIL_AUDIT_MAINTENANCE_MAX_CYCLE_SECONDS=21600
```

单次执行和常驻调度分别为：

```shell
copilotmail-control audit-maintenance
copilotmail-control audit-maintenance-worker
```

维护周期执行以下状态机：

1. 实时确认 Actor 仍属于目标租户并拥有 `audit.export`；
2. 优先读取最早的“已签名但没有 `audit_archives` 证据”的 Checkpoint；
3. 没有遗留任务时，独立验证完整哈希链，并按租户、链头和 Key ID 生成稳定 UUIDv8
   Checkpoint ID；
4. 在持久工作目录生成完整 JSONL/Manifest，发布前再次离线验签；
5. 使用由 Organization/Checkpoint 派生的稳定 Archive ID 上传及精确回读；
6. PostgreSQL 归档证据提交后才清理该 Checkpoint 的本地工作目录。

若进程在远端上传后、数据库提交前崩溃，下次运行会优先恢复同一个 Checkpoint，并
复用完全相同的 Manifest、Archive ID 和对象键。导出过程中崩溃产生的不完整目录会
被改名隔离，再从数据库签名 Checkpoint 重新导出；不会上传半成品。

Key 轮换时必须先让旧 Key 的最早未归档 Checkpoint 完成；若 Worker 发现遗留
Checkpoint 的 Key ID 或公钥指纹与当前挂载 Key 不同，会 Fail Closed 并持续输出
结构化错误，不会跳过它签署更新链头。部署告警应监控连续失败计数和周期完成日志。

Docker 开发栈提供可选 `audit-maintenance` profile：

```shell
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml --profile audit-maintenance up -d
```

需要事先放置两份密钥文件并填写租户/Actor UUID。一个实例只负责一个租户；多个
租户应使用独立容器、Secret、工作卷和最小权限身份，不能共享私钥目录。同一
PostgreSQL 集群内，相同租户的实例通过会话级 advisory lock 竞争单活权；失败的
常驻实例跳过本周期。异常退出会关闭持锁连接，由 PostgreSQL 自动释放锁，不会在
连接池中遗留锁。跨数据库部署不受该锁协调，仍须由外部调度器保证隔离。

### 健康与 Prometheus 指标

常驻 Worker 在独立监听地址提供：

- `/healthz`：进程存活；
- `/readyz`：连续失败达到阈值、最早未归档 Checkpoint 超龄或单次周期超时则返回
  `503` 及稳定原因码；
- `/metrics`：Prometheus 文本格式，只使用 Organization UUID 标签，不包含邮件内容、
  密钥、对象路径或错误详情。

核心指标包括周期成功/失败/锁冲突计数、连续失败数、周期执行状态与开始时间、最后
完成/成功时间及最早未归档 Checkpoint 年龄。建议至少配置以下告警：

```promql
copilotmail_audit_maintenance_consecutive_failures >= 3
copilotmail_audit_maintenance_pending_checkpoint_age_seconds > 172800
copilotmail_audit_maintenance_cycle_in_progress == 1
  and time() - copilotmail_audit_maintenance_cycle_started_timestamp_seconds > 21600
```

锁冲突本身不是失败；多副本部署中被动实例正常跳过周期。若所有实例长期没有成功
时间戳，或未归档年龄持续增长，则应按漏归档事件升级处理。

### 追踪关联

每次调度日志包含临时 UUIDv7 `cycle_id`；选定 Checkpoint 后，Worker 从
Organization/Checkpoint 稳定派生 UUIDv8 `trace_id`。同一 `trace_id` 会进入
Checkpoint/归档审计事件、导出与归档结构化日志、两个 S3 对象的 `trace-id`
metadata 以及单次命令结果。崩溃重试仍使用原值，因此不会破坏对象 metadata 的
精确幂等校验。该关联标识不包含邮件内容或密钥，并作为
`copilotmail.trace_id` 写入 OTLP 维护 Span；生产 Collector/Tempo 的真实故障与
查询演练仍需在部署环境完成。

## 生产 Bucket 策略

对象存储管理员至少应独立落实：

- 禁止暂停 Versioning、关闭 Object Lock 或删除 Bucket；
- 业务运行身份只允许目标前缀的条件写、HEAD 和读取必要 Bucket 配置；
- 拒绝不带 `If-None-Match` 的创建请求；
- 拒绝低于企业最低保留期或非 Compliance 的写入；
- 不授予业务运行身份 Governance Bypass、删除 Version、修改 Retention 或 Legal Hold 的权限；
- 管理员与应用身份职责分离，策略变更进入独立审计和双人审批；
- 跨区域复制、密钥生命周期、Legal Hold、恢复演练和费用告警按企业制度配置。

CopilotMail 的应用校验不能替代 Bucket Policy 和组织级 Service Control Policy。

## 故障与恢复

- 离线验签失败：没有对象上传；
- 第一个对象成功、第二个失败：保留同一导出目录和 `archive-id` 重试；
- 两个对象成功、数据库失败：同一参数重试，远程 Version 被精确复用；
- 目标 Key 存在但摘要、Metadata 或 Retention 不同：返回冲突，不能覆盖；
- Bucket 未确认 Versioning/Object Lock：Fail Closed；
- 导出保留截止时间已过去：生成新的导出包，不复用旧 Manifest；
- 数据库证据存在但参数改变：幂等冲突，需要调查，不能以新值覆盖。

## 验证边界

CI 的 MinIO 集成测试验证启用 Object Lock 的 Bucket、带保留上传、精确版本回读和幂等重放。当前开发机没有 Docker，因此本地只完成编译；MinIO 实跑由 CI 执行。生产上线还必须进行权限负向测试、Compliance Version 删除失败测试、凭据撤销、跨区恢复和到期处置演练。
