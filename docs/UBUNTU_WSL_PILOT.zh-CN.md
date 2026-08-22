# Ubuntu / WSL 与兼容原生 Linux 隔离 Pilot 部署手册

> 适用范围：单机、隔离、可回滚的开发或企业 Pilot。
>
> 明确边界：本手册不构成生产放行，不允许公网 MX 切换，也不替代容量、
> 备份恢复、HA、反滥用、法务和安全签字。

> NixOS 不是本仓库已支持的原生交付目标。共享 NixOS 宿主机必须额外遵循
> [NixOS 开发与隔离测试节点边界](NIXOS_DEVELOPMENT_NODE.zh-CN.md)，在取得单独
> 运行授权前只可同步源码和运行离线门禁。

仓库同时保留了 V1 原型与 V2。**禁止在仓库根目录裸执行
`docker compose up`**，因为 Docker Compose 会自动选择根目录的旧 V1
`docker-compose.yaml`。所有 V2 命令必须显式指定 `--env-file`，并把
`deploy/compose.dev.yaml` 放在第一个 `-f`、把
`deploy/compose.pilot.yaml` 放在最后一个 `-f`。

## 1. 宿主机边界

目标环境应满足：

- Ubuntu on WSL2，或行为等价、经单独预检批准的原生 Linux；
- Docker Engine 20.10+，建议 24+；Docker Compose v2.20+；
- Git、OpenSSL、curl、Python 3、`ip`、`ss`；
- 使用 WSL Linux 文件系统中的仓库，例如
  `/home/<user>/src/CopilotMail`，不能使用 `/mnt/c`、`/mnt/d`、SMB 或
  Windows 工作区映射；
- 仓库所在 ext4 文件系统必须由内核以 `rw` 挂载；任何 checksum、journal
  abort 或自动重挂为 `ro` 的证据都属于宿主机阻断，不能继续部署或写入测试；
- Pilot 建议至少 4 vCPU、8 GiB RAM、40 GiB 可用磁盘和 200,000
  可用 inode。该数值只是启动余量，不是生产容量结论。

Docker 构建会下载固定版本的 Rust、Node、Nginx、PostgreSQL、MinIO、
Stalwart 和可观测性镜像，因此宿主机还需要出站 DNS 与 HTTPS。

先把仓库克隆到 WSL 自身的 ext4 文件系统，并在仓库根目录执行：

```bash
cp deploy/.env.v2.example deploy/.env.v2
chmod 0600 deploy/.env.v2

bash scripts/check_ubuntu_wsl_pilot.sh \
  --access ssh-tunnel \
  --env-file deploy/.env.v2
```

检查脚本是只读的：不会启动或拉取容器，不会修改权限，不会读取 Secret
内容，也不会输出 `.env.v2` 的值。它只读取允许列出的非 Secret 配置和 Secret
文件元数据，通过 `findmnt` 确认仓库及 Docker data-root 所在文件系统仍以 `rw`
挂载，并在清洁环境中静默渲染最终 Compose JSON，核对固定项目名和所有发布端口的
`127.0.0.1` 绑定。任何 ambient Compose 控制变量或所选覆盖层使用的插值变量都会
被拒绝，配置只能来自显式 `--env-file`。

若检查报告文件系统不是安全的读写挂载，或 `dmesg` 已出现 ext4 checksum
错误、journal abort、`Remounting filesystem read-only`，必须立即停止把健康
检查当作可写性证据。不要用 `mount -o remount,rw` 继续运行数据库；先安排会
中断同机全部 WSL 工作负载的维护窗口，关闭 WSL、备份 VHD，并按照
[Microsoft WSL VHD 修复指南](https://learn.microsoft.com/en-us/windows/wsl/disk-space#how-to-repair-a-vhd-mounting-error)
离线执行 `e2fsck`。修复后需要从 migration、持久化写入和重启恢复开始重新验收。

若 Docker Desktop 的 data-root 位于不可从 WSL 读取的 VM/VHDX，检查会拒绝把
磁盘状态当作已验证。此时应先在 Windows/Docker Desktop 侧取得 VHDX 上限与剩余
容量证据；不能用 `docker system df` 的“已使用空间”替代“剩余容量”。

## 2. Compose 拓扑与磁盘

默认所有 Host 端口均绑定 `127.0.0.1`。

| 覆盖层 | Host 端口 | 用途 |
|---|---|---|
| 基础栈 | `8081` | Rust Control Plane API |
| 基础栈 | `8083` / `8084` | Admin App / Mail App |
| 基础栈 | `9000` / `9001` | MinIO S3 / Console |
| Mail PoC | `8082` | Stalwart HTTP、JMAP 与引导管理 |
| Mail PoC | `2525` / `2587` / `2465` | SMTP Relay / Submission / Implicit TLS |
| Mail PoC | `2143` / `2993` | IMAP / IMAPS |
| 可观测性 | `9091` / `3000` | Prometheus / Grafana |

PostgreSQL `5432`、OTLP、Tempo、应用 Metrics 和审计维护 Metrics 只在 Compose
网络内开放。

完整开发拓扑会创建八个持久卷：

- PostgreSQL；
- MinIO 审计对象；
- 审计维护工作目录；
- Stalwart 配置和邮件数据两个卷；
- Tempo、Prometheus、Grafana 三个卷。

仓库未定义卷配额、Docker 日志轮转、磁盘告警或生产备份恢复。Prometheus
只配置了 7 天时间保留；MinIO 开发归档使用 30 天 Governance 保留。这些设置
不能代替生产容量和 WORM Compliance 决策。

## 3. Secret 文件所有权

Control Plane 镜像以 UID `10001` 运行。原生 Linux bind mount 会保留宿主
UID 与 mode，因此“当前 WSL 用户拥有的 `0600` 文件”通常不能被容器读取。
Pilot Secret 应由受控命令安装为 UID/GID `10001:10001`，mode `0400` 或
`0600`。不要在终端打印密钥内容。

Human Session Key 必须是精确 32 字节二进制值：

```bash
umask 077
openssl rand -out /tmp/copilotmail-human-session.key 32
test "$(wc -c < /tmp/copilotmail-human-session.key)" -eq 32
sudo install -o 10001 -g 10001 -m 0400 \
  /tmp/copilotmail-human-session.key \
  deploy/secrets/human-session.key
rm -f /tmp/copilotmail-human-session.key
```

启用 Human Session 前执行：

```bash
bash scripts/check_ubuntu_wsl_pilot.sh \
  --access ssh-tunnel \
  --env-file deploy/.env.v2 \
  --with-human-session
```

rootless Docker 或 `userns-remap` 会改变 UID 映射。静态数字所有权不足以证明可读
时，检查会失败；必须额外执行不输出内容的容器内证明：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.human-session.yaml \
  -f deploy/compose.pilot.yaml \
  run --rm --no-deps --entrypoint /bin/sh api -ec \
  'test -f /run/secrets/copilotmail/human-session.key &&
   test -r /run/secrets/copilotmail/human-session.key &&
   test "$(wc -c < /run/secrets/copilotmail/human-session.key)" -eq 32'
```

该命令只验证文件类型、可读性和字节数，不输出 Secret。

## 4. 基础栈

替换 `.env.v2` 中的开发 PostgreSQL 和 MinIO 密码。它们目前仍通过环境变量
注入，只适合隔离 Pilot；生产必须改用 Secret Manager、工作负载身份或等价设施。
PostgreSQL 密码还会直接插入连接 URL，Pilot 应使用高熵 URL-safe 字符，避免
未编码的 `@`、`:`、`/` 破坏 URL。

`compose.pilot.yaml` 是单主机 Pilot 的最后一层运行策略：它为所有基础服务设置
CPU、内存、PID 上限和停止宽限期，为 API/Worker/审计维护显式使用 `SIGTERM`，
并为 Worker 增加依赖 readiness 健康检查。它不能单独运行，也不能放在功能覆盖层
之前；所有 Pilot 命令都必须以 `compose.dev.yaml` 开始、以
`compose.pilot.yaml` 结束。

通过前置检查后启动基础栈：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml \
  up -d --build

docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml ps

curl --fail http://127.0.0.1:8081/healthz
curl --fail http://127.0.0.1:8081/readyz
curl --fail http://127.0.0.1:8083/admin-healthz
curl --fail http://127.0.0.1:8084/mail-healthz
curl --fail http://127.0.0.1:9000/minio/health/ready
```

基础栈把 Human、Agent 和 Mail 的 `REQUIRE_*` 默认设为 `false`。
因此 `/readyz` 返回 `200` 只证明当前“已声明为必需”的依赖就绪，不能证明
OIDC、Agent OAuth、邮件读取、写信或发送已验收。验收时还必须检查 JSON 中每个
依赖状态，并在对应外部能力配置完成后显式打开其 `REQUIRE_*`。

基础栈还必须满足以下运行时证据，不能只看 `docker compose up` 的退出码：

- `migrate` 与 `minio-init` 为 `Exited (0)`；
- PostgreSQL、MinIO、API、Admin、Mail 与 Worker 均为 `healthy`；
- `_sqlx_migrations` 的成功数等于仓库迁移文件数，最大版本等于最新迁移版本，
  失败数为零；
- `python3 scripts/check_audit_governance.py` 通过；
- Admin/Mail 容器使用 `101:101`，并保持只读根文件系统、
  `cap_drop: ALL` 与 `no-new-privileges`；
- PostgreSQL 与 Worker 没有发布主机端口，`8081/8083/8084/9000/9001`
  仅绑定 `127.0.0.1`。

显式验证 Docker 的 `SIGTERM` 优雅关停；`stop` 后 API 和 Worker 的退出码都必须
为 `0`，`137` 表示宽限期结束后被 `SIGKILL`，必须阻断 Pilot：

```bash
compose=(
  docker compose --env-file deploy/.env.v2
  -f deploy/compose.dev.yaml
  -f deploy/compose.pilot.yaml
)

"${compose[@]}" stop --timeout 60 api worker
api_id="$("${compose[@]}" ps -aq api)"
worker_id="$("${compose[@]}" ps -aq worker)"
test "$(docker inspect --format '{{.State.ExitCode}}' "$api_id")" -eq 0
test "$(docker inspect --format '{{.State.ExitCode}}' "$worker_id")" -eq 0
"${compose[@]}" start api worker

for attempt in $(seq 1 30); do
  api_health="$(docker inspect --format '{{.State.Health.Status}}' "$api_id")"
  worker_health="$(docker inspect --format '{{.State.Health.Status}}' "$worker_id")"
  [[ "$api_health" == healthy && "$worker_health" == healthy ]] && break
  sleep 2
done
test "$api_health" = healthy
test "$worker_health" = healthy
```

MinIO 审计桶必须存在且保持版本控制；下列检查只从 Compose 环境读取凭据，
不会输出凭据值：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml \
  run --rm --no-deps --entrypoint /bin/sh minio-init -ec '
    mc alias set verify http://minio:9000 \
      "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD" >/dev/null
    mc stat verify/"$AUDIT_BUCKET" >/dev/null
    mc version info verify/"$AUDIT_BUCKET"
  '
```

最后只重启本项目的 PostgreSQL 与 MinIO，等待二者恢复健康，再重复
`/readyz`、迁移/审计摘要和 MinIO 版本控制检查。重启前后证据必须一致；
API/Worker 必须自行恢复连接。任何 Worker 崩溃循环、迁移脏记录或摘要漂移都应
阻断 Pilot 签收。

## 5. Stalwart 首次安全启动

当前仓库仅实现 Stalwart v0.16 adapter。Mail profile 启用前，`.env.v2` 必须显式
确认同一个稳定远端引用命名空间及 canonical origin：

```dotenv
COPILOTMAIL_MAIL_DATA_PLANE_ID=018f305d-3d0f-7b2a-9d68-7f4a3f6e2101
COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN=http://host.docker.internal:8082
COPILOTMAIL_STALWART_PUBLIC_URL=http://host.docker.internal:8082
```

`MailDataPlaneId` 是持久 UUID 命名空间，不是供应商名称。上述值是 Migration 0053
为既有单实例证据登记的 legacy-primary ID；凭据轮换不得改变它，另一独立实例必须走
受治理的注册/迁移流程取得新 ID。前置检查会拒绝非法 UUID，并要求 data-plane origin
与 Stalwart public URL 精确相等。

Mail 覆盖层会给 API 和 Worker 无条件挂载六个文件：

- `stalwart-jmap.token`；
- `stalwart-mail-content.token`；
- `stalwart-mail-compose.token`；
- `stalwart-mail-submission.token`；
- `stalwart-events.bearer`；
- `stalwart-events.hmac`。

若直接启动整个覆盖层，缺失的 bind source 在部分 Linux Docker 版本中会被创建
成目录；Worker 也会立即探测 Submission Token。首次引导必须只启动 Stalwart：

```bash
bash scripts/check_ubuntu_wsl_pilot.sh \
  --access ssh-tunnel \
  --env-file deploy/.env.v2 \
  --stalwart-bootstrap

docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.mail-poc.yaml \
  -f deploy/compose.mail-bootstrap.yaml \
  -f deploy/compose.pilot.yaml \
  up -d stalwart
```

随后：

1. 通过 `8082` 打开 Stalwart 引导界面；
2. 建立永久管理员，完成本地域和两个普通测试账号；
3. 创建最小权限管理 JMAP Token；
4. 分别签发 Content、Compose、Submission 三个互不复用的最小权限 Token；
5. 独立生成 WebHook Bearer 与 HMAC 值；
6. 验证永久管理员后删除 `COPILOTMAIL_DEV_STALWART_BOOTSTRAP_ADMIN` 的值，并且
   从此不再加载 `compose.mail-bootstrap.yaml`；
7. 以 UID `10001`、mode `0400` 把六个值分别安装成上述正规文件。

事件 Bearer/HMAC 可这样生成；命令不在终端显示值：

```bash
umask 077
openssl rand -base64 48 > /tmp/stalwart-events.bearer
openssl rand -base64 48 > /tmp/stalwart-events.hmac
sudo install -o 10001 -g 10001 -m 0400 \
  /tmp/stalwart-events.bearer deploy/secrets/stalwart-events.bearer
sudo install -o 10001 -g 10001 -m 0400 \
  /tmp/stalwart-events.hmac deploy/secrets/stalwart-events.hmac
rm -f /tmp/stalwart-events.bearer /tmp/stalwart-events.hmac
```

其他四个 Token 应直接从受信的 Stalwart/Token Broker 边界写入受限临时文件，
再用同样的 `install` 命令落盘，不能经过 shell 参数、剪贴板日志或 Git。

完成六个文件后：

若从 0052 或更早版本升级到 0053，必须使用停写切换，不能直接执行整栈
`up -d --build`：先备份 PostgreSQL 和对象存储，停止旧 API、Worker 与审计维护进程，
再单独运行带 Mail 覆盖层的 `migrate`。Migration 会把配置的 ID 显式、不可变地绑定到
canonical origin；失败时不得启动任何新服务。旧、新 API/Worker 不能并跑。

`migrate` 会先提交 SQL migration，再执行追加式 Origin 首次绑定。这两个动作连续执行，
但不是同一个 PostgreSQL 事务；若 Origin 绑定失败，数据库可能已经到达 0053。此时必须
保持全部新旧服务停止，修正为同一个已治理 ID 与 canonical origin 后重跑相同的
`migrate`，不能手工更新绑定行或换一个临时 UUID。

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.mail-poc.yaml \
  -f deploy/compose.pilot.yaml \
  run --rm migrate
```

迁移成功后先核对 `_sqlx_migrations`，再由新镜像执行只读 `check`，精确验证已登记的
ID、adapter kind 与 canonical origin：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.mail-poc.yaml \
  -f deploy/compose.pilot.yaml \
  run --rm --no-deps api check
```

只有 `migrate` 与 `check` 都成功后，才启动全部新镜像：

```bash
bash scripts/check_ubuntu_wsl_pilot.sh \
  --access ssh-tunnel \
  --env-file deploy/.env.v2 \
  --with-mail

docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.mail-poc.yaml \
  -f deploy/compose.pilot.yaml \
  up -d --build
```

常态 `compose.mail-poc.yaml` 不含恢复管理员，并给 API/Worker 注入同一
`MailDataPlaneId`。Mail Engine、Content、Compose 与 Submission Base URL 必须与
显式 data-plane origin 精确同源；任何角色漂移到另一集群都会在启动阶段 Fail Closed。
覆盖层同时把上述四项与 Event Ingress 五项 readiness 依赖固定为 `true`；API 和
Worker 还会等待 Stalwart 的 `/healthz/ready` 健康检查。任一正式凭据或邮件依赖未
就绪都必须阻止服务进入 ready，不能再通过 `.env.v2` 把这些检查降级为软依赖。

## 6. host-gateway 连通与 Mail 验收

开发默认把 Stalwart canonical URL 设为
`http://host.docker.internal:8082`，并通过 `host-gateway` 让 API/Worker
回到宿主发布端口。但该端口又绑定在宿主 `127.0.0.1`。原生 Linux Docker、
Docker Desktop 和 WSL mirrored/NAT 对这条回环路径的行为不完全相同，必须用
真实凭据从 API 容器完成验证：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.mail-poc.yaml \
  -f deploy/compose.pilot.yaml \
  exec api copilotmail-control check-mail-engine
```

该命令同时验证容器 DNS、host-gateway、JMAP Discovery、Origin 一致性、
`MailDataPlaneId` 配置和管理 Token。如果失败：

- 不得为了绕过问题把全局 `COPILOTMAIL_DEV_BIND_HOST` 改成 `0.0.0.0`；
- 优先把 Stalwart 放到受信 HTTPS canonical origin，并确保浏览器与容器都能
  解析和访问；
- 或为 host-gateway 单独提供只对 Docker bridge 可见的受控代理。

检查 `/readyz` 和 Worker 日志。只有普通账号 SMTP/IMAP/JMAP 往返、非开放中继、
两阶段重启持久性和
Submission 状态对账都通过，才算本地邮件 Pilot 验收。

## 7. 远程访问方式 A：SSH 隧道

此方式只用于隔离开发，不开放 LAN 服务。建议改用 SSH 公钥，不把密码写入
命令、脚本或 shell history。

在浏览器所在机器执行：

```bash
ssh -N \
  -L 127.0.0.1:8081:127.0.0.1:8081 \
  -L 127.0.0.1:8082:127.0.0.1:8082 \
  -L 127.0.0.1:8083:127.0.0.1:8083 \
  -L 127.0.0.2:8084:127.0.0.1:8084 \
  <user>@192.168.3.27
```

Admin 使用 `http://127.0.0.1:8083`，Mail 使用
`http://127.0.0.2:8084`，从而保持不同 Cookie Host。IdP 必须登记精确回调：

```text
http://127.0.0.1:8083/api/v2/auth/admin/callback
http://127.0.0.2:8084/api/v2/auth/mail/callback
```

仅该 loopback 模式可以在 `development` 环境显式设置：

```dotenv
COPILOTMAIL_OIDC_ALLOW_INSECURE_HTTP=true
COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE=true
```

企业 IdP 可能拒绝 Web Client 的 HTTP loopback 回调；若不能按安全策略批准，
必须改用 HTTPS 方式。

## 8. 远程访问方式 B：DNS 与 HTTPS

推荐为 Pilot 准备至少三个名称：

- Admin Origin，例如 `admin.pilot.example.com`；
- Mail Origin，例如 `mail.pilot.example.com`；
- Control Plane/SCIM/Agent API，例如 `control.pilot.example.com`。

Stalwart JMAP/管理边界还应使用一个浏览器与容器都能解析的 HTTPS canonical
origin。内部 Pilot 可以使用 split DNS 与公司 CA；面向公网必须使用可公开验证的
DNS 和受信证书。

仓库内两个 Nginx 容器只提供 HTTP，同仓库也没有生产 TLS Ingress。外部
Nginx、Caddy、Traefik 或企业负载均衡器必须：

- 仅在受控接口发布 `443`；
- 把 Admin/Mail 分别转发到 loopback `8083/8084`；
- 为 SCIM、Agent OAuth 和非 `/api` Control Plane 路径提供 `8081` 路由；
- 保留 `Set-Cookie`、`Location`、`Cache-Control` 和
  `Referrer-Policy`；
- 对 `/api/v2/auth/{admin|mail}/{login|callback}` 同时关闭包含 Query 的
  access/error 日志；
- 配置连接、登录与请求速率限制；
- 不直接暴露 MinIO、Stalwart Admin、Prometheus 或匿名 Grafana。

`COPILOTMAIL_DEV_BIND_HOST` 仍应保持 `127.0.0.1`。如果代理运行在容器内，应把
它加入受控 Compose 网络并通过服务名连接，而不是一次性把全部开发端口暴露到
`0.0.0.0`。

还必须确认 `192.168.3.27` 是 Windows LAN 地址、WSL mirrored 地址还是会变化的
NAT 地址，并分别验证 Windows Firewall、WSL Firewall、端口转发和另一台 LAN
主机的证书访问。

## 9. 审计维护卷权限

`audit-maintenance` 使用 UID `10001`，但首次创建的 named volume 可能是
`root:root`。生成并安装审计 RSA 文件后，先显式初始化工作卷：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml \
  --profile audit-maintenance \
  run --rm --no-deps --user root --entrypoint /bin/sh \
  audit-maintenance -ec \
  'chown 10001:10001 /var/lib/copilotmail/audit-maintenance &&
   chmod 0700 /var/lib/copilotmail/audit-maintenance'
```

再以真实运行用户做不输出 Secret 的证明：

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml \
  --profile audit-maintenance \
  run --rm --no-deps --entrypoint /bin/sh \
  audit-maintenance -ec \
  'test -w /var/lib/copilotmail/audit-maintenance &&
   test -r /run/secrets/copilotmail/audit-private.pem &&
   test -r /run/secrets/copilotmail/audit-public.pem'
```

## 10. 无企业输入时可以验收什么

Migration 0001→0053 与针对数据平面身份、Origin、Domain、Outbox 的直接 SQL 不变量
验证已在 disposable PostgreSQL 17 实际执行。当前仓库最新迁移为 0054，因此该历史
证据不覆盖 0054；完整 ignored PostgreSQL 套件和当前目标 Pilot 的 0053 停写升级仍需
各自验收。以下“可以完成”描述的是具备健康 disposable
PostgreSQL/Docker 后的可执行范围，不是当前目标主机已经完成的结论。

可以完成：

- 前置脚本自测、静态安全门禁、Compose 渲染和镜像构建；
- PostgreSQL migration、基础 `/healthz`/`/readyz`；
- PostgreSQL、MinIO、Stalwart named volume 重启持久性；
- Admin/Mail 静态页面和 same-origin `/api` 代理；
- 本地 MinIO Object Lock Governance、审计链、导出和回读；
- 本地普通账号 SMTP/IMAP/JMAP、非开放中继和重启测试。

必须等待企业或外部输入：

- 真实 IdP 登录、MFA/Conditional Access、SCIM 停用和撤销；
- 受信 DNS/HTTPS、外层代理和 WSL/Windows 网络放行；
- 公网静态 IP、PTR、TCP/25、MX、SPF、DKIM、DMARC、
  MTA-STS、TLS-RPT；
- 外部种子邮箱、真实退信/投诉、反垃圾/反病毒与发信信誉；
- 生产 PostgreSQL、S3 Object Lock Compliance、KMS/HSM、
  SIEM、备份恢复、HA、RPO/RTO 和容量签字。

任何 Pilot 结果都不得表述为“已具备生产邮件收发”。当前 Compose 固定
`COPILOTMAIL_ENVIRONMENT=development`，MinIO 使用明文 HTTP/Governance，
可观测性覆盖层启用匿名 Grafana Viewer，且没有生产 Secret、HA 或灾备编排。

## 11. 自测与最终前置检查

脚本自身拒绝逻辑不依赖 Docker：

```bash
bash -n scripts/check_ubuntu_wsl_pilot.sh
bash scripts/check_ubuntu_wsl_pilot.sh --self-test
```

完整 Mail + Human + Audit + 可观测性 Pilot 前：

```bash
bash scripts/check_ubuntu_wsl_pilot.sh \
  --access https \
  --env-file deploy/.env.v2 \
  --with-human-session \
  --with-mail \
  --with-audit-maintenance \
  --with-observability
```

脚本通过只代表宿主和静态部署前置条件满足。DNS、证书、IdP、真实邮件网络、
host-gateway、容器内 Secret 访问、卷写权限、备份恢复和外部往返仍须按本手册
保留独立运行证据。
