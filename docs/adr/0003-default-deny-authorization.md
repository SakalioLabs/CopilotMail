# ADR 0003: Default-deny tenant authorization

- Status: Accepted
- Date: 2026-07-25

## Context

CopilotMail must authorize humans, Agents, and services without treating any
credential type as inherently privileged. Tenant IDs supplied in URLs or request
bodies cannot establish the caller's tenant. Mail administration must not imply
message-content access, and emergency access must remain a separate workflow.

## Decision

- A cryptographic authentication adapter maps an external credential to one
  internal `PrincipalId`.
- PostgreSQL resolves the active principal's organization and currently valid,
  non-revoked role assignments.
- Application authorization compares that trusted organization with the target
  organization and requires an explicit `Permission`; an empty permission set is
  denied.
- Cross-tenant resource requests return not found to avoid tenant enumeration.
- `Org Owner` does not include `content.review` or `break_glass.invoke`. Those
  capabilities require distinct roles plus later ABAC case/approval context.
- If no trusted OIDC, Agent OAuth2, mTLS, or workload-identity adapter is
  configured, protected HTTP routes fail closed. Static administrator headers
  are not a fallback.

## Consequences

OIDC and Agent authentication adapters can share the same internal authorization
path. PostgreSQL is initially the source of role assignments; cache invalidation
and revocation latency must be measured before caching is introduced. High-risk
permissions remain insufficient on their own until the policy and approval
engine is implemented.
