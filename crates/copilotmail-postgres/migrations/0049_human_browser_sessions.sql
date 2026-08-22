INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('identity.human_session.created', 'identity.human_session', 'high', true),
    ('identity.human_session.revoked', 'identity.human_session', 'high', true);

-- Operational admission state is intentionally separate from immutable login
-- evidence. PostgreSQL owns the clock and the composite key, so every control
-- plane replica shares the same hard cap for each browser application.
CREATE TABLE human_login_admission_windows (
    client_key text NOT NULL CHECK (client_key IN ('admin', 'mail')),
    window_started_at timestamptz NOT NULL,
    admitted_count bigint NOT NULL CHECK (
        admitted_count BETWEEN 1 AND 100000
    ),
    PRIMARY KEY(client_key, window_started_at)
);

CREATE INDEX human_login_admission_windows_expiry_idx
    ON human_login_admission_windows(window_started_at, client_key);

-- Pre-authentication state has no tenant chosen by the browser. Both browser
-- correlation values are one-way digests and the PKCE verifier is opaque
-- ciphertext produced outside PostgreSQL.
CREATE TABLE human_login_transactions (
    id uuid PRIMARY KEY,
    client_key text NOT NULL CHECK (client_key IN ('admin', 'mail')),
    return_to text NOT NULL CHECK (
        octet_length(return_to) BETWEEN 1 AND 1024
        AND return_to = btrim(return_to)
        AND left(return_to, 1) = '/'
        AND left(return_to, 2) <> '//'
        AND strpos(return_to, chr(92)) = 0
        AND return_to !~ '[[:cntrl:]]'
    ),
    state_sha256 bytea NOT NULL UNIQUE
        CHECK (octet_length(state_sha256) = 32),
    browser_nonce_sha256 bytea NOT NULL UNIQUE
        CHECK (octet_length(browser_nonce_sha256) = 32),
    encrypted_pkce_verifier bytea NOT NULL CHECK (
        octet_length(encrypted_pkce_verifier) BETWEEN 32 AND 4096
    ),
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    CHECK (
        expires_at > created_at
        AND expires_at <= created_at + interval '10 minutes'
    )
);

CREATE INDEX human_login_transactions_expiry_idx
    ON human_login_transactions(expires_at, id);

-- Consumption is a separate append-only fact rather than a mutable status on
-- the login transaction.
CREATE TABLE human_login_transaction_consumptions (
    login_transaction_id uuid PRIMARY KEY
        REFERENCES human_login_transactions(id),
    consumed_at timestamptz NOT NULL
);

CREATE OR REPLACE FUNCTION validate_human_login_consumption_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM human_login_transactions AS login
         WHERE login.id = NEW.login_transaction_id
           AND login.created_at <= NEW.consumed_at
           AND login.expires_at > NEW.consumed_at
    ) THEN
        RAISE EXCEPTION 'Human login transaction is expired or invalid'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER human_login_consumptions_validate_insert
BEFORE INSERT ON human_login_transaction_consumptions
FOR EACH ROW EXECUTE FUNCTION validate_human_login_consumption_insert();

-- Browser session credentials remain outside PostgreSQL. The opaque cookie and
-- upstream OIDC token are represented only by independent SHA-256 digests.
CREATE TABLE human_browser_sessions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid NOT NULL,
    login_transaction_id uuid NOT NULL UNIQUE
        REFERENCES human_login_transactions(id),
    client_key text NOT NULL CHECK (client_key IN ('admin', 'mail')),
    token_sha256 bytea NOT NULL UNIQUE
        CHECK (octet_length(token_sha256) = 32),
    oidc_token_sha256 bytea NOT NULL UNIQUE
        CHECK (octet_length(oidc_token_sha256) = 32),
    oidc_jti text CHECK (
        oidc_jti IS NULL
        OR (
            octet_length(oidc_jti) BETWEEN 1 AND 200
            AND oidc_jti = btrim(oidc_jti)
            AND oidc_jti !~ '[[:cntrl:]]'
        )
    ),
    upstream_expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL,
    expires_at timestamptz NOT NULL,
    UNIQUE(id, organization_id, principal_id),
    FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    CHECK (
        upstream_expires_at > created_at
        AND expires_at > created_at
        AND expires_at <= upstream_expires_at
        AND expires_at <= created_at + interval '8 hours'
    )
);

CREATE INDEX human_browser_sessions_principal_expiry_idx
    ON human_browser_sessions(
        organization_id, principal_id, client_key, expires_at, id
    );

CREATE OR REPLACE FUNCTION validate_human_browser_session_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS principal
         WHERE principal.id = NEW.principal_id
           AND principal.organization_id = NEW.organization_id
           AND principal.kind = 'human'
           AND principal.status = 'active'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM human_login_transactions AS login
          JOIN human_login_transaction_consumptions AS consumption
            ON consumption.login_transaction_id = login.id
         WHERE login.id = NEW.login_transaction_id
           AND login.client_key = NEW.client_key
           AND consumption.consumed_at <= NEW.created_at
           AND login.created_at <= NEW.created_at
           AND login.expires_at > NEW.created_at
    )
    THEN
        RAISE EXCEPTION 'Human session lacks active Human or current consumed login evidence'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER human_browser_sessions_validate_insert
BEFORE INSERT ON human_browser_sessions
FOR EACH ROW EXECUTE FUNCTION validate_human_browser_session_insert();

-- Revocation is another append-only fact. It never rewrites the session row.
CREATE TABLE human_browser_session_revocations (
    id uuid PRIMARY KEY,
    session_id uuid NOT NULL UNIQUE,
    organization_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    cause text NOT NULL CHECK (
        cause IN (
            'logout',
            'security_response',
            'principal_deactivated',
            'upstream_revoked'
        )
    ),
    revoked_at timestamptz NOT NULL,
    FOREIGN KEY(session_id, organization_id, principal_id)
        REFERENCES human_browser_sessions(id, organization_id, principal_id)
);

CREATE INDEX human_browser_session_revocations_tenant_idx
    ON human_browser_session_revocations(
        organization_id, principal_id, revoked_at DESC, id DESC
    );

CREATE OR REPLACE FUNCTION validate_human_session_revocation_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM human_browser_sessions AS session
         WHERE session.id = NEW.session_id
           AND session.organization_id = NEW.organization_id
           AND session.principal_id = NEW.principal_id
           AND session.created_at <= NEW.revoked_at
    ) THEN
        RAISE EXCEPTION 'Human session revocation does not match session evidence'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER human_browser_session_revocations_validate_insert
BEFORE INSERT ON human_browser_session_revocations
FOR EACH ROW EXECUTE FUNCTION validate_human_session_revocation_insert();

CREATE OR REPLACE FUNCTION reject_human_session_history_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Human login and session evidence is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE OR REPLACE FUNCTION protect_human_login_transaction_history()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.expires_at <= clock_timestamp()
           AND NOT EXISTS (
               SELECT 1
                 FROM human_browser_sessions AS session
                WHERE session.login_transaction_id = OLD.id
           )
        THEN
            RETURN OLD;
        END IF;
    END IF;

    RAISE EXCEPTION 'Human login transaction evidence is append-only unless expired and session-unbound'
        USING ERRCODE = '55000';
END;
$$;

CREATE OR REPLACE FUNCTION protect_human_login_consumption_history()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF EXISTS (
            SELECT 1
              FROM human_login_transactions AS login
             WHERE login.id = OLD.login_transaction_id
               AND login.expires_at <= clock_timestamp()
               AND NOT EXISTS (
                   SELECT 1
                     FROM human_browser_sessions AS session
                    WHERE session.login_transaction_id = login.id
               )
        )
        THEN
            RETURN OLD;
        END IF;
    END IF;

    RAISE EXCEPTION 'Human login consumption evidence is append-only unless its login is expired and session-unbound'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER human_login_transactions_reject_update
BEFORE UPDATE ON human_login_transactions
FOR EACH ROW EXECUTE FUNCTION protect_human_login_transaction_history();
CREATE TRIGGER human_login_transactions_reject_delete
BEFORE DELETE ON human_login_transactions
FOR EACH ROW EXECUTE FUNCTION protect_human_login_transaction_history();
CREATE TRIGGER human_login_transactions_reject_truncate
BEFORE TRUNCATE ON human_login_transactions
FOR EACH STATEMENT EXECUTE FUNCTION protect_human_login_transaction_history();

CREATE TRIGGER human_login_consumptions_reject_update
BEFORE UPDATE ON human_login_transaction_consumptions
FOR EACH ROW EXECUTE FUNCTION protect_human_login_consumption_history();
CREATE TRIGGER human_login_consumptions_reject_delete
BEFORE DELETE ON human_login_transaction_consumptions
FOR EACH ROW EXECUTE FUNCTION protect_human_login_consumption_history();
CREATE TRIGGER human_login_consumptions_reject_truncate
BEFORE TRUNCATE ON human_login_transaction_consumptions
FOR EACH STATEMENT EXECUTE FUNCTION protect_human_login_consumption_history();

CREATE TRIGGER human_browser_sessions_reject_update
BEFORE UPDATE ON human_browser_sessions
FOR EACH ROW EXECUTE FUNCTION reject_human_session_history_mutation();
CREATE TRIGGER human_browser_sessions_reject_delete
BEFORE DELETE ON human_browser_sessions
FOR EACH ROW EXECUTE FUNCTION reject_human_session_history_mutation();
CREATE TRIGGER human_browser_sessions_reject_truncate
BEFORE TRUNCATE ON human_browser_sessions
FOR EACH STATEMENT EXECUTE FUNCTION reject_human_session_history_mutation();

CREATE TRIGGER human_browser_session_revocations_reject_update
BEFORE UPDATE ON human_browser_session_revocations
FOR EACH ROW EXECUTE FUNCTION reject_human_session_history_mutation();
CREATE TRIGGER human_browser_session_revocations_reject_delete
BEFORE DELETE ON human_browser_session_revocations
FOR EACH ROW EXECUTE FUNCTION reject_human_session_history_mutation();
CREATE TRIGGER human_browser_session_revocations_reject_truncate
BEFORE TRUNCATE ON human_browser_session_revocations
FOR EACH STATEMENT EXECUTE FUNCTION reject_human_session_history_mutation();
