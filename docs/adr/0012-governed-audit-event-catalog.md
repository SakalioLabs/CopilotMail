# ADR 0012: Governed audit event catalog

- Status: Accepted
- Date: 2026-07-25

## Context

Free-form audit action strings make it possible to misspell an event, change its
resource semantics, or add a privileged mutation without updating compliance
documentation. Free-form JSON can also accidentally persist credentials or
message content.

## Decision

The domain owns a closed audit catalog containing action, exact resource type,
risk, and privileged classification. `AuditEvent::new` rejects unregistered
actions and mismatched resource types.

The same catalog is installed by migration into `audit_event_catalog`.
`audit_events(action, resource_type)` references it, and both catalog rows and
audit rows reject update/delete.

Request, subject, source, client/session, token JTI, policy, approval, ticket,
environment, region, occurrence, and receipt context use explicit fields rather
than being hidden in free-form details. Tenant-scoped references use composite
foreign keys.

Details are limited to 64 KiB and recursively reject secrets, credentials,
message bodies, raw MIME, and attachment content.

A repository verifier compares domain, migration, and application producer
actions. It also rejects application transaction commits without audit and
Outbox writes, except explicitly marked idempotent no-op replays.

## Consequences

- Adding an audit action requires one reviewed catalog and migration change.
- Typos and action/resource drift fail before persistence.
- PostgreSQL independently rejects unregistered event pairs.
- Application code cannot silently introduce an unaudited state-changing
  transaction without failing CI.
- The catalog only covers implemented producers; future mail, domain,
  compliance, export, and operational capabilities must extend it as they land.
- Hash chaining, signed checkpoints, WORM archive, and offline verification
  remain separate subsequent controls.
