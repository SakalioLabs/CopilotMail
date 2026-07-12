# Architecture

CopilotMail is a modular monolith: one deployable Rust binary contains independently testable protocol, API, persistence, event, and web modules. This keeps operations simple without coupling the browser UI to internal code.

```text
Internet SMTP :25 ---> smtp.rs ----> store.rs (SQLite WAL)
                                  +-> events.rs (broadcast)
Browser/Agent :8080 -> api.rs -----+-> outbound.rs -> DNS MX -> remote SMTP :25
                  |-> WebSocket
                  `-> web/ static frontend
```

## Components

- `smtp.rs`: Tokio SMTP state machine, envelope validation, dot unescaping, RFC 5322 parsing, and local mailbox delivery. No mail-catcher service is used.
- `outbound.rs`: MX lookup and direct SMTP delivery with multiline response handling and dot stuffing. No SMTP relay is required.
- `api.rs`: Axum JSON API, Argon2 mailbox login, JWT authorization, mailbox/message operations, sends, WebSocket upgrade, and static frontend delivery.
- `store.rs`: SQLite WAL database. Mailbox passwords are Argon2 hashes; messages retain parsed text/HTML and raw RFC 5322 data.
- `events.rs`: bounded Tokio broadcast channel. API WebSockets filter events by authenticated mailbox ID.
- `web/`: browser client that calls only the public HTTP and WebSocket APIs.
- `skills/copilotmail/`: distributable Agent Skill, API reference, and deterministic standard-library client.

## Trust boundaries

The administrator key may create mailboxes. Mailbox JWTs can only access their own mailbox ID and messages. SMTP recipients must exist locally and match the configured domain. The browser is untrusted and never receives the administrator key.

## Known production hardening work

The first release intentionally keeps deployment self-contained. Internet-scale operation should add SMTP STARTTLS, DKIM signing, SPF/DMARC guidance, durable outbound queues with retries and bounce state, quotas, per-origin CORS, and reverse-proxy rate limits.
