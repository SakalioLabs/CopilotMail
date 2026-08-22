# Local development secrets

Files in this directory are ignored by Git except this notice.

The optional `compose.human-session.yaml` overlay expects
`human-session.key`, an exact 32-byte binary key used to protect transient
OIDC PKCE material. Generate it without Base64 or a trailing newline:

```sh
openssl rand -out deploy/secrets/human-session.key 32
test "$(wc -c < deploy/secrets/human-session.key)" -eq 32
chmod 0600 deploy/secrets/human-session.key
```

The overlay mounts this file read-only at
`/run/secrets/copilotmail/human-session.key` and only into the API container.
Do not copy its bytes into `.env.v2`, reuse another application key, or commit
the file. Production deployments should mount an independently generated
32-byte value from their secrets manager at the same container path.

After completing the Stalwart development bootstrap, save the one-time API-key
secret as `stalwart-jmap.token`. Never commit this key. Production deployments
must use a secrets manager or orchestrator-mounted secret instead.

For authenticated Stalwart telemetry, create two independent random files:

- `stalwart-events.bearer` for HTTP Bearer authentication;
- `stalwart-events.hmac` for the Base64 HMAC-SHA256 `X-Signature`.

Each value must contain at least 32 printable ASCII bytes. Do not reuse the
JMAP API key or either telemetry credential for another purpose.

Human inbox use requires a separate `stalwart-mail-content.token` containing
an account-scoped OAuth Bearer access token with JMAP Email query/get and set
permissions. CopilotMail limits set operations to `$seen` and `$flagged`
PatchObjects; the credential must not have submission or administrative
permissions. Stalwart management API keys cannot access mail and must never be
reused here. Rotate the file atomically using an external token broker; this
repository does not persist refresh tokens.

Human draft editing requires another independent
`stalwart-mail-compose.token`. Scope it to the exact mailbox accounts and only
the JMAP session, Mailbox/get, and Email create/destroy operations needed to
replace immutable drafts. Do not grant EmailSubmission, Identity mutation,
mail-content query, or administrative permissions. This separation ensures an
inbox credential cannot create mail and a compose credential cannot read inbox
content. Rotate it using the same external broker model.

Formal outbound submission requires a fourth independent
`stalwart-mail-submission.token`, mounted only into the durable Worker. The API
persists Intent requests without this credential. Grant JMAP discovery,
`Identity/get`, `Mailbox/get`, `EmailSubmission/query`,
`EmailSubmission/get`, `EmailSubmission/set` create, and the `Email/set`
permission needed by `onSuccessUpdateEmail` to clear `$draft` and move the
exact frozen Email from Drafts to Sent. The adapter never reads or changes
message bodies and must not receive Identity mutation or administration. The
Worker never constructs an envelope; Stalwart derives it from the frozen
Email headers.

The optional `audit-maintenance` Compose profile also expects:

- `audit-private.pem`: PKCS#8 RSA private key, mounted only into the dedicated
  tenant maintenance worker;
- `audit-public.pem`: matching SubjectPublicKeyInfo RSA public key.

Use at least RSA-2048. Production should replace file keys with a reviewed
HSM/KMS signer adapter; never mount the private key into the API or general
Outbox Worker container.
