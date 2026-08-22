-- Explicit replies bind a draft to one externally safe inbound message UUID.
-- Provider document identifiers and RFC reply headers remain data-plane only.
ALTER TABLE inbound_mail_engine_events
    ADD CONSTRAINT inbound_mail_engine_events_message_tenant_mailbox_unique
        UNIQUE(message_id, organization_id, mailbox_id);

ALTER TABLE mail_drafts
    ADD COLUMN reply_to_message_id uuid,
    ADD CONSTRAINT mail_drafts_reply_target_fkey
        FOREIGN KEY(reply_to_message_id, organization_id, mailbox_id)
        REFERENCES inbound_mail_engine_events(message_id, organization_id, mailbox_id);

CREATE INDEX mail_drafts_reply_target_idx
    ON mail_drafts(organization_id, mailbox_id, reply_to_message_id)
    WHERE reply_to_message_id IS NOT NULL;

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
        OR NEW.created_at <> OLD.created_at
        OR NEW.version <> OLD.version + 1
        OR NEW.updated_at < OLD.updated_at
        OR NOT valid_mail_draft_recipient_evidence(
            NEW.recipient_refs,
            NEW.recipient_domains
        )
    THEN
        RAISE EXCEPTION 'mail draft identity, reply target, evidence, or version transition is invalid'
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
