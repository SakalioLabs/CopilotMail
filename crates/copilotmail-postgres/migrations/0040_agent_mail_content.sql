INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('agent.mail_content.read', 'mail.message', 'high', true);

INSERT INTO role_permissions(role_id, permission)
SELECT role.id, 'agent.mail.read'
  FROM roles AS role
 WHERE role.system_key = 'agent_operator'
ON CONFLICT DO NOTHING;
