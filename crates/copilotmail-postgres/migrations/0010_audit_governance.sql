CREATE TABLE audit_event_catalog (
    action text PRIMARY KEY,
    resource_type text NOT NULL,
    risk text NOT NULL CHECK (risk IN ('low', 'medium', 'high', 'critical')),
    privileged boolean NOT NULL,
    UNIQUE(action, resource_type)
);

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('organization.bootstrapped', 'organization.tenant', 'critical', true),
    ('identity.principal.created', 'identity.principal', 'high', true),
    ('identity.oidc_owner.bound', 'identity.principal', 'critical', true),
    ('identity.agent.registered', 'identity.agent', 'high', true),
    ('identity.agent_credential.added', 'identity.agent_credential', 'high', true),
    ('identity.agent_mtls_credential.added', 'identity.agent_credential', 'high', true),
    ('identity.agent_credential.revoked', 'identity.agent_credential', 'high', true),
    ('identity.scim_token.issued', 'identity.scim_token', 'high', true),
    ('identity.scim_token.revoked', 'identity.scim_token', 'high', true),
    ('identity.scim_user.provisioned', 'identity.scim_user', 'medium', true),
    ('identity.scim_user.adopted', 'identity.scim_user', 'medium', true),
    ('identity.scim_user.updated', 'identity.scim_user', 'high', true),
    ('identity.scim_group.provisioned', 'identity.scim_group', 'medium', true),
    ('identity.scim_group.updated', 'identity.scim_group', 'medium', true),
    ('identity.scim_group.deleted', 'identity.scim_group', 'high', true),
    ('authorization.approval_policy.created', 'authorization.approval_policy', 'high', true),
    ('authorization.approval_policy.versioned', 'authorization.approval_policy', 'high', true),
    ('authorization.approval.requested', 'authorization.approval_request', 'medium', true),
    ('authorization.approval.decided', 'authorization.approval_request', 'high', true),
    ('mailbox.created', 'mailbox', 'medium', true),
    ('authorization.mailbox_delegation.granted', 'authorization.mailbox_delegation', 'high', true),
    ('authorization.mailbox_delegation.revoked', 'authorization.mailbox_delegation', 'high', true);

ALTER TABLE audit_events
    ADD COLUMN received_at timestamptz,
    ADD COLUMN request_id text,
    ADD COLUMN subject_principal_id uuid,
    ADD COLUMN source_ip inet,
    ADD COLUMN client_id text,
    ADD COLUMN session_id text,
    ADD COLUMN token_jti text,
    ADD COLUMN policy_id uuid,
    ADD COLUMN policy_version_id uuid,
    ADD COLUMN approval_request_id uuid,
    ADD COLUMN ticket_reference text,
    ADD COLUMN environment text,
    ADD COLUMN region text;

UPDATE audit_events
   SET received_at = occurred_at,
       request_id = trace_id;

ALTER TABLE audit_events
    ALTER COLUMN received_at SET NOT NULL,
    ALTER COLUMN request_id SET NOT NULL,
    ADD CONSTRAINT audit_events_catalog_fk
        FOREIGN KEY(action, resource_type)
        REFERENCES audit_event_catalog(action, resource_type),
    ADD CONSTRAINT audit_events_actor_tenant_fk
        FOREIGN KEY(actor_principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT audit_events_on_behalf_tenant_fk
        FOREIGN KEY(on_behalf_of_principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT audit_events_subject_tenant_fk
        FOREIGN KEY(subject_principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT audit_events_policy_tenant_fk
        FOREIGN KEY(policy_id, organization_id)
        REFERENCES policies(id, organization_id),
    ADD CONSTRAINT audit_events_policy_version_tenant_fk
        FOREIGN KEY(policy_version_id, organization_id)
        REFERENCES policy_versions(id, organization_id),
    ADD CONSTRAINT audit_events_approval_tenant_fk
        FOREIGN KEY(approval_request_id, organization_id)
        REFERENCES approval_requests(id, organization_id),
    ADD CONSTRAINT audit_events_request_id_check
        CHECK (char_length(request_id) BETWEEN 1 AND 200),
    ADD CONSTRAINT audit_events_no_top_level_secrets_check CHECK (
        NOT details ?| ARRAY[
            'password', 'access_token', 'refresh_token', 'client_assertion',
            'authorization', 'cookie', 'secret', 'private_key', 'plaintext',
            'body', 'raw_mime', 'attachment_content', 'message_body'
        ]
    ),
    ADD CONSTRAINT audit_events_context_check CHECK (
        (client_id IS NULL OR char_length(client_id) BETWEEN 1 AND 500)
        AND (session_id IS NULL OR char_length(session_id) BETWEEN 1 AND 500)
        AND (token_jti IS NULL OR char_length(token_jti) BETWEEN 1 AND 500)
        AND (ticket_reference IS NULL OR char_length(ticket_reference) BETWEEN 1 AND 500)
        AND (environment IS NULL OR char_length(environment) BETWEEN 1 AND 100)
        AND (region IS NULL OR char_length(region) BETWEEN 1 AND 100)
    );

CREATE INDEX audit_events_request_idx
    ON audit_events(organization_id, request_id, occurred_at DESC);
CREATE INDEX audit_events_approval_idx
    ON audit_events(organization_id, approval_request_id, occurred_at DESC)
    WHERE approval_request_id IS NOT NULL;

CREATE OR REPLACE FUNCTION reject_audit_catalog_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'audit_event_catalog is migration-managed' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER audit_event_catalog_immutable
BEFORE UPDATE OR DELETE ON audit_event_catalog
FOR EACH ROW EXECUTE FUNCTION reject_audit_catalog_mutation();
