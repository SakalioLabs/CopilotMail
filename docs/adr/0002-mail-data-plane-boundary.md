# ADR 0002: Production mail data-plane boundary

- Status: Accepted boundary; Stalwart selection remains provisional until the
  P0 proof of concept passes
- Date: 2026-07-25

## Context

Production Internet mail requires substantially more than the prototype SMTP
state machine: SMTP submission, IMAP/JMAP access, TLS, durable queues, retries,
DSNs, authentication results, abuse controls, and protocol interoperability.
Continuing to extend `src/smtp.rs` would couple business development to a large
protocol implementation and security surface.

## Decision

- Evaluate Stalwart first as the Rust mail data plane.
- Keep a `MailEngine` adapter owned by CopilotMail so another mature stack can
  replace it.
- Keep mail-engine-neutral types and port traits in
  `copilotmail-application::mail_ports`. The application layer owns these
  contracts; `copilotmail-mail-engine` implements them and keeps
  provider-specific wire types private.
- Forbid direct and transitive dependencies from `copilotmail-application` to
  `copilotmail-mail-engine` or any other infrastructure adapter. The adapter
  may depend on the application-owned ports, never the reverse.
- Use only documented JMAP, management API, webhook, and telemetry contracts.
- Keep CopilotMail control-plane data and mail-engine data in separate
  PostgreSQL databases or schemas with separate roles.
- Never write directly to private mail-engine tables.
- Preserve the prototype SMTP server only as a regression fixture until V2 mail
  integration replaces its tests.

The first adapter increment implements authenticated JMAP discovery and
management readiness in `copilotmail-mail-engine`. It deliberately treats a
Stalwart management API key as a management credential: API keys do not become
mailbox credentials and cannot be used for user JMAP Mail, IMAP, or SMTP
Submission.

CI runs `python scripts/check_architecture.py --self-test` against both Cargo
manifests and the complete resolved all-features dependency graph from the
canonical Domain, Audit, and Application package roots, including root dev/target
edges and non-workspace intermediate packages. Each root is checked against its
layer allowlist by resolved package identity, so workspace dependency aliases cannot
bypass the rule. The check also treats every Mail Engine `src/**/*.rs` file as one
corpus and rejects additional private imports, re-exports, crate aliases, shadowing,
or adapter-owned definitions of the shared mail-port contracts.

## P0 acceptance gate

- License and support model accepted by the company.
- SMTP/Submission/IMAP/JMAP interoperability demonstrated.
- PostgreSQL/S3 persistence and restart recovery demonstrated.
- Required delivery, authentication, queue, and audit events can be ingested.
- Upgrade, backup, restore, and rollback paths are documented.
- No required enterprise capability forces direct dependency on private schema.
