-- Anonymous login-failure ledger for platform operations (B-07).
--
-- The ledger records only a stable reason code, the database-clock time,
-- and an optional one-way deployment-salted digest of the request source.
-- OAuth parameters, cookies, tokens, PKCE material, return_to values,
-- e-mail addresses, and plaintext client metadata must never enter this
-- table. It is not tenant audit data and is never joined to audit_events.
CREATE TABLE human_login_failures (
    id uuid PRIMARY KEY,
    client_key text NOT NULL CHECK (client_key IN ('admin', 'mail')),
    reason text NOT NULL CHECK (reason IN (
        'login_admission_limited',
        'login_begin_rejected',
        'callback_state_missing',
        'login_cookie_rejected',
        'login_transaction_rejected',
        'callback_client_mismatch',
        'login_material_rejected',
        'oidc_callback_rejected',
        'principal_rejected',
        'authorization_unavailable',
        'session_establish_failed'
    )),
    occurred_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    source_sha256 bytea CHECK (
        source_sha256 IS NULL OR octet_length(source_sha256) = 32
    )
);

CREATE INDEX human_login_failures_occurred_idx
    ON human_login_failures(occurred_at DESC, id DESC);

CREATE INDEX human_login_failures_reason_idx
    ON human_login_failures(reason, occurred_at DESC, id DESC);

-- Entries are immutable for the retention window. Only the bounded
-- pruning that accompanies a new append may delete rows older than the
-- 30-day horizon; updates and truncation are always rejected.
CREATE OR REPLACE FUNCTION guard_human_login_failure_ledger()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        IF OLD.occurred_at <= clock_timestamp() - interval '30 days' THEN
            RETURN OLD;
        END IF;
        RAISE EXCEPTION 'human login failure entries are retained for 30 days'
            USING ERRCODE = '55000';
    END IF;
    RAISE EXCEPTION 'human login failure entries are append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER human_login_failures_reject_update
BEFORE UPDATE ON human_login_failures
FOR EACH ROW EXECUTE FUNCTION guard_human_login_failure_ledger();

CREATE TRIGGER human_login_failures_reject_delete
BEFORE DELETE ON human_login_failures
FOR EACH ROW EXECUTE FUNCTION guard_human_login_failure_ledger();

CREATE TRIGGER human_login_failures_reject_truncate
BEFORE TRUNCATE ON human_login_failures
FOR EACH STATEMENT EXECUTE FUNCTION guard_human_login_failure_ledger();
