ALTER TABLE role_permissions
    DROP CONSTRAINT role_permissions_permission_check;
ALTER TABLE role_permissions
    ADD CONSTRAINT role_permissions_permission_check CHECK (
        permission IN (
            'organization.read',
            'organization.manage',
            'identity.manage',
            'authorization.manage',
            'mail.admin',
            'security.admin',
            'audit.read',
            'audit.export',
            'legal_hold.manage',
            'retention.manage',
            'content.review',
            'agent.operate',
            'mailbox.use',
            'break_glass.invoke'
        )
    );

INSERT INTO role_permissions(role_id, permission)
SELECT id, 'retention.manage'
FROM roles
WHERE system_key = 'org_owner'
ON CONFLICT DO NOTHING;

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
        'mail.bulk_delete'
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
        'mail.bulk_delete'
    ));

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('compliance.retention_policy.created', 'compliance.retention_policy', 'high', true),
    ('compliance.retention_policy.versioned', 'compliance.retention_policy', 'high', true),
    ('compliance.retention_policy.activated', 'compliance.retention_policy', 'critical', true),
    ('compliance.retention_policy.queried', 'compliance.retention_policy_query', 'high', true);

-- Policy identity is immutable. Every operational change is an append-only
-- version and is separately activated under an exact dual approval.
CREATE TABLE retention_policies (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    policy_key text NOT NULL CHECK (
        char_length(policy_key) BETWEEN 1 AND 100
        AND policy_key = lower(policy_key)
        AND policy_key ~ '^[a-z0-9]+([.][a-z0-9]+)*$'
    ),
    display_name text NOT NULL CHECK (
        char_length(display_name) BETWEEN 1 AND 200
        AND display_name = btrim(display_name)
        AND display_name !~ '[[:cntrl:]]'
    ),
    resource_kind text NOT NULL CHECK (
        resource_kind IN ('audit.evidence', 'mail.content')
    ),
    active_version_id uuid,
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    UNIQUE(organization_id, policy_key),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE TABLE retention_policy_versions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    policy_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    minimum_retention_days integer NOT NULL CHECK (
        minimum_retention_days BETWEEN 1 AND 36500
    ),
    archive_after_days integer CHECK (
        archive_after_days BETWEEN 1 AND 36500
        AND archive_after_days <= minimum_retention_days
    ),
    delete_after_days integer CHECK (
        delete_after_days BETWEEN 1 AND 36500
        AND delete_after_days >= minimum_retention_days
    ),
    rule jsonb NOT NULL CHECK (
        jsonb_typeof(rule) = 'object'
        AND pg_column_size(rule) <= 16384
    ),
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    UNIQUE(id, organization_id, policy_id),
    UNIQUE(policy_id, version),
    FOREIGN KEY(policy_id, organization_id)
        REFERENCES retention_policies(id, organization_id),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id)
);

ALTER TABLE retention_policies
    ADD CONSTRAINT retention_policies_active_version_fk
    FOREIGN KEY(active_version_id, organization_id, id)
    REFERENCES retention_policy_versions(id, organization_id, policy_id);

CREATE TABLE retention_policy_activations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    policy_id uuid NOT NULL,
    policy_version_id uuid NOT NULL,
    approval_request_id uuid NOT NULL UNIQUE,
    activated_by uuid NOT NULL,
    activated_at timestamptz NOT NULL,
    UNIQUE(policy_version_id),
    UNIQUE(id, organization_id),
    FOREIGN KEY(policy_id, organization_id)
        REFERENCES retention_policies(id, organization_id),
    FOREIGN KEY(policy_version_id, organization_id, policy_id)
        REFERENCES retention_policy_versions(id, organization_id, policy_id),
    FOREIGN KEY(approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id),
    FOREIGN KEY(activated_by, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE OR REPLACE FUNCTION validate_retention_policy_activation_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS actor
         WHERE actor.id = NEW.activated_by
           AND actor.organization_id = NEW.organization_id
           AND actor.kind = 'human'
           AND actor.status = 'active'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM approval_requests AS request
         WHERE request.id = NEW.approval_request_id
           AND request.organization_id = NEW.organization_id
           AND request.action = 'compliance.retention_change'
           AND request.resource_kind = 'compliance.retention_policy_version'
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
                 FROM approval_steps AS rejected
                WHERE rejected.organization_id = NEW.organization_id
                  AND rejected.approval_request_id = request.id
                  AND rejected.decision = 'rejected'
           )
    )
    THEN
        RAISE EXCEPTION 'retention policy activation is not exactly approved'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER retention_policy_activations_validate_insert
BEFORE INSERT ON retention_policy_activations
FOR EACH ROW EXECUTE FUNCTION validate_retention_policy_activation_insert();

CREATE OR REPLACE FUNCTION validate_retention_policy_active_version_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    old_version integer;
    new_version integer;
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.organization_id IS DISTINCT FROM OLD.organization_id
       OR NEW.policy_key IS DISTINCT FROM OLD.policy_key
       OR NEW.display_name IS DISTINCT FROM OLD.display_name
       OR NEW.resource_kind IS DISTINCT FROM OLD.resource_kind
       OR NEW.created_by IS DISTINCT FROM OLD.created_by
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR NEW.active_version_id IS NULL
       OR NEW.active_version_id IS NOT DISTINCT FROM OLD.active_version_id
    THEN
        RAISE EXCEPTION 'retention policy identity is immutable'
            USING ERRCODE = '55000';
    END IF;

    SELECT version INTO new_version
      FROM retention_policy_versions
     WHERE id = NEW.active_version_id
       AND organization_id = NEW.organization_id
       AND policy_id = NEW.id;
    IF new_version IS NULL
       OR NOT EXISTS (
           SELECT 1
             FROM retention_policy_activations AS activation
            WHERE activation.organization_id = NEW.organization_id
              AND activation.policy_id = NEW.id
              AND activation.policy_version_id = NEW.active_version_id
       )
    THEN
        RAISE EXCEPTION 'retention policy version lacks activation evidence'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.active_version_id IS NOT NULL THEN
        SELECT version INTO old_version
          FROM retention_policy_versions
         WHERE id = OLD.active_version_id
           AND organization_id = OLD.organization_id
           AND policy_id = OLD.id;
        IF old_version IS NULL OR new_version <= old_version THEN
            RAISE EXCEPTION 'retention policy activation must move forward'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER retention_policies_validate_active_version
BEFORE UPDATE ON retention_policies
FOR EACH ROW EXECUTE FUNCTION validate_retention_policy_active_version_update();

CREATE OR REPLACE FUNCTION reject_retention_policy_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'retention policy history is immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER retention_policies_reject_delete
BEFORE DELETE ON retention_policies
FOR EACH ROW EXECUTE FUNCTION reject_retention_policy_history_mutation();

CREATE TRIGGER retention_policies_reject_truncate
BEFORE TRUNCATE ON retention_policies
FOR EACH STATEMENT EXECUTE FUNCTION reject_retention_policy_history_mutation();

CREATE TRIGGER retention_policy_versions_reject_update
BEFORE UPDATE ON retention_policy_versions
FOR EACH ROW EXECUTE FUNCTION reject_retention_policy_history_mutation();

CREATE TRIGGER retention_policy_versions_reject_delete
BEFORE DELETE ON retention_policy_versions
FOR EACH ROW EXECUTE FUNCTION reject_retention_policy_history_mutation();

CREATE TRIGGER retention_policy_versions_reject_truncate
BEFORE TRUNCATE ON retention_policy_versions
FOR EACH STATEMENT EXECUTE FUNCTION reject_retention_policy_history_mutation();

CREATE TRIGGER retention_policy_activations_reject_update
BEFORE UPDATE ON retention_policy_activations
FOR EACH ROW EXECUTE FUNCTION reject_retention_policy_history_mutation();

CREATE TRIGGER retention_policy_activations_reject_delete
BEFORE DELETE ON retention_policy_activations
FOR EACH ROW EXECUTE FUNCTION reject_retention_policy_history_mutation();

CREATE TRIGGER retention_policy_activations_reject_truncate
BEFORE TRUNCATE ON retention_policy_activations
FOR EACH STATEMENT EXECUTE FUNCTION reject_retention_policy_history_mutation();
