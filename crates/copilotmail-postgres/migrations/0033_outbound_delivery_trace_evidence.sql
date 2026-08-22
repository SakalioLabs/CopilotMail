CREATE TABLE outbound_mail_engine_events (
    event_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    source text NOT NULL CHECK (source = 'stalwart'),
    source_event_id text NOT NULL
        CHECK (char_length(source_event_id) BETWEEN 1 AND 128),
    source_event_sha256 bytea NOT NULL
        CHECK (octet_length(source_event_sha256) = 32),
    queue_ref text NOT NULL
        CHECK (
            char_length(queue_ref) BETWEEN 1 AND 20
            AND queue_ref ~ '^(0|[1-9][0-9]{0,19})$'
        ),
    event_kind text NOT NULL
        CHECK (
            event_kind IN (
                'delivered',
                'permanent_failure',
                'rescheduled',
                'completed'
            )
        ),
    recipient_ref bytea
        CHECK (recipient_ref IS NULL OR octet_length(recipient_ref) = 32),
    smtp_code smallint CHECK (smtp_code IS NULL OR smtp_code BETWEEN 200 AND 599),
    next_retry_at timestamptz,
    source_occurred_at timestamptz NOT NULL,
    received_at timestamptz NOT NULL,
    CONSTRAINT outbound_mail_engine_events_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id),
    CONSTRAINT outbound_mail_engine_events_source_event_unique
        UNIQUE(source, source_event_id),
    CHECK (
        source_occurred_at <= received_at + interval '5 minutes'
        AND source_occurred_at >= received_at - interval '7 days'
    ),
    CHECK (
        (event_kind = 'delivered'
            AND recipient_ref IS NOT NULL
            AND smtp_code BETWEEN 200 AND 299
            AND next_retry_at IS NULL)
        OR (event_kind = 'permanent_failure'
            AND recipient_ref IS NOT NULL
            AND smtp_code IS NULL
            AND next_retry_at IS NULL)
        OR (event_kind = 'rescheduled'
            AND recipient_ref IS NULL
            AND smtp_code IS NULL
            AND next_retry_at > source_occurred_at)
        OR (event_kind = 'completed'
            AND recipient_ref IS NULL
            AND smtp_code IS NULL
            AND next_retry_at IS NULL)
    )
);

CREATE INDEX outbound_mail_engine_events_tenant_mailbox_queue_idx
    ON outbound_mail_engine_events(
        organization_id,
        mailbox_id,
        queue_ref,
        source_occurred_at,
        event_id
    );

CREATE INDEX outbound_mail_engine_events_terminal_idx
    ON outbound_mail_engine_events(
        organization_id,
        source_occurred_at DESC,
        event_id DESC
    )
    WHERE event_kind IN ('permanent_failure', 'completed');

CREATE OR REPLACE FUNCTION reject_outbound_mail_engine_event_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'outbound_mail_engine_events is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER outbound_mail_engine_events_reject_update_delete
BEFORE UPDATE OR DELETE ON outbound_mail_engine_events
FOR EACH ROW EXECUTE FUNCTION reject_outbound_mail_engine_event_mutation();

CREATE TRIGGER outbound_mail_engine_events_reject_truncate
BEFORE TRUNCATE ON outbound_mail_engine_events
FOR EACH STATEMENT EXECUTE FUNCTION reject_outbound_mail_engine_event_mutation();
