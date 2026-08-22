# ADR 0001: V2 modular control plane

- Status: Accepted
- Date: 2026-07-25

## Context

The prototype combines HTTP, SMTP, SQLite, authentication, event delivery, and
static web hosting in one crate. That shape is useful for demonstrating the
happy path but does not provide enforceable dependency boundaries, durable
events, independently scalable process roles, or enterprise audit guarantees.

## Decision

CopilotMail V2 uses a Cargo workspace and a modular-monolith codebase:

- Domain crates contain no HTTP, SQL, or mail-engine dependencies.
- Application crates own use cases and infrastructure ports.
- Adapter crates implement PostgreSQL, object storage, identity provider, and
  mail-engine boundaries.
- API, worker, migration, audit ingestion, and control commands run as separate
  processes while sharing one versioned codebase.
- PostgreSQL Transactional Outbox is the initial durable event source.

The existing V1 crate remains temporarily available as a regression baseline.

## Consequences

- Process roles can scale and fail independently without early microservices.
- Infrastructure can be replaced without rewriting business rules.
- CI must test the entire workspace and enforce dependency direction.
- V1 and V2 coexist during migration, so their deployment ports and environment
  variables must remain distinct.
