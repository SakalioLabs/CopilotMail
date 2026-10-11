# CopilotMail

Language: [Language entry](README.md) · [简体中文](README.zh-CN.md) · [English](README.en.md)

CopilotMail is evolving from a self-hosted mail MVP for AI agents into an
enterprise mail system where humans and agents are both first-class
principals. The repository preserves the V1 prototype as a regression baseline
while building a modular Rust V2 control plane. The handwritten V1 SMTP server
is not intended to become the production mail data plane.

Current work is code-only; deployment is deferred. Business applications own
verification-code generation and validation. Existing Human, Agent, and Admin
APIs remain, with standard SMTP/IMAP access for application email and external
mail clients. The unconnected hosted-verification prototype has been archived
and removed from the build. See the [current code scope](docs/CODE_DEVELOPMENT_SCOPE.zh-CN.md)
and [external-client boundaries](docs/EXTERNAL_MAIL_CLIENTS.zh-CN.md).

See the [code-quality review](docs/CODE_QUALITY_REVIEW.zh-CN.md) for modularization,
authorization/transaction fixes, test traceability and measured coverage.
Deployment acceptance comes last. Unexecuted PostgreSQL, Object Lock and real-client
scenarios are not passing evidence, and static gates do not replace final review.

The authoritative product objective is in [GOAL.md](GOAL.md). See the
[enterprise implementation plan](docs/IMPLEMENTATION_PLAN.zh-CN.md), current
[implementation status](docs/IMPLEMENTATION_STATUS.zh-CN.md),
[design and interaction contract](DESIGN.md),
[Chinese design-system implementation guide](docs/DESIGN_SYSTEM.zh-CN.md),
[Chinese V2 visual-refactor execution spec](docs/DESIGN_DEVELOPMENT_SPEC_V2.zh-CN.md),
[Chinese design development blueprint](docs/DESIGN_DEVELOPMENT_BLUEPRINT.zh-CN.md),
[design governance gate](docs/DESIGN_GOVERNANCE.zh-CN.md),
[Agent Inspector control-plane plan](docs/AGENT_INSPECTOR_API_PLAN.zh-CN.md),
[enterprise threat model](docs/THREAT_MODEL.zh-CN.md),
[P0 enterprise input and sign-off checklist](docs/ENTERPRISE_INPUT_CHECKLIST.zh-CN.md),
[V2 OpenAPI](docs/openapi-v2.yaml), and [V2 AsyncAPI](docs/asyncapi-v2.yaml)
for scope and evidence. Detailed implementation documentation is currently
Chinese-first and is being translated incrementally.

> Upgrading an existing V2 database from Migration 0052 or earlier to 0053 is
> a stop-the-world operation. Do not run a full-stack `up --build`: stop the old
> API, Worker, and audit-maintenance processes, take the required backup, run
> the new `migrate` command separately, verify the data-plane origin binding,
> and then start only the new binaries. Follow the
> [Ubuntu / WSL Pilot runbook](docs/UBUNTU_WSL_PILOT.zh-CN.md).

> The latest repository migration is `0061_human_session_governance.sql`; the new migrations have not been deployed. On 2026-10-12 a local disposable PostgreSQL 17.11 database named `copilotmail_test_<suffix>` (not CI) applied migrations 0001–0061 and ran the ignored integration tests. That run is not a deployment and is not a CI pass. The
> 0053 stop-the-world rule still applies only to upgrades that cross 0053;
> Migrations 0054 through 0061 and the full dynamic chain require a clean, isolated
> PostgreSQL acceptance run. NixOS is currently a constrained development or
> isolated-test node only, not an approved deployment target; see the
> [NixOS development-node boundary](docs/NIXOS_DEVELOPMENT_NODE.zh-CN.md).
> Migration 0056 uses `CREATE INDEX CONCURRENTLY`: let exactly one normal
> migrator complete it, then require read-only `check` before starting this
> binary; never replace it with a blocking manual index build on production
> submission data.

## Current V2 capabilities

- Modular Rust workspace with domain, application, contract, PostgreSQL, S3,
  identity, mail-engine, DNS, webhook, audit, and platform boundaries.
- Unified Human, Agent, and Service Principal model with default-deny RBAC and
  resource-aware authorization.
- Server-side Human OIDC Authorization Code + S256 PKCE, opaque Host-only
  HttpOnly browser sessions, and SCIM 2.0 user/group provisioning. Provider
  tokens never enter either SPA; see the
  [Human OIDC BFF session guide](docs/HUMAN_OIDC_SESSION.zh-CN.md).
- Agent OAuth2 Client Credentials with `private_key_jwt`, short-lived scoped
  tokens, native mTLS binding, overlapping rotation, and immediate revocation.
- A distributable [`copilotmail` Agent Skill](skills/copilotmail/SKILL.md)
  and dependency-free V2 CLI for policy inspection, processing subscriptions,
  Claim/Lease/Ack/Nack, dead-letter and Cursor recovery, bounded plain-text
  reads, and idempotent Draft/Submission sending. The CLI consumes a
  short-lived token file or ephemeral environment value and never creates or
  prints credentials. It also exposes a non-delivering Sandbox capture for
  Testing Agents.
- Production-Agent outbound mail requires an immutable Human-authored policy,
  exact two-person activation, and a PostgreSQL-atomic admission that enforces
  recipient domains, hourly/per-recipient quotas, a conservative thread
  circuit breaker, and exact first-contact approval. Only scoped recipient
  digests enter the control plane, and the Worker cannot lease a production
  Intent without committed admission evidence. A principal-bound
  `GET /api/v2/agent/mail-policy` endpoint exposes the Agent's own effective
  permissions, active policy, and aggregate hourly usage without recipient or
  thread ledger evidence.
- All Agent-authored drafts receive a server-derived RFC 3834
  `Auto-Submitted` marker. The JMAP adapter classifies exact reply sources,
  Agent replies to automatic sources fail closed, and an independent rolling
  24-hour exact-body breaker rejects the eleventh duplicate. Testing Agents
  cannot create deliverable Intents; they can only freeze an exact draft into
  immutable evidence with `delivery_suppressed=true`. Both application and
  PostgreSQL enforce this boundary. See the
  [Agent Sandbox and loop-guard guide](docs/AGENT_MAIL_SANDBOX_AND_LOOP_GUARDS.zh-CN.md).
- Inbound automation uses processing subscriptions that are separate from
  Human read/flag state and webhook delivery. An Agent with mailbox Read ABAC
  can Claim a message under a bounded lease, extend it, Ack it, or Nack it for
  retry and eventual dead-lettering. Exact `Idempotency-Key` replay is stable;
  foreign, mismatched, and expired leases fail closed. The owning Agent can
  page only its terminal failures through a subscription-bound, validated
  cursor without receiving raw provider errors or another principal's state.
  Idempotent lifecycle transitions support pause, resume, and terminal
  retirement. Pause retains durable inbound backlog, stops new Claim work,
  and allows a valid lease to drain; retirement is rejected while a live
  lease remains.
- Versioned two-person approval workflows with an audited, bilingual approval
  center for actionable and requester-owned queues, decisions, and cancellation.
- An immutable nine-role catalog and audited, time-bound role assignments.
  Grants consume exact two-person privilege-elevation approvals; revocation
  protects the tenant's last effective Organization Owner.
- Governed append-only audit events, tenant hash chains, signed checkpoints,
  offline verification, and S3 Object Lock/WORM archives.
- Compliance cases and exact-version Legal Hold for immutable audit archives.
- Signed Agent webhooks, bounded cursor recovery, dead-letter replay, and SSE.
- Governed mail-domain lifecycle, DNS evidence, a stable UUID identity for each
  remote-reference namespace, server-selected binding/data-plane affinity, and a
  replaceable adapter boundary. Stalwart v0.16 is currently the only adapter;
  this does not claim multi-provider support.
- A Human-only mail slice with an authorized mailbox directory, Shared Mailbox
  switching, an exact readable `role=inbox`, privacy-preserving bounded search,
  safe plain-text detail, read/flag state, serial auto-saved drafts, a private
  own-draft directory, integrity-checked restore editing, a private accepted-
  submission/delivery-status directory, and recoverable formal submission.
  Mail-session-only `GET /api/v2/mailboxes`
  requires an active Human with `mailbox.use` and lists only mailboxes with a
  current data-plane binding and effective resource Read. Its minimized items
  expose only mailbox UUID, kind, primary address, and `can_send` derived from
  exact `Send(None)`; canonical cursors fail closed after authorization or
  binding drift. A SendAs-only mailbox remains read-only. The composer shows an
  explicit immutable From address, while switching mailboxes invalidates stale
  requests and clears search, detail, selection, and draft state. The optional
  deployment `mailboxId` is only an initial preference and grants no access.
  The picker groups server-returned mailboxes into Human, Agent, Shared, and
  Group `optgroup` sections. Selecting an Agent mailbox binds the visible
  identity, address, and subsequent inbox request to that exact mailbox. A
  zero-result search has its own empty state; a detail `503` retains the
  selected-message context and retries in place. Draft restore confirms before
  replacing local edits; after a `409`, it refreshes the latest server metadata
  and retries CAS only against that exact version without forking the draft.
  The current browser contract proves this path with metadata version 5.
  Shared Mailbox membership never exposes another principal's drafts: list
  items contain no subject, address, or body, and a directory page is disclosed
  only after its atomic `mail.drafts.queried` audit/Outbox commit. Restore performs one exact
  PostgreSQL-proven JMAP `Email/get`, verifies the Drafts mailbox, `$draft`,
  From, Thread, and content evidence, then returns content only after the final
  authorization/version recheck and atomic `mail.draft.read` audit/Outbox commit.
  The sent directory lists only the caller's durable accepted submissions whose
  frozen Draft is sent. It exposes acceptance time, recipient count, and an
  optional aggregate delivery summary—never subject, body, address, recipient
  digest, SMTP reply, Draft/provider/data-plane reference, or remote error.
  It performs no JMAP call, survives a valid data-plane rebind, and rechecks
  Active Human, `mailbox.use`, Read/Send, ownership, accepted/sent provenance,
  and the current delivery-snapshot identity before its
  `mail.submissions.queried` audit/Outbox commit.
  Lists and searches expose only
  pre-existing safe inbound Message UUID projections; raw JMAP Account, Email,
  Thread, and Query State values never enter the browser, cursor, or durable
  evidence. Envelope and body disclosure each fail closed until minimized
  audit/Outbox evidence commits. Human mail responses are `no-store`, and stale
  detail requests are aborted and discarded during rapid selection changes.
  Human authorization and permanent tenant/mailbox ownership of data-plane-scoped
  accounts protect the read boundary. Content, draft, and submission
  credentials are isolated; a frozen Draft, durable Intent,
  query-before-create JMAP reconciliation, lease takeover, and atomic terminal
  audit/Outbox evidence prevent blind duplicate submission after an ambiguous
  response. Although `manual_review` is a server-terminal state, the Human UI
  locks it as exact attention-required evidence: closing the composer only hides
  that evidence, and Compose restores the identical recipients, subject, body,
  and Submission UUID. Creating another draft or submission, switching mailbox,
  replying, and restoring a different draft remain blocked until it is handled.
  After JMAP acceptance, the Worker reconciles exact per-recipient
  delivery state into immutable snapshots. Queue-level Stalwart Webhooks are
  retained in a separate immutable operational ledger without heuristically
  changing user-visible state; public-network bounce and DSN drills remain open.
- Explicit Human/Agent replies accept only a safe inbound Message UUID. The
  server resolves the exact source Email and derives `In-Reply-To` and
  `References`; raw headers and provider Email/Thread identifiers are rejected.
- Optional OTLP/HTTP span export with W3C Trace Context, an OpenTelemetry
  Collector/Tempo development overlay, and local Prometheus/Grafana.
- A bilingual React/TypeScript administration workflow for approvals, role
  governance, immutable audit search, compliance cases, Legal Holds, and an
  explicit, single-Agent control-plane inspector. The Inspector never
  prefetches or invents runtime health; it reads an entered UUID only through
  the server-side OIDC BFF session boundary. Admin locale is restricted to
  `zh-CN`/`en`, persisted strictly, and restores `<html lang>` after reload.
  The session is revalidated at expiry, on window focus, and after a protected
  `401`; an identity or expiry-context change remounts the workspace and removes
  the prior operator's evidence. Case and Legal Hold terminal ledgers and
  committed evidence survive navigation within the same session. Ambiguous
  writes unlock only after exact-read reconciliation. Approval create,
  decision, and cancellation do not yet have a backend idempotency ledger, so
  an unknown result freezes every approval write and permits GET-only
  reconciliation, never another POST. Only operations with an exact
  idempotency contract may retain the identical request for replay; lazy routes
  expose bounded retry or reload recovery.
  Malformed and all-zero Inspector UUIDs are rejected locally without a
  request; `404` causes remain unified, `503` retry follows the response flag,
  and a failed refresh retains the prior snapshot and focus without rendering
  internal errors, traces, or sensitive response fields.

This is active development and has not reached Enterprise GA. Production
internet mail delivery, a complete Human mail application, enterprise IdP
interoperability, HA, migration, and disaster-recovery acceptance remain open.

## Development stack

Copy the non-secret development configuration and start the V2 stack:

```bash
cp deploy/.env.v2.example deploy/.env.v2
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.pilot.yaml up --build

curl http://127.0.0.1:8081/healthz
curl http://127.0.0.1:8081/readyz

# After configuring both public OIDC clients and the 32-byte session key:
docker compose --env-file deploy/.env.v2 \
  -f deploy/compose.dev.yaml \
  -f deploy/compose.human-session.yaml \
  -f deploy/compose.pilot.yaml up --build
```

The API listens on `http://127.0.0.1:8081` and the administration application
on `http://127.0.0.1:8083` by default. Configure the Human OIDC issuer,
audience, browser client IDs/public origins, organization ID, and a mounted
32-byte session encryption key in `deploy/.env.v2` before signing in. The Rust
BFF performs Authorization Code + S256 PKCE and gives the browser only an
opaque HttpOnly session cookie.

Ubuntu/WSL and compatible native-Linux Pilot operators must follow the
[Ubuntu/WSL Pilot runbook](docs/UBUNTU_WSL_PILOT.zh-CN.md) and pass its
read-only, fail-closed preflight before starting the stack.
On a shared NixOS host, also follow the
[NixOS development-node boundary](docs/NIXOS_DEVELOPMENT_NODE.zh-CN.md)
before any runtime action.
Before every V2 PostgreSQL schema upgrade, follow the Chinese-first
[logical backup, isolated restore, and consistency runbook](docs/BACKUP_RESTORE.zh-CN.md).
The verification uses native PostgreSQL clients and does not require Docker.

See the [deployment guide](docs/DEPLOYMENT.md),
[administration application guide](docs/ADMIN_APP.zh-CN.md),
[Human OIDC BFF session guide](docs/HUMAN_OIDC_SESSION.zh-CN.md),
[Human mail application security boundary](docs/HUMAN_MAIL_APP.zh-CN.md),
[Human and Agent authorization matrix](docs/AUTHORIZATION_MATRIX.zh-CN.md),
[Agent authentication guide](docs/AGENT_AUTH.zh-CN.md), and
[audit archive export authorization, exact-version download, and immutable
download auditing](docs/AUDIT_EXPORT_AUTHORIZATION.zh-CN.md), and
[versioned production retention policy governance](docs/RETENTION_POLICIES.zh-CN.md).
See also the Chinese-first
[observability and distributed tracing guide](docs/OBSERVABILITY.zh-CN.md).

## Web development

```bash
npm --prefix web ci
python scripts/check_web_i18n.py --self-test
python scripts/check_design_governance.py --self-test
cd web
npm audit --audit-level=moderate
npm run check
npm test
npm run e2e:install
npm run e2e
```

## Rust verification

```bash
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --locked
python scripts/check_architecture.py --self-test
python scripts/check_audit_governance.py
python scripts/check_agent_skill.py
python scripts/check_documentation.py
python tests/e2e.py
```

`check_architecture.py` is enforced locally and in CI. It resolves the complete
dependency graph from the canonical Domain, Audit, and Application package roots,
rejects outward direct, transitive, aliased, and target-specific edges against each
layer allowlist, and scans every Mail Engine Rust source so neutral ports remain
Application-owned and privately imported by the adapter.

`check_web_i18n.py` is also enforced by CI so production Human Mail components
cannot bypass the typed Simplified Chinese/English catalog and its safe
fallback behavior.

`check_design_governance.py` is also enforced by CI. It prevents raw feature
colours outside the shared token layer, unregistered Image 2 delivery assets,
emoji/dingbat pseudo-icons in locale catalogs, and `EmptyState` illustrations
without explicit Alt text. Its deliberate non-mechanical review boundaries are
documented in the [design governance gate](docs/DESIGN_GOVERNANCE.zh-CN.md).

Each Web Dockerfile runs its application-specific bundle gate during image
construction. The gate rejects source-map files and `sourceMappingURL`, caps
initial and total JavaScript, CSS, and Image 2 WebP/AVIF asset sizes, and has 13
negative self-test fixtures. The Nginx templates return an exact `404` for a
missing `/assets/` path; only successful static assets receive a one-year
immutable cache, while HTML, other responses, and asset `404`s are `no-store`.
These are repository build/configuration guarantees, not evidence of a completed
Docker or production NixOS deployment.

Web Vitest has 390 passing tests (shared design system 16, Admin 195, Mail 179).
`npm run e2e` builds both Web applications and runs 82 isolated Chromium tests
(Admin 53, Mail 29) against loopback-only Vite previews. Service workers are
blocked; a browser network allowlist and local BFF fixtures reject undeclared
APIs and every external request. The 66 Windows PNG baselines (Admin 34, Mail
32) cover both languages, responsive layouts, management list-detail flows,
and important Mail states including search, detail failure, draft conflict,
and locked `manual_review` evidence. Browser contracts also cover Admin session
revalidation, same-session terminal-evidence recovery, ambiguous-write
freeze/reconciliation, route recovery, and the Inspector fail-closed matrix.
Mail covers mailbox-switch binding, in-place detail retry,
`409 → metadata v5 → exact CAS` restore, exact replay after an unknown
submission, rendered 44×44px targets at 390px, and exact manual-review recovery.
Representative pages and states continue to receive strict Axe A/AA scans;
this is not a complete WCAG claim. Use `npm run e2e:update` only for an
intentional, manually reviewed update to those baselines.

These same-origin fixtures do not prove real RBAC, tenant isolation, backend
CAS/idempotency, or real dependencies. They are not a cross-OS pixel baseline
or a claim of complete WCAG 2.2 AA or production IdP/mail-system acceptance.
The remaining Admin information architecture, real touch and screen-reader
testing, and real-dependency acceptance remain open.

The complete Migration 0001→0053 chain and direct SQL invariant checks for
data-plane identity, origin, domain, and Outbox routing have been exercised on
disposable PostgreSQL 17. That evidence does not claim the complete ignored
PostgreSQL suite, a target Pilot upgrade, or the S3-compatible Object Lock
scenarios; those external-runtime acceptance criteria remain separate CI or
Pilot gates.
The repository now contains Migrations 0054 through 0061; the existing 0001→0053
evidence does not cover them or constitute a NixOS target-host runtime
acceptance result.

## Production mail boundary

The V2 control plane delegates SMTP, Submission, IMAP/JMAP, durable queues,
anti-spam, and public delivery to a replaceable production mail data plane.
A `MailDataPlaneId` UUID identifies one stable opaque-reference namespace; it is
not a vendor name. Binding clients submit only `account_ref` and an expected
version, while the authenticated server injects the configured identity and
returns it as `data_plane_id`. Every enabled management, content, compose, and
submission role must share the explicitly configured canonical origin. See
[ADR 0037](docs/adr/0037-stable-mail-data-plane-identity.md).

Stalwart v0.16 is the only adapter currently implemented and may only be accessed
through public protocols or that adapter. Production readiness requires a public-domain pilot
covering TLS, PTR, SPF, DKIM, DMARC, anti-abuse controls, durable retries,
bounces, upgrades, recovery, and non-open-relay verification.

The repository includes a declarative ordinary-account bootstrap plan and a
bounded SMTP/IMAP/JMAP interoperability, non-open-relay, and two-phase restart
durability harness. See the
[Stalwart data-plane PoC runbook](docs/STALWART_POC.zh-CN.md). Its offline
self-tests run in CI; an actual external Stalwart run is still required and is
not implied by those checks.

## License

[Apache License 2.0](LICENSE).
