-- Owner-visible draft-creation attempt ledger for orphan detection (B-10).
-- One row is appended immediately before the remote provider draft write.
-- The row is reconciled inside the same transaction that persists the local
-- draft metadata, so an unreconciled row proves a remote write was attempted
-- while its local record never committed. No body, recipient, provider
-- object reference, or secret enters this table. Remote probing and any
-- automatic cleanup stay out of scope; the owner sees only their own
-- unreconciled attempts through a dedicated audited read.
INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('mail.draft_attempts.queried', 'mail.draft_attempt_query', 'medium', true);

CREATE TABLE mail_draft_creation_attempts (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    created_by uuid NOT NULL,
    draft_id uuid NOT NULL,
    mail_data_plane_id uuid NOT NULL,
    attempted_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    reconciled_at timestamptz,
    CONSTRAINT mail_draft_attempts_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id),
    CONSTRAINT mail_draft_attempts_creator_tenant_fkey
        FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT mail_draft_attempts_data_plane_fkey
        FOREIGN KEY(mail_data_plane_id) REFERENCES mail_data_planes(id),
    CONSTRAINT mail_draft_attempts_reconcile_clock_check
        CHECK (reconciled_at IS NULL OR reconciled_at >= attempted_at)
);

CREATE INDEX mail_draft_attempts_owner_pending_idx
    ON mail_draft_creation_attempts(organization_id, created_by, id DESC)
    WHERE reconciled_at IS NULL;

-- The only permitted mutation is the one-time reconcile marker written by
-- the draft-metadata transaction. Every other update, every delete, and any
-- truncate is rejected so attempt evidence can neither be rewritten nor
-- removed.
CREATE OR REPLACE FUNCTION guard_mail_draft_creation_attempt()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'UPDATE'
        AND OLD.reconciled_at IS NULL
        AND NEW.reconciled_at IS NOT NULL
        AND NEW.id = OLD.id
        AND NEW.organization_id = OLD.organization_id
        AND NEW.mailbox_id = OLD.mailbox_id
        AND NEW.created_by = OLD.created_by
        AND NEW.draft_id = OLD.draft_id
        AND NEW.mail_data_plane_id = OLD.mail_data_plane_id
        AND NEW.attempted_at = OLD.attempted_at
    THEN
        RETURN NEW;
    END IF;
    RAISE EXCEPTION 'mail draft creation attempts are append and reconcile only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_draft_attempts_guard_update
BEFORE UPDATE ON mail_draft_creation_attempts
FOR EACH ROW EXECUTE FUNCTION guard_mail_draft_creation_attempt();

CREATE TRIGGER mail_draft_attempts_reject_delete
BEFORE DELETE ON mail_draft_creation_attempts
FOR EACH ROW EXECUTE FUNCTION guard_mail_draft_creation_attempt();

CREATE TRIGGER mail_draft_attempts_reject_truncate
BEFORE TRUNCATE ON mail_draft_creation_attempts
FOR EACH STATEMENT EXECUTE FUNCTION guard_mail_draft_creation_attempt();
