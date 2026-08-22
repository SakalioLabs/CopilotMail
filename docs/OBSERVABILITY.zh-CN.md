# 可观测性与分布式 Trace

CopilotMail V2 始终输出结构化 JSON 日志，并可选择把受控的 Span 元数据通过
OTLP/HTTP Protobuf 发送到 OpenTelemetry Collector。Trace 是运行诊断信号，
不是审计证据；Collector 不可用不会放宽授权、跳过审计或改变业务事务结果。

## 数据边界

- OTLP 只导出 Span，不导出任意 `tracing` Event 或错误字符串。
- HTTP Span 只记录方法、URL Path 和服务端生成/接受的 `x-request-id`，不记录
  Query、Header、Body、Token、Cookie、邮件正文或附件。
- 审计维护 Span 记录 Organization、执行 Principal、周期 ID、稳定业务 Trace ID
  和 Checkpoint ID；可用 `copilotmail.trace_id` 与审计、对象 metadata 对账。
- 本地 JSON 日志仍是独立信号。生产日志平台必须从容器标准输出采集，并继续应用
  脱敏、访问控制和保留策略。

## 配置

未设置 Endpoint 时只启用 JSON 日志，不会建立任何遥测网络连接。

| 变量 | 默认值 | 约束 |
|---|---|---|
| `COPILOTMAIL_OTEL_EXPORTER_OTLP_ENDPOINT` | 未启用 | OTLP/HTTP 基础 URL；不能含 Userinfo、Query 或 Fragment |
| `COPILOTMAIL_OTEL_SERVICE_NAME` | `copilotmail-control` | 1–100 个 ASCII 字母、数字、`.`、`_`、`-` |
| `COPILOTMAIL_OTEL_EXPORT_TIMEOUT_SECONDS` | `10` | 1–30 秒 |
| `COPILOTMAIL_OTEL_ALLOW_INSECURE_HTTP` | `false` | 仅 development/testing 可设为 `true` |
| `RUST_LOG` | `copilotmail=info,tower_http=info` | 本地日志与可见 Span 过滤 |

生产环境 Endpoint 必须使用 HTTPS。明文 HTTP、URL 内嵌凭据或非法配置会阻止
进程启动。Collector 认证 Header 可使用标准
`OTEL_EXPORTER_OTLP_HEADERS` / `OTEL_EXPORTER_OTLP_TRACES_HEADERS` 注入，但应由
Secret Manager 或编排平台提供，不能提交到仓库、打印到日志或放入 Endpoint。

SDK 使用有界批处理器，业务请求不等待远程导出；正常退出时最多等待 5 秒刷新。
Collector 的失败、队列和拒绝指标必须由 Prometheus 告警。遥测中断不替代审计
Fail Closed：关键操作仍以 PostgreSQL 中同事务的 Audit/Outbox 为准。

## Trace Context

HTTP API 接受 W3C `traceparent` / `tracestate`。合法远程 Context 会成为服务端
Span 的父级；畸形 Context 被忽略并创建新的本地 Root Span。应用继续使用
`x-request-id` 作为稳定用户支持标识，两者用途不同：

- OpenTelemetry Trace ID：跨服务调用树和时延分析；
- `x-request-id`：单次 HTTP 请求定位；
- `copilotmail.trace_id`：审计、WORM 对象与维护周期的稳定业务关联键。

任何上游传入的 Trace Context 都不能改变租户、身份、权限或审计主体。

## 本地观测覆盖层

覆盖层提供 Collector、Tempo、Prometheus 和 Grafana，仅用于开发/演练：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.observability.yaml \
  --profile audit-maintenance up --build
```

- Grafana：`http://127.0.0.1:3000`
- Prometheus：`http://127.0.0.1:9091`
- OTLP、Tempo 和维护指标端口只暴露在 Compose 网络内。
- Agent SSE 指标由 API 的独立 `9091` 监听器提供，Prometheus 以 `api:9091`
  抓取；不包含租户、Principal、Subscription 或邮件标签。
- 同一监听器公开 Agent Cursor/SSE 与邮件提交 API Ingress 的固定 Outcome 计数：
  `allowed`、`tenant_limited`、`principal_limited`、`failure`。限流判定以
  PostgreSQL 请求窗口为准，指标只用于容量和异常观测。

Grafana 在该覆盖层中启用匿名 Viewer，绝不能直接用于生产。生产应使用企业 SSO、
独立 Collector Gateway、TLS/mTLS、网络隔离、外部持久化、高可用及符合公司要求的
Trace/指标/日志保留期。本开发机没有 Docker，因此覆盖层只完成 YAML 与构建期校验；
真实 Collector→Tempo 写入、查询和故障告警演练仍由有 Docker 的环境执行。

## 验收

1. 使用合法 `traceparent` 请求 `/healthz`，在 Tempo 中查询同一 Trace。
2. 使用畸形 `traceparent`，请求仍成功但产生新的 Root Span。
3. 按 `request.id` 找到 HTTP Span；按 `copilotmail.trace_id` 找到审计维护 Span。
4. 停止 Collector，确认业务与审计事务不受影响，Collector 失败指标触发告警。
5. 检查导出的全部 Attribute，不得出现 Token、Cookie、Query、正文、附件或 Secret。
6. 正常停止进程，确认批次在 5 秒刷新窗口内送达。
7. 建立并断开 Agent SSE，确认 Active Gauge 回落；分别触发一分钟租户/Agent
   Ingress 窗口和并发租约配额，确认稳定 `429`、速率拒绝含 `Retry-After`、
   `rejected_total` 分 Scope 增长，且指标文本不含业务标识。
