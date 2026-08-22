INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('mail.submission.requested', 'mail.submission', 'high', true);

CREATE TABLE mail_drafts (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    created_by uuid NOT NULL,
    engine_source text NOT NULL CHECK (engine_source = 'stalwart'),
    engine_account_ref text NOT NULL
        CHECK (
            char_length(engine_account_ref) BETWEEN 1 AND 512
            AND btrim(engine_account_ref) = engine_account_ref
            AND engine_account_ref !~ '[[:cntrl:]]'
        ),
    engine_email_ref text NOT NULL
        CHECK (
            char_length(engine_email_ref) BETWEEN 1 AND 512
            AND btrim(engine_email_ref) = engine_email_ref
            AND engine_email_ref !~ '[[:cntrl:]]'
        ),
    engine_thread_ref text NOT NULL
        CHECK (
            char_length(engine_thread_ref) BETWEEN 1 AND 512
            AND btrim(engine_thread_ref) = engine_thread_ref
            AND engine_thread_ref !~ '[[:cntrl:]]'
        ),
    content_sha256 bytea NOT NULL CHECK (octet_length(content_sha256) = 32),
    version bigint NOT NULL CHECK (version > 0),
    status text NOT NULL CHECK (status IN ('active', 'frozen', 'sent')),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    CONSTRAINT mail_drafts_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id),
    CONSTRAINT mail_drafts_creator_tenant_fkey
        FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT mail_drafts_engine_email_unique
        UNIQUE(engine_source, engine_account_ref, engine_email_ref),
    CHECK (updated_at >= created_at)
);

CREATE INDEX mail_drafts_owner_active_idx
    ON mail_drafts(organization_id, created_by, mailbox_id, updated_at DESC, id DESC)
    WHERE status = 'active';

CREATE OR REPLACE FUNCTION validate_mail_draft_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.version <> 1 OR NEW.status <> 'active' THEN
        RAISE EXCEPTION 'new mail draft must start active at version one'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS principal
         WHERE principal.id = NEW.created_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'human'
           AND principal.status = 'active'
    ) THEN
        RAISE EXCEPTION 'mail draft creator must be an active Human in the tenant'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM mail_engine_mailbox_bindings AS binding
         WHERE binding.organization_id = NEW.organization_id
           AND binding.mailbox_id = NEW.mailbox_id
           AND binding.source = NEW.engine_source
           AND binding.account_ref = NEW.engine_account_ref
    ) THEN
        RAISE EXCEPTION 'mail draft engine account must match the current mailbox binding'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_drafts_validate_insert
BEFORE INSERT ON mail_drafts
FOR EACH ROW EXECUTE FUNCTION validate_mail_draft_insert();

CREATE OR REPLACE FUNCTION validate_mail_draft_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
        OR NEW.organization_id <> OLD.organization_id
        OR NEW.mailbox_id <> OLD.mailbox_id
        OR NEW.created_by <> OLD.created_by
        OR NEW.engine_source <> OLD.engine_source
        OR NEW.engine_account_ref <> OLD.engine_account_ref
        OR NEW.created_at <> OLD.created_at
        OR NEW.version <> OLD.version + 1
        OR NEW.updated_at < OLD.updated_at
    THEN
        RAISE EXCEPTION 'mail draft identity or version transition is invalid'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.status = 'active' AND NEW.status = 'active' THEN
        IF NEW.engine_email_ref = OLD.engine_email_ref
            OR NEW.content_sha256 = OLD.content_sha256
        THEN
            RAISE EXCEPTION 'active draft replacement must advance remote identity and content'
                USING ERRCODE = '23514';
        END IF;
    ELSIF (OLD.status = 'active' AND NEW.status = 'frozen')
        OR (OLD.status = 'frozen' AND NEW.status = 'sent')
    THEN
        IF NEW.engine_email_ref <> OLD.engine_email_ref
            OR NEW.engine_thread_ref <> OLD.engine_thread_ref
            OR NEW.content_sha256 <> OLD.content_sha256
        THEN
            RAISE EXCEPTION 'draft lifecycle transition cannot change frozen content evidence'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        RAISE EXCEPTION 'mail draft status transition is invalid'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_drafts_validate_update
BEFORE UPDATE ON mail_drafts
FOR EACH ROW EXECUTE FUNCTION validate_mail_draft_update();

CREATE OR REPLACE FUNCTION reject_mail_draft_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail draft provenance cannot be deleted or truncated'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_drafts_reject_delete
BEFORE DELETE ON mail_drafts
FOR EACH ROW EXECUTE FUNCTION reject_mail_draft_mutation();

CREATE TRIGGER mail_drafts_reject_truncate
BEFORE TRUNCATE ON mail_drafts
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_draft_mutation();

CREATE TABLE mail_submission_intents (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    requested_by uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    draft_id uuid NOT NULL,
    draft_version bigint NOT NULL CHECK (draft_version > 0),
    idempotency_key_sha256 bytea NOT NULL
        CHECK (octet_length(idempotency_key_sha256) = 32),
    request_sha256 bytea NOT NULL CHECK (octet_length(request_sha256) = 32),
    engine_source text NOT NULL CHECK (engine_source = 'stalwart'),
    engine_account_ref text NOT NULL
        CHECK (
            char_length(engine_account_ref) BETWEEN 1 AND 512
            AND btrim(engine_account_ref) = engine_account_ref
            AND engine_account_ref !~ '[[:cntrl:]]'
        ),
    engine_email_ref text NOT NULL
        CHECK (
            char_length(engine_email_ref) BETWEEN 1 AND 512
            AND btrim(engine_email_ref) = engine_email_ref
            AND engine_email_ref !~ '[[:cntrl:]]'
        ),
    engine_identity_ref text
        CHECK (
            engine_identity_ref IS NULL
            OR (
                char_length(engine_identity_ref) BETWEEN 1 AND 512
                AND btrim(engine_identity_ref) = engine_identity_ref
                AND engine_identity_ref !~ '[[:cntrl:]]'
            )
        ),
    engine_submission_ref text
        CHECK (
            engine_submission_ref IS NULL
            OR (
                char_length(engine_submission_ref) BETWEEN 1 AND 512
                AND btrim(engine_submission_ref) = engine_submission_ref
                AND engine_submission_ref !~ '[[:cntrl:]]'
            )
        ),
    status text NOT NULL
        CHECK (status IN ('pending', 'reconciling', 'accepted', 'rejected', 'manual_review')),
    state_version bigint NOT NULL CHECK (state_version > 0),
    lease_id uuid,
    lease_expires_at timestamptz,
    attempt_count integer NOT NULL CHECK (attempt_count >= 0),
    error_code text
        CHECK (
            error_code IS NULL
            OR error_code ~ '^[a-z][a-z0-9_]{0,99}$'
        ),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    accepted_at timestamptz,
    UNIQUE(id, organization_id),
    UNIQUE(organization_id, requested_by, idempotency_key_sha256),
    CONSTRAINT mail_submission_intents_requester_tenant_fkey
        FOREIGN KEY(requested_by, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT mail_submission_intents_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id),
    CONSTRAINT mail_submission_intents_draft_tenant_fkey
        FOREIGN KEY(draft_id, organization_id)
        REFERENCES mail_drafts(id, organization_id),
    CHECK (updated_at >= created_at),
    CHECK (
        (lease_id IS NULL AND lease_expires_at IS NULL)
        OR (lease_id IS NOT NULL AND lease_expires_at > updated_at)
    ),
    CHECK (
        (status = 'accepted'
            AND engine_identity_ref IS NOT NULL
            AND engine_submission_ref IS NOT NULL
            AND accepted_at IS NOT NULL
            AND error_code IS NULL
            AND lease_id IS NULL)
        OR (status IN ('rejected', 'manual_review')
            AND accepted_at IS NULL
            AND error_code IS NOT NULL
            AND lease_id IS NULL)
        OR (status IN ('pending', 'reconciling')
            AND engine_submission_ref IS NULL
            AND accepted_at IS NULL
            AND error_code IS NULL)
    )
);

CREATE INDEX mail_submission_intents_work_idx
    ON mail_submission_intents(status, lease_expires_at, created_at, id)
    WHERE status IN ('pending', 'reconciling');

CREATE OR REPLACE FUNCTION validate_mail_submission_intent_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.status <> 'pending'
        OR NEW.state_version <> 1
        OR NEW.attempt_count <> 0
        OR NEW.lease_id IS NOT NULL
        OR NEW.engine_identity_ref IS NOT NULL
        OR NEW.engine_submission_ref IS NOT NULL
        OR NEW.error_code IS NOT NULL
        OR NEW.accepted_at IS NOT NULL
    THEN
        RAISE EXCEPTION 'new mail submission intent must start pending'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS principal
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'human'
           AND principal.status = 'active'
    ) THEN
        RAISE EXCEPTION 'mail submission requester must be an active Human'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM mail_drafts AS draft
         WHERE draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id
           AND draft.mailbox_id = NEW.mailbox_id
           AND draft.created_by = NEW.requested_by
           AND draft.version = NEW.draft_version
           AND draft.status = 'frozen'
           AND draft.engine_source = NEW.engine_source
           AND draft.engine_account_ref = NEW.engine_account_ref
           AND draft.engine_email_ref = NEW.engine_email_ref
    ) THEN
        RAISE EXCEPTION 'mail submission intent must bind one exact frozen owned draft'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_submission_intents_validate_insert
BEFORE INSERT ON mail_submission_intents
FOR EACH ROW EXECUTE FUNCTION validate_mail_submission_intent_insert();

CREATE OR REPLACE FUNCTION validate_mail_submission_intent_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
        OR NEW.organization_id <> OLD.organization_id
        OR NEW.requested_by <> OLD.requested_by
        OR NEW.mailbox_id <> OLD.mailbox_id
        OR NEW.draft_id <> OLD.draft_id
        OR NEW.draft_version <> OLD.draft_version
        OR NEW.idempotency_key_sha256 <> OLD.idempotency_key_sha256
        OR NEW.request_sha256 <> OLD.request_sha256
        OR NEW.engine_source <> OLD.engine_source
        OR NEW.engine_account_ref <> OLD.engine_account_ref
        OR NEW.engine_email_ref <> OLD.engine_email_ref
        OR NEW.created_at <> OLD.created_at
        OR NEW.state_version <> OLD.state_version + 1
        OR NEW.updated_at < OLD.updated_at
        OR NEW.attempt_count < OLD.attempt_count
    THEN
        RAISE EXCEPTION 'mail submission intent identity or version transition is invalid'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.status = 'pending'
        AND NEW.status NOT IN ('pending', 'reconciling', 'rejected', 'manual_review')
    THEN
        RAISE EXCEPTION 'pending mail submission transition is invalid'
            USING ERRCODE = '23514';
    ELSIF OLD.status = 'reconciling'
        AND NEW.status NOT IN ('reconciling', 'accepted', 'rejected', 'manual_review')
    THEN
        RAISE EXCEPTION 'reconciling mail submission transition is invalid'
            USING ERRCODE = '23514';
    ELSIF OLD.status IN ('accepted', 'rejected', 'manual_review') THEN
        RAISE EXCEPTION 'terminal mail submission intent is immutable'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER mail_submission_intents_validate_update
BEFORE UPDATE ON mail_submission_intents
FOR EACH ROW EXECUTE FUNCTION validate_mail_submission_intent_update();

CREATE OR REPLACE FUNCTION reject_mail_submission_intent_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail submission intents cannot be deleted or truncated'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_submission_intents_reject_delete
BEFORE DELETE ON mail_submission_intents
FOR EACH ROW EXECUTE FUNCTION reject_mail_submission_intent_mutation();

CREATE TRIGGER mail_submission_intents_reject_truncate
BEFORE TRUNCATE ON mail_submission_intents
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_submission_intent_mutation();
