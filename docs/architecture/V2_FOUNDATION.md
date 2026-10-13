# CopilotMail V2 foundation

## Process roles

The initial V2 binary is `copilotmail-control`:

- `serve`: V2 HTTP liveness, readiness, and versioned API host.
- `worker`: durable Transactional Outbox worker.
- `migrate`: applies PostgreSQL control-plane migrations.
- `check`: verifies database connectivity and schema readiness.
- `check-mail-engine`: performs authenticated JMAP discovery against the
  configured, replaceable mail data plane without opening PostgreSQL.
- `check-domain-dns`: performs a bounded, read-only DNS policy preflight from
  a challenge token file without database configuration or state mutation.

Later phases add authenticated human, Agent, and administration interfaces
without changing the domain-to-adapter dependency direction.

## Local startup

```bash
docker compose -f deploy/compose.dev.yaml up --build
curl http://127.0.0.1:8081/healthz
curl http://127.0.0.1:8081/readyz
```

The legacy prototype remains on port 8080. V2 defaults to port 8081 so both can
run during migration.

The optional Stalwart PoC is isolated in `deploy/compose.mail-poc.yaml`. Its
adapter consumes only documented JMAP discovery and management capabilities,
reads the API key from a mounted file, rejects redirects and cross-origin
advertised API URLs, and contributes `mail_data_plane` to `/readyz`. See the
[Stalwart PoC runbook](../STALWART_POC.zh-CN.md).

The system-wide trust boundaries, abuse cases, implemented controls, residual
risks, and Pilot release gates are tracked in the
[enterprise threat model](../THREAT_MODEL.zh-CN.md). A feature crossing a new
external boundary must update that model and its negative verification before
it is treated as production-ready.

After migrations, bootstrap the first tenant through the offline CLI:

```bash
copilotmail-control bootstrap-organization \
  --slug example-corp \
  --display-name "Example Corporation"

copilotmail-control bind-oidc-owner \
  --organization-id 01900000-0000-7000-8000-000000000000 \
  --subject stable-idp-subject \
  --display-name "Initial Organization Owner"

copilotmail-control issue-scim-token \
  --organization-id 01900000-0000-7000-8000-000000000000 \
  --label "Enterprise IdP provisioning" \
  --expires-in-days 90
```

Bootstrap does not create a reusable administrator secret. It atomically creates
the organization, an active root service principal retained for audit
continuity, an audit event, and an Outbox event. PostgreSQL advisory locking
makes concurrent calls for the same canonical slug idempotent. The root identity
must later be governed through the configured enterprise identity and policy
plane. The OIDC owner handover creates the first active Human Owner and then
revokes every bootstrap role assignment and permanently retires the bootstrap
Service Principal in the same transaction.

All HTTP responses propagate `x-request-id`. Machine-readable failures carry a
stable error code, request ID, trace ID, retryability, and safe structured
details.

## Authorization boundary

Authentication adapters may verify enterprise OIDC, Agent OAuth2, mTLS, or
workload identity, but they only produce an internal principal ID. PostgreSQL
then resolves the principal's active, tenant-scoped role assignments. Every
application action requires an explicit permission and matching trusted tenant;
request path data never establishes tenancy.

The built-in Organization Owner role deliberately excludes content review and
break-glass permissions. The first protected organization endpoint therefore
fails closed while no trusted authentication adapter is configured.

The human authentication adapter uses OIDC Discovery and a refreshable JWKS
cache. It requires an exact issuer, audience, subject, expiry, RS256 signature,
and key ID before resolving `(issuer, subject)` to an active Human Principal.
Provider redirects are disabled and production provider URLs require HTTPS.
Set `COPILOTMAIL_OIDC_ISSUER` and `COPILOTMAIL_OIDC_AUDIENCE` together;
`COPILOTMAIL_REQUIRE_HUMAN_AUTH=true` makes a missing provider fail readiness.
See
[ADR 0003](../adr/0003-default-deny-authorization.md) and the
[V2 OpenAPI contract](../openapi-v2.yaml).

SCIM is an independent HTTP adapter mounted at `/scim/v2`; its bearer tokens
are not interchangeable with OIDC access tokens. It projects Users onto the
same Human Principal/HumanProfile identity and stores Groups as tenant-scoped
directory policy inputs. Token digests, expiry, revocation, user lifecycle,
group membership, audit, and Outbox writes remain on the PostgreSQL side of the
application ports. See [ADR 0006](../adr/0006-scim-authoritative-directory.md)
and the [SCIM operations guide](../SCIM.zh-CN.md).

Agent authentication is a separate adapter using OAuth 2.0 Client Credentials
and RFC 7523 `private_key_jwt`. Agent Principal IDs are client IDs and each
rotatable public-key credential has its own JWT `kid`. Assertion replay
protection and live credential/RBAC checks remain behind application ports, so
revocation invalidates already-issued tokens on the next protected request.
See [ADR 0007](../adr/0007-agent-private-key-jwt.md) and the
[Agent operations guide](../AGENT_AUTH.zh-CN.md).

The optional native rustls listener requests and verifies Agent client
certificates against a dedicated CA. The verified leaf fingerprint exists only
as in-process connection evidence. mTLS-issued access tokens carry RFC 8705
`cnf.x5t#S256` and protected requests require the same certificate, in addition
to live credential and RBAC validation. See
[ADR 0008](../adr/0008-native-mtls-certificate-bound-tokens.md).

The Agent authorization server shares one atomically replaceable signing key
ring between token issuance, verification, and public JWKS discovery. A
file-backed adapter can prepublish verification keys, switch the active signer,
and retain old keys for the maximum token lifetime without restarting. See
[ADR 0009](../adr/0009-hot-reloaded-agent-signing-key-ring.md) and the
[signing-key rotation runbook](../AGENT_SIGNING_KEY_ROTATION.zh-CN.md).

The Agent MCP channel is a JSON-RPC 2.0 WebSocket adapter mounted at
`GET /api/v2/mcp` under the `mcp` subprotocol. It reuses the same Agent
scoped-token authenticator, live authorization resolution, ingress limits,
application use cases, and error contract as the REST surface; the transport
contains no business logic. Session establishment records
`agent.mcp_session.opened` audit and outbox evidence atomically before the
upgrade is accepted. See the [MCP channel guide](../MCP.zh-CN.md).

## Transaction invariant

Privileged state changes are complete only when all three writes commit in one
PostgreSQL transaction:

1. domain state;
2. append-only audit event;
3. durable outbox event.

An API must not report success before that transaction commits. Workers claim
outbox rows with `FOR UPDATE SKIP LOCKED` semantics and recover abandoned leases.

## Data ownership

- CopilotMail owns organizations, principals, policy, delegation, Agent
  processing state, audit, approvals, idempotency, and integration delivery.
- The selected mail engine owns RFC message content, folders, protocol state,
  and SMTP delivery queues.
- Stable IDs map across the boundary; private mail-engine tables are not an API.
