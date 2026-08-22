# ADR 0007: Agent identity uses private_key_jwt with live credential binding

## Status

Accepted.

## Context

Agents are first-class principals and cannot safely share human sessions,
passwords, static API keys, or one organization-wide secret. Their credentials
must support least privilege, attribution, overlap during rotation, replay
resistance, and emergency revocation.

## Decision

CopilotMail uses OAuth 2.0 Client Credentials with RFC 7523
`private_key_jwt` as the initial Agent authentication profile. The Principal
UUID is the `client_id`; every public-key credential has a separate UUID used
as JWT `kid`. Private client keys never enter CopilotMail.

Client assertions require RS256, exact issuer/subject/client identity, exact
token-endpoint audience, bounded issuance and expiry, and a one-time `jti`.
The `jti` digest is consumed atomically in PostgreSQL. The server issues
short-lived RS256 JWT access tokens with `typ=at+jwt`.

Effective scopes are the intersection of scopes registered on the credential
and current tenant-scoped RBAC permissions. Authentication of every protected
request revalidates the active Agent Principal, credential, expiry, revocation,
and token scopes in PostgreSQL. This live binding deliberately trades a
database lookup for immediate revocation.

Multiple credentials may be active for one Agent. Rotation is add new
credential, switch workload, verify, then revoke old credential. Registration,
rotation, revocation, audit, and Outbox changes commit atomically.

Authorization-server signing keys and Agent client keys are separate trust
domains. Production endpoints require HTTPS and keys are supplied through
read-only secret files.

## Consequences

Agents receive individually attributable, narrowly scoped credentials and
old access tokens stop working immediately after credential revocation.
PostgreSQL availability is part of Agent authentication availability; a stale
local revocation cache is not accepted.

Initial operations use an offline administration CLI. Native mTLS and
certificate-bound tokens are addressed separately by ADR 0008. Workload
identity federation, public signing-key discovery, and managed server-key
rotation remain follow-up decisions.
