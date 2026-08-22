# Agent 授权服务器签名密钥轮换

CopilotMail Agent Access Token 使用 RS256 签名。生产环境应使用可热加载的多密钥
Key Ring，而不是长期固定的单密钥配置。

## 公开验证端点

授权服务器公开：

```text
GET /oauth2/jwks.json
```

响应是 RFC 7517 JWKS，包含当前轮换窗口内的全部验证公钥。每个 JWK 都包含
`kty=RSA`、`use=sig`、`alg=RS256`、唯一 `kid`、`n` 和 `e`。
端点支持 ETag、`If-None-Match` 和 60 秒缓存。

Token 验证方必须：

1. 读取 JWT Header 的 `kid`；
2. 只选择 JWKS 中同 `kid`、`alg=RS256`、`use=sig` 的 RSA Key；
3. 遇到未知 `kid` 时限流刷新 JWKS；
4. 不能在未知 `kid` 时尝试全部密钥，也不能接受 Token Header 提供的 `jwk`/`jku`。

## Key Set Manifest

配置：

```dotenv
COPILOTMAIL_AGENT_OAUTH_SIGNING_KEY_SET_FILE=/run/secrets/agent-oauth-keyset.json
COPILOTMAIL_AGENT_OAUTH_SIGNING_KEY_RELOAD_SECONDS=30
```

启用 Key Set Manifest 时，不能同时设置旧的
`SIGNING_KEY_ID/PRIVATE_KEY_FILE/PUBLIC_KEY_FILE` 三个变量。

Manifest 示例：

```json
{
  "active_key_id": "agent-as-2026-10",
  "keys": [
    {
      "key_id": "agent-as-2026-07",
      "public_key_file": "agent-as-2026-07-public.pem"
    },
    {
      "key_id": "agent-as-2026-10",
      "private_key_file": "agent-as-2026-10-private.pem",
      "public_key_file": "agent-as-2026-10-public.pem"
    }
  ]
}
```

相对路径以 Manifest 所在目录为基准。活动 Key 必须提供 PKCS#8 RSA 私钥；
只用于验证旧 Token 的 Key 应只提供公钥。所有公钥必须是 SubjectPublicKeyInfo PEM，
RSA 不低于 2048 位。Key Ring 最多包含 10 个 Key。

应用周期性读取 Manifest 和实际 Key 文件。只有全部 JSON、Key ID、RSA Key Pair
和保留窗口验证成功后，才一次性替换内存 Key Ring；半写文件、错配 Key Pair 或危险删除
只会记录错误，旧 Key Ring 继续服务。

## 无中断轮换步骤

假设旧活动 Key 为 `old`，新 Key 为 `new`，Access Token TTL 为 300 秒：

1. 在密钥设施内生成 `new`，先写入所有 Key 文件。
2. 更新 Manifest，同时保留 `old`，加入 `new`。此时可以仍让 `old` 活动。
3. 原子替换 Manifest，例如在同一文件系统写临时文件后执行 rename。
4. 等所有实例的 `/oauth2/jwks.json` 都同时出现 `old` 和 `new`。
5. 将 `active_key_id` 切换为 `new`，保留 `old` 公钥。
6. 确认新签发 Token Header 的 `kid=new`，并确认旧 Token 仍可验证。
7. 至少等待“Access Token 最大 TTL + 30 秒时钟偏差窗口”；当前默认需要 330 秒。
8. 从 Manifest 删除 `old`，再次原子替换。
9. 确认 JWKS 仅在保留窗口结束后移除 `old`，保存变更单与监控证据。

进程启动时会保守地将 Manifest 中每个 Key 视为可能仍有存量 Token，因此重启不会缩短
保留窗口。滚动部署期间，所有实例必须看到同时包含新旧 Key 的同一 Manifest；不能让一部分
实例只知道 `old`、另一部分只知道 `new`。

## 强制安全规则

- `kid` 不能复用到不同 RSA Key Material，即使旧 Key 已过期。
- 活动 Key 缺少私钥时拒绝加载。
- 同一个 Manifest 不能出现重复 `kid`。
- 仍可能存在有效 Token 时，删除旧 Key 会以 `UnsafeKeyRemoval` 被拒绝。
- 新 Key Set 验证失败不会清空或部分更新当前 Key Ring。
- 私钥不得出现在 JWKS、日志、数据库、镜像或 Git 中。

## 回滚

如果切换后发现新 Key 异常：

1. 保持新旧公钥都在 Manifest 中；
2. 将 `active_key_id` 原子切回 `old`，且为 `old` 恢复其私钥文件引用；
3. 验证新签发 Token 使用 `kid=old`；
4. 继续保留 `new` 公钥，直到所有已签发 `new` Token 的保留窗口结束；
5. 再移除 `new` 并调查根因。

不要通过直接删除新 Key 来回滚，这会立即破坏已经签发的 `new` Token。
