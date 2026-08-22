ALTER TABLE mailboxes
    ADD CONSTRAINT mailboxes_id_organization_unique UNIQUE(id, organization_id);

CREATE TABLE mail_engine_mailbox_bindings (
    mailbox_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    source text NOT NULL CHECK (source = 'stalwart'),
    account_ref text NOT NULL
        CHECK (
            char_length(account_ref) BETWEEN 1 AND 20
            AND account_ref ~ '^(0|[1-9][0-9]{0,19})$'
        ),
    version bigint NOT NULL CHECK (version > 0),
    updated_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT mail_engine_mailbox_bindings_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id),
    CONSTRAINT mail_engine_mailbox_bindings_actor_tenant_fkey
        FOREIGN KEY(updated_by, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT mail_engine_mailbox_bindings_source_account_unique
        UNIQUE(source, account_ref),
    CHECK (updated_at >= created_at)
);

CREATE INDEX mail_engine_mailbox_bindings_tenant_idx
    ON mail_engine_mailbox_bindings(organization_id, mailbox_id);

CREATE OR REPLACE FUNCTION reject_mail_engine_binding_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail_engine_mailbox_bindings cannot be deleted'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_engine_mailbox_bindings_reject_delete
BEFORE DELETE ON mail_engine_mailbox_bindings
FOR EACH ROW EXECUTE FUNCTION reject_mail_engine_binding_delete();

CREATE TRIGGER mail_engine_mailbox_bindings_reject_truncate
BEFORE TRUNCATE ON mail_engine_mailbox_bindings
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_engine_binding_delete();

CREATE TABLE inbound_mail_engine_events (
    event_id uuid PRIMARY KEY,
    message_id uuid NOT NULL UNIQUE,
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    source text NOT NULL CHECK (source = 'stalwart'),
    source_event_id text NOT NULL
        CHECK (char_length(source_event_id) BETWEEN 1 AND 128),
    source_event_sha256 bytea NOT NULL
        CHECK (octet_length(source_event_sha256) = 32),
    account_ref text NOT NULL
        CHECK (
            char_length(account_ref) BETWEEN 1 AND 20
            AND account_ref ~ '^(0|[1-9][0-9]{0,19})$'
        ),
    document_ref text NOT NULL
        CHECK (
            char_length(document_ref) BETWEEN 1 AND 20
            AND document_ref ~ '^(0|[1-9][0-9]{0,19})$'
        ),
    classification text NOT NULL CHECK (classification IN ('ham', 'spam')),
    source_occurred_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL,
    CONSTRAINT inbound_mail_engine_events_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id),
    CONSTRAINT inbound_mail_engine_events_source_event_unique
        UNIQUE(source, source_event_id),
    CONSTRAINT inbound_mail_engine_events_source_message_unique
        UNIQUE(source, account_ref, document_ref),
    CHECK (
        source_occurred_at <= received_at + interval '5 minutes'
        AND source_occurred_at >= received_at - interval '7 days'
    )
);

CREATE INDEX inbound_mail_engine_events_tenant_mailbox_idx
    ON inbound_mail_engine_events(organization_id, mailbox_id, received_at, event_id);

CREATE OR REPLACE FUNCTION reject_inbound_mail_engine_event_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'inbound_mail_engine_events is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER inbound_mail_engine_events_reject_update_delete
BEFORE UPDATE OR DELETE ON inbound_mail_engine_events
FOR EACH ROW EXECUTE FUNCTION reject_inbound_mail_engine_event_mutation();

CREATE TRIGGER inbound_mail_engine_events_reject_truncate
BEFORE TRUNCATE ON inbound_mail_engine_events
FOR EACH STATEMENT EXECUTE FUNCTION reject_inbound_mail_engine_event_mutation();

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('mail_engine.mailbox_binding.updated', 'mail_engine.mailbox_binding', 'high', true);
