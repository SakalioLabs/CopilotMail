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
           AND principal.status = 'active'
           AND (
               principal.kind = 'human'
               OR (
                   principal.kind = 'agent'
                   AND EXISTS (
                       SELECT 1
                         FROM agent_profiles AS agent
                        WHERE agent.principal_id = principal.id
                          AND agent.environment IN ('development', 'testing')
                   )
               )
           )
    ) THEN
        RAISE EXCEPTION 'mail draft creator must be an eligible active Human or Agent'
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
           AND principal.status = 'active'
           AND (
               principal.kind = 'human'
               OR (
                   principal.kind = 'agent'
                   AND EXISTS (
                       SELECT 1
                         FROM agent_profiles AS agent
                        WHERE agent.principal_id = principal.id
                          AND agent.environment IN ('development', 'testing')
                   )
               )
           )
    ) THEN
        RAISE EXCEPTION 'mail submission requester must be an eligible active Human or Agent'
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
