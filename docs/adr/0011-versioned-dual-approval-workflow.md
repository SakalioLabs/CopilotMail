# ADR 0011: Versioned, scoped dual approval

- Status: Accepted
- Date: 2026-07-25

## Context

High-risk actions such as Send As, content review, audit export, privilege
elevation, Legal Hold, break-glass, and bulk deletion require accountable human
control. A mutable boolean such as `approved=true` cannot prove which policy,
resource, action, approvers, or validity window authorized an operation.

Approval must not turn into a second authorization system that grants access
otherwise denied by tenant RBAC or mailbox ABAC.

## Decision

An approval request is bound at creation to:

- one tenant;
- one immutable policy version;
- one action;
- one resource kind and stable resource ID;
- one requester;
- one reason and optional ticket;
- a required number of distinct approvers;
- a maximum 30-day validity window.

Policy changes append a new `policy_versions` row. Existing requests retain the
exact version that governed their creation. Policy versions and approval steps
are append-only at the database layer.

The requester cannot approve their own request. Each approver may decide only
once. One rejection is terminal. Approval requires the configured number of
distinct positive decisions and expires at the request boundary.

Decision processing locks the request row before appending a step and updating
aggregate state. The state mutation, audit event, and Outbox event commit in one
transaction.

At execution time the approval gate evaluates:

`base identity/RBAC/resource authorization AND exact approval proof`

It checks tenant, request ID, action, resource kind, resource ID, approved
decision count, absence of rejection, and expiry. A denied base decision returns
false without consulting approval storage.

## Consequences

- An approval cannot create or broaden authority.
- Reusing an approval for another mailbox, query, tenant, or action fails.
- Concurrent decisions serialize on the request and cannot double-count one
  approver.
- Approval evidence remains attributable to the policy version active when the
  request was submitted.
- The first implementation requires `authorization.manage` for decisions.
  Later policy versions may narrow eligible approver groups, but may not remove
  separation of duty.
- Production migration verification runs against PostgreSQL 17 in CI; local
  environments without PostgreSQL can only compile the adapter and tests.
