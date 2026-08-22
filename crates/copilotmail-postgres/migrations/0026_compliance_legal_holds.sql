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
            'content.review',
            'agent.operate',
            'mailbox.use',
            'break_glass.invoke'
        )
    );

INSERT INTO role_permissions(role_id, permission)
SELECT id, 'legal_hold.manage'
FROM roles
WHERE system_key = 'org_owner'
ON CONFLICT DO NOTHING;

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('compliance.case.created', 'compliance.case', 'high', true),
    ('compliance.case.closed', 'compliance.case', 'high', true),
    ('compliance.case.queried', 'compliance.case_query', 'high', true),
    ('compliance.legal_hold.applied', 'compliance.legal_hold', 'critical', true),
    ('compliance.legal_hold.released', 'compliance.legal_hold', 'critical', true),
    ('compliance.legal_hold.queried', 'compliance.legal_hold_query', 'high', true);

ALTER TABLE audit_archives
    ADD CONSTRAINT audit_archives_tenant_id_unique UNIQUE(id, organization_id);

CREATE TABLE compliance_cases (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    external_reference text NOT NULL,
    title text NOT NULL,
    purpose text NOT NULL,
    status text NOT NULL CHECK (status IN ('open', 'closed')),
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    closed_by uuid,
    closed_at timestamptz,
    UNIQUE(id, organization_id),
    UNIQUE(organization_id, external_reference),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(closed_by, organization_id)
        REFERENCES principals(id, organization_id),
    CHECK (
        char_length(external_reference) BETWEEN 1 AND 200
        AND external_reference = btrim(external_reference)
        AND external_reference !~ '[[:cntrl:]]'
        AND char_length(title) BETWEEN 1 AND 200
        AND title = btrim(title)
        AND title !~ '[[:cntrl:]]'
        AND char_length(purpose) BETWEEN 1 AND 2000
        AND purpose = btrim(purpose)
        AND purpose !~ '[[:cntrl:]]'
    ),
    CHECK (
        (status = 'open' AND closed_by IS NULL AND closed_at IS NULL)
        OR
        (status = 'closed' AND closed_by IS NOT NULL
         AND closed_at IS NOT NULL AND closed_at >= created_at)
    )
);

CREATE TABLE legal_holds (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    case_id uuid NOT NULL,
    archive_id uuid NOT NULL,
    reason text NOT NULL CHECK (
        char_length(reason) BETWEEN 1 AND 2000
        AND reason = btrim(reason)
        AND reason !~ '[[:cntrl:]]'
    ),
    status text NOT NULL CHECK (status IN ('active', 'released')),
    apply_approval_request_id uuid NOT NULL,
    applied_events_version_id text NOT NULL,
    applied_manifest_version_id text NOT NULL,
    applied_verified_at timestamptz NOT NULL,
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    release_approval_request_id uuid,
    released_events_version_id text,
    released_manifest_version_id text,
    released_verified_at timestamptz,
    released_by uuid,
    released_at timestamptz,
    UNIQUE(id, organization_id),
    UNIQUE(apply_approval_request_id),
    UNIQUE(release_approval_request_id),
    FOREIGN KEY(case_id, organization_id)
        REFERENCES compliance_cases(id, organization_id),
    FOREIGN KEY(archive_id, organization_id)
        REFERENCES audit_archives(id, organization_id),
    FOREIGN KEY(apply_approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id),
    FOREIGN KEY(release_approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(released_by, organization_id)
        REFERENCES principals(id, organization_id),
    CHECK (
        char_length(applied_events_version_id) BETWEEN 1 AND 1024
        AND char_length(applied_manifest_version_id) BETWEEN 1 AND 1024
        AND applied_events_version_id = btrim(applied_events_version_id)
        AND applied_manifest_version_id = btrim(applied_manifest_version_id)
        AND applied_events_version_id !~ '[[:cntrl:]]'
        AND applied_manifest_version_id !~ '[[:cntrl:]]'
        AND applied_verified_at <= created_at
    ),
    CHECK (
        (status = 'active'
         AND release_approval_request_id IS NULL
         AND released_events_version_id IS NULL
         AND released_manifest_version_id IS NULL
         AND released_verified_at IS NULL
         AND released_by IS NULL
         AND released_at IS NULL)
        OR
        (status = 'released'
         AND release_approval_request_id IS NOT NULL
         AND release_approval_request_id <> apply_approval_request_id
         AND released_events_version_id IS NOT NULL
         AND released_manifest_version_id IS NOT NULL
         AND released_verified_at IS NOT NULL
         AND released_by IS NOT NULL
         AND released_at IS NOT NULL
         AND released_at >= created_at
         AND released_verified_at <= released_at)
    )
);

CREATE INDEX compliance_cases_list_idx
    ON compliance_cases(organization_id, status, created_at DESC, id DESC);
CREATE UNIQUE INDEX legal_holds_one_active_per_archive
    ON legal_holds(organization_id, archive_id)
    WHERE status = 'active';
CREATE INDEX legal_holds_case_status_idx
    ON legal_holds(
        organization_id, case_id, status, created_at DESC, id DESC
    );
CREATE INDEX legal_holds_list_idx
    ON legal_holds(organization_id, status, created_at DESC, id DESC);

CREATE OR REPLACE FUNCTION validate_legal_hold_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM compliance_cases AS compliance_case
         WHERE compliance_case.id = NEW.case_id
           AND compliance_case.organization_id = NEW.organization_id
           AND compliance_case.status = 'open'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM principals AS actor
         WHERE actor.id = NEW.created_by
           AND actor.organization_id = NEW.organization_id
           AND actor.kind = 'human'
           AND actor.status = 'active'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM audit_archives AS archive
         WHERE archive.id = NEW.archive_id
           AND archive.organization_id = NEW.organization_id
           AND archive.events_version_id = NEW.applied_events_version_id
           AND archive.manifest_version_id = NEW.applied_manifest_version_id
           AND NEW.applied_verified_at >= archive.verified_at
    )
    OR NOT EXISTS (
        SELECT 1
          FROM approval_requests AS request
         WHERE request.id = NEW.apply_approval_request_id
           AND request.organization_id = NEW.organization_id
           AND request.action = 'compliance.legal_hold'
           AND request.resource_kind = 'audit.archive'
           AND request.resource_id = NEW.archive_id::text
           AND request.requested_by = NEW.created_by
           AND request.status = 'approved'
           AND request.expires_at > NEW.created_at
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
        RAISE EXCEPTION 'Legal Hold apply evidence is not exact and authorized'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER legal_holds_validate_insert
BEFORE INSERT ON legal_holds
FOR EACH ROW EXECUTE FUNCTION validate_legal_hold_insert();

CREATE OR REPLACE FUNCTION enforce_compliance_case_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.id <> NEW.id
       OR OLD.organization_id <> NEW.organization_id
       OR OLD.external_reference <> NEW.external_reference
       OR OLD.title <> NEW.title
       OR OLD.purpose <> NEW.purpose
       OR OLD.created_by <> NEW.created_by
       OR OLD.created_at <> NEW.created_at
       OR OLD.status <> 'open'
       OR NEW.status <> 'closed'
       OR EXISTS (
           SELECT 1 FROM legal_holds
           WHERE organization_id = OLD.organization_id
             AND case_id = OLD.id
             AND status = 'active'
       )
    THEN
        RAISE EXCEPTION 'invalid compliance case transition' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER compliance_cases_controlled_update
BEFORE UPDATE ON compliance_cases
FOR EACH ROW EXECUTE FUNCTION enforce_compliance_case_transition();

CREATE OR REPLACE FUNCTION enforce_legal_hold_transition()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.id <> NEW.id
       OR OLD.organization_id <> NEW.organization_id
       OR OLD.case_id <> NEW.case_id
       OR OLD.archive_id <> NEW.archive_id
       OR OLD.reason <> NEW.reason
       OR OLD.apply_approval_request_id <> NEW.apply_approval_request_id
       OR OLD.applied_events_version_id <> NEW.applied_events_version_id
       OR OLD.applied_manifest_version_id <> NEW.applied_manifest_version_id
       OR OLD.applied_verified_at <> NEW.applied_verified_at
       OR OLD.created_by <> NEW.created_by
       OR OLD.created_at <> NEW.created_at
       OR OLD.status <> 'active'
       OR NEW.status <> 'released'
       OR NOT EXISTS (
           SELECT 1
             FROM principals AS actor
            WHERE actor.id = NEW.released_by
              AND actor.organization_id = NEW.organization_id
              AND actor.kind = 'human'
              AND actor.status = 'active'
       )
       OR NOT EXISTS (
           SELECT 1
             FROM audit_archives AS archive
            WHERE archive.id = NEW.archive_id
              AND archive.organization_id = NEW.organization_id
              AND archive.events_version_id = NEW.released_events_version_id
              AND archive.manifest_version_id = NEW.released_manifest_version_id
              AND NEW.released_verified_at >= archive.verified_at
       )
       OR NOT EXISTS (
           SELECT 1
             FROM approval_requests AS request
            WHERE request.id = NEW.release_approval_request_id
              AND request.organization_id = NEW.organization_id
              AND request.action = 'compliance.legal_hold'
              AND request.resource_kind = 'compliance.legal_hold'
              AND request.resource_id = NEW.id::text
              AND request.requested_by = NEW.released_by
              AND request.status = 'approved'
              AND request.expires_at > NEW.released_at
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
        RAISE EXCEPTION 'invalid legal hold transition' USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER legal_holds_controlled_update
BEFORE UPDATE ON legal_holds
FOR EACH ROW EXECUTE FUNCTION enforce_legal_hold_transition();

CREATE OR REPLACE FUNCTION reject_compliance_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'compliance evidence cannot be deleted' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER compliance_cases_reject_delete
BEFORE DELETE ON compliance_cases
FOR EACH ROW EXECUTE FUNCTION reject_compliance_delete();
CREATE TRIGGER legal_holds_reject_delete
BEFORE DELETE ON legal_holds
FOR EACH ROW EXECUTE FUNCTION reject_compliance_delete();
CREATE TRIGGER compliance_cases_reject_truncate
BEFORE TRUNCATE ON compliance_cases
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_truncate();
CREATE TRIGGER legal_holds_reject_truncate
BEFORE TRUNCATE ON legal_holds
FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_truncate();
