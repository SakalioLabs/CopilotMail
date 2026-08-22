CREATE TABLE policies (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    policy_key text NOT NULL CHECK (
        char_length(policy_key) BETWEEN 1 AND 100
        AND policy_key = lower(policy_key)
        AND policy_key ~ '^[a-z0-9]+([.][a-z0-9]+)*$'
    ),
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 200),
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    retired_at timestamptz,
    UNIQUE(id, organization_id),
    UNIQUE(organization_id, policy_key),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE TABLE policy_versions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    policy_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    action text NOT NULL CHECK (action IN (
        'mail.send_as',
        'mail.send_on_behalf_of',
        'compliance.content_review',
        'audit.export',
        'authorization.privilege_elevation',
        'compliance.legal_hold',
        'security.break_glass',
        'mail.bulk_delete'
    )),
    required_approvals smallint NOT NULL CHECK (required_approvals BETWEEN 1 AND 5),
    rule jsonb NOT NULL CHECK (jsonb_typeof(rule) = 'object'),
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    UNIQUE(id, organization_id, policy_id),
    UNIQUE(policy_id, version),
    FOREIGN KEY(policy_id, organization_id)
        REFERENCES policies(id, organization_id),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id)
);

ALTER TABLE policies ADD COLUMN active_version_id uuid;
ALTER TABLE policies ADD CONSTRAINT policies_active_version_tenant_fk
    FOREIGN KEY(active_version_id, organization_id, id)
    REFERENCES policy_versions(id, organization_id, policy_id);

CREATE TABLE approval_requests (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    policy_version_id uuid NOT NULL,
    action text NOT NULL CHECK (action IN (
        'mail.send_as',
        'mail.send_on_behalf_of',
        'compliance.content_review',
        'audit.export',
        'authorization.privilege_elevation',
        'compliance.legal_hold',
        'security.break_glass',
        'mail.bulk_delete'
    )),
    resource_kind text NOT NULL CHECK (char_length(resource_kind) BETWEEN 1 AND 100),
    resource_id text NOT NULL CHECK (char_length(resource_id) BETWEEN 1 AND 500),
    requested_by uuid NOT NULL,
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 1000),
    ticket_reference text CHECK (char_length(ticket_reference) BETWEEN 1 AND 500),
    required_approvals smallint NOT NULL CHECK (required_approvals BETWEEN 1 AND 5),
    status text NOT NULL CHECK (status IN (
        'pending', 'approved', 'rejected', 'expired', 'cancelled'
    )),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    completed_at timestamptz,
    cancelled_at timestamptz,
    UNIQUE(id, organization_id),
    FOREIGN KEY(policy_version_id, organization_id)
        REFERENCES policy_versions(id, organization_id),
    FOREIGN KEY(requested_by, organization_id)
        REFERENCES principals(id, organization_id),
    CHECK (expires_at > created_at),
    CHECK (expires_at <= created_at + interval '30 days')
);

CREATE INDEX approval_requests_pending_idx
    ON approval_requests(organization_id, expires_at, created_at)
    WHERE status = 'pending';

CREATE TABLE approval_steps (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    approval_request_id uuid NOT NULL,
    approver_principal_id uuid NOT NULL,
    decision text NOT NULL CHECK (decision IN ('approved', 'rejected')),
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 1000),
    decided_at timestamptz NOT NULL,
    UNIQUE(approval_request_id, approver_principal_id),
    FOREIGN KEY(approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id) ON DELETE RESTRICT,
    FOREIGN KEY(approver_principal_id, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE OR REPLACE FUNCTION reject_policy_version_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'policy_versions is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER policy_versions_append_only
BEFORE UPDATE OR DELETE ON policy_versions
FOR EACH ROW EXECUTE FUNCTION reject_policy_version_mutation();

CREATE OR REPLACE FUNCTION enforce_approval_separation_of_duty()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
          FROM approval_requests
         WHERE id = NEW.approval_request_id
           AND organization_id = NEW.organization_id
           AND requested_by = NEW.approver_principal_id
    ) THEN
        RAISE EXCEPTION 'requester cannot approve own request' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER approval_steps_separation_of_duty
BEFORE INSERT ON approval_steps
FOR EACH ROW EXECUTE FUNCTION enforce_approval_separation_of_duty();

CREATE OR REPLACE FUNCTION reject_approval_step_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'approval_steps is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER approval_steps_append_only
BEFORE UPDATE OR DELETE ON approval_steps
FOR EACH ROW EXECUTE FUNCTION reject_approval_step_mutation();
