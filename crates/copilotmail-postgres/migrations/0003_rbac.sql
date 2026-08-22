CREATE TABLE roles (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    system_key text CHECK (
        system_key IS NULL OR system_key IN (
            'org_owner',
            'mail_admin',
            'security_admin',
            'compliance_auditor',
            'content_reviewer',
            'helpdesk',
            'agent_operator',
            'employee',
            'break_glass'
        )
    ),
    display_name text NOT NULL CHECK (char_length(display_name) BETWEEN 1 AND 200),
    created_at timestamptz NOT NULL,
    UNIQUE(organization_id, system_key),
    UNIQUE(id, organization_id)
);

CREATE TABLE role_permissions (
    role_id uuid NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission text NOT NULL CHECK (
        permission IN (
            'organization.read',
            'organization.manage',
            'identity.manage',
            'authorization.manage',
            'mail.admin',
            'security.admin',
            'audit.read',
            'audit.export',
            'content.review',
            'agent.operate',
            'mailbox.use',
            'break_glass.invoke'
        )
    ),
    PRIMARY KEY(role_id, permission)
);

CREATE TABLE role_assignments (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL,
    role_id uuid NOT NULL,
    created_by uuid NOT NULL,
    valid_from timestamptz NOT NULL,
    valid_until timestamptz,
    revoked_at timestamptz,
    created_at timestamptz NOT NULL,
    CHECK (valid_until IS NULL OR valid_until > valid_from),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at),
    FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(role_id, organization_id)
        REFERENCES roles(id, organization_id)
);

CREATE UNIQUE INDEX role_assignments_active_idx
    ON role_assignments(principal_id, role_id)
    WHERE revoked_at IS NULL;
CREATE INDEX role_assignments_resolution_idx
    ON role_assignments(principal_id, organization_id, valid_from, valid_until)
    WHERE revoked_at IS NULL;

-- Backfill organizations created before the RBAC migration.
INSERT INTO roles(id, organization_id, system_key, display_name, created_at)
SELECT gen_random_uuid(), organization.id, 'org_owner', 'Organization Owner', organization.created_at
FROM organizations AS organization
ON CONFLICT (organization_id, system_key) DO NOTHING;

INSERT INTO role_permissions(role_id, permission)
SELECT role.id, permission.value
FROM roles AS role
CROSS JOIN (
    VALUES
        ('organization.read'),
        ('organization.manage'),
        ('identity.manage'),
        ('authorization.manage'),
        ('mail.admin'),
        ('security.admin'),
        ('audit.read'),
        ('audit.export'),
        ('agent.operate'),
        ('mailbox.use')
) AS permission(value)
WHERE role.system_key = 'org_owner'
ON CONFLICT DO NOTHING;

INSERT INTO role_assignments(
    id, organization_id, principal_id, role_id, created_by,
    valid_from, valid_until, revoked_at, created_at
)
SELECT gen_random_uuid(),
       bootstrap.organization_id,
       bootstrap.root_principal_id,
       role.id,
       bootstrap.root_principal_id,
       bootstrap.created_at,
       NULL,
       NULL,
       bootstrap.created_at
FROM organization_bootstraps AS bootstrap
JOIN roles AS role
  ON role.organization_id = bootstrap.organization_id
 AND role.system_key = 'org_owner'
ON CONFLICT (principal_id, role_id) WHERE revoked_at IS NULL DO NOTHING;
