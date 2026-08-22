# ADR 0004: Enterprise human authentication through OIDC

- Status: Accepted
- Date: 2026-07-25

## Context

Enterprise users must authenticate through the company's identity provider.
CopilotMail must not become an identity provider, accept long-lived browser
Bearer credentials without validation, or infer an internal user from mutable
claims such as email address.

## Decision

- Human access tokens are verified through an adapter implementing the shared
  authentication port.
- The adapter retrieves standard OpenID Provider metadata and requires the
  discovered issuer to exactly equal the configured issuer.
- Production issuer and JWKS URLs require HTTPS. Redirects are disabled and
  metadata bodies have a fixed size limit.
- The first implementation accepts only RS256 keys selected by a required
  `kid`. Embedded token keys and remote `jku` headers are rejected.
- `exp`, `iss`, `aud`, and `sub` are required; `nbf` is enforced when present.
- Unknown key IDs may trigger JWKS refresh, with concurrency serialization and
  rate limiting. Normal refresh keeps rotation independent from process restarts.
- Only the exact `(issuer, subject)` pair maps to an internal active Human
  Principal. Email, display name, groups, and other mutable claims do not
  establish identity or authorization.
- OIDC configuration can be required by deployment policy and then participates
  in readiness.

The behavior follows [OpenID Connect Discovery
1.0](https://openid.net/specs/openid-connect-discovery-1_0.html) and JWT/JWK
validation semantics. SCIM will provision and deactivate the internal mapping;
OIDC remains responsible for authentication.

## Consequences

Organizations need an explicit initial binding or SCIM provisioning flow before
SSO can succeed. Additional IdP algorithms may be added only through an explicit
allow-list and interoperability tests. Token revocation and session semantics
remain a subsequent P2 deliverable.

The initial binding and bootstrap retirement transaction is defined by
[ADR 0005](0005-oidc-owner-handover.md).
