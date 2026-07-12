# Production deployment

## 1. Configuration

Generate independent random values for `COPILOTMAIL_JWT_SECRET` and `COPILOTMAIL_ADMIN_KEY`. Set `COPILOTMAIL_DOMAIN` to the mail domain, for example `agents.example.com`. Do not commit `.env`.

## 2. DNS and network

For `agents.example.com`:

```text
mx.example.com.       A     PUBLIC_IPV4
agents.example.com.   MX 10 mx.example.com.
```

Forward public TCP/25 to the Compose host and set `SMTP_PORT=25`. Permit outbound TCP/25 for direct MX delivery. Put HTTP behind an HTTPS reverse proxy; WebSocket upgrade must be enabled for `/v1/events`.

## 3. Start and inspect

```bash
cp .env.example .env
docker compose up -d --build
docker compose ps
docker compose logs -f copilotmail
curl https://mail-api.example.com/healthz
```

The named volume `copilotmail-data` stores SQLite data. Back it up while the service is stopped, or use SQLite's online backup mechanism.

## 4. Security boundaries

- Keep `X-Admin-Key` server-side; it can create mailboxes.
- Rate-limit login, mailbox creation, sending and WebSocket connection attempts at the proxy.
- Redact the `token` query parameter from access logs.
- Restrict API CORS origins before internet exposure; the development default allows all origins.
- Add TLS/STARTTLS, DKIM signing, bounce processing and a durable retry queue before relying on external delivery for production workloads.
- Rotate any credential that was pasted into chat or logs.

