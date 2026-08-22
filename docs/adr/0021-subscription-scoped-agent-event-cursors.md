# ADR 0021: Use subscription-scoped Agent event cursors

- Status: Accepted
- Date: 2026-07-26

## Context

At-least-once webhooks need a pull recovery path when a receiver is unavailable,
loses local state, or needs to reconcile a gap. Exposing the generic Outbox
would leak identity and governance events. Allowing Human subscription managers
to read the feed would also turn `identity.manage` into implicit message access.
Treating an arbitrary UUID as an ordering key could silently skip a range.

## Decision

- Cursor reads are bound to one enabled webhook subscription and its exact
  active Agent owner. Human managers cannot use `identity.manage` to read it.
- Both real-time RBAC and token scope must include `agent.operate`.
- PostgreSQL filters the feed through the subscription's explicit external
  event allowlist, tenant ID, creation time, and owner. Generic Outbox events
  never enter the response.
- The stateless cursor is versioned and contains the subscription and last
  event IDs. The server validates that the anchor is an eligible event for the
  exact subscription before using it.
- Pages are ordered by UUIDv7 Outbox event ID. Late ingestion is therefore not
  lost merely because its business `occurred_at` predates a previously ingested
  event; events predating subscription creation remain intentionally excluded.
- Responses reuse the signed webhook envelope. A non-empty page records only
  count and first/last IDs in audit and governance Outbox evidence; message
  payloads are never copied into audit.
- A missing anchor returns HTTP 410 so clients stop and reconcile instead of
  silently advancing.

## Consequences

Agents can recover webhook gaps and deduplicate push/pull delivery with the same
event ID. Cursor reads remain stateless per consumer and do not mutate delivery
or human read state. A future retention job may delete old Outbox events; its
published retention window must be documented and 410 monitoring must be in
place before that job is enabled.
