# API reference

All JSON routes use the configured server base URL.

| Operation | Request | Auth |
|---|---|---|
| Health | `GET /healthz` | none |
| Create mailbox | `POST /v1/mailboxes` `{"address":"agent@mail.local","password":"..."}` | `X-Admin-Key` |
| Login | `POST /v1/auth/token` with the same fields | none |
| List messages | `GET /v1/messages` | Bearer |
| Read message | `GET /v1/messages/{id}` | Bearer |
| Mark seen | `PATCH /v1/messages/{id}` `{"seen":true}` | Bearer |
| Delete message | `DELETE /v1/messages/{id}` | Bearer |
| Send | `POST /v1/messages/send` `{"to":"...","subject":"...","text":"...","html":"..."}` | Bearer |
| Delete mailbox | `DELETE /v1/mailboxes/{mailbox_id}` | Bearer |

Successful mailbox creation returns `201`; send returns `202`; mutation without a body returns `204`.

## WebSocket

Connect to `ws://host/v1/events?token=BEARER_TOKEN`. Each event is JSON:

```json
{"type":"message.received","mailbox_id":"...","message_id":"..."}
```

Fetch the indicated message over HTTP. Reconnect with bounded exponential backoff and refresh the inbox after reconnecting so events lost while disconnected are recovered.

