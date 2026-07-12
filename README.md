# CopilotMail

CopilotMail 是面向 AI Agent 的自托管邮件平台。一个 Rust 二进制提供自研 SMTP 收件/公网直投、邮箱 API、JWT 鉴权、WebSocket 新邮件事件、SQLite 持久化和静态前端。它不依赖 Inbucket、Mailpit、Mail.tm 或第三方 SMTP 服务。

## 能力

- 创建独立 Agent 邮箱并通过密码换取短期 Token
- 自研 Tokio SMTP 服务接收 RFC 5322 邮件
- API 发送本地域邮件；外域邮件查询 MX 后直接投递到目标 SMTP
- 收件箱、消息详情、已读、删除与邮箱销毁 API
- `/v1/events` WebSocket 按邮箱隔离推送新邮件事件
- 同仓库、同镜像发布的解耦 Web 前端
- 可分发的 [`copilotmail` Agent Skill](skills/copilotmail/SKILL.md)
- Dockerfile、Compose、单元测试和全链路 E2E 测试

## Docker 启动

```bash
cp .env.example .env
# 将 JWT 和管理员密钥替换为随机值
docker compose up -d --build
curl http://127.0.0.1:8080/healthz
```

打开 `http://服务器:8080` 使用控制台。SMTP 默认映射到主机 `2525`；正式收取公网邮件时，将 `.env` 的 `SMTP_PORT` 设置为 `25`。

## 第一个邮箱

```bash
curl -X POST http://127.0.0.1:8080/v1/mailboxes \
  -H 'Content-Type: application/json' \
  -H 'X-Admin-Key: your-admin-key' \
  -d '{"address":"agent@mail.local","password":"change-me-now"}'

curl -X POST http://127.0.0.1:8080/v1/auth/token \
  -H 'Content-Type: application/json' \
  -d '{"address":"agent@mail.local","password":"change-me-now"}'
```

完整接口见 [OpenAPI](docs/openapi.yaml)，设计见 [架构说明](docs/ARCHITECTURE.md)，生产部署见 [部署指南](docs/DEPLOYMENT.md)。

## 验证

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build
python tests/e2e.py
python ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py skills/copilotmail
docker compose config
docker compose build
```

E2E 测试会启动临时服务，并覆盖邮箱创建、登录、SMTP 投递、WebSocket 事件、消息 CRUD、API 本地发信和前端加载。

## 公网邮件要求

为邮件域配置 MX 记录，将 TCP/25 转发到 CopilotMail，并确保云厂商/运营商允许入站与出站 25。API 应放在 HTTPS 反向代理后。直接 MX 投递不等于高送达率；生产发信还需要正确的 PTR、SPF、DKIM、DMARC、队列与退避策略。

