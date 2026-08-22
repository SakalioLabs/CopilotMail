# ADR 0015: Verified S3 Object Lock audit archives

- Status: Accepted
- Date: 2026-07-26

## Context

Signed checkpoints and portable exports prove origin and internal continuity, but a
mutable filesystem cannot provide regulated retention. A successful object upload
also does not prove that versioning, Object Lock, checksum, and retention were
actually applied.

## Decision

Introduce a dedicated S3-compatible adapter behind the audit workflow. It verifies
bucket Versioning and Object Lock, conditionally creates each object, supplies a
SHA-256 checksum and per-object retention, and reads the exact returned version
back. Only matching version, checksum, length, metadata, mode, and retention date
produce archive evidence.

The command verifies the portable export with a separately trusted public key
before any upload. It archives the events and manifest separately, then records
both exact versions in an append-only PostgreSQL table. The archive row,
`audit.archive.completed`, and its outbox event commit atomically.

S3 and PostgreSQL are not presented as one distributed transaction. A stable
archive ID, conditional creation, and exact replay make recovery explicit when
remote writes succeed before database recording.

Production requires HTTPS and Compliance mode. Credentials come from the AWS SDK
provider chain so workload identity can replace static keys. Development Compose
uses an Object Lock enabled MinIO bucket and Governance mode.

## Consequences

- “Archived” now means verified remote retention evidence, not PUT success.
- Operators can retry partial failures without overwriting evidence or producing a
  second database completion event.
- Bucket policy, legal hold, replication, and administrator separation remain
  infrastructure governance responsibilities.
- The AWS SDK is pinned exactly because generated-client patch drift can break
  reproducible builds.
