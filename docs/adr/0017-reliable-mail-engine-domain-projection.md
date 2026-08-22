# ADR 0017: Reliable mail-engine domain projection

- Status: Accepted for the Stalwart v0.16 PoC
- Date: 2026-07-26

## Context

Activating a control-plane domain and creating its mail-engine representation
cannot share one database transaction. Calling the engine inside the HTTP
transaction would leave ambiguous state after timeouts and couple domain
governance to one vendor.

Stalwart v0.16 replaced its former REST administration endpoints with
management objects over JMAP. A local mail domain is an `x:Domain` object and
activation is represented by `isEnabled`.

## Decision

- The control-plane transaction remains authoritative and emits an Outbox
  event; no Stalwart call occurs inside that transaction.
- `MailDomainProjector` is a vendor-neutral adapter. The Stalwart implementation
  uses only session-discovered, same-origin JMAP `x:Domain/query`, `get`, and
  `set`.
- Reconciliation is idempotent by exact canonical domain name and then binds
  the opaque engine ID once in `mail_domains.mail_engine_domain_ref`.
- A conflicting ID/name binding fails closed.
- Activation creates or enables the engine domain. Suspension disables it.
  The worker never destroys a domain object, because deletion can orphan
  dependent mail data.
- Failed projections remain unpublished and use bounded exponential retry
  scheduling. A later lifecycle state wins: the worker reads current
  control-plane state rather than blindly replaying stale event payload.
- Before remote I/O, a worker compare-and-swaps the previously observed attempt
  timestamp to a new attempt token. Success and failure writes require the exact
  lifecycle state, data-plane ID, and attempt token, so a slower concurrent
  attempt for the same lifecycle state cannot overwrite a newer result.
- Domain projections expose `not_projected`, `pending`, `ready`, or `failed`
  plus the observed engine enablement, stable error code, and attempt/success
  timestamps. A lifecycle transition to Active or Suspended resets the state
  to pending in the PostgreSQL adapter.
- The Stalwart API key is read from a file and requires only domain query/get/
  create/update permissions. Destroy permission is not required.

## Consequences

The HTTP action can return after durable governance commit while engine
projection is still pending. Domain list/detail responses expose that
eventual-consistency state and never equate lifecycle Active with ready to
receive mail. The PoC still requires a real Stalwart container test; protocol
mocks prove contract handling, not vendor interoperability.
