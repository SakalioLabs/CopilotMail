ALTER TABLE mail_domains
    ADD COLUMN ownership_challenge_version BIGINT NOT NULL DEFAULT 1,
    ADD CONSTRAINT mail_domains_ownership_challenge_version_positive
        CHECK (ownership_challenge_version > 0);

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('mail_domain.challenge_rotated', 'mail_domain', 'high', true);
