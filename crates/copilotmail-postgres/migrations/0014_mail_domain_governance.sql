CREATE FUNCTION copilotmail_valid_mail_domain_name(value text)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
STRICT
AS $$
    SELECT char_length(value) BETWEEN 3 AND 253
       AND value = lower(value)
       AND value = btrim(value)
       AND value LIKE '%.%'
       AND NOT EXISTS (
            SELECT 1
              FROM unnest(string_to_array(value, '.')) AS labels(label)
             WHERE char_length(label) NOT BETWEEN 1 AND 63
                OR label !~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$'
       )
$$;

CREATE TABLE mail_domains (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    name text NOT NULL UNIQUE,
    status text NOT NULL,
    route_mode text NOT NULL,
    ownership_token_sha256 bytea NOT NULL,
    mail_engine_domain_ref text,
    created_by uuid NOT NULL,
    created_at timestamptz NOT NULL,
    updated_at timestamptz NOT NULL,
    verified_at timestamptz,
    activated_at timestamptz,
    suspended_at timestamptz,
    CONSTRAINT mail_domains_id_tenant_unique UNIQUE(id, organization_id),
    CONSTRAINT mail_domains_creator_fk
        FOREIGN KEY(created_by, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT mail_domains_tenant_name_unique
        UNIQUE(organization_id, name),
    CONSTRAINT mail_domains_name_check
        CHECK (copilotmail_valid_mail_domain_name(name)),
    CONSTRAINT mail_domains_status_check CHECK (
        status IN ('draft', 'pending_dns', 'verified', 'active', 'suspended', 'retired')
    ),
    CONSTRAINT mail_domains_route_mode_check CHECK (
        route_mode IN ('direct_mx', 'smart_host')
    ),
    CONSTRAINT mail_domains_ownership_digest_check CHECK (
        octet_length(ownership_token_sha256) = 32
    ),
    CONSTRAINT mail_domains_engine_ref_check CHECK (
        mail_engine_domain_ref IS NULL
        OR (
            char_length(mail_engine_domain_ref) BETWEEN 1 AND 512
            AND mail_engine_domain_ref = btrim(mail_engine_domain_ref)
            AND mail_engine_domain_ref !~ '[[:cntrl:]]'
        )
    ),
    CONSTRAINT mail_domains_timestamps_check CHECK (
        updated_at >= created_at
        AND (verified_at IS NULL OR verified_at >= created_at)
        AND (activated_at IS NULL OR activated_at >= created_at)
        AND (suspended_at IS NULL OR suspended_at >= created_at)
    )
);

CREATE TABLE mail_domain_dns_observations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    mail_domain_id uuid NOT NULL,
    ownership_token_sha256 bytea NOT NULL,
    observed_at timestamptz NOT NULL,
    valid_until timestamptz NOT NULL,
    ownership_txt boolean NOT NULL,
    mx_present boolean NOT NULL,
    mx_hosts_public boolean NOT NULL,
    spf_present boolean NOT NULL,
    dkim_present boolean NOT NULL,
    dmarc_present boolean NOT NULL,
    raw_answers jsonb NOT NULL DEFAULT '{}'::jsonb,
    failure_codes text[] NOT NULL DEFAULT '{}'::text[],
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT mail_domain_dns_observations_domain_fk
        FOREIGN KEY(mail_domain_id, organization_id)
        REFERENCES mail_domains(id, organization_id),
    CONSTRAINT mail_domain_dns_observations_window_check CHECK (
        valid_until > observed_at
        AND valid_until <= observed_at + interval '24 hours'
        AND created_at >= observed_at
    ),
    CONSTRAINT mail_domain_dns_observations_digest_check CHECK (
        octet_length(ownership_token_sha256) = 32
    ),
    CONSTRAINT mail_domain_dns_observations_answers_check CHECK (
        jsonb_typeof(raw_answers) = 'object'
        AND octet_length(raw_answers::text) <= 65536
    ),
    CONSTRAINT mail_domain_dns_observations_failures_check CHECK (
        cardinality(failure_codes) <= 32
        AND array_position(failure_codes, NULL) IS NULL
        AND octet_length(array_to_string(failure_codes, ',')) <= 3232
        AND array_to_string(failure_codes, ',') ~ '^[a-z0-9_.-]*(,[a-z0-9_.-]+)*$'
    )
);

CREATE TABLE mail_domain_routes (
    mail_domain_id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    version bigint NOT NULL DEFAULT 1,
    inbound_enabled boolean NOT NULL DEFAULT false,
    outbound_enabled boolean NOT NULL DEFAULT false,
    smart_host_ref text,
    dkim_key_ref text,
    updated_by uuid NOT NULL,
    updated_at timestamptz NOT NULL,
    CONSTRAINT mail_domain_routes_domain_fk
        FOREIGN KEY(mail_domain_id, organization_id)
        REFERENCES mail_domains(id, organization_id),
    CONSTRAINT mail_domain_routes_updater_fk
        FOREIGN KEY(updated_by, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT mail_domain_routes_version_check CHECK (version > 0),
    CONSTRAINT mail_domain_routes_refs_check CHECK (
        (
            smart_host_ref IS NULL
            OR (
                char_length(smart_host_ref) BETWEEN 1 AND 512
                AND smart_host_ref = btrim(smart_host_ref)
                AND smart_host_ref !~ '[[:cntrl:]]'
            )
        )
        AND (
            dkim_key_ref IS NULL
            OR (
                char_length(dkim_key_ref) BETWEEN 1 AND 512
                AND dkim_key_ref = btrim(dkim_key_ref)
                AND dkim_key_ref !~ '[[:cntrl:]]'
            )
        )
    )
);

CREATE OR REPLACE FUNCTION reject_mail_domain_dns_observation_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mail_domain_dns_observations is append-only' USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER mail_domain_dns_observations_append_only
BEFORE UPDATE OR DELETE ON mail_domain_dns_observations
FOR EACH ROW EXECUTE FUNCTION reject_mail_domain_dns_observation_mutation();

CREATE TRIGGER mail_domain_dns_observations_reject_truncate
BEFORE TRUNCATE ON mail_domain_dns_observations
FOR EACH STATEMENT EXECUTE FUNCTION reject_mail_domain_dns_observation_mutation();

CREATE INDEX mail_domains_organization_status_idx
    ON mail_domains(organization_id, status, name);

CREATE INDEX mail_domain_dns_observations_latest_idx
    ON mail_domain_dns_observations(organization_id, mail_domain_id, observed_at DESC);
