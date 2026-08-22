ALTER TABLE outbox_events
    ADD COLUMN next_attempt_at timestamptz NOT NULL DEFAULT now();

DROP INDEX outbox_unpublished_idx;
CREATE INDEX outbox_unpublished_idx
    ON outbox_events(next_attempt_at, occurred_at, id)
    WHERE published_at IS NULL;

CREATE INDEX mail_domains_engine_ref_idx
    ON mail_domains(mail_engine_domain_ref)
    WHERE mail_engine_domain_ref IS NOT NULL;
