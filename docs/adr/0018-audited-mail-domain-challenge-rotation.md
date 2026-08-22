# ADR 0018: Audited mail-domain ownership challenge rotation

- Status: Accepted
- Date: 2026-07-26

## Context

The ownership token returned during domain registration is intentionally
one-time. Without rotation, losing that response leaves an administrator unable
to prove control again. Re-registering the same domain is unsafe and is blocked
by the global canonical-name reservation.

Rotation also creates a takeover risk: replacing the proof for a live domain
must not silently preserve an already-active routing state or accept evidence
created for the old secret.

## Decision

Each managed domain has a positive, monotonically increasing
`ownership_challenge_version`, starting at 1. The database stores only the
SHA-256 digest of the current high-entropy token.

The authenticated `mail.admin` rotation use case:

1. locks the tenant-bound domain row;
2. compares the caller's `expected_version` with the locked current version;
3. rejects `active` and `retired` domains;
4. rejects zero, unchanged, or overflowed challenge values;
5. replaces the digest, increments the public version, clears `verified_at`,
   and moves the domain to `pending_dns`;
6. appends `mail_domain.challenge_rotated` audit and Outbox records in the same
   transaction; and
7. returns the server-generated plaintext token exactly once after commit.

An active domain must first be suspended through the separately audited
suspension action. Historical DNS observations remain append-only but current
queries join them to the new digest, so old evidence cannot verify or activate
the rotated challenge.

Audit records, events, logs, and read APIs may contain the challenge version but
must never contain the plaintext token or its digest.

## Consequences

- Lost one-time tokens can be replaced without releasing the global domain
  reservation.
- Rotation is visible to Human and Agent clients through a stable version.
- A live domain cannot retain active routing while ownership proof changes.
- Concurrent requests for the same version cannot both return apparently valid
  tokens; one commits and the other receives a conflict.
- Clients must update DNS and complete verification again after every rotation.
- Retired domains require a separate future recovery policy rather than an
  implicit resurrection path.
