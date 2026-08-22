# PostgreSQL 逻辑备份、隔离恢复与一致性验收

本文是 CopilotMail V2 **控制平面 PostgreSQL** 的操作手册。仓库提供的
`scripts/check_postgres_backup_restore.sh` 会创建 custom-format 逻辑备份，将它
恢复到脚本自行生成的一次性数据库，比较关键一致性证据，最后只删除这个精确的临时
数据库。只有全部步骤以及清理都成功，命令才返回成功并发布备份和证据文件。

该脚本不会把备份恢复到源数据库，也不是生产切换工具。

## 1. 范围与非目标

本流程覆盖：

- V2 控制平面的 PostgreSQL schema、数据、函数和触发器；
- SQLx migration 账本、审计事件目录、关键不可变触发器；
- 一个已知企业及其至少一条审计链证据；
- `pg_dump` custom-format 归档的可枚举性和实际隔离恢复。

本流程不覆盖：

- V1 SQLite 数据；
- Stalwart/JMAP 邮件数据平面、附件和邮件队列；
- S3/Object Lock 审计归档、Legal Hold 对象或其他对象存储；
- OIDC、KMS、Agent 签名密钥、数据库口令和其他 Secret；
- 集群级角色、角色口令、原始对象 owner / ACL、数据库级连接设置和原有 tablespace
  布局；本流程有意使用 `--no-owner`、`--no-acl`、`--no-tablespaces`；
- WAL 归档、时间点恢复（PITR）、跨故障域复制、HA、RPO/RTO 或整站灾难恢复。

因此，一次通过证明的是“此逻辑归档可在当前 PostgreSQL 兼容环境中恢复并保留所检查
的关键证据”，不能替代生产容量、恢复时长、跨区域故障或业务连续性验收。

## 2. Fail-Closed 安全约束

脚本执行下列硬约束：

- 所有源库查询和 `pg_dump` 会话均设置
  `default_transaction_read_only=on`；脚本不会向源库写入。
- `pg_dump` 固定使用 custom format、`--no-owner`、`--no-acl` 和一致性快照。
- 恢复库名称只能由脚本生成，固定前缀为
  `copilotmail_restore_verify_`；命令行没有指定恢复库名称的参数。
- 在任何 `CREATE DATABASE` 或 `DROP DATABASE` 前，脚本都会再次验证名称格式、
  源库与维护库名称，并确认三个名称互不混淆。
- 临时库创建后会写入本次运行独有的数据库 comment 标记。退出 trap 只有在名称和
  标记均精确匹配时才删除它；标记缺失或不一致时会返回失败并留给 DBA 审核，不会
  执行通配删除。
- 备份目标必须是绝对路径、此前不存在，且直接父目录必须归当前用户所有并且 group /
  other 权限均为零。发布使用同目录原子 hard link，不会覆盖既有归档或
  `.evidence` 文件；先发布 sidecar，最后出现的归档路径是完成标记。
- 口令只交给 libpq 环境、`PGPASSFILE` 或 libpq service 配置。脚本没有口令参数，
  不打印连接字符串或口令。
- 临时文件、最终归档和证据文件均以仅所有者可访问的权限创建。普通错误和可捕获信号
  会删除本次运行拥有的半成品；若进程被 `SIGKILL` 或主机掉电，可能只留下孤立的
  `.evidence`，但不会先暴露一个缺少 sidecar 的归档。孤立 sidecar 必须人工核对后
  删除，不能视为备份。

`--self-test` 不连接数据库，专门验证危险目标名、源库同名、SQL 形状名称、既有归档
和不安全目录等输入均被拒绝：

```bash
bash scripts/check_postgres_backup_restore.sh --self-test
```

修改脚本、升级 Bash 或变更运行镜像后，必须先通过该测试。

## 3. 前置条件

### 3.1 工具与主机

- 使用 Linux / WSL Bash 和 GNU coreutils。
- 主机原生安装 `psql`、`pg_dump` 和 `pg_restore`；**不要求 Docker，也不能只依赖
  `docker exec` 内的客户端**。
- `psql`、`pg_dump` 与 `pg_restore` 应来自服务端的同一 PostgreSQL 主版本；脚本
  会明确要求 `pg_dump`、`pg_restore` 与服务端主版本完全相同，避免把新客户端输出
  反向恢复到旧服务端。
- 私有备份目录应位于 Linux 文件系统或能可靠执行 POSIX 权限和同目录 hard link 的
  加密卷上。不要把
  WSL 生产备份直接写到无法保证 `0700`/`0600` 语义的 Windows 挂载目录。
- 可用空间至少容纳一份 custom-format 归档和一个完整的临时恢复数据库，并预留索引
  重建空间。

### 3.2 数据库身份与权限

使用专用备份验收身份，不要使用应用身份。它至少需要：

- 连接源库并读取全部待备份对象；
- 连接维护库（默认 `postgres`）；
- `CREATEDB`，以创建随机命名的隔离库；该身份作为临时库 owner 可以在验收后删除它。

不要授予不必要的角色管理或复制权限。即使该身份因环境限制仍具有源库写权限，脚本
自己的源库会话也会强制为只读；这不限制其他并发客户端，所以仍须执行下一节的业务
静默。

“隔离数据库”只表示不覆盖源数据库，不是针对恶意 dump 的安全沙箱。`pg_restore`
会执行归档中由源库高权限身份控制的 SQL 和函数代码。只有信任源集群管理员时，才可
在同一生产集群运行本流程；若源库可能已被攻陷，应在独立、可销毁的隔离集群中，由
最小权限身份检查并恢复归档，不能在生产集群直接演练。

### 3.3 一致性窗口与验证企业

在生产执行前：

1. 阻止新 API 写入、暂停 Worker、migration、SCIM 同步和邮件事件接入。
2. 等待在途事务和 Outbox 租约进入已知状态，并确认没有其他 schema 变更。
3. 选择一个已知 `organization_id`。该企业必须在 `organizations` 中恰有一行，并在
   `audit_events` 中至少有一条有效链记录。新环境可先通过受控的
   `bootstrap-organization` 建立该证据。
4. 记录静默开始时间、应用镜像 digest、变更单和操作者。

`pg_dump` 自身会取得一致性快照，但脚本还会比较备份前后所采样的治理证据。若其他
写入改变了这些证据，脚本会失败；静默窗口也用于保证业务上可解释的恢复点。

## 4. 安全配置连接

`PGDATABASE` 必须是源库的**纯数据库名**，不能是 URL；维护库由另一个纯名称变量
指定。主机、端口、用户、TLS 和认证继续使用标准 libpq 环境或 service 文件。
推荐把口令放进由 Secret 管理器挂载且权限为 `0600` 的 passfile：

```bash
export PGHOST=db.internal.example
export PGPORT=5432
export PGUSER=copilotmail_backup_verify
export PGDATABASE=copilotmail
export PGPASSFILE=/run/secrets/copilotmail-backup.pgpass
export COPILOTMAIL_RESTORE_MAINTENANCE_DATABASE=postgres

export PGSSLMODE=verify-full
export PGSSLROOTCERT=/run/secrets/enterprise-postgres-ca.pem
```

不要把口令写进命令行、Shell history、工单或归档文件名。若平台必须使用
`PGPASSWORD`，只通过受控的临时进程环境注入，命令结束后立即清除；不要开启
`set -x`。`PGSERVICE`/`PGSERVICEFILE` 也可以承载主机、TLS 与认证配置，但
`PGDATABASE` 仍需显式给出源库纯名称。

首先确认当前用户拥有私有、加密的备份根目录，再为本次运行创建新目录：

```bash
install -d -m 0700 /var/backups/copilotmail
archive_directory="$(
  mktemp -d /var/backups/copilotmail/restore-check.XXXXXX
)"
chmod 0700 "$archive_directory"
```

脚本拒绝已经存在的目标文件，因此每次重试都必须使用新目录或新文件名。

## 5. 执行备份和隔离恢复

把经过审批的企业 UUID 放入本地变量，再运行：

```bash
verify_organization_id="018f13b9-1111-7bc8-965d-48a28866f073"

bash scripts/check_postgres_backup_restore.sh \
  --archive "$archive_directory/control-plane.dump" \
  --verify-organization-id "$verify_organization_id"
```

命令顺序如下：

1. 验证归档路径、数据库纯名称、源库与维护库不相同，并生成固定前缀的随机恢复库名。
2. 验证实际连接的源库、只读会话、维护库和 PostgreSQL 客户端版本；源库 encoding、
   locale provider 与 locale 创建属性必须和 `template0` 一致，否则在创建临时库前
   失败关闭。新临时库创建后的 collation version 另在恢复证据中与源库比较。
3. 从源库采集第一份证据。
4. 以只读一致性快照运行：

   ```text
   pg_dump --format=custom --no-owner --no-acl --no-password \
     --serializable-deferrable
   ```

5. 用 `pg_restore --list` 验证归档目录，再次采集源库证据；两次不相同则失败。
6. 在维护库中创建脚本生成的隔离数据库并写入本次运行标记。
7. 使用
   `pg_restore --exit-on-error --single-transaction --no-owner --no-acl --no-password --no-tablespaces`
   恢复归档；所有对象落在临时库默认 tablespace。
8. 在恢复库的只读会话中采集同一证据并与源库逐字比较。
9. 创建归档 SHA-256 和证据 sidecar，先删除精确匹配的临时库，再以 no-clobber
   hard link 先发布 sidecar、最后发布归档完成标记，然后返回最终状态。

成功时会留下：

```text
control-plane.dump
control-plane.dump.evidence
```

失败时命令返回非零。不得忽略返回码，也不得把失败运行留下的文件登记为可恢复备份。

## 6. 验收证据

源库备份前、备份后以及恢复库必须一致通过：

- 数据库 encoding、locale provider、locale、collation version digest 一致；
- `_sqlx_migrations` 非空、没有 `success=false`，并记录 migration 数量、最高版本和
  全账本 checksum digest；
- `audit_event_catalog` 非空，并记录排序后的目录 digest；
- 下列五个不可变触发器存在、不是 internal trigger，且状态为 enabled/origin 或
  always：

  - `audit_events_append_only`
  - `audit_events_hash_chain`
  - `audit_events_reject_truncate`
  - `audit_event_catalog_immutable`
  - `audit_event_catalog_reject_truncate`

- 指定企业行存在且唯一，其受控字段 digest 一致；
- 指定企业至少有一条审计事件，并保留最大链序号和 64 位十六进制链头；
- 归档可被 `pg_restore` 枚举，并记录源数据库纯名称、验证企业 UUID、归档
  SHA-256、服务端版本、客户端版本和 UTC 验收时间。

可以在不连接数据库的情况下复核归档 digest：

```bash
expected_sha="$(
  awk -F= '$1 == "archive_sha256" { print $2 }' \
    "$archive_directory/control-plane.dump.evidence"
)"
actual_sha="$(
  sha256sum "$archive_directory/control-plane.dump" | awk '{ print $1 }'
)"
test -n "$expected_sha" && test "$actual_sha" = "$expected_sha"
```

`.evidence` 是本次运行的本地可读报告，不是独立签名证明。成功后应立即把归档和
sidecar 一起加密、复制到受控备份系统，并由该系统生成不可变版本、签名 manifest
或等效的外部完整性证据。逻辑 dump 本身**未加密**，其中包含业务与审计敏感数据。

## 7. 失败处理

| 失败 | 处理 |
| --- | --- |
| 源库/维护库名称无效或相同 | 修正纯名称配置；不要放宽名称校验。 |
| 归档路径存在或目录权限过宽 | 创建新的 owner-only `0700` 目录；不要覆盖或临时 `chmod` 共享目录。 |
| 客户端主版本不一致 | 安装与服务端主版本完全相同的 `pg_dump` 和 `pg_restore`。 |
| 源库 locale / encoding 创建属性与 `template0` 不同，或恢复库 collation version 不一致 | 停止本脚本；由 DBA 设计显式复制数据库属性的独立恢复流程，不能降低比较标准。 |
| 备份前后证据变化 | 保持服务静默，排查并发写入或 migration，使用全新目标重新执行。 |
| migration、目录或触发器校验失败 | 视为 schema / 治理漂移，停止升级并修复；不能把 dump 标记为已验收。 |
| 验证企业或审计事件不存在 | 选择有真实审计证据的企业，或在受控环境先完成 bootstrap。 |
| 创建、恢复或空间不足 | 修正最小权限、连接或容量后，用新目标从头执行。 |
| 临时库清理失败 | 命令会失败关闭。保留日志并由 DBA 核对精确名称和 comment 标记；严禁按前缀批量删除。 |

清理失败时，可以先执行只读盘点：

```sql
SELECT datname,
       pg_catalog.shobj_description(oid, 'pg_database') AS verification_marker
  FROM pg_catalog.pg_database
 WHERE left(
         datname,
         length('copilotmail_restore_verify_')
       ) = 'copilotmail_restore_verify_'
 ORDER BY datname;
```

只允许 DBA 在变更单中确认“本次日志中的完整随机名称”和
`copilotmail-restore-verify:` 标记后，按组织标准流程处理。本文故意不提供通配
`DROP DATABASE` 命令。

## 8. Schema 升级与回滚

在每次可能改变 schema 或数据语义的发布前：

1. 完成业务静默并执行本流程。
2. 把 `.dump`、`.evidence`、应用镜像 digest、migration 目标版本和变更单绑定到
   同一不可变备份记录。
3. 只有隔离恢复验收成功后才运行生产 migration。
4. migration 后执行应用健康、关键租户、审计链、Outbox 和 Worker 恢复检查，再结束
   静默窗口。

若升级失败，不要把旧 schema 直接覆盖回仍在运行的源库，也不要假设 migration 可以
安全逆向。保留故障库用于取证，由 DBA 从最后一份已验收归档创建**另一个替换库**，
按 IaC / migration 清单重建应用角色与数据库权限，将对象 owner / ACL 重新分配给
批准的运行身份，再以真实应用身份执行连接、读写、关键一致性检查和 smoke test，
最后通过受控配置切换。恢复点之后产生的数据可能丢失，必须按已签字的 RPO 和变更
策略决策。

本脚本在验收结束时会删除自己的随机临时库，所以不会自动完成生产回切。生产切换、
DNS/连接池处理、旧库保留期和最终删除必须由独立的恢复变更单及双人复核控制。

## 9. CI 证据与剩余验收

GitHub Actions 的 `postgres` job 会：

- 启动 PostgreSQL 17 service；
- 在 runner 上安装并直接调用原生 PostgreSQL 17 客户端，而不是把脚本封装进
  Docker；
- 执行 migration 两次并运行控制平面检查；
- bootstrap 一个带审计事件的专用企业；
- 实际执行完整 dump、隔离 restore 和证据比较；
- 确认归档与 sidecar 非空，并确认没有遗留
  `copilotmail_restore_verify_` 临时数据库；
- 另行运行 PostgreSQL integration tests。

CI 不是生产恢复演练：其数据库很小、身份近似管理员、备份不会跨故障域持久化，也
不验证生产 TLS、KMS、存储保留、PITR、容量、真实恢复时长或 RPO/RTO。每个目标生产
环境仍须按季度及重大升级前执行隔离恢复演练并留存审核证据。
