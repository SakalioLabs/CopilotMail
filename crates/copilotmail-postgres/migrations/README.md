# Control-plane migrations

These migrations are forward-only and are applied by `copilotmail-control migrate`.
Production rollback restores the pre-upgrade PostgreSQL backup or runs an explicitly
reviewed compensating migration. Application code must never edit the mail engine's
private schema.

- `0001_control_plane.sql` establishes the initial control-plane schema.
- `0002_organization_bootstrap.sql` records the root service identity for an
  idempotently bootstrapped organization and enforces tenant consistency.
- `0003_rbac.sql` creates tenant-scoped roles, permissions, and time-bounded
  assignments, then backfills the owner grant for existing bootstraps.
- `0004_oidc_owner_handover.sql` constrains external human mappings and records
  the one-time, auditable handover from bootstrap service to Human Org Owner.
- `0005_scim_directory.sql` adds hashed, rotatable provisioning credentials
  and the tenant-scoped SCIM Human projection.
- `0006_agent_oauth.sql` adds Agent `private_key_jwt` public credentials,
  assertion replay protection, and the least-privilege Agent runtime role.
- `0007_agent_mtls.sql` adds certificate-fingerprint credentials for native
  TLS client authentication and certificate-bound Agent access tokens.
- `0008_mailbox_authorization.sql` hardens mailbox membership and delegation
  tenant constraints and indexes resource-level authorization resolution.
- `0009_approval_workflow.sql` adds immutable policy versions, scoped approval
  requests, append-only decisions, and database-enforced separation of duty.
- `0010_audit_governance.sql` installs the governed event catalog, explicit
  request/policy/approval context, tenant-bound references, and catalog immutability.
- `0011_audit_hash_chain.sql` backfills and enforces a concurrent, tenant-scoped
  SHA-256 audit chain, canonical persisted hash payloads, and truncate rejection.
- `0012_signed_audit_checkpoints.sql` binds immutable chain prefixes to versioned
  RSA signatures, exact signed payloads, tenant event foreign keys, and append-only
  checkpoint storage.
- `0013_audit_worm_archives.sql` records exact S3 Object Lock versions, checksums,
  retention evidence, and append-only archive completion events.
- `0014_mail_domain_governance.sql` reserves globally unique canonical mail
  domains, stores challenge-bound immutable DNS observations, and separates
  versioned route enablement from domain verification state.
- `0015_mail_domain_application_events.sql` registers the domain lifecycle,
  DNS observation, activation, suspension, and route-change audit vocabulary.
- `0016_mail_engine_domain_projection.sql` adds bounded Outbox retry scheduling
  and lookup support for stable mail-engine domain bindings.
- `0017_mail_engine_projection_status.sql` records pending, ready, and failed
  engine reconciliation state without exposing credentials or vendor payloads.
- `0018_mail_domain_challenge_rotation.sql` versions domain ownership
  challenges and registers the audited rotation action without storing
  plaintext ownership tokens.
- `0019_webhook_delivery_ledger.sql` hardens subscription bounds and records
  append-only retry, delivery, and dead-letter outcomes without response bodies.
- `0020_webhook_subscription_governance.sql` restricts externally deliverable
  event types, enforces tenant-bound Agent ownership, makes delivery evidence
  immutable, and registers subscription lifecycle audit actions.
- `0021_webhook_dead_letter_replay.sql` indexes privileged dead-letter
  investigation and governs audited, append-only manual replay.
- `0022_agent_event_cursor.sql` indexes the externally safe Outbox feed and
  governs non-empty Agent cursor reads without auditing message payloads.
- `0023_mail_engine_event_ingress.sql` adds versioned tenant mailbox bindings,
  append-only authenticated source receipts, two-key replay constraints, and
  the governed Human binding action.
- `0024_agent_event_sse.sql` governs successful bounded Agent SSE stream
  establishment; event delivery continues to use the subscription-scoped
  Cursor feed rather than a separate message state.
- `0025_audit_event_search.sql` indexes tenant/actor audit searches and
  registers the governed, fail-closed audit query action.
- `0026_compliance_legal_holds.sql` adds immutable compliance cases and
  exact-version Legal Hold evidence with approval and lifecycle constraints.
- `0027_audit_export_grants.sql` adds immutable, short-lived audit export
  grants bound to one archive, one requesting Human, and one dual approval.
- `0028_audit_export_downloads.sql` adds the append-only download ledger and
  database revalidation that every served object matches the grant, archive,
  requesting Human, exact object version, digest, size, and validity window.
- `0029_retention_policies.sql` adds immutable tenant retention-policy
  identities and versions, Active Human dual-approval activation evidence,
  monotonic version activation, and a dedicated `retention.manage`
  capability.
- `0030_mail_drafts_and_submission_intents.sql` gives remote JMAP drafts stable
  tenant/actor/mailbox provenance, irreversible freeze/sent transitions, and
  a leased recoverable submission-intent ledger whose idempotency and request
  hashes are immutable.
- `0031_mail_submission_terminal_audit.sql` registers accepted, rejected, and
  manual-review terminal evidence produced atomically by the leased submission
  executor.
- `0032_mail_delivery_evidence.sql` adds immutable, recipient-level JMAP
  delivery snapshots for an exact accepted submission, with deferred
  database validation that aggregate counts match every stored recipient
  outcome.
- `0033_outbound_delivery_trace_evidence.sql` stores authenticated Stalwart
  queue-level delivery, expiry, reschedule, and completion evidence without
  remote response text or raw recipient addresses.
- `0034_approval_request_center.sql` adds tenant/requester and actionable queue
  indexes, immutable cancellation/query audit events, guarded one-way request
  transitions, and delete/truncate protection for approval history.
- `0035_builtin_role_catalog.sql` backfills the complete nine-role
  least-privilege system catalog for every tenant; runtime organization
  bootstrap creates the same catalog for future tenants.
- `0036_governed_role_assignments.sql` adds dual-approval grant evidence,
  bounded assignment indexes, immutable role catalogs and grant ledgers, and
  one-way revocation-only assignment mutation guards.
- `0037_agent_event_stream_leases.sql` adds exact tenant/Agent/subscription
  short-lived leases for cluster-wide SSE concurrency enforcement.
- `0038_agent_ingress_rate_limits.sql` adds database-authoritative, one-minute
  tenant and active-Agent request windows for cluster-wide Cursor/SSE ingress
  throttling.
- `0039_agent_mail_submission.sql` permits exact active development/testing
  Agents to own drafts and durable submission intents without weakening
  mailbox Send ABAC, CAS, or idempotency bindings; production Agents remain
  fail closed pending quota, approval, and loop controls.
- `0040_agent_mail_content.sql` adds the explicit Agent plain-text read
  permission and governed content-disclosure audit event.
- `0041_agent_mail_policy_versions.sql` adds immutable, Human-authored
  production-Agent outbound policy candidates with canonical recipient-domain
  allowlists and hard quota/first-contact/auto-reply safety bounds. Exact live
  dual approvals are recorded in a monotonic immutable activation ledger.
- `0042_agent_mail_admission.sql` stores only canonical recipient domains and
  scoped address digests with each draft, adds exact `agent.first_contact`
  approvals, and serializes production-Agent domain, hourly, per-recipient,
  and thread-circuit-breaker checks into an immutable admission ledger.
- `0043_agent_message_processing.sql` hardens self-bound mailbox processing
  subscriptions, fans inbound messages into independent Agent state, enforces
  lease-owned Claim/extend/Ack/Nack transitions, and records immutable
  idempotency evidence without changing Human read or flag state.
- `0044_agent_processing_dead_letter_query.sql` adds the owner-scoped
  dead-letter pagination index and governed query audit vocabulary.
- `0045_agent_processing_subscription_lifecycle.sql` adds guarded
  pause/resume/terminal-retirement transitions, exact idempotency evidence,
  immutable lifecycle history, and retirement protection for live leases.
- `0046_explicit_mail_reply_semantics.sql` binds a draft to one safe inbound
  message UUID with a tenant/mailbox composite foreign key and makes that
  reply target immutable across replacement, freeze, and sent transitions.
- `0047_agent_automatic_reply_loop_guard.sql` persists trusted automatic-reply
  and tenant/Agent-scoped exact-body evidence, blocks Testing Agents from the
  deliverable Intent path, adds immutable non-delivering Sandbox captures,
  and enforces automatic-source and duplicate-body circuit breakers.
- `0048_human_mail_search_audit.sql` registers the privileged, query-content-free
  Human mailbox search audit action used by the application transaction boundary.
- `0049_human_browser_sessions.sql` adds digest-only, one-time OIDC login
  transactions, opaque Human browser sessions, append-only revocations,
  cluster-wide login admission, bounded cleanup of expired unsuccessful
  pre-authentication state, and atomic session Audit/Outbox evidence.
- `0050_human_mail_content_reads.sql` registers governed Human message-body
  read evidence and adds a tenant/mailbox/current-binding covering index for
  projecting immutable inbound engine document references to safe message
  UUIDs without creating identities on demand.
- `0051_mail_engine_account_ownership.sql` permanently binds every mail-engine
  account reference to its first tenant mailbox. Its transactional upgrade
  locks legacy binding writers, recovers retired accounts from append-only
  audit history, and rejects ambiguous history before installing the
  concurrent claim trigger. Current bindings may rotate forward, but a retired
  account can never be reassigned across a mailbox or tenant after an in-flight
  content operation resolved it. CI exercises the real `0050 -> latest`
  ownership upgrade and conflict rollback with
  `scripts/check_mail_engine_account_ownership_upgrade.sh`.
- `0052_mail_engine_account_owner_insert_guard.sql` locks both sides of the
  ownership boundary, strictly revalidates the persisted successful-binding
  audit payload, append-only repairs missing proven owners, and rejects
  ambiguous, conflicting, or unexplained ownership with `23514`. A deferred
  constraint trigger then requires every new owner claim to have the exact
  current binding at commit, preventing standalone INSERTs from claiming an
  unbound or differently bound account and preventing failed binding CAS
  attempts from forging permanent ownership.
- `0053_mail_data_plane_identity.sql` introduces an append-only stable UUID
  and canonical-origin registry for independently operated mail data planes,
  repairs the pre-existing permission-catalog and Agent credential-scope drift
  by admitting `agent.mail.read` to `role_permissions_permission_check` and to
  both Agent credential scope constraints (raising their bounded cardinality to
  four) before another tenant bootstrap,
  backfills the legacy Stalwart installation, and moves mailbox bindings,
  permanent account ownership, ingress replay keys, drafts, submission
  intents, delivery evidence, domain projection affinity, and internal Outbox
  routes to that UUID. Legacy provider strings remain as historical and
  rollback evidence but no longer route or scope opaque references. This is a
  stop-the-world upgrade: stop all old API and Worker binaries, back up, run
  the migration, and atomically start only the new binaries. Mixed-version or
  rolling operation across 0053 is unsupported.
- `0054_human_mail_draft_reads.sql` registers the governed
  `mail.drafts.queried` and `mail.draft.read` evidence used by the Human Mail
  draft directory and exact restore flow. A metadata page is selected from the
  caller's active drafts, then disclosed only after Active Human, mailbox RBAC,
  Read/Send access, the current data-plane binding, and every disclosed Draft
  UUID/owner/active version are rechecked in the transaction that appends Audit
  and Outbox records. Restored content has the stronger exact recheck in that
  transaction: remote identifiers and persisted content/recipient evidence must
  also still match before disclosure.
- `0055_human_mail_sent_directory.sql` registers the governed
  `mail.submissions.queried` action used by a Human's own sent directory. It
  creates no mutable projection: the
  application selects only accepted Intents whose frozen Draft is durably
  `sent`, optionally reads the newest immutable delivery **aggregate**, and
  rechecks Active Human, `mailbox.use`, Read/Send access, requester ownership,
  accepted/sent provenance, and every disclosed snapshot identity in the
  Audit/Outbox transaction. Recipient evidence, SMTP replies, message content,
  Draft/provider references, and current data-plane binding remain outside the
  directory contract.
- `0056_human_mail_sent_directory_index.sql` is intentionally marked
  `-- no-transaction` and uses `CREATE INDEX CONCURRENTLY` to add the
  acceptance-time partial index without blocking submission writes. SQLx's
  migration advisory lock remains in effect. A retry after a process failure
  uses `IF NOT EXISTS`; runtime readiness independently requires the resulting
  index to be valid and ready before serving this binary.
  `scripts/check_mail_engine_account_ownership_upgrade.sh` follows the same
  SQLx 0.9 prefix: only a file whose bytes start with `-- no-transaction` runs
  outside a transaction. After a file succeeds, the script records the version,
  description, and SHA-384 checksum `schema_ready` compares. A failed file
  returns before that insert, so a rolled-back migration is not marked applied.
- `0057_agent_inspector.sql` registers the successful, privileged,
  content-free single-Agent Inspector read. The inspector remains a
  transaction-bound projection of existing identity, credential, RBAC, mail
  policy, and processing records; it creates no mutable snapshot table.
- `0058_agent_mail_submission_evidence.sql` registers the successful,
  privileged, Human-only exact production-Agent mail-submission evidence read.
  It creates no mutable evidence projection: the read is bound to an explicit
  Agent UUID and submission UUID, and can disclose only immutable
  admission/policy-approval facts, an optional latest aggregate delivery
  observation, and minimal audit references. Runtime health, execution,
  delegation, mailbox, recipient, provider/SMTP, and message-content data stay
  outside this contract.
- `0059_retention_runtime_authorization.sql` keeps existing retention history
  immutable, but assigns new activation timestamps from the database after
  approval lock waits and rejects expired approval evidence when advancing an
  active version. Application audit/outbox uses the returned stored timestamp.
  This migration must be applied and exercised on an isolated database during
  the deferred deployment-validation stage; compilation is not SQL execution.
- `0060_audit_export_runtime_clock.sql` assigns grant/download timestamps from
  the database after mutable actor/approval locks. Backdated application input
  cannot revive an expired approval or export grant; returned timestamps are
  used by audit/outbox records. Historical immutable evidence is not rewritten.
- `0061_human_session_governance.sql` adds `session.revoke_others` to the
  permission and approval-action checks, grants it to existing Organization
  Owner roles, registers `identity.human_sessions.revoked`, and creates the
  append-only `human_session_governance_executions` table. It does not rewrite
  session or approval history. This migration has not been applied in this
  code-only change. A later local disposable PostgreSQL 17.11 run, database
  name `copilotmail_test_<suffix>`, not CI and not a deployment, applied this
  file.
- `0062_human_login_failure_ledger.sql` creates the append-only
  `human_login_failures` ledger. Rows carry only a closed reason code, a
  client key, a database-stamped timestamp, and an optional salted source
  digest; update, delete, and truncate are rejected and pruning is bounded.
  It holds no OAuth material, cookies, tokens, PKCE data, or plaintext
  request metadata, and it is not tenant audit data. This migration has not
  been applied in this code-only change. A later local disposable
  PostgreSQL 17.11 run, database name `copilotmail_test_<suffix>`, not CI
  and not a deployment, applied this file.
- `0063_mail_draft_creation_attempts.sql` registers
  `mail.draft_attempts.queried` and creates the content-free
  `mail_draft_creation_attempts` ledger. One row is appended before each
  remote provider draft write and is marked reconciled only inside the local
  draft-metadata transaction; every other update, every delete, and any
  truncate is rejected. No body, recipient, provider object reference, or
  secret enters the table. This migration has not been applied in this
  code-only change. A later local disposable PostgreSQL 17.11 run, database
  name `copilotmail_test_<suffix>`, not CI and not a deployment, applied
  this file.
- `0064_agent_mcp_session.sql` registers `agent.mcp_session.opened` so the
  MCP-over-WebSocket agent channel records the same governed
  connection-opened evidence as the SSE event stream. The session row lives
  only in audit and outbox evidence; no token, Origin, or transport payload
  is stored. This migration has not been applied in this code-only change.

Offline SQL and PL/pgSQL grammar checks use the pinned PostgreSQL 17 parser in
`scripts/requirements-quality.txt`: run
`python scripts/check_postgres_syntax.py --self-test` after installing those
development-only dependencies. Parsing does not execute migrations. Runtime
readiness now also requires the complete compiled migration ledger with exact
checksums, so an older schema cannot silently miss these trigger changes.

Integration tests require `COPILOTMAIL_ALLOW_ISOLATED_DB_TESTS=1` and a dedicated
`copilotmail_test_<suffix>` database URI without query/fragment overrides.
The only exception is the loopback `copilotmail` service in CI. Never point
this write/migration suite at a shared or production database.
