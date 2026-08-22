# ADR 0013: Tenant-scoped audit hash chain

- Status: Accepted
- Date: 2026-07-25

## Context

Append-only row triggers stop ordinary update and delete statements, but do not
prove that an export is complete or that rows were not changed through elevated
database access. Concurrent audit writers also need one unambiguous order
without serializing unrelated tenants.

Reconstructing a PostgreSQL `jsonb` representation in another language is
fragile. A verifier must hash exactly the bytes used by the writer, rather than
an independently guessed serialization.

## Decision

PostgreSQL owns hash-chain metadata. A `BEFORE INSERT` trigger obtains a
transaction advisory lock derived from the organization ID, reads that tenant's
head, allocates the next sequence, stores the exact UTF-8 hash payload, and
calculates:

`event_hash = SHA-256(previous_hash || hash_payload)`

The first event omits `previous_hash`. Version 1 payloads cover immutable event,
actor, resource, request, policy, time, and detail fields plus their chain
sequence and hash version. Client-supplied chain metadata is rejected.

Migration backfill assigns a deterministic order by organization,
`occurred_at`, and event ID. Sequence uniqueness and hash lengths are enforced
by constraints. Audit events and the governed catalog reject truncate in
addition to update and delete.

`copilotmail-control verify-audit-chain` opens a repeatable-read, read-only
snapshot, reads one tenant in bounded keyset pages, and recomputes every digest
inside the Rust process. It reports the count, first and last event, chain head,
and hash version as JSON or fails at the exact invalid event.

## Consequences

- Concurrent writers for one tenant form one chain; writers for other tenants
  do not contend on the same advisory lock.
- Persisting the exact hash payload makes verification independent of JSON
  serializer behavior.
- Keyset pagination bounds verifier memory use for large tenants.
- Chain gaps, reordering, cross-tenant mixing, payload edits, and broken links
  are detectable.
- The chain proves consistency, not authenticity. A database owner could
  replace rows and rebuild the chain until a trusted external checkpoint exists.
- Signed checkpoints, export manifests, KMS-backed keys, and S3 Object
  Lock/WORM remain required before claiming offline cryptographic verification.
