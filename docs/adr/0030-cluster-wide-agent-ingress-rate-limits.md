# ADR 0030: Cluster-wide Agent Cursor/SSE ingress rate limits

- Status: Accepted
- Date: 2026-07-27

## Context

Subscription-scoped cursors and bounded SSE connections prevent unbounded
responses, but a valid Agent credential could still exhaust API and PostgreSQL
capacity by opening short requests across multiple control-plane replicas.
Per-process counters are not authoritative in a horizontally scaled service.

## Decision

Authenticated Agent requests to Cursor pull, SSE open, draft save, durable
submission creation, and submission status endpoints consume two one-minute
PostgreSQL windows before cursor parsing, mail data-plane access, or state
lookup:

- one tenant-wide window;
- one exact active-Agent window.

The database clock defines the window. A tenant advisory transaction lock makes
both increments atomic across replicas. If the Agent window is exhausted, the
tenant increment rolls back. PostgreSQL triggers require an exact active Agent,
allow only monotonic counter increments, reject deletion of live windows and
reject truncation. Expired windows may be deleted as operational state.

Human governance callers do not consume Agent ingress capacity. Limits are
validated so the Agent value is positive and no greater than the tenant value.
Rejection happens before HTTP 200 with a stable retryable `429` code and a
bounded `Retry-After` value derived from the database clock. Store failure is
fail-closed as retryable `503`.

Metrics use only the fixed `allowed`, `tenant_limited`, `principal_limited` and
`failure` outcomes. Tenant, Principal, subscription, token and message values
are never metric labels.

## Consequences

The fixed window is simple and deterministic, but allows a burst on both sides
of a minute boundary. Capacity testing must choose limits with that worst case
in mind. PostgreSQL is in the admission path; its latency and lock contention
require load testing before Pilot.

This decision covers the current Agent Cursor/SSE and outbound mail API. It
does not claim to limit OAuth token issuance, Human administration, internal
mail-engine callbacks or Webhook egress; each new ingress boundary must declare
and test its own admission policy.
