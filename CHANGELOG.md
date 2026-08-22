# Changelog / 变更记录

All notable CopilotMail V2 changes are recorded here at release level. Detailed
implementation evidence remains in
[the implementation status](docs/IMPLEMENTATION_STATUS.zh-CN.md).

重要的 CopilotMail V2 变更在此按发布级别记录；逐项实现证据以
[实施状态](docs/IMPLEMENTATION_STATUS.zh-CN.md)为准。

## [Unreleased]

### Added

- Modular Rust V2 control plane with PostgreSQL migrations, transactional
  Outbox, governed append-only audit evidence, and replaceable external
  adapters.
- Separate Human OIDC browser-session and Agent OAuth2/mTLS authentication
  paths with default-deny RBAC, Scope, tenant, and mailbox authorization.
- Human mail and administration React applications with Simplified Chinese and
  English localization.
- Human Mail responsive list/detail navigation, accessible modal composition, and
  generation-bound asynchronous state that discards stale draft, submission, and
  inbox responses.
- Agent processing subscriptions, leases, Ack/Nack, dead letters, Cursor/SSE
  recovery, signed Webhooks, idempotent mail submission, and non-delivering
  Testing-Agent Sandbox.
- Governed mail-domain lifecycle, DNS verification, Stalwart adapter boundary,
  WORM audit archives, Legal Hold, retention policies, and two-person
  approvals.
- Docker development/Pilot delivery, observability overlay, backup/restore
  verifier, and security governance checks.

### Security

- Privileged operations fail closed when live authorization or audit evidence
  cannot be established.
- Browser and Agent credentials, provider identifiers, recipient evidence, and
  mail content are minimized or excluded from public responses and audit data.
- Immutable evidence tables and migration/readiness gates detect disabled or
  missing enforcement controls.
- Audit-export downloads now require an active Human Principal, verify the
  exact object version in bounded chunks before returning bytes, distinguish
  integrity conflicts from transient object-store failures, and record each
  verified retrieval in the immutable audit/outbox chain.
- The Web build chain pins patched `nanoid` 3.3.18, resolving
  GHSA-2v37-7h3g-55p8 in the prior transitive dependency.

### Known acceptance boundaries

- Enterprise IdP interoperability, real-domain public mail delivery, anti-spam
  and malware integration, HA, capacity, disaster recovery, and security
  acceptance remain open.
- The legacy Ubuntu WSL Pilot is no longer a validation dependency because of
  recurring I/O risk. Any future runtime test requires a separately approved,
  isolated Linux Docker environment; a shared NixOS host currently holds only
  the development snapshot and offline checks.

## [0.1.0-prototype]

- Original self-hosted Agent-mail MVP retained as a regression baseline.
- This version is not a supported production mail data plane.
