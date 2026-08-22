-- Trusted Auto-Submitted classification is derived by the mail-engine adapter,
-- never accepted from API callers. Any Agent reply to an automatic source is
-- rejected before a durable submission Intent can be committed.
INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('agent.mail_sandbox.captured', 'agent.mail_sandbox_submission', 'high', true);

ALTER TABLE mail_drafts
    ADD COLUMN reply_to_automatic boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT mail_drafts_automatic_reply_source_check CHECK (
        NOT reply_to_automatic OR reply_to_message_id IS NOT NULL
    );

ALTER TABLE mail_drafts
    ADD COLUMN body_ref bytea;
ALTER TABLE mail_drafts DISABLE TRIGGER mail_drafts_validate_update;
UPDATE mail_drafts
   SET body_ref = content_sha256
 WHERE body_ref IS NULL;
ALTER TABLE mail_drafts ENABLE TRIGGER mail_drafts_validate_update;
ALTER TABLE mail_drafts
    ALTER COLUMN body_ref SET NOT NULL,
    ADD CONSTRAINT mail_drafts_body_ref_check CHECK (
        octet_length(body_ref) = 32
    );

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
        OR NEW.reply_to_message_id IS DISTINCT FROM OLD.reply_to_message_id
        OR NEW.reply_to_automatic <> OLD.reply_to_automatic
        OR NEW.created_at <> OLD.created_at
        OR NEW.version <> OLD.version + 1
        OR NEW.updated_at < OLD.updated_at
        OR NOT valid_mail_draft_recipient_evidence(
            NEW.recipient_refs,
            NEW.recipient_domains
        )
    THEN
        RAISE EXCEPTION 'mail draft identity, reply source, evidence, or version transition is invalid'
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
            OR NEW.reply_to_message_id IS DISTINCT FROM OLD.reply_to_message_id
            OR NEW.reply_to_automatic <> OLD.reply_to_automatic
            OR NEW.body_ref <> OLD.body_ref
            OR NEW.content_sha256 <> OLD.content_sha256
            OR NEW.recipient_refs <> OLD.recipient_refs
            OR NEW.recipient_domains <> OLD.recipient_domains
        THEN
            RAISE EXCEPTION 'draft lifecycle transition cannot change frozen evidence'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        RAISE EXCEPTION 'mail draft status transition is invalid'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TABLE agent_mail_sandbox_submissions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    agent_id uuid NOT NULL,
    mailbox_id uuid NOT NULL,
    draft_id uuid NOT NULL,
    draft_version bigint NOT NULL CHECK (draft_version > 1),
    idempotency_key_sha256 bytea NOT NULL
        CHECK (octet_length(idempotency_key_sha256) = 32),
    request_sha256 bytea NOT NULL CHECK (octet_length(request_sha256) = 32),
    body_ref bytea NOT NULL CHECK (octet_length(body_ref) = 32),
    recipient_count integer NOT NULL CHECK (recipient_count BETWEEN 1 AND 100),
    captured_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    UNIQUE(organization_id, agent_id, idempotency_key_sha256),
    FOREIGN KEY(agent_id, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id),
    FOREIGN KEY(draft_id, organization_id)
        REFERENCES mail_drafts(id, organization_id)
);

CREATE INDEX agent_mail_sandbox_submissions_agent_idx
    ON agent_mail_sandbox_submissions(
        organization_id,
        agent_id,
        captured_at DESC,
        id
    );

CREATE OR REPLACE FUNCTION validate_agent_mail_sandbox_submission_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS principal
          JOIN agent_profiles AS profile
            ON profile.principal_id = principal.id
         WHERE principal.id = NEW.agent_id
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'agent'
           AND principal.status = 'active'
           AND profile.environment = 'testing'
    ) THEN
        RAISE EXCEPTION 'mail sandbox is restricted to active testing Agents'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM mail_drafts AS draft
         WHERE draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id
           AND draft.mailbox_id = NEW.mailbox_id
           AND draft.created_by = NEW.agent_id
           AND draft.version = NEW.draft_version
           AND draft.status = 'frozen'
           AND draft.body_ref = NEW.body_ref
           AND cardinality(draft.recipient_refs) = NEW.recipient_count
    ) THEN
        RAISE EXCEPTION 'mail sandbox must bind one exact frozen testing-Agent draft'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_mail_sandbox_submissions_validate_insert
BEFORE INSERT ON agent_mail_sandbox_submissions
FOR EACH ROW EXECUTE FUNCTION validate_agent_mail_sandbox_submission_insert();

CREATE OR REPLACE FUNCTION reject_agent_mail_sandbox_submission_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent mail sandbox evidence is immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_mail_sandbox_submissions_reject_update
BEFORE UPDATE ON agent_mail_sandbox_submissions
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_sandbox_submission_mutation();
CREATE TRIGGER agent_mail_sandbox_submissions_reject_delete
BEFORE DELETE ON agent_mail_sandbox_submissions
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_sandbox_submission_mutation();
CREATE TRIGGER agent_mail_sandbox_submissions_reject_truncate
BEFORE TRUNCATE ON agent_mail_sandbox_submissions
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_mail_sandbox_submission_mutation();

CREATE OR REPLACE FUNCTION validate_mail_submission_intent_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    current_body_ref bytea;
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
          LEFT JOIN agent_profiles AS agent
            ON agent.principal_id = principal.id
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.status = 'active'
           AND (
               principal.kind = 'human'
               OR (
                   principal.kind = 'agent'
                   AND (
                       agent.environment IN ('development', 'testing')
                       OR (
                           agent.environment = 'production'
                           AND EXISTS (
                               SELECT 1
                                 FROM agent_mail_policy_activations AS activation
                                WHERE activation.organization_id = NEW.organization_id
                                  AND activation.agent_id = NEW.requested_by
                           )
                       )
                   )
               )
           )
    ) THEN
        RAISE EXCEPTION 'mail submission requester is not eligible'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1
          FROM principals AS principal
          JOIN agent_profiles AS profile
            ON profile.principal_id = principal.id
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'agent'
           AND profile.environment = 'testing'
    ) THEN
        RAISE EXCEPTION 'testing Agent must use the non-delivering sandbox'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_testing_requires_sandbox';
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
    IF EXISTS (
        SELECT 1
          FROM principals AS principal
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'agent'
    ) THEN
        PERFORM pg_advisory_xact_lock(
            hashtextextended(
                NEW.organization_id::text || ':' || NEW.requested_by::text,
                0
            )
        );
        SELECT draft.body_ref
          INTO STRICT current_body_ref
          FROM mail_drafts AS draft
         WHERE draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id;
        IF (
            SELECT count(*)
              FROM mail_submission_intents AS prior
              JOIN mail_drafts AS prior_draft
                ON prior_draft.id = prior.draft_id
               AND prior_draft.organization_id = prior.organization_id
             WHERE prior.organization_id = NEW.organization_id
               AND prior.requested_by = NEW.requested_by
               AND prior.created_at > NEW.created_at - interval '1 day'
               AND prior.created_at <= NEW.created_at
               AND prior_draft.body_ref = current_body_ref
        ) >= 10
        THEN
            RAISE EXCEPTION 'Agent duplicate content circuit breaker opened'
                USING ERRCODE = '23514',
                      CONSTRAINT = 'agent_mail_duplicate_content_circuit_breaker';
        END IF;
    END IF;
    IF EXISTS (
        SELECT 1
          FROM principals AS principal
          JOIN mail_drafts AS draft
            ON draft.id = NEW.draft_id
           AND draft.organization_id = NEW.organization_id
         WHERE principal.id = NEW.requested_by
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'agent'
           AND draft.reply_to_automatic
    ) THEN
        RAISE EXCEPTION 'Agent automatic reply loop guard rejected submission'
            USING ERRCODE = '23514',
                  CONSTRAINT = 'agent_mail_automatic_reply_loop';
    END IF;
    RETURN NEW;
END;
$$;
