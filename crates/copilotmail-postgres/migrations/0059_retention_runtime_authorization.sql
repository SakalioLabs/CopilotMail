-- Retention activation timestamps and approval freshness are database-owned.
-- Existing immutable evidence is not rewritten. Only future transitions change.

CREATE OR REPLACE FUNCTION validate_retention_policy_activation_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    -- Match application/FK ordering before approval validation. Locking the
    -- policy also prevents a concurrent activation insert becoming a phantom
    -- between the active-pointer trigger's lock and freshness recheck.
    PERFORM id FROM principals
     WHERE id = NEW.activated_by AND organization_id = NEW.organization_id
     FOR UPDATE;
    PERFORM id FROM retention_policies
     WHERE id = NEW.policy_id AND organization_id = NEW.organization_id
     FOR UPDATE;
    -- Serialize expiry/decision validation with this exact approval request.
    -- Caller timestamps never authorize an action after a database lock wait.
    PERFORM id FROM approval_requests
     WHERE id = NEW.approval_request_id
       AND organization_id = NEW.organization_id
     FOR UPDATE;
    NEW.activated_at := clock_timestamp();
    IF NOT EXISTS (
        SELECT 1
          FROM principals AS actor
         WHERE actor.id = NEW.activated_by
           AND actor.organization_id = NEW.organization_id
           AND actor.kind = 'human'
           AND actor.status = 'active'
    )
    OR NOT EXISTS (
        SELECT 1
          FROM approval_requests AS request
         WHERE request.id = NEW.approval_request_id
           AND request.organization_id = NEW.organization_id
           AND request.action = 'compliance.retention_change'
           AND request.resource_kind = 'compliance.retention_policy_version'
           AND request.resource_id = NEW.policy_version_id::text
           AND request.requested_by = NEW.activated_by
           AND request.status = 'approved'
           AND request.required_approvals >= 2
           AND request.created_at <= NEW.activated_at
           AND request.expires_at > NEW.activated_at
           AND (
               SELECT count(*)
                 FROM approval_steps AS step
                WHERE step.organization_id = NEW.organization_id
                  AND step.approval_request_id = request.id
                  AND step.decision = 'approved'
           ) >= request.required_approvals
           AND NOT EXISTS (
               SELECT 1
                 FROM approval_steps AS rejected
                WHERE rejected.organization_id = NEW.organization_id
                  AND rejected.approval_request_id = request.id
                  AND rejected.decision = 'rejected'
           )
    )
    THEN
        RAISE EXCEPTION 'retention policy activation is not exactly approved'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_retention_policy_active_version_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    old_version integer;
    new_version integer;
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.organization_id IS DISTINCT FROM OLD.organization_id
       OR NEW.policy_key IS DISTINCT FROM OLD.policy_key
       OR NEW.display_name IS DISTINCT FROM OLD.display_name
       OR NEW.resource_kind IS DISTINCT FROM OLD.resource_kind
       OR NEW.created_by IS DISTINCT FROM OLD.created_by
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR NEW.active_version_id IS NULL
       OR NEW.active_version_id IS NOT DISTINCT FROM OLD.active_version_id
    THEN
        RAISE EXCEPTION 'retention policy identity is immutable'
            USING ERRCODE = '55000';
    END IF;

    SELECT version INTO new_version
      FROM retention_policy_versions
     WHERE id = NEW.active_version_id
       AND organization_id = NEW.organization_id
       AND policy_id = NEW.id;
    -- An activation row may have been committed by an earlier transaction.
    -- Do not rely on the INSERT path still owning its approval lock here.
    PERFORM request.id
      FROM retention_policy_activations AS activation
      JOIN approval_requests AS request
        ON request.id = activation.approval_request_id
       AND request.organization_id = activation.organization_id
     WHERE activation.organization_id = NEW.organization_id
       AND activation.policy_id = NEW.id
       AND activation.policy_version_id = NEW.active_version_id
     ORDER BY request.id
     FOR UPDATE OF request;
    IF new_version IS NULL
       OR NOT EXISTS (
           SELECT 1
             FROM retention_policy_activations AS activation
             JOIN approval_requests AS request
               ON request.id = activation.approval_request_id
              AND request.organization_id = activation.organization_id
            WHERE activation.organization_id = NEW.organization_id
              AND activation.policy_id = NEW.id
              AND activation.policy_version_id = NEW.active_version_id
              AND request.status = 'approved'
              AND request.expires_at > clock_timestamp()
              AND request.requested_by = activation.activated_by
       )
    THEN
        RAISE EXCEPTION 'retention policy version lacks activation evidence'
            USING ERRCODE = '23514';
    END IF;

    IF OLD.active_version_id IS NOT NULL THEN
        SELECT version INTO old_version
          FROM retention_policy_versions
         WHERE id = OLD.active_version_id
           AND organization_id = OLD.organization_id
           AND policy_id = OLD.id;
        IF old_version IS NULL OR new_version <= old_version THEN
            RAISE EXCEPTION 'retention policy activation must move forward'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
