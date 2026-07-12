# CopilotMail

CopilotMail 是面向 AI Agent 的自托管邮件平台。单个 Rust 服务提供自研 SMTP 接收、邮箱/API 鉴权和持久化；前端与后端在同一项目发布，但通过 HTTP API 解耦。

## 快速启动

```bash
cp .env.example .env
# 修改两个密钥
docker compose up -d --build
curl http://127.0.0.1:8080/healthz
```

创建邮箱：

```bash
curl -X POST http://127.0.0.1:8080/v1/mailboxes \
  -H 'Content-Type: application/json' -H 'X-Admin-Key: your-admin-key' \
  -d '{"address":"agent@mail.local","password":"change-me-now"}'
```

当前为首个可运行里程碑。路线：消息详情/删除、出站 SMTP、WebSocket 事件、附件、前端控制台、Agent Skill 与完整集成测试。

