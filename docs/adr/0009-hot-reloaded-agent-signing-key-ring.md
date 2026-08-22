# ADR 0009: Agent access-token signing uses a hot-reloaded key ring

## Status

Accepted.

## Context

A single configured RSA key requires a coordinated restart and creates either a
token-verification outage or unsafe overlap during rotation. External Agent
clients and internal services also need a stable public-key discovery contract.

## Decision

The authorization server uses an atomically replaceable in-memory key ring. One
key is active for signing and up to nine additional keys may verify tokens
during rotation. Access-token verification selects exactly one key by JWT
`kid`. Key IDs are immutable and cannot be reused for different RSA material.

A JSON manifest references secret files and identifies the active key. A
background adapter polls the manifest and referenced files. It validates the
entire candidate key set before a single write-lock swap; failures retain the
previous ring.

The active key requires a matching PKCS#8 private key and SubjectPublicKeyInfo
public key. Verify-only keys need only the public key. Removal is rejected until
the last possible token lifetime plus clock leeway has elapsed. At startup all
manifest keys are conservatively treated as potential recent issuers, covering
process restarts.

`GET /oauth2/jwks.json` publishes every verification key as RFC 7517 JWKS with
ETag and bounded caching. It never publishes private material.

## Consequences

Operators can prepublish a new key, atomically switch signing, preserve old
tokens, and retire the old key without restarting the service. Every replica
must consume an overlapping manifest during rolling deployment.

The manifest and key files must be updated atomically and supplied by a secret
manager or read-only volume. The first implementation polls files rather than
watching a KMS API and does not perform HSM signing.
