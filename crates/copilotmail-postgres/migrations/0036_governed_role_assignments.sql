INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('authorization.role_assignment.granted', 'authorization.role_assignment', 'critical', true),
    ('authorization.role_assignment.revoked', 'authorization.role_assignment', 'critical', true),
    ('authorization.role_assignment.queried', 'authorization.role_assignment_query', 'high', true);

ALTER TABLE role_assignments
    ADD CONSTRAINT role_assignments_id_organization_unique
    UNIQUE(id, organization_id);

CREATE TABLE role_assignment_grants (
    assignment_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    approval_request_id uuid NOT NULL UNIQUE,
    granted_by uuid NOT NULL,
    granted_at timestamptz NOT NULL,
    FOREIGN KEY(assignment_id, organization_id)
        REFERENCES role_assignments(id, organization_id),
    FOREIGN KEY(approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id),
    FOREIGN KEY(granted_by, organization_id)
        REFERENCES principals(id, organization_id)
);

CREATE INDEX role_assignments_tenant_page_idx
    ON role_assignments(
        organization_id,
        created_at DESC,
        id DESC
    );

CREATE INDEX role_assignments_tenant_principal_page_idx
    ON role_assignments(
        organization_id,
        principal_id,
        created_at DESC,
        id DESC
    );

CREATE OR REPLACE FUNCTION validate_role_assignment_grant_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    assignment_record role_assignments%ROWTYPE;
    approval_record approval_requests%ROWTYPE;
    target_kind text;
    target_status text;
    grantor_kind text;
    grantor_status text;
    role_key text;
    expected_resource_id text;
    approval_count bigint;
    rejection_count bigint;
BEGIN
    SELECT *
      INTO assignment_record
      FROM role_assignments
     WHERE id = NEW.assignment_id
       AND organization_id = NEW.organization_id;

    IF NOT FOUND
        OR assignment_record.created_by <> NEW.granted_by
        OR assignment_record.created_at <> NEW.granted_at
        OR assignment_record.valid_from <> NEW.granted_at
        OR assignment_record.valid_until IS NULL
        OR assignment_record.valid_until <= assignment_record.valid_from
        OR assignment_record.valid_until >
           assignment_record.valid_from + interval '365 days'
        OR assignment_record.revoked_at IS NOT NULL
    THEN
        RAISE EXCEPTION 'invalid governed role assignment evidence'
            USING ERRCODE = '23514';
    END IF;

    SELECT kind, status
      INTO target_kind, target_status
      FROM principals
     WHERE id = assignment_record.principal_id
       AND organization_id = NEW.organization_id;

    SELECT kind, status
      INTO grantor_kind, grantor_status
      FROM principals
     WHERE id = NEW.granted_by
       AND organization_id = NEW.organization_id;

    SELECT system_key
      INTO role_key
      FROM roles
     WHERE id = assignment_record.role_id
       AND organization_id = NEW.organization_id;

    IF target_status <> 'active'
        OR grantor_kind <> 'human'
        OR grantor_status <> 'active'
        OR role_key IS NULL
        OR (target_kind <> 'human' AND role_key <> 'agent_operator')
    THEN
        RAISE EXCEPTION 'role assignment principal or role is ineligible'
            USING ERRCODE = '23514';
    END IF;

    expected_resource_id :=
        'v1:' || assignment_record.principal_id::text ||
        ':' || assignment_record.role_id::text ||
        ':' || floor(
            extract(epoch FROM assignment_record.valid_until) * 1000
        )::bigint::text;

    SELECT *
      INTO approval_record
      FROM approval_requests
     WHERE id = NEW.approval_request_id
       AND organization_id = NEW.organization_id;

    SELECT count(*) FILTER (WHERE decision = 'approved'),
           count(*) FILTER (WHERE decision = 'rejected')
      INTO approval_count, rejection_count
      FROM approval_steps
     WHERE organization_id = NEW.organization_id
       AND approval_request_id = NEW.approval_request_id;

    IF approval_record.id IS NULL
        OR approval_record.action <> 'authorization.privilege_elevation'
        OR approval_record.resource_kind <> 'authorization.role_assignment'
        OR approval_record.resource_id <> expected_resource_id
        OR approval_record.requested_by <> NEW.granted_by
        OR approval_record.status <> 'approved'
        OR approval_record.expires_at <= NEW.granted_at
        OR approval_record.required_approvals < 2
        OR approval_count < approval_record.required_approvals
        OR rejection_count <> 0
    THEN
        RAISE EXCEPTION 'role assignment approval evidence is invalid'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER role_assignment_grants_validate_insert
BEFORE INSERT ON role_assignment_grants
FOR EACH ROW EXECUTE FUNCTION validate_role_assignment_grant_insert();

CREATE OR REPLACE FUNCTION validate_role_assignment_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
        OR NEW.organization_id <> OLD.organization_id
        OR NEW.principal_id <> OLD.principal_id
        OR NEW.role_id <> OLD.role_id
        OR NEW.created_by <> OLD.created_by
        OR NEW.valid_from <> OLD.valid_from
        OR NEW.valid_until IS DISTINCT FROM OLD.valid_until
        OR NEW.created_at <> OLD.created_at
        OR OLD.revoked_at IS NOT NULL
        OR NEW.revoked_at IS NULL
        OR NEW.revoked_at < OLD.created_at
    THEN
        RAISE EXCEPTION 'invalid role assignment mutation'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER role_assignments_validate_update
BEFORE UPDATE ON role_assignments
FOR EACH ROW EXECUTE FUNCTION validate_role_assignment_update();

CREATE OR REPLACE FUNCTION reject_role_governance_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'role governance history is immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER roles_reject_update
BEFORE UPDATE ON roles
FOR EACH ROW EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER roles_reject_delete
BEFORE DELETE ON roles
FOR EACH ROW EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER roles_reject_truncate
BEFORE TRUNCATE ON roles
FOR EACH STATEMENT EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_permissions_reject_update
BEFORE UPDATE ON role_permissions
FOR EACH ROW EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_permissions_reject_delete
BEFORE DELETE ON role_permissions
FOR EACH ROW EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_permissions_reject_truncate
BEFORE TRUNCATE ON role_permissions
FOR EACH STATEMENT EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_assignments_reject_delete
BEFORE DELETE ON role_assignments
FOR EACH ROW EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_assignments_reject_truncate
BEFORE TRUNCATE ON role_assignments
FOR EACH STATEMENT EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_assignment_grants_reject_update
BEFORE UPDATE ON role_assignment_grants
FOR EACH ROW EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_assignment_grants_reject_delete
BEFORE DELETE ON role_assignment_grants
FOR EACH ROW EXECUTE FUNCTION reject_role_governance_mutation();

CREATE TRIGGER role_assignment_grants_reject_truncate
BEFORE TRUNCATE ON role_assignment_grants
FOR EACH STATEMENT EXECUTE FUNCTION reject_role_governance_mutation();
