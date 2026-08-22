# CopilotMail V2 Agent API

## Authentication

Exchange a configured Agent credential at `POST /oauth2/token` using
`application/x-www-form-urlencoded`:

```text
grant_type=client_credentials
client_id=<agent-principal-uuid>
scope=agent.operate mailbox.use agent.mail.read
client_assertion_type=urn:ietf:params:oauth:client-assertion-type:jwt-bearer
client_assertion=<single-use-rs256-jwt>
```

For native mTLS, omit both assertion fields and present the registered client
certificate on the TLS connection. A token lives for at most 900 seconds.
Requested scopes must be a subset of the credential scopes and live RBAC.
Never persist a client assertion for reuse: its `jti` is single-use.

Use `Authorization: Bearer …` for API calls. The bundled script reads the token
from `COPILOTMAIL_TOKEN_FILE` or `COPILOTMAIL_TOKEN`; it intentionally does not
create or print credentials.

## Processing mailbox messages

| Operation | Request | Required scopes |
|---|---|---|
| Effective policy | `GET /api/v2/agent/mail-policy` | `agent.operate` |
| Ensure processing subscription | `PUT /api/v2/agent/mailboxes/{mailbox_id}/processing-subscription` | `agent.operate mailbox.use` |
| Pause/resume/retire | `PATCH /api/v2/agent/processing-subscriptions/{subscription_id}` + `Idempotency-Key` + `{"expected_status":"active","status":"paused"}` | `agent.operate mailbox.use` |
| Claim next | `POST /api/v2/agent/processing-subscriptions/{subscription_id}/claims` + `Idempotency-Key` + `{"lease_seconds":60}` | `agent.operate mailbox.use` |
| Read safe content | `GET /api/v2/agent/mailboxes/{mailbox_id}/messages/{message_id}/content` | `agent.operate agent.mail.read mailbox.use` |
| Extend lease | `POST .../{message_id}/lease-extension` + key + `{"lease_id":"…","lease_seconds":60}` | `agent.operate mailbox.use` |
| Acknowledge | `POST .../{message_id}/acknowledgement` + key + `{"lease_id":"…"}` | `agent.operate mailbox.use` |
| Nack | `POST .../{message_id}/nack` + key + `{"lease_id":"…","reason_code":"dependency_timeout"}` | `agent.operate mailbox.use` |
| Dead letters | `GET /api/v2/agent/processing-subscriptions/{subscription_id}/dead-letters?limit=50&cursor=…` | `agent.operate mailbox.use` |

The claim response contains `message_id`, `lease_id`, `state`,
`attempt_count`, and `lease_expires_at`. Exact retries must reuse the same
idempotency key and body. A `204` claim means no message is currently
claimable. After five failed attempts the message becomes `dead_lettered`.

Content responses deliberately omit HTML, raw MIME, headers, recipients,
attachments, Blob/Thread/JMAP identifiers, and Human read/flag state. The
`untrusted_external_content` flag is always true.

Dead-letter pages expose only Message UUID, attempts, bounded reason code, and
failure time. Treat `next_cursor` as opaque and subscription-scoped.

Legal subscription transitions are `active → paused`, `paused → active`, and
`active/paused → retired`. Pausing keeps inbound Fanout as durable pending
backlog but immediately stops new Claim work; an already leased message may
still be extended, acknowledged, or nacked. Retirement is terminal and fails
with `409` while an unexpired lease exists. Use a new idempotency key for each
distinct transition and reuse it only for an exact retry.

## Event recovery

Pull missed events:

```text
GET /api/v2/webhook-subscriptions/{subscription_id}/events?limit=50&cursor=<opaque>
```

The owner Agent needs `agent.operate`. Pages and signed Webhooks share stable
event IDs and versioned envelopes. Deduplicate the ID, persist the returned
Cursor only after processing the page, and restart from the last committed
Cursor. A `410` means the anchor is no longer valid and requires an explicit
reconciliation workflow.

SSE is available at
`GET /api/v2/webhook-subscriptions/{subscription_id}/stream`. Send the token in
the Authorization header, never in a query parameter. Resume with
`Last-Event-ID` or a `cursor` query parameter, not both with different values.
Connections are bounded and must reconnect with capped exponential backoff.

## Sending plain-text mail

Save a draft:

```http
POST /api/v2/mailboxes/{mailbox_id}/drafts
Content-Type: application/json

{
  "draft_id": null,
  "expected_version": null,
  "reply_to_message_id": null,
  "to": ["recipient@example.com"],
  "cc": [],
  "bcc": [],
  "subject": "Subject",
  "text_body": "Body"
}
```

The server derives `From`. Preserve the returned `draft_id` and `version`.
Replace a draft by supplying both values; never guess a remote mail-engine ID.
For an explicit reply, set `reply_to_message_id` to the safe UUID received from
the Agent event/content workflow (or use CLI `draft --reply-to-message-id`).
The server resolves the exact source Email and derives `In-Reply-To` and
`References`; never send raw RFC headers, JMAP Email IDs, or Thread IDs.

Create a durable submission:

```http
POST /api/v2/mailboxes/{mailbox_id}/submissions
Idempotency-Key: <stable-22-to-200-char-key>
Content-Type: application/json

{
  "draft_id": "<uuid>",
  "expected_draft_version": 1,
  "first_contact_approval_request_id": null
}
```

Testing Agents must not call the deliverable endpoint. Capture the exact draft
in the non-delivering sandbox instead:

```http
POST /api/v2/agent/mailboxes/{mailbox_id}/sandbox-submissions
Idempotency-Key: <stable-22-to-200-char-key>
Content-Type: application/json

{
  "draft_id": "<uuid>",
  "expected_draft_version": 1
}
```

A new capture returns `201`; an exact replay returns `200` with
`replayed: true`. Both return `delivery_suppressed: true`. No JMAP
EmailSubmission is created. The regular submission endpoint returns
`agent_mail_sandbox_required` for Testing Agents.

Poll `GET /api/v2/mail-submissions/{submission_id}`. Intent states are
`pending`, `reconciling`, `accepted`, `rejected`, and `manual_review`.
`accepted` means JMAP accepted the submission; inspect the separate delivery
projection for recipient-minimized delivered/failed/queued/unknown counts.

Production policy failures use stable codes:

- `agent_first_contact_approval_required` (`409`)
- `agent_mail_recipient_policy_denied` (`422`)
- `agent_mail_hourly_quota_exceeded` (`429`)
- `agent_mail_recipient_daily_quota_exceeded` (`429`)
- `agent_mail_thread_circuit_breaker_open` (`429`)
- `agent_mail_duplicate_content_circuit_breaker_open` (`429`)
- `agent_mail_automatic_reply_blocked` (`409`)
- `agent_mail_sandbox_required` (`409`, Testing Agent)

Do not parse human message text or infer the rejected address.

## Common response handling

- `400`: invalid bounded input; fix it rather than retrying unchanged.
- `401`: refresh the short-lived token, then retry only safe/exact operations.
- `403`: authorization or policy denial; stop.
- `404`: resource is hidden, absent, or no longer authorized.
- `409`: stale state, lease, approval, or idempotency conflict; reconcile.
- `410`: Cursor anchor invalid or unavailable.
- `429`: wait for `Retry-After`; keep the exact idempotency key.
- `503`: bounded retry with jitter; preserve exact operation parameters.

Log only status, stable error `code`, `request_id`, and retry metadata. Do not
log message content, addresses, assertions, tokens, private keys, or raw
provider responses.
