# ADR 0032: Testing Agent Mail Sandbox and Loop Guards

- Status: Accepted
- Date: 2026-07-27
- Decision owners: Security, Mail Platform, Agent Platform

## Context

Testing Agents previously shared the same deliverable submission path as
development Agents. A mistaken environment selection or API call could
therefore create a real JMAP EmailSubmission. Production policy had a
thread-level limiter, but the platform did not yet persist trusted
`Auto-Submitted` classification or detect exact repeated bodies across
threads.

RFC 3834 recommends that automatic responders do not respond to messages whose
`Auto-Submitted` value is anything other than `no`. The marker must be chosen
by the trusted mail adapter, not by an API caller.

## Decision

1. All Agent-authored drafts receive a server-derived RFC 3834
   `Auto-Submitted` header: `auto-generated` for new mail and `auto-replied`
   for explicit replies. Human drafts do not.
2. Explicit-reply source lookup reads a bounded `Auto-Submitted` value from
   the exact authorized JMAP Email. The derived automatic-source flag becomes
   immutable draft evidence.
3. PostgreSQL rejects every Agent submission that replies to an automatic
   source.
4. Every draft stores a tenant- and principal-scoped SHA-256 exact-body
   reference. PostgreSQL serializes Agent submissions and rejects the eleventh
   identical body in a rolling 24-hour window.
5. Testing Agents cannot create deliverable submission Intents. They use a
   dedicated sandbox capture that freezes the exact draft, creates immutable
   minimized evidence, and atomically commits Audit and Outbox records without
   calling a mail delivery adapter.
6. Both application and database layers enforce the Testing environment
   boundary. Sandbox evidence rejects update, delete, and truncate.

## Consequences

- A Testing Agent cannot be used for real delivery, even if application
  routing regresses.
- Agent-originated mail is interoperably identifiable by downstream automatic
  responders.
- Exact duplicate detection is deterministic and privacy-minimized, but it is
  not semantic similarity detection and deliberate small mutations require
  other quotas and abuse controls.
- A draft captured in Sandbox is frozen and cannot later be submitted. A new
  draft version is required for another test.
- The design adds one Agent-environment lookup during draft creation and an
  Agent-scoped advisory lock during Sandbox or formal submission.

## Rejected alternatives

- Redirect Testing mail to a sink address: rejected because address rewriting
  can regress and does not prove absence of a delivery Intent.
- Trust a request-body `sandbox` or `automatic` flag: rejected because a
  compromised Agent could choose the weaker path.
- Compare raw bodies in policy queries: rejected because it expands sensitive
  data exposure.
- Treat the production thread limiter as sufficient: rejected because loops
  may cross thread identities and repeated campaigns may vary recipients.

## Verification

- Rust adapter/application/contract tests.
- PostgreSQL migration and ignored integration scenario in CI.
- Audit catalog governance and transaction coverage gate.
- Agent Skill route-drift test and OpenAPI validation.
- Pilot-only Stalwart header round-trip, concurrency, and Agent-to-Agent loop
  drills remain required.
