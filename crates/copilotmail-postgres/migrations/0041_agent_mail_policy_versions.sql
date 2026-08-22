-- Immutable candidate controls for production Agent outbound mail. This
-- migration intentionally does not activate a policy or relax the existing
-- production-Agent draft/submission/worker denial.
ALTER TABLE policy_versions
    DROP CONSTRAINT policy_versions_action_check;
ALTER TABLE policy_versions
    ADD CONSTRAINT policy_versions_action_check CHECK (action IN (
        'mail.send_as',
        'mail.send_on_behalf_of',
        'compliance.content_review',
        'audit.export',
        'compliance.retention_change',
        'authorization.privilege_elevation',
        'compliance.legal_hold',
        'security.break_glass',
        'mail.bulk_delete',
        'agent.production_mail'
    ));

ALTER TABLE approval_requests
    DROP CONSTRAINT approval_requests_action_check;
ALTER TABLE approval_requests
    ADD CONSTRAINT approval_requests_action_check CHECK (action IN (
        'mail.send_as',
        'mail.send_on_behalf_of',
        'compliance.content_review',
        'audit.export',
        'compliance.retention_change',
        'authorization.privilege_elevation',
        'compliance.legal_hold',
        'security.break_glass',
        'mail.bulk_delete',
        'agent.production_mail'
    ));

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('agent.mail_policy.activated', 'agent.mail_policy', 'critical', true),
    ('agent.mail_policy.versioned', 'agent.mail_policy', 'high', true),
    ('agent.mail_policy.queried', 'agent.mail_policy_query', 'high', true);

CREATE TABLE agent_mail_policy_versions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    agent_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    allowed_recipient_domains text[] NOT NULL CHECK (
        cardinality(allowed_recipient_domains) BETWEEN 1 AND 100
    ),
    max_recipients_per_message integer NOT NULL CHECK (
        max_recipients_per_message BETWEEN 1 AND 100
    ),
    max_messages_per_hour integer NOT NULL CHECK (
        max_messages_per_hour BETWEEN 1 AND 10000
    ),
    max_messages_per_recipient_per_day integer NOT NULL CHECK (
        max_messages_per_recipient_per_day BETWEEN 1 AND 1000
    ),
    require_first_contact_approval boolean NOT NULL CHECK (
        require_first_contact_approval
    ),
    max_auto_replies_per_thread_per_day integer NOT NULL CHECK (
        max_auto_replies_per_thread_per_day BETWEEN 0 AND 10
    ),
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    UNIQUE(organization_id, agent_id, version),
    FOREIGN KEY(agent_id, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE OR REPLACE FUNCTION validate_agent_mail_policy_version_insert()
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
           AND profile.environment = 'production'
    ) THEN
        RAISE EXCEPTION 'mail policy target must be an active production Agent'
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
        RAISE EXCEPTION 'mail policy author must be an active Human'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1
          FROM unnest(NEW.allowed_recipient_domains) AS domain(value)
         WHERE value <> lower(value)
            OR value <> btrim(value)
            OR value !~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?([.][a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$'
            OR char_length(value) > 253
    ) THEN
        RAISE EXCEPTION 'recipient domain allowlist must be canonical ASCII DNS names'
            USING ERRCODE = '23514';
    END IF;
    IF cardinality(NEW.allowed_recipient_domains)
        <> cardinality(ARRAY(
            SELECT DISTINCT value
              FROM unnest(NEW.allowed_recipient_domains) AS domain(value)
        ))
    THEN
        RAISE EXCEPTION 'recipient domain allowlist must not contain duplicates'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_mail_policy_versions_validate_insert
BEFORE INSERT ON agent_mail_policy_versions
FOR EACH ROW EXECUTE FUNCTION validate_agent_mail_policy_version_insert();

CREATE OR REPLACE FUNCTION reject_agent_mail_policy_version_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent mail policy versions are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_mail_policy_versions_reject_update
BEFORE UPDATE ON agent_mail_policy_versions
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_policy_version_mutation();
CREATE TRIGGER agent_mail_policy_versions_reject_delete
BEFORE DELETE ON agent_mail_policy_versions
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_policy_version_mutation();
CREATE TRIGGER agent_mail_policy_versions_reject_truncate
BEFORE TRUNCATE ON agent_mail_policy_versions
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_mail_policy_version_mutation();

CREATE TABLE agent_mail_policy_activations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    agent_id uuid NOT NULL,
    policy_version_id uuid NOT NULL,
    policy_version integer NOT NULL CHECK (policy_version > 0),
    approval_request_id uuid NOT NULL UNIQUE,
    activated_by uuid NOT NULL,
    activated_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    UNIQUE(policy_version_id),
    FOREIGN KEY(agent_id, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(policy_version_id, organization_id)
        REFERENCES agent_mail_policy_versions(id, organization_id),
    FOREIGN KEY(approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id),
    FOREIGN KEY(activated_by, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE INDEX agent_mail_policy_activations_current_idx
    ON agent_mail_policy_activations(
        organization_id,
        agent_id,
        policy_version DESC,
        activated_at DESC
    );

CREATE OR REPLACE FUNCTION validate_agent_mail_policy_activation_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended(NEW.organization_id::text || ':' || NEW.agent_id::text, 0)
    );
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS actor
         WHERE actor.id = NEW.activated_by
           AND actor.organization_id = NEW.organization_id
           AND actor.kind = 'human'
           AND actor.status = 'active'
    ) THEN
        RAISE EXCEPTION 'mail policy activator must be an active Human'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM agent_mail_policy_versions AS version
         WHERE version.id = NEW.policy_version_id
           AND version.organization_id = NEW.organization_id
           AND version.agent_id = NEW.agent_id
           AND version.version = NEW.policy_version
    ) THEN
        RAISE EXCEPTION 'mail policy activation must bind one exact version'
            USING ERRCODE = '23514';
    END IF;
    IF NOT EXISTS (
        SELECT 1
          FROM approval_requests AS request
         WHERE request.id = NEW.approval_request_id
           AND request.organization_id = NEW.organization_id
           AND request.action = 'agent.production_mail'
           AND request.resource_kind = 'agent.mail_policy_version'
           AND request.resource_id = NEW.policy_version_id::text
           AND request.requested_by = NEW.activated_by
           AND request.status = 'approved'
           AND request.required_approvals >= 2
           AND request.created_at <= NEW.activated_at
           AND request.expires_at > NEW.activated_at
           AND (
               SELECT count(*)
                 FROM approval_steps AS step
                WHERE step.organization_id = NEW.organization_id
                  AND step.approval_request_id = request.id
                  AND step.decision = 'approved'
           ) >= request.required_approvals
           AND NOT EXISTS (
               SELECT 1
                 FROM approval_steps AS step
                WHERE step.organization_id = NEW.organization_id
                  AND step.approval_request_id = request.id
                  AND step.decision = 'rejected'
           )
    ) THEN
        RAISE EXCEPTION 'mail policy activation requires exact live dual approval'
            USING ERRCODE = '23514';
    END IF;
    IF EXISTS (
        SELECT 1
          FROM agent_mail_policy_activations AS activation
         WHERE activation.organization_id = NEW.organization_id
           AND activation.agent_id = NEW.agent_id
           AND activation.policy_version >= NEW.policy_version
    ) THEN
        RAISE EXCEPTION 'mail policy activation must advance monotonically'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_mail_policy_activations_validate_insert
BEFORE INSERT ON agent_mail_policy_activations
FOR EACH ROW EXECUTE FUNCTION validate_agent_mail_policy_activation_insert();

CREATE OR REPLACE FUNCTION reject_agent_mail_policy_activation_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent mail policy activations are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_mail_policy_activations_reject_update
BEFORE UPDATE ON agent_mail_policy_activations
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_policy_activation_mutation();
CREATE TRIGGER agent_mail_policy_activations_reject_delete
BEFORE DELETE ON agent_mail_policy_activations
FOR EACH ROW EXECUTE FUNCTION reject_agent_mail_policy_activation_mutation();
CREATE TRIGGER agent_mail_policy_activations_reject_truncate
BEFORE TRUNCATE ON agent_mail_policy_activations
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_mail_policy_activation_mutation();
