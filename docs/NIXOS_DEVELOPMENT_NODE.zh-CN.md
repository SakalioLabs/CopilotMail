# NixOS 开发与隔离测试节点边界

> 适用范围：在承载其他生产应用的 NixOS 主机上保存 CopilotMail 源码、执行离线
> 代码门禁，或在获得单独批准后准备一次性隔离测试。
>
> 本文不是 NixOS 生产部署手册，也不授权启动 Compose、执行数据库迁移、创建对象存储
> Bucket、开放端口、修改 NixOS 配置或影响同机应用。

CopilotMail V2 当前没有 `flake.nix`、NixOS module、systemd unit 或 Podman 交付物。
现有运行资产是面向隔离 Docker Compose Pilot 的开发配置，不能因为宿主机运行 NixOS
而推导为 NixOS 原生支持或生产可用。

## 1. 源码收敛目录

开发快照应放在由指定开发用户拥有的专用目录，例如
`/data/actons/CopilotMail`，目录模式为 `0750`。同步前必须形成可追溯 Git 提交或
经 SHA-256 校验的归档；同步后先核对提交 ID 和干净工作树：

```bash
git -C /data/actons/CopilotMail rev-parse HEAD
git -C /data/actons/CopilotMail status --porcelain=v1
```

不得把真实 `.env`、私钥、OIDC Client Secret、数据库密码或生产对象存储凭据复制到
该目录。凭据曾出现在聊天、终端记录或其他非受控通道时，必须轮换。

## 2. 仅只读的宿主机预检

在执行任何构建、拉取镜像或启动容器前，先由有权负责人确认以下只读信息：

```bash
findmnt -T /data/actons
df -h /data/actons
df -i /data/actons
docker version
docker compose version
docker info --format '{{.DockerRootDir}}|{{json .SecurityOptions}}'
docker compose ls
docker ps -a
docker volume ls
ss -ltn
```

要求仓库目录和 Docker data-root 都是可写、受控且具有至少 40 GiB 可用空间和
200,000 可用 inode 的文件系统。若 Docker data-root 与其他生产应用共享，或可用空间、
端口、CPU、内存余量不足，则本机不具备 Compose 测试条件；不得通过删除镜像、卷、
容器或重启 Docker 来腾挪资源。

还必须确认不存在 `copilotmail-v2-pilot`、`copilotmail-v2-dev` 或旧 V1
`copilotmail` 的同名 Compose 资源。`docker` 组对 rootful Docker 通常等同于高权限，
不能把它视为最小权限边界。

## 3. 本阶段允许的操作

在生产宿主机上，本阶段只允许源码同步和不访问外部运行时的验证，例如：

```bash
cd /data/actons/CopilotMail
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/check_architecture.py --self-test
python3 scripts/check_audit_governance.py
python3 scripts/check_compose_security.py --self-test
python3 scripts/check_documentation.py
python3 scripts/check_web_i18n.py --self-test
npm --prefix web run check
npm --prefix web test
npm --prefix web run build
```

这些命令仍可能消耗编译缓存和磁盘，因此应先确认资源余量。没有明确批准时，不在
生产宿主机执行依赖网络或外部状态的 ignored PostgreSQL/MinIO 测试。

## 4. 后续隔离 Compose 测试的放行条件

任何 Compose 测试必须单独取得授权，并优先使用独立测试机或专用 Docker daemon /
data-root。开始前创建非秘密开发配置后，先只渲染，不启动：

```bash
cd /data/actons/CopilotMail
cp deploy/.env.v2.example deploy/.env.v2
chmod 0600 deploy/.env.v2

env -u COMPOSE_PROJECT_NAME -u COMPOSE_FILE -u COMPOSE_PROFILES -u COMPOSE_ENV_FILES \
  docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml config >/dev/null
```

即使渲染通过，也不表示可启动。启动会执行 migration、创建或配置 MinIO Bucket、占用
共享 BuildKit/镜像缓存和端口，并可能使用约 7.75 GiB 内存及 10.5 CPU 的启动峰值。
Mail PoC、Human Session、`audit-maintenance`、可观测性和 bootstrap 覆盖层均不属于
首次测试范围。

严禁在仓库根目录裸执行 `docker compose up`：它会选择保留的 V1
`docker-compose.yaml`。任何获批准的 V2 Compose 命令都必须显式指定
`--env-file`，以 `deploy/compose.dev.yaml` 为第一个 `-f`，并以
`deploy/compose.pilot.yaml` 为最后一个 `-f`。不得使用 `down --volumes`、
`--remove-orphans`、`prune`、`up --build`、修改防火墙、`nixos-rebuild` 或重启 Docker
来解决测试问题。

更完整的 Pilot 前置检查、迁移、备份恢复和邮件边界分别见
[Ubuntu / WSL 隔离 Pilot 手册](UBUNTU_WSL_PILOT.zh-CN.md)、
[备份恢复手册](BACKUP_RESTORE.zh-CN.md) 和
[Stalwart 邮件数据平面 PoC](STALWART_POC.zh-CN.md)。它们均不构成生产放行。
