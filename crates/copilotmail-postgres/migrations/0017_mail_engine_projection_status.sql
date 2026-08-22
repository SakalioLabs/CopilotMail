ALTER TABLE mail_domains
    ADD COLUMN mail_engine_projection_status text NOT NULL DEFAULT 'not_projected',
    ADD COLUMN mail_engine_projected_enabled boolean,
    ADD COLUMN mail_engine_projection_last_attempt_at timestamptz,
    ADD COLUMN mail_engine_projection_succeeded_at timestamptz,
    ADD COLUMN mail_engine_projection_error_code text,
    ADD CONSTRAINT mail_domains_projection_status_check CHECK (
        mail_engine_projection_status IN ('not_projected', 'pending', 'ready', 'failed')
    ),
    ADD CONSTRAINT mail_domains_projection_error_check CHECK (
        mail_engine_projection_error_code IS NULL
        OR (
            char_length(mail_engine_projection_error_code) BETWEEN 1 AND 100
            AND mail_engine_projection_error_code ~ '^[a-z0-9_.-]+$'
        )
    ),
    ADD CONSTRAINT mail_domains_projection_ready_check CHECK (
        mail_engine_projection_status <> 'ready'
        OR (
            mail_engine_domain_ref IS NOT NULL
            AND mail_engine_projected_enabled IS NOT NULL
            AND mail_engine_projection_succeeded_at IS NOT NULL
            AND mail_engine_projection_error_code IS NULL
        )
    ),
    ADD CONSTRAINT mail_domains_projection_failed_check CHECK (
        mail_engine_projection_status <> 'failed'
        OR (
            mail_engine_projection_last_attempt_at IS NOT NULL
            AND mail_engine_projection_error_code IS NOT NULL
        )
    );

UPDATE mail_domains
   SET mail_engine_projection_status = 'pending'
 WHERE status IN ('active', 'suspended');

CREATE INDEX mail_domains_projection_status_idx
    ON mail_domains(organization_id, mail_engine_projection_status, updated_at);
