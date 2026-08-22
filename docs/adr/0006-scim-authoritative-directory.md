# ADR 0006: SCIM is authoritative for enterprise directory lifecycle

## Status

Accepted.

## Context

OIDC authenticates an already provisioned human but cannot reliably communicate
joiner, mover, leaver, or group-membership changes. CopilotMail must not create
a second identity namespace that diverges from the enterprise directory.

## Decision

SCIM 2.0 Users map to the existing Human Principal and HumanProfile. The exact
SCIM `externalId` is the OIDC `sub`; OIDC issuer remains deployment
configuration. An existing initial OIDC Owner is adopted by SCIM instead of
duplicated. `active=false` suspends the Principal and therefore fails both OIDC
identity resolution and authorization.

SCIM Groups are tenant-scoped directory resources whose direct members must be
SCIM-managed Users from the same tenant. Group mutations use compare-and-swap
versions. Nested groups are not accepted in the first supported profile.

Provisioning tokens are tenant-scoped, expire, can be revoked, and are stored
only as SHA-256 digests. The token's creator is retained as the audit actor and
must remain active. Every lifecycle mutation, audit record, and Outbox event is
committed atomically.

The supported protocol profile is advertised by `ServiceProviderConfig`.
Unsupported Bulk, sorting, password changes, and complex filters are rejected
rather than silently approximated.

## Consequences

IdP administrators must map SCIM `externalId` to the same stable identifier used
as OIDC `sub`. Changing that identifier is an identity migration, not a normal
profile update. Direct shell access is required for initial token issuance and
emergency revocation until the audited administration UI is delivered.
