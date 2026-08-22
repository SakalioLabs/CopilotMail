CREATE TABLE agent_ingress_rate_limit_windows (
    organization_id uuid NOT NULL REFERENCES organizations(id),
    principal_id uuid,
    window_started_at timestamptz NOT NULL,
    window_ends_at timestamptz NOT NULL,
    request_count integer NOT NULL CHECK (request_count > 0),
    updated_at timestamptz NOT NULL,
    CONSTRAINT agent_ingress_rate_limit_window_shape
        CHECK (
            window_started_at = date_trunc('minute', window_started_at)
            AND window_ends_at = window_started_at + interval '1 minute'
            AND updated_at >= window_started_at
        ),
    CONSTRAINT agent_ingress_rate_limit_principal_fk
        FOREIGN KEY (principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    UNIQUE NULLS NOT DISTINCT (
        organization_id,
        principal_id,
        window_started_at
    )
);

CREATE INDEX agent_ingress_rate_limit_expiry_idx
    ON agent_ingress_rate_limit_windows(window_ends_at, organization_id);

CREATE OR REPLACE FUNCTION validate_agent_ingress_rate_limit_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.request_count <> 1 THEN
        RAISE EXCEPTION 'Agent ingress rate-limit windows must start at one'
            USING ERRCODE = '23514';
    END IF;
    IF NEW.principal_id IS NOT NULL AND NOT EXISTS (
        SELECT 1
          FROM principals
         WHERE organization_id = NEW.organization_id
           AND id = NEW.principal_id
           AND kind = 'agent'
           AND status = 'active'
    ) THEN
        RAISE EXCEPTION 'Agent ingress rate limits require an exact active Agent'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_ingress_rate_limit_insert_guard
BEFORE INSERT ON agent_ingress_rate_limit_windows
FOR EACH ROW EXECUTE FUNCTION validate_agent_ingress_rate_limit_insert();

CREATE OR REPLACE FUNCTION validate_agent_ingress_rate_limit_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.organization_id <> OLD.organization_id
       OR NEW.principal_id IS DISTINCT FROM OLD.principal_id
       OR NEW.window_started_at <> OLD.window_started_at
       OR NEW.window_ends_at <> OLD.window_ends_at
       OR NEW.request_count <> OLD.request_count + 1
       OR NEW.updated_at < OLD.updated_at
       OR NEW.updated_at >= OLD.window_ends_at THEN
        RAISE EXCEPTION 'Agent ingress rate-limit updates must be monotonic'
            USING ERRCODE = '55000';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_ingress_rate_limit_update_guard
BEFORE UPDATE ON agent_ingress_rate_limit_windows
FOR EACH ROW EXECUTE FUNCTION validate_agent_ingress_rate_limit_update();

CREATE OR REPLACE FUNCTION validate_agent_ingress_rate_limit_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.window_ends_at > clock_timestamp() THEN
        RAISE EXCEPTION 'Active Agent ingress rate-limit windows cannot be deleted'
            USING ERRCODE = '55000';
    END IF;
    RETURN OLD;
END;
$$;

CREATE TRIGGER agent_ingress_rate_limit_delete_guard
BEFORE DELETE ON agent_ingress_rate_limit_windows
FOR EACH ROW EXECUTE FUNCTION validate_agent_ingress_rate_limit_delete();

CREATE OR REPLACE FUNCTION reject_agent_ingress_rate_limit_truncate()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent ingress rate-limit windows cannot be truncated'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_ingress_rate_limit_truncate_guard
BEFORE TRUNCATE ON agent_ingress_rate_limit_windows
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_ingress_rate_limit_truncate();
