CREATE UNIQUE INDEX principals_id_organization_idx
    ON principals(id, organization_id);

CREATE TABLE organization_bootstraps (
    organization_id uuid PRIMARY KEY REFERENCES organizations(id),
    root_principal_id uuid NOT NULL UNIQUE,
    created_at timestamptz NOT NULL,
    FOREIGN KEY(root_principal_id, organization_id)
        REFERENCES principals(id, organization_id)
);
