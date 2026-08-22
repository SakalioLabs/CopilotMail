# ADR 0019: Signed and SSRF-resistant webhook delivery

- Status: Accepted
- Date: 2026-07-26

## Context

Agent integrations need low-latency event delivery, but a webhook endpoint is
tenant-controlled network input. A naïve HTTP client can expose internal
services through redirects, host-file overrides, private DNS answers, DNS
rebinding, oversized payloads, or secrets embedded in endpoint URLs.

At-least-once delivery also requires a stable receiver identity independent of
an individual attempt.

## Decision

`copilotmail-webhook` is a replaceable outbound adapter. It serializes a stable
JSON envelope containing the event ID, tenant ID, event type, schema version,
business timestamp, aggregate and sanitized data.

Every attempt sends:

- `CopilotMail-Webhook-Id`;
- `CopilotMail-Webhook-Timestamp`;
- `CopilotMail-Webhook-Key-Id`; and
- `CopilotMail-Webhook-Signature: v1=<base64url HMAC-SHA256>`.

The signature input is the exact concatenation of timestamp, event ID and body,
separated by periods. Receivers must validate the timestamp window, compute the
HMAC over the unmodified body, compare in constant time, and deduplicate the
stable event ID.

Production endpoints require HTTPS, may not contain userinfo, query secrets or
fragments, and never follow redirects. DNS uses the host-file-disabled resolver;
every answer must be public and is pinned into the HTTP client for that attempt.
Payloads are capped at 1 MiB and responses are classified by status without
reading or persisting their bodies.

Secrets live in read-only files referenced by a bounded JSON key-set manifest.
Subscriptions select a stable key ID, allowing old and new secrets to overlap
during rotation. Secrets and signatures are never logged.

HTTP 408, 425, 429 and 5xx responses are retryable. Other non-2xx responses are
permanent receiver rejection. Network and DNS failures use stable non-secret
error codes.

## Consequences

- Tenant endpoint configuration cannot directly target private control-plane
  networks in production.
- A DNS answer cannot change between policy validation and connection setup.
- Redirect destinations never bypass the original endpoint policy.
- Receivers can safely deduplicate at-least-once attempts.
- PostgreSQL keeps append-only attempts per subscription/event. A successful or
  dead-lettered subscription is terminal and is not selected again; other
  subscriptions may retry without redelivering the successful one.
- Subscription management, manual replay and Cursor recovery remain separate
  product capabilities.
