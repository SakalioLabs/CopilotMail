CREATE TABLE agent_mtls_credentials (
    credential_id uuid PRIMARY KEY REFERENCES credentials(id) ON DELETE CASCADE,
    allowed_scopes text[] NOT NULL CHECK (
        cardinality(allowed_scopes) BETWEEN 1 AND 3
        AND allowed_scopes <@ ARRAY[
            'organization.read',
            'agent.operate',
            'mailbox.use'
        ]::text[]
    )
);

-- The generic credentials table already enforces a globally unique public
-- fingerprint. For kind=mtls this is the lowercase SHA-256 digest of the
-- TLS-verified leaf certificate DER, preventing one certificate from being
-- assigned to more than one Agent.
