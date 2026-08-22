# 审计 Checkpoint、导出与离线验签

> 状态：V2 审计证据基线，2026-07-26。该能力提供签名链头、便携证据包和经服务端回读确认的 S3 Object Lock/WORM 归档。

## 信任模型

每个 Checkpoint 签名一个租户审计链的不可变前缀，包含：

- 租户、Checkpoint ID；
- 连续链序范围与事件总数；
- 首尾事件 ID；
- 事件时间范围；
- SHA-256 链头与哈希版本；
- 签名 Key ID、公钥 SHA-256 指纹；
- Checkpoint 创建时间。

首版签名算法固定为 `RSASSA-PKCS1-v1_5-SHA256`，仅接受 2048–8192 位 RSA PKCS#8 私钥和 SubjectPublicKeyInfo 公钥。算法与格式均有显式版本，不能由 Manifest 降级。

私钥只通过命令参数指向的只读挂载文件加载，不写入数据库、日志、Manifest 或导出目录。离线验签必须单独提供受信公钥；导出包不能声明自己的信任根。

## 生成密钥

开发或试点可使用 OpenSSL。正式环境应由企业 KMS/HSM 生成或托管，并限制签名权限：

```shell
openssl genpkey -algorithm RSA \
  -pkeyopt rsa_keygen_bits:3072 \
  -out audit-checkpoint-private.pem

openssl pkey \
  -in audit-checkpoint-private.pem \
  -pubout \
  -out audit-checkpoint-public.pem
```

私钥文件不得加入镜像或 Git。生产容器应通过 Docker Secret、Kubernetes Secret CSI 或 KMS adapter 只读挂载。

## 创建 Checkpoint

调用人必须是目标租户内 Active Principal，并实时拥有 `audit.export`：

```shell
copilotmail-control create-audit-checkpoint \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --checkpoint-id "$CHECKPOINT_ID" \
  --key-id audit-2026-01 \
  --private-key-file /run/secrets/audit-private.pem \
  --public-key-file /run/secrets/audit-public.pem \
  --archive-object-key "audit/$ORGANIZATION_ID/$CHECKPOINT_ID/events.jsonl"
```

`--checkpoint-id` 可省略并自动生成 UUIDv7；自动化任务应预先生成并持久化该 ID，以便网络超时后精确重试。同一 ID、租户、密钥和对象键的重放返回原结果，不增加第二个 Checkpoint、审计事件或 Outbox。

创建前会完整重验当前租户哈希链。Checkpoint、`audit.checkpoint.created` 和 Outbox 在同一 PostgreSQL 事务提交。创建事件本身进入 Checkpoint 之后的下一段链，这是预期行为。

`archive-object-key` 是 WORM 事件对象的稳定目标名；创建 Checkpoint 不执行上传，归档由独立命令完成，避免把数据库事务与远程对象写入伪装成一个原子操作。

## 导出证据包

输出目录必须不存在，避免覆盖既有证据：

```shell
copilotmail-control export-audit-checkpoint \
  --organization-id "$ORGANIZATION_ID" \
  --checkpoint-id "$CHECKPOINT_ID" \
  --output-directory ./audit-export-2026-07
```

目录包含：

- `events.jsonl`：从链序 1 到 Checkpoint 链头的完整验证材料；
- `manifest.json`：签名 Checkpoint、事件文件摘要、格式版本和导出时间。

导出按 1000 条分页，不把整个租户历史加载到内存。只有两个最终文件均写入并 `fsync` 后，临时文件才会改为正式文件名。异常中断可能留下以点开头的临时文件；包含临时文件的目录不是完整证据包，应使用新目录重新导出。

## 完全离线验签

此命令在读取任何数据库配置前执行，可在隔离主机运行：

```shell
copilotmail-control verify-audit-export \
  --export-directory ./audit-export-2026-07 \
  --public-key-file ./trusted-audit-public.pem
```

验签顺序：

1. 严格解析 Manifest，拒绝未知字段、算法或版本；
2. 核对独立公钥指纹并验证 Checkpoint 签名；
3. 限制 Manifest 和单行事件大小；
4. 逐行重算租户、链序、前序哈希和事件哈希；
5. 核对事件数、首尾事件、签名链头与 `events.jsonl` 文件摘要。

任何一步失败均返回非零退出码，不输出“部分成功”。

## WORM 归档

完成离线验证后，使用相同受信公钥执行：

```shell
copilotmail-control archive-audit-export \
  --organization-id "$ORGANIZATION_ID" \
  --actor-principal-id "$ACTOR_PRINCIPAL_ID" \
  --checkpoint-id "$CHECKPOINT_ID" \
  --archive-id "$ARCHIVE_ID" \
  --export-directory ./audit-export-2026-07 \
  --public-key-file ./trusted-audit-public.pem
```

自动化任务应预先持久化 `archive-id`。命令会先离线验签，再上传 `events.jsonl` 和 Manifest，最后回读精确 Version ID、SHA-256、长度、Metadata、Retention Mode 与 Retain Until；全部吻合后才在 PostgreSQL 原子写入归档证据、`audit.archive.completed` 和 Outbox。详细配置、失败恢复和生产策略见[审计 WORM 归档](AUDIT_WORM_ARCHIVE.zh-CN.md)。

## 密钥轮换和保留

- 新 Key ID 必须对应新的公钥，禁止复用 Key ID 替换密钥材料；
- 旧公钥至少保留到其签名的全部审计证据超过法定保留期；
- 每次轮换前后各创建一个 Checkpoint，并把公钥指纹交由独立配置管理系统留档；
- 私钥泄露后立即停止使用该 Key ID，保留公钥用于历史验证，并按安全事件流程重新锚定后续链头；
- 后续 KMS adapter 必须保持相同的签名 Payload 和算法标识语义。

## 当前边界

签名可证明证据来自持有受信私钥的一方，哈希链可证明导出的前缀内部连续一致。当前尚未完成：

- KMS/HSM 远程签名 adapter；
- 定时 Checkpoint Worker 和漏签告警；
- Legal Hold、保留策略和 SIEM 投递。
