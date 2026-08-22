# ADR 0014: Signed audit checkpoints and portable exports

- Status: Accepted
- Date: 2026-07-26

## Context

A hash chain detects internal inconsistency, but a database owner can replace
rows and rebuild the chain until its head is anchored by a separate trust root.
Compliance evidence must also remain verifiable when CopilotMail and its
PostgreSQL database are unavailable.

Embedding a public key in an export would let an attacker replace both evidence
and trust root. Loading a complete tenant history into memory would make large
exports operationally unsafe.

## Decision

A dedicated `copilotmail-audit` module owns versioned checkpoint payloads,
RSA/SHA-256 signatures, portable JSONL records, manifests, and bounded-memory
verification. Version 1 uses `RSASSA-PKCS1-v1_5-SHA256` with 2048–8192 bit RSA
keys. Private and public keys are loaded from explicit files; private material
is never persisted.

The signed payload binds tenant, checkpoint ID, chain range, count, first and
last event, time range, chain root, hash version, key ID, public-key
fingerprint, and creation time. PostgreSQL stores the exact canonical payload
and detached signature. Composite foreign keys bind the first and last claims,
including root hash, to real events in the same tenant. Checkpoints reject
update, delete, and truncate.

Creation requires live `audit.export` authorization. Checkpoint state,
`audit.checkpoint.created`, and Outbox are committed atomically. A stable
checkpoint ID provides exact idempotent replay.

Exports use bounded keyset pages and write `events.jsonl` plus `manifest.json`
into a new directory. Offline verification receives a separately trusted public
key, verifies its fingerprint and signature, then streams the event file while
recomputing every link and the exact file digest. It does not load process
settings or connect to PostgreSQL.

## Consequences

- Evidence can be verified on an isolated host without CopilotMail.
- The export cannot substitute its own public key as a trust root.
- Format and algorithm downgrade attempts fail closed.
- Large exports have bounded application memory use.
- Key IDs and public keys need independent lifecycle and retention management.
- Local file signing is an initial adapter, not the final production key
  custody model.
- S3 Object Lock/WORM upload, KMS/HSM signing, scheduled checkpoints, and
  external timestamping remain subsequent controls.
