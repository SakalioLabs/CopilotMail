CREATE TABLE organizations (
    id uuid PRIMARY KEY,
    slug text NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$'),
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE TABLE principals (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    kind text NOT NULL CHECK (kind IN ('human', 'agent', 'service')),
    status text NOT NULL CHECK (status IN ('draft', 'active', 'suspended', 'retired')),
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);
CREATE INDEX principals_organization_idx ON principals(organization_id, kind, status);

CREATE TABLE human_profiles (
    principal_id uuid PRIMARY KEY REFERENCES principals(id) ON DELETE CASCADE,
    identity_provider text NOT NULL,
    identity_subject text NOT NULL,
    UNIQUE(identity_provider, identity_subject)
);

CREATE TABLE agent_profiles (
    principal_id uuid PRIMARY KEY REFERENCES principals(id) ON DELETE CASCADE,
    owner_principal_id uuid NOT NULL REFERENCES principals(id),
    environment text NOT NULL CHECK (environment IN ('development', 'testing', 'production')),
    risk_level text NOT NULL CHECK (risk_level IN ('low', 'medium', 'high')),
    purpose text NOT NULL DEFAULT '' CHECK (char_length(purpose) <= 1000),
    review_due_at timestamptz
);

CREATE TABLE mailboxes (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    kind text NOT NULL CHECK (kind IN ('human', 'agent', 'shared', 'group')),
    primary_address text NOT NULL,
    mail_engine_resource_id text,
    created_at timestamptz NOT NULL,
    UNIQUE(organization_id, primary_address),
    UNIQUE(mail_engine_resource_id)
);

CREATE TABLE mailbox_memberships (
    mailbox_id uuid NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    principal_id uuid NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('owner', 'member', 'delegate')),
    valid_from timestamptz NOT NULL,
    valid_until timestamptz,
    created_by uuid NOT NULL REFERENCES principals(id),
    PRIMARY KEY(mailbox_id, principal_id, role),
    CHECK (valid_until IS NULL OR valid_until > valid_from)
);

CREATE TABLE credentials (
    id uuid PRIMARY KEY,
    principal_id uuid NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('private_key_jwt', 'mtls', 'workload', 'development_secret')),
    public_fingerprint text NOT NULL,
    status text NOT NULL CHECK (status IN ('active', 'revoked', 'expired')),
    created_at timestamptz NOT NULL,
    expires_at timestamptz,
    revoked_at timestamptz,
    last_used_at timestamptz,
    UNIQUE(principal_id, public_fingerprint)
);

CREATE TABLE delegations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    grantor_principal_id uuid NOT NULL REFERENCES principals(id),
    grantee_principal_id uuid NOT NULL REFERENCES principals(id),
    mailbox_id uuid NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    scopes text[] NOT NULL,
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 1000),
    valid_from timestamptz NOT NULL,
    valid_until timestamptz NOT NULL,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL,
    CHECK (valid_until > valid_from),
    CHECK (grantor_principal_id <> grantee_principal_id)
);

CREATE TABLE agent_subscriptions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    mailbox_id uuid NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 200),
    status text NOT NULL CHECK (status IN ('active', 'paused', 'retired')),
    cursor_value text,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE(principal_id, mailbox_id, name)
);

CREATE TABLE agent_message_states (
    subscription_id uuid NOT NULL REFERENCES agent_subscriptions(id) ON DELETE CASCADE,
    message_id uuid NOT NULL,
    principal_id uuid REFERENCES principals(id),
    state text NOT NULL CHECK (state IN ('pending', 'leased', 'acknowledged', 'dead_lettered')),
    lease_id uuid,
    lease_expires_at timestamptz,
    attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_error text,
    updated_at timestamptz NOT NULL,
    PRIMARY KEY(subscription_id, message_id),
    CHECK (
        (state = 'leased' AND lease_id IS NOT NULL AND lease_expires_at IS NOT NULL)
        OR
        (state <> 'leased' AND lease_id IS NULL AND lease_expires_at IS NULL)
    )
);
CREATE INDEX agent_message_pending_idx
    ON agent_message_states(subscription_id, state, updated_at);

CREATE TABLE idempotency_keys (
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL REFERENCES principals(id),
    operation text NOT NULL,
    key_hash bytea NOT NULL,
    request_hash bytea NOT NULL,
    response_status integer,
    response_body jsonb,
    resource_id text,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    PRIMARY KEY(organization_id, principal_id, operation, key_hash),
    CHECK (expires_at > created_at)
);

CREATE TABLE outbox_events (
    id uuid PRIMARY KEY,
    event_type text NOT NULL,
    aggregate_type text NOT NULL,
    aggregate_id text NOT NULL,
    schema_version integer NOT NULL CHECK (schema_version > 0),
    payload jsonb NOT NULL,
    occurred_at timestamptz NOT NULL,
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    locked_at timestamptz,
    locked_by uuid,
    published_at timestamptz,
    last_error text
);
CREATE INDEX outbox_unpublished_idx
    ON outbox_events(occurred_at, id)
    WHERE published_at IS NULL;

CREATE TABLE webhook_subscriptions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL REFERENCES principals(id),
    endpoint_url text NOT NULL,
    event_types text[] NOT NULL,
    signing_key_reference text NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL
);

CREATE TABLE webhook_deliveries (
    id uuid PRIMARY KEY,
    subscription_id uuid NOT NULL REFERENCES webhook_subscriptions(id) ON DELETE CASCADE,
    event_id uuid NOT NULL,
    attempt integer NOT NULL CHECK (attempt > 0),
    status text NOT NULL CHECK (status IN ('pending', 'delivered', 'retrying', 'dead_lettered')),
    response_status integer,
    next_attempt_at timestamptz,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE(subscription_id, event_id, attempt)
);

CREATE TABLE audit_events (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    actor_principal_id uuid NOT NULL REFERENCES principals(id),
    on_behalf_of_principal_id uuid REFERENCES principals(id),
    credential_id text,
    action text NOT NULL,
    resource_type text NOT NULL,
    resource_id text NOT NULL,
    outcome text NOT NULL CHECK (outcome IN ('success', 'failure', 'denied')),
    trace_id text NOT NULL,
    details jsonb NOT NULL DEFAULT '{}'::jsonb,
    occurred_at timestamptz NOT NULL,
    previous_hash bytea,
    event_hash bytea,
    signature_batch_id uuid
);
CREATE INDEX audit_events_search_idx
    ON audit_events(organization_id, occurred_at DESC, action);
CREATE INDEX audit_events_resource_idx
    ON audit_events(organization_id, resource_type, resource_id, occurred_at DESC);

CREATE TABLE audit_checkpoints (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    period_start timestamptz NOT NULL,
    period_end timestamptz NOT NULL,
    root_hash bytea NOT NULL,
    signature bytea NOT NULL,
    key_id text NOT NULL,
    object_key text NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE(organization_id, period_start, period_end),
    CHECK (period_end > period_start)
);

CREATE OR REPLACE FUNCTION reject_audit_event_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_events_append_only
BEFORE UPDATE OR DELETE ON audit_events
FOR EACH ROW EXECUTE FUNCTION reject_audit_event_mutation();
