# ADR 0029: Cluster-wide Agent SSE quotas

## Status

Accepted for implementation on 2026-07-26.

## Context

Agent SSE connections are authenticated, subscription-scoped, re-authorized
while polling, and limited to five minutes. They are nevertheless long-lived
HTTP requests. Without an explicit quota, one valid Agent can consume file
descriptors, tasks, PostgreSQL queries, and proxy capacity.

An in-memory semaphore would only limit one process. A tenant could bypass it
by spreading connections across control-plane replicas, and a rolling restart
would forget every reservation. The quota therefore needs a shared,
fail-closed authority.

## Decision

PostgreSQL stores short-lived `agent_event_stream_leases`.

- Every lease is bound by foreign keys to the exact tenant, active Agent, and
  enabled owned subscription.
- The lease expiry is later than its open time and no more than five minutes
  later, matching the protocol lifetime.
- Acquisition takes a tenant-scoped PostgreSQL transaction advisory lock,
  removes expired leases for that tenant, and counts active tenant and Agent
  leases before inserting.
- Limits are validated process settings. The Agent limit cannot exceed the
  tenant limit.
- Lease insertion, `agent.event_stream.opened` Audit, and its governance
  Outbox event commit in one transaction. A quota or evidence failure returns
  an ordinary machine-readable HTTP error before SSE status `200`.
- Client disconnect triggers best-effort deletion. Expiry remains authoritative
  when a process crashes or cleanup cannot reach PostgreSQL.
- Leases are operational coordination state, not durable evidence. They may be
  deleted after disconnect or expiry, cannot be updated, and never contain an
  IP address, Token, event payload, or message data.

The control plane exposes SSE metrics on a separate configurable listener. It
reports process-local active connections and monotonic open, rejection, stream
error, and cleanup-failure counters without tenant or Principal labels. The
database lease is the quota authority; metrics are not used for authorization.

## Stable behavior

- Tenant capacity rejection: HTTP `429`, code
  `agent_event_stream_tenant_capacity_reached`, retryable.
- Agent capacity rejection: HTTP `429`, code
  `agent_event_stream_principal_capacity_reached`, retryable.
- Storage failure while reserving capacity: HTTP `503`, existing safe storage
  error mapping.
- Quota rejection does not create an “opened” event because no stream opened.
  The rejection counter and ingress access log provide operational evidence
  without creating an attacker-controlled critical audit flood.
- Limits are evaluated after authentication, RBAC/Scope, ownership, and Cursor
  validation so unauthenticated callers cannot probe tenant capacity.

## Consequences

- Quotas hold across replicas and survive rolling restarts.
- Abrupt disconnects can retain a slot for at most five minutes.
- Opening an SSE stream requires PostgreSQL write availability and fails closed
  when the lease cannot be committed.
- The tenant advisory lock serializes only stream acquisition/cleanup for that
  tenant; event polling remains concurrent.
- Production still requires ingress connection/time limits and capacity tests.
