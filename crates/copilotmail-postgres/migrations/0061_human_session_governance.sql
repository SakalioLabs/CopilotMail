-- Fine-grained revocation of another principal's browser sessions.
-- Organization Owner receives session.revoke_others. Helpdesk
-- (identity.manage) and Security Administrator (security.admin) do not.
-- Executing the revocation consumes one exact dual-approval request.

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
            'session.revoke_others',
            'audit.read',
            'audit.export',
            'legal_hold.manage',
            'retention.manage',
            'content.review',
            'agent.operate',
            'agent.mail.read',
            'mailbox.use',
            'break_glass.invoke'
        )
    );

INSERT INTO role_permissions(role_id, permission)
SELECT role.id, 'session.revoke_others'
  FROM roles AS role
 WHERE role.system_key = 'org_owner'
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
        'mail.bulk_delete',
        'agent.production_mail',
        'agent.first_contact',
        'session.revoke_others'
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
        'agent.production_mail',
        'agent.first_contact',
        'session.revoke_others'
    ));

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('identity.human_sessions.revoked', 'identity.principal', 'high', true);

-- One approval can revoke the sessions that exist at execution time. It cannot
-- be replayed later against sessions created after this row.
CREATE TABLE human_session_governance_executions (
    approval_request_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    target_principal_id uuid NOT NULL,
    requested_by uuid NOT NULL,
    excluded_session_id uuid,
    executed_at timestamptz NOT NULL,
    FOREIGN KEY (approval_request_id, organization_id)
        REFERENCES approval_requests (id, organization_id),
    FOREIGN KEY (requested_by, organization_id)
        REFERENCES principals (id, organization_id),
    FOREIGN KEY (excluded_session_id)
        REFERENCES human_browser_sessions (id)
);

CREATE INDEX human_session_governance_executions_tenant_idx
    ON human_session_governance_executions (organization_id, target_principal_id, executed_at);

CREATE OR REPLACE FUNCTION reject_human_session_governance_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'human session governance execution is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER human_session_governance_executions_reject_update
BEFORE UPDATE ON human_session_governance_executions
FOR EACH ROW EXECUTE FUNCTION reject_human_session_governance_mutation();
CREATE TRIGGER human_session_governance_executions_reject_delete
BEFORE DELETE ON human_session_governance_executions
FOR EACH ROW EXECUTE FUNCTION reject_human_session_governance_mutation();
CREATE TRIGGER human_session_governance_executions_reject_truncate
BEFORE TRUNCATE ON human_session_governance_executions
FOR EACH STATEMENT EXECUTE FUNCTION reject_human_session_governance_mutation();
