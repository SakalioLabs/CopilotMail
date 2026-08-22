ALTER TABLE human_profiles
    ADD CONSTRAINT human_profiles_provider_format
        CHECK (
            char_length(identity_provider) BETWEEN 1 AND 2048
            AND identity_provider = btrim(identity_provider)
            AND identity_provider !~ '[[:cntrl:]]'
        ),
    ADD CONSTRAINT human_profiles_subject_format
        CHECK (
            char_length(identity_subject) BETWEEN 1 AND 512
            AND identity_subject = btrim(identity_subject)
            AND identity_subject !~ '[[:cntrl:]]'
        );

CREATE UNIQUE INDEX human_profiles_mapping_idx
    ON human_profiles(principal_id, identity_provider, identity_subject);

CREATE TABLE oidc_owner_bindings (
    organization_id uuid PRIMARY KEY REFERENCES organizations(id),
    principal_id uuid NOT NULL UNIQUE,
    identity_provider text NOT NULL,
    identity_subject text NOT NULL,
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    UNIQUE(identity_provider, identity_subject),
    FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    FOREIGN KEY(principal_id, identity_provider, identity_subject)
        REFERENCES human_profiles(principal_id, identity_provider, identity_subject)
);
