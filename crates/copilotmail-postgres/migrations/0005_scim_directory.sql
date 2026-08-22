CREATE TABLE scim_tokens (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    actor_principal_id uuid NOT NULL,
    token_hash bytea NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    label text NOT NULL CHECK (
        char_length(label) BETWEEN 1 AND 200
        AND label = btrim(label)
        AND label !~ '[[:cntrl:]]'
    ),
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    created_at timestamptz NOT NULL,
    expires_at timestamptz,
    revoked_at timestamptz,
    last_used_at timestamptz,
    CHECK (expires_at IS NULL OR expires_at > created_at),
    CHECK (
        (status = 'active' AND revoked_at IS NULL)
        OR (status = 'revoked' AND revoked_at IS NOT NULL)
    ),
    FOREIGN KEY(actor_principal_id, organization_id)
        REFERENCES principals(id, organization_id)
);
CREATE INDEX scim_tokens_active_hash_idx
    ON scim_tokens(token_hash)
    WHERE status = 'active';

CREATE TABLE scim_users (
    principal_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    external_id text NOT NULL CHECK (
        char_length(external_id) BETWEEN 1 AND 512
        AND external_id = btrim(external_id)
        AND external_id !~ '[[:cntrl:]]'
    ),
    user_name text NOT NULL CHECK (
        char_length(user_name) BETWEEN 1 AND 320
        AND user_name = btrim(user_name)
        AND user_name !~ '[[:cntrl:]]'
    ),
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE(organization_id, external_id),
    FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id)
);
CREATE INDEX scim_users_tenant_page_idx
    ON scim_users(organization_id, created_at, principal_id);
CREATE UNIQUE INDEX scim_users_tenant_user_name_idx
    ON scim_users(organization_id, lower(user_name));
CREATE UNIQUE INDEX scim_users_principal_tenant_idx
    ON scim_users(principal_id, organization_id);

CREATE TABLE scim_groups (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    external_id text NOT NULL CHECK (
        char_length(external_id) BETWEEN 1 AND 512
        AND external_id = btrim(external_id)
        AND external_id !~ '[[:cntrl:]]'
    ),
    display_name text NOT NULL CHECK (
        char_length(display_name) BETWEEN 1 AND 200
        AND display_name = btrim(display_name)
        AND display_name !~ '[[:cntrl:]]'
    ),
    version bigint NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    UNIQUE(organization_id, external_id),
    UNIQUE(id, organization_id)
);
CREATE UNIQUE INDEX scim_groups_tenant_display_name_idx
    ON scim_groups(organization_id, lower(display_name));
CREATE INDEX scim_groups_tenant_page_idx
    ON scim_groups(organization_id, created_at, id);

CREATE TABLE scim_group_members (
    group_id uuid NOT NULL,
    organization_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    created_at timestamptz NOT NULL,
    PRIMARY KEY(group_id, principal_id),
    FOREIGN KEY(group_id, organization_id)
        REFERENCES scim_groups(id, organization_id) ON DELETE CASCADE,
    FOREIGN KEY(principal_id, organization_id)
        REFERENCES scim_users(principal_id, organization_id)
);
