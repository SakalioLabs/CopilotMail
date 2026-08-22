-- Every tenant receives the complete stable role catalog. Runtime bootstrap
-- creates the same rows for organizations added after this migration.
INSERT INTO roles(id, organization_id, system_key, display_name, created_at)
SELECT gen_random_uuid(),
       organization.id,
       role.system_key,
       role.display_name,
       organization.created_at
  FROM organizations AS organization
 CROSS JOIN (
    VALUES
        ('org_owner', 'Organization Owner'),
        ('mail_admin', 'Mail Administrator'),
        ('security_admin', 'Security Administrator'),
        ('compliance_auditor', 'Compliance Auditor'),
        ('content_reviewer', 'Content Reviewer'),
        ('helpdesk', 'Helpdesk'),
        ('agent_operator', 'Agent Operator'),
        ('employee', 'Employee'),
        ('break_glass', 'Break Glass Operator')
 ) AS role(system_key, display_name)
ON CONFLICT (organization_id, system_key) DO NOTHING;

INSERT INTO role_permissions(role_id, permission)
SELECT role.id, permission.permission
  FROM roles AS role
  JOIN (
    VALUES
        ('org_owner', 'organization.read'),
        ('org_owner', 'organization.manage'),
        ('org_owner', 'identity.manage'),
        ('org_owner', 'authorization.manage'),
        ('org_owner', 'mail.admin'),
        ('org_owner', 'security.admin'),
        ('org_owner', 'audit.read'),
        ('org_owner', 'audit.export'),
        ('org_owner', 'legal_hold.manage'),
        ('org_owner', 'retention.manage'),
        ('org_owner', 'agent.operate'),
        ('org_owner', 'mailbox.use'),
        ('mail_admin', 'organization.read'),
        ('mail_admin', 'mail.admin'),
        ('mail_admin', 'mailbox.use'),
        ('security_admin', 'organization.read'),
        ('security_admin', 'security.admin'),
        ('security_admin', 'audit.read'),
        ('compliance_auditor', 'organization.read'),
        ('compliance_auditor', 'audit.read'),
        ('compliance_auditor', 'audit.export'),
        ('content_reviewer', 'organization.read'),
        ('content_reviewer', 'content.review'),
        ('helpdesk', 'organization.read'),
        ('helpdesk', 'identity.manage'),
        ('agent_operator', 'organization.read'),
        ('agent_operator', 'agent.operate'),
        ('agent_operator', 'mailbox.use'),
        ('employee', 'organization.read'),
        ('employee', 'mailbox.use'),
        ('break_glass', 'organization.read'),
        ('break_glass', 'break_glass.invoke')
  ) AS permission(system_key, permission)
    ON permission.system_key = role.system_key
ON CONFLICT DO NOTHING;
