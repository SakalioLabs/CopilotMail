# ADR 0016: Challenge-bound bounded DNS evidence

- Status: Accepted
- Date: 2026-07-26

## Context

Mail-domain activation depends on attacker-controlled DNS answers. A resolver
can return excessive data, stale records, private MX targets, a Null MX, weak
DKIM material, or answers for an old ownership challenge. Treating a successful
lookup as authorization would create tenant-takeover, SSRF, and misrouting
risks.

## Decision

- Keep DNS lookup behind the `DnsLookup` port; Hickory is an adapter.
- Disable the system hosts file and query absolute names only.
- Apply one total timeout plus record-count, TXT-size, MX-count, and evidence
  TTL limits.
- Resolve every MX target and reject the complete candidate if any returned
  address is not an acceptable public mail target.
- Follow the IANA special-purpose registries conservatively for IPv4 and IPv6.
- Recognize RFC 7505 Null MX as an explicit no-mail result.
- Require exactly one SPF and DMARC policy; validate the DMARC disposition.
- Decode DKIM RSA public-key DER and require at least 2048 bits.
- Bind each observation to the SHA-256 digest of the current ownership
  challenge and never persist or print the plaintext challenge.
- Produce immutable candidate evidence only. A separate authorized,
  transactionally audited action moves `Verified` to `Active`.

## Consequences

An operator can run a read-only preflight without PostgreSQL, but successful
output cannot enable mail. Conservative address filtering may reject unusual
transition networks; such exceptions require a reviewed policy change rather
than a caller-controlled bypass. DNSSEC validation and recursive-resolver
pinning remain deployment choices for a later hardening increment.
