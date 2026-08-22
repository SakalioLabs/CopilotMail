# ADR 0010: Mailbox resource authorization

- Status: Accepted
- Date: 2026-07-25

## Context

Tenant-scoped RBAC answers whether a Principal may use mailbox features in
general. It cannot answer whether that Principal may read or send through one
specific mailbox. Treating `mailbox.use` as access to every mailbox would turn
an Agent or employee role assignment into tenant-wide message access.

Human and Agent principals require the same security boundary, while their
clients may optimize different interaction flows.

## Decision

Mailbox authorization is the intersection of:

1. an authenticated, active Principal;
2. an exact tenant match established from that identity;
3. the RBAC `mailbox.use` ceiling;
4. a current mailbox membership or explicit time-bounded delegation;
5. an operation-specific mailbox capability.

Direct Owner membership grants all mailbox capabilities. Member membership
grants read and normal send. The legacy Delegate membership label grants
nothing by itself; delegated authority must name explicit scopes.

Delegable scopes are `mailbox.read`, `mailbox.send`, and `mailbox.send_as`.
Delegation management cannot itself be delegated. `mailbox.send_as` additionally
requires the request's `on_behalf_of` Principal to equal the delegation grantor.

Missing, cross-tenant, cross-mailbox, expired, revoked, and insufficiently
scoped access all produce the same hidden result. Persistence uses composite
tenant foreign keys, and unknown stored roles or scopes fail closed.

Mailbox creation and delegation mutation atomically persist the business
change, append-only audit event, and Outbox event.

## Consequences

- `mailbox.use` is never sufficient by itself.
- Resource checks remain identical for Human and Agent callers.
- Revocation and expiry take effect on the next authorization resolution.
- A mailbox API must return a resource-hidden response for all denied cases.
- Approval/case context is a later narrowing layer for high-risk actions; it
  cannot broaden RBAC or mailbox resource authority.
- Existing databases must apply migration `0008_mailbox_authorization.sql`
  before the control plane is considered ready.
