CREATE UNIQUE INDEX credentials_public_fingerprint_idx
    ON credentials(public_fingerprint);

ALTER TABLE credentials
    ADD COLUMN last_used_source text CHECK (
        last_used_source IS NULL
        OR (
            char_length(last_used_source) BETWEEN 1 AND 200
            AND last_used_source !~ '[[:cntrl:]]'
        )
    );

CREATE TABLE agent_private_key_credentials (
    credential_id uuid PRIMARY KEY REFERENCES credentials(id) ON DELETE CASCADE,
    public_key_pem text NOT NULL CHECK (
        octet_length(public_key_pem) BETWEEN 256 AND 16384
        AND public_key_pem LIKE '-----BEGIN PUBLIC KEY-----%'
    ),
    allowed_scopes text[] NOT NULL CHECK (
        cardinality(allowed_scopes) BETWEEN 1 AND 3
        AND allowed_scopes <@ ARRAY[
            'organization.read',
            'agent.operate',
            'mailbox.use'
        ]::text[]
    )
);

CREATE TABLE oauth_client_assertion_jtis (
    credential_id uuid NOT NULL REFERENCES credentials(id) ON DELETE CASCADE,
    jti_hash bytea NOT NULL CHECK (octet_length(jti_hash) = 32),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY(credential_id, jti_hash),
    CHECK (expires_at > created_at)
);
CREATE INDEX oauth_client_assertion_expiry_idx
    ON oauth_client_assertion_jtis(expires_at);

-- Backfill the least-privilege runtime role for organizations created before
-- this migration. New organizations create the same role in application code.
INSERT INTO roles(id, organization_id, system_key, display_name, created_at)
SELECT gen_random_uuid(), organization.id, 'agent_operator', 'Agent Operator',
       organization.created_at
FROM organizations AS organization
ON CONFLICT (organization_id, system_key) DO NOTHING;

INSERT INTO role_permissions(role_id, permission)
SELECT role.id, permission.value
FROM roles AS role
CROSS JOIN (
    VALUES
        ('organization.read'),
        ('agent.operate'),
        ('mailbox.use')
) AS permission(value)
WHERE role.system_key = 'agent_operator'
ON CONFLICT DO NOTHING;
