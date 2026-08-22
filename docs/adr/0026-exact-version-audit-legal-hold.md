# ADR 0026: Exact-version audit Legal Hold

- Status: Accepted
- Date: 2026-07-26

## Context

Audit exports consist of two independently versioned S3 objects: the event
stream and its signed manifest. A database-only flag would not protect either
remote version, and S3 cannot atomically update Legal Hold across both objects.

## Decision

The first Legal Hold boundary covers one immutable audit archive and both exact
S3 Version IDs.

- Every hold belongs to an open compliance case.
- Apply and release require `legal_hold.manage`, an active Human, and distinct
  approved `compliance.legal_hold` requests.
- Apply approval targets `audit.archive/<archive-id>`; release targets
  `compliance.legal_hold/<hold-id>`.
- A PostgreSQL session advisory lock keyed by tenant and archive serializes the
  remote-operation saga across control-plane hosts.
- Every S3 mutation is followed by an exact-version readback. PostgreSQL changes
  only after both versions return the requested state.
- Approval IDs are single-consumption keys; exact replays are idempotent and
  conflicting reuse fails closed.
- Cases and holds use controlled one-way transitions and reject deletion or
  truncation.
- Critical audit events carry the exact Approval Request ID and commit with the
  state transition and Outbox event.

For release, PostgreSQL remains `active` until both versions read back `Off`.
Partial remote work returns an error and is completed by retry. Dropping the
lock guard closes its database session so cancellation cannot leak a lock.

## Consequences

The database never claims completion based only on caller intent. A partial
release remains an operational incident because S3 has no two-object
transaction, but local compliance state remains active and the operation is
recoverable.

The model can later cover versioned mail-content evidence. It does not yet
place ordinary mailbox messages or attachments on hold.
