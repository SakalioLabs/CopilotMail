# 企业管理端

CopilotMail 企业管理端位于 `web/admin-app`，当前提供审批中心、角色授权治理、
不可变审计检索、合规案件、审计归档 Legal Hold 与受控证据导出工作流。它使用
React、TypeScript 和 Vite，并从 `web/design-system` 复用基础组件和视觉变量。

## 已有能力

- Rust BFF 执行 OIDC Authorization Code + S256 PKCE，浏览器 JavaScript
  不接收 Provider Access Token、Refresh Token 或 PKCE verifier。
- 浏览器只持有 Host-only、HttpOnly、Secure、SameSite=Strict 的 opaque
  Session Cookie；刷新页面会重新解析服务端 Session，注销会追加不可变撤销证据。
- 审批中心分为“待我审批”和“我的申请”：前者只显示当前 Active Human 可决策的
  请求，后者让 Human 与 Agent 查看自己的申请；支持状态筛选、绑定查询条件的
  有界翻页、固定策略/资源详情和不可变决策历史。
- 审批人可在详情内以最少交互批准或拒绝并填写理由；原请求人可确认后取消仍有效的
  Pending 请求。服务端禁止自审、重复决策、他人取消和过期请求状态变更。
- 从服务端读取九个不可变内置角色和租户授权分配，支持按 Principal 精确筛选历史；
  界面直接生成由目标 Principal、角色和失效时间绑定的审批资源 ID，并在审批完成后
  授予最长 365 天的限时角色或撤销现有授权。最后一个有效 Owner 与机器主体的角色
  边界由服务端强制保护。
- 合规案件按状态筛选、有界翻页、建立、查看和结案。
- Legal Hold 按状态或案件筛选、有界翻页、查看、施加和解除。
- 审计事件按最长 31 天的精确窗口检索，支持 Action、Actor、资源、结果和
  Request ID 筛选；续页锁定服务端返回的窗口和全部条件。
- 审计详情显示治理上下文、脱敏详情、前后 Hash、链序号、签名批次和查询证据
  ID；PostgreSQL BIGINT 链序号以十进制字符串传输，避免浏览器精度损失。
- 通过与归档精确绑定的双人审批创建最长 15 分钟的导出凭证，并下载经 Version ID、
  长度和 SHA-256 重新校验的事件或 Manifest；每次下载均先写入不可变审计账本。
- 创建和读取审计/邮件保留策略、追加不可变草稿版本，并通过与待激活版本精确绑定的双人审批
  激活最新版本；界面明确区分“最新草稿”和“当前生效版本”。
- 施加和解除操作要求输入与服务端资源精确绑定的双人审批请求 ID。
- 服务端稳定错误码和 Request ID 原样呈现，便于审计和支持排障。
- 简体中文和英文切换、键盘焦点约束、减少动画支持、移动端布局。

管理端不会自行判断用户权限。每次读取和变更仍由控制平面实时验证 Session、
RBAC、Active Principal/Human、租户边界和审批证据。“待我审批”与决策要求
Active Human 和 `authorization.manage`；“我的申请”精确绑定当前请求人。特权读取由
服务端先提交 Audit + Outbox，证据失败时不会返回结果。

## 本地开发

```bash
cd web
npm ci
npm run check
npm test
npm run build
npm run dev --workspace @copilotmail/admin-app
```

Vite 开发服务器默认监听 `http://127.0.0.1:4173`，并把 `/api` 转发到
`http://127.0.0.1:8081`。编辑
`web/admin-app/public/runtime-config.js` 只适合本地开发，不能提交真实租户或
凭据。

## Docker 运行时配置

开发 Compose 在 `http://127.0.0.1:8083` 暴露管理端。配置以下变量后重建或
重启 `admin` 服务：

| 变量 | 用途 |
|---|---|
| `COPILOTMAIL_ADMIN_ORGANIZATION_ID` | 管理端绑定的唯一企业租户 UUID |
| `COPILOTMAIL_ADMIN_OIDC_CLIENT_ID` | BFF 使用的 IdP 公共客户端 ID |
| `COPILOTMAIL_ADMIN_PUBLIC_ORIGIN` | 管理端精确 HTTPS Origin |
| `COPILOTMAIL_OIDC_ISSUER` | 与控制平面相同的 OIDC Issuer |
| `COPILOTMAIL_OIDC_AUDIENCE` | 与控制平面验证值相同的 Access Token Audience |
| `COPILOTMAIL_ADMIN_OIDC_SCOPE` | 默认 `openid profile email` |
| `COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE` | API 容器中只读的 32 字节密钥路径 |
| `COPILOTMAIL_REQUIRE_HUMAN_SESSION` | 生产必须为 `true` |
| `COPILOTMAIL_ADMIN_HTTP_PORT` | 主机端口，默认 `8083` |

IdP 应启用 Authorization Code + S256 PKCE，不得配置 Client Secret，并须把
精确回调地址（开发环境通常为
`http://127.0.0.1:8083/api/v2/auth/admin/callback`）加入允许列表。生产环境的
页面、API 与 IdP 必须使用 HTTPS。完整 Cookie、CSRF、反向代理日志和验收要求见
[Human OIDC BFF 会话](HUMAN_OIDC_SESSION.zh-CN.md)。

```bash
docker compose --env-file deploy/.env.v2 -f deploy/compose.dev.yaml up \
  --build admin
```

容器通过同源 Nginx 代理访问 `api:8081`，运行时配置写入只读根文件系统之外的
临时目录。容器删除全部 Linux capabilities、启用 `no-new-privileges`，并设置
CSP、禁止嵌入、禁止 MIME 嗅探和 `no-store` 运行时配置缓存。

## 验收

```bash
cd web
npm ci
npm audit --audit-level=moderate
npm run check
npm test
npm run build

docker build -f deploy/docker/Dockerfile.admin-app \
  -t copilotmail-admin:local .
```

当前自动化测试覆盖运行时配置安全限制、BFF Session/Login/Logout 调用、查询参数编码、审计
窗口边界、审批视图与筛选绑定、审批决策/取消路径、筛选/游标复用、导出下载路径与摘要响应、
租户路径绑定、Cookie 凭证发送、Provider Token 不进入浏览器，
以及稳定 API 错误透传。真实 IdP 登录、
双人审批和 S3 Object Lock 状态变更仍须在企业 Pilot 环境执行端到端验收。
