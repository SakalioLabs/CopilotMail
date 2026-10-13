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
- A Human Mail Session-only sent directory for the caller's own durable accepted
  submissions, with paged aggregate delivery status, final authorization
  rechecks, and atomic `mail.submissions.queried` audit/Outbox evidence.
- Agent processing subscriptions, leases, Ack/Nack, dead letters, Cursor/SSE
  recovery, signed Webhooks, idempotent mail submission, and non-delivering
  Testing-Agent Sandbox.
- Governed mail-domain lifecycle, DNS verification, Stalwart adapter boundary,
  WORM audit archives, Legal Hold, retention policies, and two-person
  approvals.
- Owner-private orphan-draft detection: a content-free
  `mail_draft_creation_attempts` row is appended before every remote provider
  draft write and reconciled inside the same transaction that persists local
  draft metadata. `GET /api/v2/draft-attempts` (Human Mail Session only)
  returns the caller's own unreconciled attempts — attempt, mailbox, and local
  draft UUIDs plus the database-stamped attempt time, never content,
  recipients, provider references, or secrets — behind a final
  authorization/ownership recheck and the dedicated
  `mail.draft_attempts.queried` Audit/Outbox commit. Shared-mailbox membership
  never widens the list, HEAD is rejected, and no remote enumeration or
  cleanup path exists.
- Self-service device list: `GET /api/v2/auth/{client}/sessions` returns the
  caller's own still-active browser sessions (session UUID, client key,
  created/expires times, and a `current` flag) after the cookie is resolved to
  the presenting principal. Only persisted non-secret attributes cross the
  contract; opaque tokens, PKCE material, and IdP-side device sessions do not,
  and no per-device revocation is offered.
- Platform-operations login-failure ledger: `human_login_failures` is an
  append-only control-plane table recording only a closed reason code, the
  client key, the database-stamped time, and an optional deployment-salted
  one-way digest of the request source. Login rejection paths append
  best-effort entries that prune expired rows in bounded batches; the only
  read surface is a read-only CLI behind control-plane database credentials.
- Docker development/Pilot delivery, observability overlay, backup/restore
  verifier, and security governance checks.

### Security

- Organization Owners can revoke another principal's current Admin and Mail
  browser sessions with the dedicated `session.revoke_others` permission and
  one exact two-person approval (`security_response`). The caller's current
  session is left in place. A principal can sign out of every current device
  (`logout`); omitting `retain_current_session` also revokes the current
  session. SCIM deactivation revokes that principal's control-plane browser
  sessions (`principal_deactivated`), and an in-flight authorized request fails
  closed at commit once the principal is no longer active. Single-session
  token revocation takes `FOR KEY SHARE` on the append-only session row and
  inserts the unique revocation row, so it can commit while an in-flight
  request holds `FOR SHARE`. That request then fails at its commit recheck.
  `FOR UPDATE` cannot do this: it waits until the request ends. A revocation
  or suspension that commits after the recheck statement returns and before
  `COMMIT` is still not visible. The SCIM commit recheck no longer requires
  `principal.status = 'active'`, because bind already holds `FOR UPDATE` on
  that principal and deprovisioning the token actor updates the status in the
  same transaction. The next call fails at bind. Human-session and Agent
  credential rechecks still require an active principal. Authorized outcomes
  do not disclose whether the target exists or how many sessions were revoked.
  This does not perform IdP back-channel logout or add a step-up requirement.
- Protected request contracts now fail closed on adversarial JSON without
  echoing the payload. `parse_closed_json` covers every V2 request DTO for
  unknown fields, type confusion, bounds, and malformed documents. Axum's
  plain-text JSON, query, and path rejections are rewritten to the existing
  error contract; OAuth and SCIM bodies stay on their protocol schemas. Draft
  subjects, inbox search text, human return paths, and mail domain names also
  reject Unicode format and bidi marks. This is not a fuzzer, pentest, mail
  import, or MX rollout.
- The P1 “request id on every error and worker log” remainder was rechecked
  against the V2 control plane and closed without a new correlation field.
  `ErrorResponse` still carries `request_id` and `trace_id`, and the router
  still propagates `x-request-id`. OAuth and SCIM error bodies stay on their
  protocol schemas. Outbox and audit-maintenance workers keep `worker_id`,
  `event_id`, `cycle_id`, and the stable maintenance `trace_id`.
- Request-authorized audit events now record the verified Human browser
  session id. Agent, SCIM, legacy bearer, and trusted local maintenance paths
  do not receive an invented browser session.
- Privileged operations fail closed when live authorization or audit evidence
  cannot be established.
- Browser and Agent credentials, provider identifiers, recipient evidence, and
  mail content are minimized or excluded from public responses and audit data.
- The sent-directory contract excludes Draft identifiers, content, addresses,
  recipient digests, SMTP replies, provider/data-plane references, and remote
  errors; it never queries JMAP and does not lose local history after a safe
  data-plane rebind.
- The shared submissions route authenticates before decoding its mailbox UUID;
  the Human-only GET route rejects HEAD after Mail Session authentication so a
  bodyless probe cannot create disclosure Audit/Outbox evidence.
- The sent-directory index is created through a separate SQLx
  `CREATE INDEX CONCURRENTLY` migration and is part of the fail-closed schema
  readiness check, avoiding a blocking production submission-table index build.
- Immutable evidence tables and migration/readiness gates detect disabled or
  missing enforcement controls.
- Audit-export downloads now require an active Human Principal, verify the
  exact object version in bounded chunks before returning bytes, distinguish
  integrity conflicts from transient object-store failures, and record each
  verified retrieval in the immutable audit/outbox chain.
- The Web build chain pins patched `nanoid` 3.3.18, resolving
  GHSA-2v37-7h3g-55p8 in the prior transitive dependency.
- The login-failure ledger can never carry OAuth parameters, cookies, tokens,
  PKCE material, `return_to` values, e-mail addresses, or plaintext client
  metadata; updates and truncation are always rejected, deletes only pass for
  rows past the 30-day retention horizon, and it is never joined to tenant
  `audit_events` or exposed to the tenant admin UI.

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
