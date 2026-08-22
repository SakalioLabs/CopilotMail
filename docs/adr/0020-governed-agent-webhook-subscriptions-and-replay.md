# ADR 0020: Govern Agent webhook subscriptions and append-only replay

- Status: Accepted
- Date: 2026-07-26

## Context

Outbound webhooks cross the enterprise trust boundary. A wildcard subscription,
an arbitrary owner, a secret stored in control-plane rows, or a replay that
rewrites history could expose identity/audit events and destroy forensic
evidence. Human operators and Agents also need different interaction semantics
over the same tenant and authorization model.

## Decision

- Externally routable event types use an explicit versioned allowlist. Wildcard
  subscriptions are rejected in the domain, application, database, and Worker.
- A subscription belongs to one active Agent in the same tenant. An Agent may
  manage only its own subscriptions with `agent.operate`; managing another
  Agent additionally requires `identity.manage` in both RBAC and token scope.
- Endpoints are HTTPS-only and exclude userinfo, query, and fragment. Rows store
  only a bounded signing-key reference, never a signing secret.
- Updates use a monotonic version and compare-and-swap. Disabling is a distinct
  governed audit transition.
- Dead-letter reads and manual replay require `security.admin`, which is absent
  from Agent token scopes.
- Delivery evidence is append-only. Replay locks the exact subscription and
  original Outbox event, verifies the latest result is `dead_lettered`, appends
  a `manual_replay_requested` retry marker, and reschedules the original event
  in the same transaction as audit and governance Outbox writes.
- Retry budgeting is calculated since the latest replay marker. Prior attempts
  remain immutable and visible for forensics.

## Consequences

Successful subscriptions are not redelivered when one failed subscription is
replayed. Repeated or concurrent replay fails once the latest result is no
longer a dead letter. Operators cannot replay disabled subscriptions. Cursor
recovery remains a separate protocol and does not weaken the append-only
delivery ledger.
