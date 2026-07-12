# Architecture

- `SMTP listener`：Rust/Tokio 实现 SMTP 会话和信封校验，不依赖 Inbucket、Mailpit 等服务。
- `API`：Axum JSON API，管理员密钥创建邮箱，邮箱密码换取 JWT。
- `Store`：SQLite WAL，保留原始 RFC 5322 邮件和解析后的正文。
- `Events`：下一里程碑通过 WebSocket 按邮箱推送新邮件事件。
- `Web`：独立静态前端，只依赖版本化 API。
- `Skill`：机器可读的 Agent 使用说明和调用示例。

生产环境应将 API 放在 HTTPS 反向代理之后，并把公网 TCP/25 转发到容器 SMTP 端口。
