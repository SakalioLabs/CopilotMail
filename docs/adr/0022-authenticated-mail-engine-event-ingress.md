# ADR 0022: Authenticated mail-engine event ingress

## Status

Accepted for the inbound-message slice. Outbound delivery-status ingestion is
deferred until a send/queue reference can be bound without guessing tenant
ownership.

## Context

The Agent webhook and cursor contracts previously had no real mail data-plane
producer. Consuming Stalwart tables would couple CopilotMail to one vendor and
would bypass a stable authentication, tenant-mapping, and replay boundary.
Telemetry payloads may also contain message identifiers, blob references,
addresses, sizes, or contents that must not become general Agent events.

Stalwart v0.16 signs the exact serialized WebHook request body with
HMAC-SHA256 and Base64-encodes the tag in `X-Signature`. Its SMTP ingest events
carry numeric `accountId`, `documentId`, and `mailboxId` fields.

## Decision

- The internal callback requires two independent file-backed credentials:
  an exact Bearer token and the Stalwart `X-Signature`. Authentication is
  checked before the bounded request body is read, and the HMAC is verified
  before JSON deserialization.
- Only `message-ingest.ham` and `message-ingest.spam` are accepted. Unknown
  event types, unknown fields in the envelope, malformed resource identifiers,
  stale/future timestamps, oversized batches, and unmapped accounts fail
  closed.
- A Human with live `mail.admin` permission creates a versioned mapping from
  the Stalwart account reference to one tenant mailbox. Agent runtime tokens
  cannot perform this governance operation.
- The mail-engine adapter emits a vendor-neutral minimal event. The
  application stores an append-only source receipt and atomically enqueues
  `message.received.v1`. The external payload contains only internal tenant,
  mailbox, and message UUIDs plus `ham`/`spam`; source account/document IDs,
  headers, addresses, blobs, sizes, and contents are excluded.
- Unique source-event and source-message keys provide at-least-once
  idempotency. Reusing a source event ID with changed normalized content is a
  conflict, not a successful replay. A batch commits in full or not at all.
- Machine telemetry is not falsely attributed to a Human audit actor. Its
  immutable source receipt and matching Outbox event are operational evidence.
  The Human mailbox-binding mutation remains in the governed audit catalog.
- Receipt and binding tables reject destructive mutation. Binding changes use
  optimistic concurrency and cannot delete historical receipts.

## Consequences

Inbound SMTP events now drive the existing signed webhook and subscription
cursor path without exposing vendor payloads. Production still requires a
real Stalwart callback exercise and PostgreSQL integration run. Delivery
status remains unavailable until CopilotMail owns a stable outbound queue
reference; accepting those events earlier would risk cross-tenant attribution.

Bearer and HMAC files are re-read for each request, so an atomic file
replacement takes effect without restarting CopilotMail. The current Stalwart
configuration supports one active value for each credential, so coordinated
rotation may briefly reject callbacks; overlapping ingress-key rotation is a
future hardening item.

## Upstream evidence

- [Stalwart WebHooks](https://stalw.art/docs/telemetry/webhooks/)
- [Stalwart event reference](https://stalw.art/docs/ref/events/)
- [Stalwart v0.16.14 WebHook implementation](https://github.com/stalwartlabs/stalwart/blob/v0.16.14/crates/common/src/telemetry/webhooks/mod.rs)
- [Stalwart v0.16.14 message-ingest fields](https://github.com/stalwartlabs/stalwart/blob/v0.16.14/crates/email/src/message/ingest.rs)
