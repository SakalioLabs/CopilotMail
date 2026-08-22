INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('authorization.approval.cancelled', 'authorization.approval_request', 'high', true),
    ('authorization.approval.queried', 'authorization.approval_request_query', 'high', true);

CREATE INDEX approval_requests_requested_page_idx
    ON approval_requests(
        organization_id,
        requested_by,
        created_at DESC,
        id DESC
    );

CREATE INDEX approval_requests_actionable_page_idx
    ON approval_requests(
        organization_id,
        status,
        created_at DESC,
        id DESC
    );

CREATE OR REPLACE FUNCTION validate_approval_request_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
        OR NEW.organization_id <> OLD.organization_id
        OR NEW.policy_version_id <> OLD.policy_version_id
        OR NEW.action <> OLD.action
        OR NEW.resource_kind <> OLD.resource_kind
        OR NEW.resource_id <> OLD.resource_id
        OR NEW.requested_by <> OLD.requested_by
        OR NEW.reason <> OLD.reason
        OR NEW.ticket_reference IS DISTINCT FROM OLD.ticket_reference
        OR NEW.required_approvals <> OLD.required_approvals
        OR NEW.created_at <> OLD.created_at
        OR NEW.expires_at <> OLD.expires_at
    THEN
        RAISE EXCEPTION 'approval request identity is immutable'
            USING ERRCODE = '55000';
    END IF;

    IF OLD.status <> 'pending'
        OR NEW.status NOT IN ('pending', 'approved', 'rejected', 'cancelled')
    THEN
        RAISE EXCEPTION 'invalid approval request status transition'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.status = 'pending' THEN
        IF NEW.completed_at IS NOT NULL
            OR NEW.cancelled_at IS NOT NULL
        THEN
            RAISE EXCEPTION 'pending approval evidence is invalid'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.status IN ('approved', 'rejected') THEN
        IF NEW.completed_at IS NULL
            OR NEW.cancelled_at IS NOT NULL
        THEN
            RAISE EXCEPTION 'approval completion evidence is invalid'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.status = 'cancelled' THEN
        IF NEW.completed_at IS NOT NULL
            OR NEW.cancelled_at IS NULL
        THEN
            RAISE EXCEPTION 'approval cancellation evidence is invalid'
                USING ERRCODE = '23514';
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER approval_requests_validate_update
BEFORE UPDATE ON approval_requests
FOR EACH ROW EXECUTE FUNCTION validate_approval_request_update();

CREATE OR REPLACE FUNCTION reject_approval_request_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'approval requests are immutable history'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER approval_requests_reject_delete
BEFORE DELETE ON approval_requests
FOR EACH ROW EXECUTE FUNCTION reject_approval_request_delete();

CREATE TRIGGER approval_requests_reject_truncate
BEFORE TRUNCATE ON approval_requests
FOR EACH STATEMENT EXECUTE FUNCTION reject_approval_request_delete();

CREATE TRIGGER approval_steps_reject_truncate
BEFORE TRUNCATE ON approval_steps
FOR EACH STATEMENT EXECUTE FUNCTION reject_approval_step_mutation();
