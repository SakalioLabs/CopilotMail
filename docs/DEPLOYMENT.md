# Deployment boundaries

> **Do not treat this document as a V2 production deployment runbook.** The
> repository has no NixOS-native delivery artifact, and a shared production
> host is not authorized for a Compose startup merely because it has a source
> checkout. The V1 material below is archival only. Never run its commands on
> a host carrying production workloads.

For a NixOS development or isolated-test node, first follow the
[NixOS development-node boundary](NIXOS_DEVELOPMENT_NODE.zh-CN.md). It permits
source synchronization and offline checks only until a separate runtime test is
explicitly approved.

## Historical V1 prototype (archival; not a V2 deployment path)

### 1. Configuration

Generate independent random values for `COPILOTMAIL_JWT_SECRET` and `COPILOTMAIL_ADMIN_KEY`. Set `COPILOTMAIL_DOMAIN` to the mail domain, for example `agents.example.com`. Do not commit `.env`.

### 2. DNS and network

For `agents.example.com`:

```text
mx.example.com.       A     PUBLIC_IPV4
agents.example.com.   MX 10 mx.example.com.
```

Forward public TCP/25 to the Compose host and set `SMTP_PORT=25`. Permit outbound TCP/25 for direct MX delivery. Put HTTP behind an HTTPS reverse proxy; WebSocket upgrade must be enabled for `/v1/events`.

### 3. Start and inspect

```bash
cp .env.example .env
docker compose up -d --build
docker compose ps
docker compose logs -f copilotmail
curl https://mail-api.example.com/healthz
```

The named volume `copilotmail-data` stores SQLite data. Back it up while the service is stopped, or use SQLite's online backup mechanism.

### 4. Security boundaries

- Keep `X-Admin-Key` server-side; it can create mailboxes.
- Rate-limit login, mailbox creation, sending and WebSocket connection attempts at the proxy.
- Redact the `token` query parameter from access logs.
- Restrict API CORS origins before internet exposure; the development default allows all origins.
- Add TLS/STARTTLS, DKIM signing, bounce processing and a durable retry queue before relying on external delivery for production workloads.
- Rotate any credential that was pasted into chat or logs.

## V2 isolated Pilot / development control plane

The sections above describe the retained V1 prototype and are not a V2
production runbook. V2 development uses `deploy/compose.dev.yaml`; a
single-host Pilot must additionally apply `deploy/compose.pilot.yaml` as the
last override so CPU, memory, PID, health, and graceful-stop policy cannot be
silently omitted. Optional capabilities use `deploy/compose.mail-poc.yaml`,
PostgreSQL, enterprise OIDC/SCIM, file-mounted Agent and audit keys, and a
replaceable mail data plane.

For an isolated Ubuntu/WSL Pilot, first follow the
[Ubuntu / WSL Pilot 部署手册](UBUNTU_WSL_PILOT.zh-CN.md) and pass its
read-only, fail-closed host preflight.

Before every V2 PostgreSQL schema upgrade, follow the
[logical backup, isolated restore, and consistency runbook](BACKUP_RESTORE.zh-CN.md).
It uses native PostgreSQL clients, keeps all source sessions read-only, restores
only to a script-generated database, and fails unless migration, immutable-audit
trigger/catalog, and seeded-row evidence match.

The latest repository migration is currently
`0056_human_mail_sent_directory_index.sql`. Migration 0055 commits the
sent-directory audit catalog entry; 0056 is deliberately an online
`CREATE INDEX CONCURRENTLY` migration. Run the normal single migrator only,
wait for it to finish, and require the read-only `check` command to report the
schema ready before starting this binary. Do not substitute a manual
non-concurrent index build on a live submission table.
Migration 0053 still has an additional stop-the-world requirement: an
installation at 0052 or earlier must stop every old API, Outbox Worker, and
audit-maintenance binary before migration; old and new binaries cannot overlap. Run the new
`migrate` command separately with the governed `MailDataPlaneId` and canonical
origin. That command first commits the SQL migration and then performs the
append-only origin bind, so a bind failure must leave all services stopped until
the exact configuration is corrected and `migrate` is safely retried. Run the
read-only `check` command to verify the registered ID, adapter kind, and origin,
then atomically start only the new binaries. The exact commands are in the
[Ubuntu / WSL Pilot 部署手册](UBUNTU_WSL_PILOT.zh-CN.md).

The Compose stack builds separate React administration and Human mail
containers on ports 8083 and 8084. Each proxies `/api` to the control plane so
the browser keeps a same-origin BFF boundary. The Rust control plane performs
OIDC Authorization Code + S256 PKCE and returns only a Host-only HttpOnly
opaque session cookie; provider tokens never enter either SPA. Configure the
two public clients, exact origins, tenant/mailbox bindings, and a read-only
32-byte session key as described in [Human OIDC BFF 会话](HUMAN_OIDC_SESSION.zh-CN.md)
and [企业管理端](ADMIN_APP.zh-CN.md).

`COPILOTMAIL_MAIL_APP_MAILBOX_ID` is optional and is only an initial UI
preference. The authenticated `GET /api/v2/mailboxes` directory remains
authoritative; an absent, unavailable, or unauthorized preference is ignored.
Never treat this client-visible value as a mailbox grant or access-control input.

Human sessions are opt-in in the base development stack so an unconfigured
checkout can still run health and migration checks. Enable the fail-closed
overlay after creating `deploy/secrets/human-session.key`:

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.human-session.yaml \
  -f deploy/compose.pilot.yaml up --build
```

Production must publish both application origins and exact callback paths over
trusted HTTPS. The reverse proxy must suppress query-bearing access and error
logs for `/api/v2/auth/{admin|mail}/{login|callback}`. Database login admission
is a cluster-wide hard stop; edge IP/connection rate limits remain mandatory.
Every request still re-evaluates Active Human, tenant, RBAC, resource ABAC, and
approval evidence.

Set `COPILOTMAIL_HUMAN_LOGIN_REQUESTS_PER_MINUTE` in `deploy/.env.v2` to the
cluster-wide per-minute login-start limit for each registered browser client
(`admin` and `mail` are counted independently). The accepted range is
`1..=100000` and the Compose default is `600`. The base API service owns this
single default; `compose.human-session.yaml` inherits it. This database limit
is not an IP reputation control, so the HTTPS edge must still enforce
source-aware request and connection limits.

The optional `audit-maintenance` Compose profile runs a dedicated,
single-tenant Checkpoint/export/WORM worker. It requires explicit tenant and
`audit.export` Service Principal IDs, read-only RSA key files, and a persistent
work volume so remote/DB split failures resume the exact signed package. See
[审计 WORM 归档](AUDIT_WORM_ARCHIVE.zh-CN.md).
Workers connected to the same PostgreSQL cluster use a tenant-scoped session
advisory lock for cross-host single-active execution. Separate database
clusters still require scheduler-level isolation.
The worker exposes Prometheus metrics and `/healthz`/`/readyz` on port 9090
inside the Compose network. Readiness fails on configured consecutive-failure,
pending-checkpoint-age, or cycle-duration thresholds.

An optional observability overlay adds an internal OpenTelemetry Collector,
Tempo, Prometheus, and Grafana:

```bash
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.observability.yaml \
  -f deploy/compose.pilot.yaml \
  --profile audit-maintenance up --build
```

The application exports only bounded span metadata; JSON logs remain on
standard output. The overlay binds Grafana and Prometheus to loopback and
enables anonymous Grafana Viewer access, so it is development-only. Production
requires HTTPS/mTLS to an authenticated Collector, enterprise Grafana auth,
external durable storage, HA, alerting, and independently governed log
collection. Configuration and verification are documented in
[可观测性与分布式 Trace](OBSERVABILITY.zh-CN.md).

For the current Stalwart pilot, follow
[Stalwart 邮件数据平面 PoC](STALWART_POC.zh-CN.md) and
[邮件数据平面事件接入](MAIL_EVENT_INGRESS.zh-CN.md). Production must expose
the Stalwart callback only over an internal HTTPS path and mount separate
Bearer, HMAC, and JMAP credentials. Configure only the documented inbound and
queue-delivery event allowlist; the queue evidence remains operational and
must not be treated as a heuristic JMAP Submission correlation. Completing
First-time recovery credentials are isolated in
`deploy/compose.mail-bootstrap.yaml`; omit that one-time overlay after the
permanent administrator is verified. The normal Mail overlay requires every
mail adapter and event-ingress dependency to be ready and cannot be weakened
through `.env.v2`. Completing those configuration steps does
not replace the outstanding public-MX, queue-recovery, anti-abuse, HA,
cross-failure-domain backup/restore, and upgrade acceptance tests listed in the
implementation plan.
