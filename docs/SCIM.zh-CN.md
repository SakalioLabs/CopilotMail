# CopilotMail SCIM 2.0 接入指南

CopilotMail 将 SCIM 作为企业人员与群组生命周期的权威输入，将 OIDC
保留为人员登录认证协议。两者必须使用同一个企业身份提供方：

- SCIM `User.externalId` 必须等于 OIDC Token 中稳定且不可复用的 `sub`；
- `User.userName` 用于企业目录展示和精确过滤，不作为认证主键；
- `active=false` 或 `DELETE /Users/{id}` 会暂停 Human Principal，立即阻止新的
  OIDC 登录和授权解析，同时保留审计引用；
- 再次同步 `active=true` 可以恢复未退休人员；
- Group 仅允许直接引用同租户、已由 SCIM 管理的 User，不支持嵌套 Group。

实现依据 RFC 7643 与 RFC 7644。当前支持：

- `ServiceProviderConfig`、`ResourceTypes` 与 `Schemas` 发现端点；
- Users/Groups 的创建、单项查询、分页列表、PUT、PATCH 与 DELETE；
- `externalId eq "..."`、User `userName eq "..."` 和 Group
  `displayName eq "..."` 精确过滤；
- 弱 ETag 与 `If-Match` 乐观并发控制；
- Group `members` 整体替换，以及 PATCH add/remove 直接成员；
- 标准 `application/scim+json` 响应和 SCIM Error Schema。

Bulk、排序、密码修改、嵌套群组和任意复杂过滤会在
`ServiceProviderConfig` 中明确声明为不支持。

## 1. 签发和轮换令牌

先完成 Organization Bootstrap 和首位 OIDC Owner 接管，然后执行：

```bash
docker compose --env-file deploy/.env.v2 -f deploy/compose.dev.yaml run --rm \
  api issue-scim-token \
  --organization-id "$ORGANIZATION_ID" \
  --label "Microsoft Entra provisioning" \
  --expires-in-days 90
```

命令只显示一次明文 `token`；数据库仅保存 SHA-256 摘要。请立即将明文写入企业
Secrets Manager 和 IdP Provisioning 配置，不要写入 `.env`、日志或工单。

轮换时先签发新令牌并在 IdP 验证同步成功，再撤销旧令牌：

```bash
docker compose --env-file deploy/.env.v2 -f deploy/compose.dev.yaml run --rm \
  api revoke-scim-token \
  --organization-id "$ORGANIZATION_ID" \
  --token-id "$OLD_TOKEN_ID"
```

令牌绑定单一 Organization、到期时间和创建它的 Human Owner。Owner 被停用、
令牌到期或显式撤销后，认证立即失败。

## 2. IdP 配置

- Tenant URL：`https://mail.company.example/scim/v2`
- Secret Token：签发命令输出的 `token`
- Unique identifier for users：`externalId`
- User `externalId`：映射到与 OIDC `sub` 完全一致的稳定目录属性
- Group `externalId`：映射到 IdP 中不可变的群组对象 ID

先按需分配一个测试用户和测试群组，验证创建、改名、成员增删和停用，再扩大
Assignment 范围。不要在验证停用恢复流程前启用全员同步。

## 3. 运维与安全

- SCIM Base URL 必须通过 HTTPS 暴露；不要将 8081 直接暴露到公网。
- 令牌属于高权限 provisioning credential，应由专用 Secret 管理和最小人员范围维护。
- 每次签发、撤销、User/Group 创建、更新、停用和删除都会在同一数据库事务中写入
  append-only 审计事件和 Transactional Outbox。
- User/Group 写入使用租户外键和版本 CAS；跨租户成员、未知成员和并发旧版本会失败。
- IdP 应将失败重试告警接入运维值班；建议以企业离职 SLA 作为 SCIM 停用告警阈值。
