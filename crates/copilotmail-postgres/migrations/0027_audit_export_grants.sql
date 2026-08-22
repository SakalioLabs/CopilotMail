INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('audit.export.authorized', 'audit.export_grant', 'critical', true);

CREATE TABLE audit_export_grants (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    archive_id uuid NOT NULL,
    approval_request_id uuid NOT NULL UNIQUE,
    requested_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    UNIQUE(id, organization_id),
    FOREIGN KEY(archive_id, organization_id)
        REFERENCES audit_archives(id, organization_id),
    FOREIGN KEY(approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id),
    FOREIGN KEY(requested_by, organization_id)
        REFERENCES principals(id, organization_id),
    CHECK (
        expires_at > created_at
        AND expires_at <= created_at + interval '30 minutes'
    )
);

CREATE INDEX audit_export_grants_tenant_created_idx
    ON audit_export_grants(organization_id, created_at DESC, id DESC);

CREATE OR REPLACE FUNCTION validate_audit_export_grant_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS actor
         WHERE actor.id = NEW.requested_by
           AND actor.organization_id = NEW.organization_id
           AND actor.kind = 'human'
           AND actor.status = 'active'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM audit_archives AS archive
         WHERE archive.id = NEW.archive_id
           AND archive.organization_id = NEW.organization_id
    )
    OR NOT EXISTS (
        SELECT 1
          FROM approval_requests AS request
         WHERE request.id = NEW.approval_request_id
           AND request.organization_id = NEW.organization_id
           AND request.action = 'audit.export'
           AND request.resource_kind = 'audit.archive'
           AND request.resource_id = NEW.archive_id::text
           AND request.requested_by = NEW.requested_by
           AND request.status = 'approved'
           AND request.required_approvals >= 2
           AND request.expires_at > NEW.created_at
           AND NEW.expires_at <= request.expires_at
           AND (
               SELECT count(*)
                 FROM approval_steps AS step
                WHERE step.approval_request_id = request.id
                  AND step.organization_id = request.organization_id
                  AND step.decision = 'approved'
           ) >= request.required_approvals
           AND NOT EXISTS (
               SELECT 1
                 FROM approval_steps AS step
                WHERE step.approval_request_id = request.id
                  AND step.organization_id = request.organization_id
                  AND step.decision = 'rejected'
           )
    )
    THEN
        RAISE EXCEPTION 'audit export grant is not exact and authorized'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER audit_export_grants_validate_insert
BEFORE INSERT ON audit_export_grants
FOR EACH ROW EXECUTE FUNCTION validate_audit_export_grant_insert();

CREATE OR REPLACE FUNCTION reject_audit_export_grant_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit export grants are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_export_grants_reject_update
BEFORE UPDATE ON audit_export_grants
FOR EACH ROW EXECUTE FUNCTION reject_audit_export_grant_mutation();

CREATE TRIGGER audit_export_grants_reject_delete
BEFORE DELETE ON audit_export_grants
FOR EACH ROW EXECUTE FUNCTION reject_audit_export_grant_mutation();

CREATE TRIGGER audit_export_grants_reject_truncate
BEFORE TRUNCATE ON audit_export_grants
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_export_grant_mutation();
