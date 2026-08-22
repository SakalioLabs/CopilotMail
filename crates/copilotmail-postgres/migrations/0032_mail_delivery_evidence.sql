CREATE TABLE mail_delivery_snapshots (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    submission_id uuid NOT NULL,
    source text NOT NULL CHECK (source = 'stalwart_jmap'),
    source_state text NOT NULL
        CHECK (
            char_length(source_state) BETWEEN 1 AND 512
            AND btrim(source_state) = source_state
            AND source_state !~ '[[:cntrl:]]'
        ),
    source_sha256 bytea NOT NULL CHECK (octet_length(source_sha256) = 32),
    recipient_count integer NOT NULL CHECK (recipient_count BETWEEN 1 AND 100),
    delivered_count integer NOT NULL CHECK (delivered_count BETWEEN 0 AND recipient_count),
    failed_count integer NOT NULL CHECK (failed_count BETWEEN 0 AND recipient_count),
    queued_count integer NOT NULL CHECK (queued_count BETWEEN 0 AND recipient_count),
    unknown_count integer NOT NULL CHECK (unknown_count BETWEEN 0 AND recipient_count),
    terminal boolean NOT NULL,
    observed_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL,
    UNIQUE(id, organization_id, submission_id),
    UNIQUE(organization_id, submission_id, source_sha256),
    CONSTRAINT mail_delivery_snapshot_submission_tenant_fkey
        FOREIGN KEY(submission_id, organization_id)
        REFERENCES mail_submission_intents(id, organization_id),
    CHECK (
        delivered_count + failed_count + queued_count + unknown_count = recipient_count
    ),
    CHECK (terminal = (queued_count = 0 AND unknown_count = 0)),
    CHECK (recorded_at >= observed_at)
);

CREATE INDEX mail_delivery_snapshots_latest_idx
    ON mail_delivery_snapshots(
        organization_id,
        submission_id,
        observed_at DESC,
        id DESC
    );

CREATE TABLE mail_delivery_recipient_evidence (
    snapshot_id uuid NOT NULL,
    organization_id uuid NOT NULL,
    submission_id uuid NOT NULL,
    recipient_ref bytea NOT NULL CHECK (octet_length(recipient_ref) = 32),
    state text NOT NULL
        CHECK (state IN ('queued', 'delivered', 'permanent_failure', 'unknown')),
    smtp_reply text NOT NULL
        CHECK (
            char_length(smtp_reply) BETWEEN 1 AND 1024
            AND smtp_reply !~ '[[:cntrl:]]'
        ),
    PRIMARY KEY(snapshot_id, recipient_ref),
    CONSTRAINT mail_delivery_recipient_snapshot_fkey
        FOREIGN KEY(snapshot_id, organization_id, submission_id)
        REFERENCES mail_delivery_snapshots(id, organization_id, submission_id)
);

CREATE INDEX mail_delivery_recipient_latest_idx
    ON mail_delivery_recipient_evidence(
        organization_id,
        submission_id,
        recipient_ref,
        snapshot_id
    );

CREATE OR REPLACE FUNCTION validate_mail_delivery_snapshot_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM mail_submission_intents AS submission
         WHERE submission.id = NEW.submission_id
           AND submission.organization_id = NEW.organization_id
           AND submission.status = 'accepted'
           AND submission.engine_source = 'stalwart'
           AND submission.engine_identity_ref IS NOT NULL
           AND submission.engine_submission_ref IS NOT NULL
           AND submission.accepted_at IS NOT NULL
           AND NEW.observed_at >= submission.accepted_at
    ) THEN
        RAISE EXCEPTION 'delivery evidence requires an accepted bound submission'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_delivery_snapshots_validate_insert
BEFORE INSERT ON mail_delivery_snapshots
FOR EACH ROW EXECUTE FUNCTION validate_mail_delivery_snapshot_insert();

CREATE OR REPLACE FUNCTION validate_mail_delivery_snapshot_complete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    actual_total integer;
    actual_delivered integer;
    actual_failed integer;
    actual_queued integer;
    actual_unknown integer;
BEGIN
    SELECT count(*)::integer,
           count(*) FILTER (WHERE state = 'delivered')::integer,
           count(*) FILTER (WHERE state = 'permanent_failure')::integer,
           count(*) FILTER (WHERE state = 'queued')::integer,
           count(*) FILTER (WHERE state = 'unknown')::integer
      INTO actual_total,
           actual_delivered,
           actual_failed,
           actual_queued,
           actual_unknown
      FROM mail_delivery_recipient_evidence
     WHERE snapshot_id = NEW.id
       AND organization_id = NEW.organization_id
       AND submission_id = NEW.submission_id;

    IF actual_total <> NEW.recipient_count
        OR actual_delivered <> NEW.delivered_count
        OR actual_failed <> NEW.failed_count
        OR actual_queued <> NEW.queued_count
        OR actual_unknown <> NEW.unknown_count
    THEN
        RAISE EXCEPTION 'delivery snapshot counts do not match recipient evidence'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER mail_delivery_snapshots_validate_complete
AFTER INSERT ON mail_delivery_snapshots
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION validate_mail_delivery_snapshot_complete();

CREATE OR REPLACE FUNCTION reject_mail_delivery_snapshot_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail delivery snapshots are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_delivery_snapshots_reject_update
BEFORE UPDATE ON mail_delivery_snapshots
FOR EACH ROW EXECUTE FUNCTION reject_mail_delivery_snapshot_mutation();

CREATE TRIGGER mail_delivery_snapshots_reject_delete
BEFORE DELETE ON mail_delivery_snapshots
FOR EACH ROW EXECUTE FUNCTION reject_mail_delivery_snapshot_mutation();

CREATE TRIGGER mail_delivery_snapshots_reject_truncate
BEFORE TRUNCATE ON mail_delivery_snapshots
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_delivery_snapshot_mutation();

CREATE OR REPLACE FUNCTION reject_mail_delivery_recipient_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail delivery recipient evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_delivery_recipient_reject_update
BEFORE UPDATE ON mail_delivery_recipient_evidence
FOR EACH ROW EXECUTE FUNCTION reject_mail_delivery_recipient_mutation();

CREATE TRIGGER mail_delivery_recipient_reject_delete
BEFORE DELETE ON mail_delivery_recipient_evidence
FOR EACH ROW EXECUTE FUNCTION reject_mail_delivery_recipient_mutation();

CREATE TRIGGER mail_delivery_recipient_reject_truncate
BEFORE TRUNCATE ON mail_delivery_recipient_evidence
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_delivery_recipient_mutation();
