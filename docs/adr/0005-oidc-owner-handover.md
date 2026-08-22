# ADR 0005: One-time OIDC owner handover

- Status: Accepted
- Date: 2026-07-25

## Context

OIDC token validation alone cannot create a trustworthy first administrator:
the external issuer/subject still needs an explicit internal Principal and role.
Leaving the bootstrap service active after that mapping would preserve a second,
non-human privileged identity indefinitely.

## Decision

CopilotMail provides an offline `bind-oidc-owner` operation after organization
bootstrap and OIDC configuration:

- The issuer is taken from validated process configuration, never from a free
  command argument.
- The operator supplies the organization ID, stable OIDC `sub`, and display
  name.
- A per-organization PostgreSQL advisory lock serializes concurrent handovers.
- One transaction creates and activates the Human Principal, writes the exact
  HumanProfile mapping, grants the existing Org Owner role, records the
  immutable handover marker, appends audit and Outbox events, revokes all active
  bootstrap role assignments, and retires the bootstrap Service Principal.
- An exact replay returns the same Human Principal without new writes. A
  different issuer or subject cannot replace the initial owner through this
  command.
- The OIDC subject is not included in audit details or integration events.

## Consequences

The bootstrap identity remains available only as historical audit attribution
and cannot authenticate or authorize after handover. Subsequent owner changes
must use authenticated role-management and approval workflows rather than the
offline bootstrap path. Disaster recovery uses a separately designed,
audited break-glass workflow.
