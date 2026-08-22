INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('agent.processing_subscription.paused', 'agent.processing_subscription', 'medium', true),
       ('agent.processing_subscription.resumed', 'agent.processing_subscription', 'medium', true),
       ('agent.processing_subscription.retired', 'agent.processing_subscription', 'high', true);

DROP TRIGGER agent_processing_subscription_reject_update_delete
    ON agent_subscriptions;
DROP TRIGGER agent_processing_subscription_reject_truncate
    ON agent_subscriptions;
DROP FUNCTION reject_agent_processing_subscription_mutation();

CREATE OR REPLACE FUNCTION validate_agent_processing_subscription_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id <> OLD.id
       OR NEW.organization_id <> OLD.organization_id
       OR NEW.principal_id <> OLD.principal_id
       OR NEW.mailbox_id <> OLD.mailbox_id
       OR NEW.name <> OLD.name
       OR NEW.cursor_value IS DISTINCT FROM OLD.cursor_value
       OR NEW.created_at <> OLD.created_at
       OR NEW.updated_at <= OLD.updated_at
       OR NOT (
           (OLD.status = 'active' AND NEW.status IN ('paused', 'retired'))
           OR (OLD.status = 'paused' AND NEW.status IN ('active', 'retired'))
       )
       OR NOT EXISTS (
           SELECT 1
             FROM principals AS principal
            WHERE principal.id = NEW.principal_id
              AND principal.organization_id = NEW.organization_id
              AND principal.kind = 'agent'
              AND principal.status = 'active'
       )
       OR NOT (
           EXISTS (
               SELECT 1
                 FROM mailbox_memberships AS membership
                WHERE membership.organization_id = NEW.organization_id
                  AND membership.mailbox_id = NEW.mailbox_id
                  AND membership.principal_id = NEW.principal_id
                  AND membership.role IN ('owner', 'member')
                  AND membership.valid_from <= NEW.updated_at
                  AND (
                      membership.valid_until IS NULL
                      OR membership.valid_until > NEW.updated_at
                  )
           )
           OR EXISTS (
               SELECT 1
                 FROM delegations AS delegation
                WHERE delegation.organization_id = NEW.organization_id
                  AND delegation.mailbox_id = NEW.mailbox_id
                  AND delegation.grantee_principal_id = NEW.principal_id
                  AND 'mailbox.read' = ANY(delegation.scopes)
                  AND delegation.revoked_at IS NULL
                  AND delegation.valid_from <= NEW.updated_at
                  AND delegation.valid_until > NEW.updated_at
           )
       )
       OR (
           NEW.status = 'retired'
           AND EXISTS (
               SELECT 1
                 FROM agent_message_states AS state
                WHERE state.organization_id = NEW.organization_id
                  AND state.subscription_id = NEW.id
                  AND state.state = 'leased'
                  AND state.lease_expires_at > NEW.updated_at
           )
       )
    THEN
        RAISE EXCEPTION 'invalid Agent processing subscription transition'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_processing_subscription_validate_update
BEFORE UPDATE ON agent_subscriptions
FOR EACH ROW EXECUTE FUNCTION validate_agent_processing_subscription_update();

CREATE OR REPLACE FUNCTION reject_agent_processing_subscription_removal()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent processing subscription removal is forbidden'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_processing_subscription_reject_delete
BEFORE DELETE ON agent_subscriptions
FOR EACH ROW EXECUTE FUNCTION reject_agent_processing_subscription_removal();

CREATE TRIGGER agent_processing_subscription_reject_truncate
BEFORE TRUNCATE ON agent_subscriptions
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_processing_subscription_removal();

CREATE OR REPLACE FUNCTION validate_agent_message_state_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state <> 'pending'
       OR NEW.attempt_count <> 0
       OR NEW.lease_id IS NOT NULL
       OR NEW.last_lease_id IS NOT NULL
       OR NEW.last_error IS NOT NULL
       OR NEW.state_version <> 0
       OR NEW.acknowledged_at IS NOT NULL
       OR NEW.failed_at IS NOT NULL
       OR NOT EXISTS (
           SELECT 1
             FROM agent_subscriptions AS subscription
            WHERE subscription.id = NEW.subscription_id
              AND subscription.organization_id = NEW.organization_id
              AND subscription.principal_id = NEW.principal_id
              AND subscription.status IN ('active', 'paused')
       )
    THEN
        RAISE EXCEPTION 'invalid initial Agent message state'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TABLE agent_processing_subscription_operations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    subscription_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    operation text NOT NULL CHECK (operation IN ('paused', 'resumed', 'retired')),
    previous_status text NOT NULL CHECK (previous_status IN ('active', 'paused')),
    result_status text NOT NULL CHECK (result_status IN ('active', 'paused', 'retired')),
    idempotency_key_sha256 bytea NOT NULL
        CHECK (octet_length(idempotency_key_sha256) = 32),
    request_sha256 bytea NOT NULL CHECK (octet_length(request_sha256) = 32),
    occurred_at timestamptz NOT NULL,
    UNIQUE(organization_id, principal_id, operation, idempotency_key_sha256),
    FOREIGN KEY(subscription_id, organization_id)
        REFERENCES agent_subscriptions(id, organization_id),
    FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    CHECK (
        (operation = 'paused'
            AND previous_status = 'active' AND result_status = 'paused')
        OR
        (operation = 'resumed'
            AND previous_status = 'paused' AND result_status = 'active')
        OR
        (operation = 'retired'
            AND previous_status IN ('active', 'paused') AND result_status = 'retired')
    )
);

CREATE INDEX agent_processing_subscription_operations_lookup_idx
    ON agent_processing_subscription_operations(
        organization_id, principal_id, subscription_id, occurred_at
    );

CREATE OR REPLACE FUNCTION validate_agent_processing_subscription_operation_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM agent_subscriptions AS subscription
         WHERE subscription.id = NEW.subscription_id
           AND subscription.organization_id = NEW.organization_id
           AND subscription.principal_id = NEW.principal_id
           AND subscription.status = NEW.result_status
           AND subscription.updated_at = NEW.occurred_at
    )
    THEN
        RAISE EXCEPTION 'Agent processing subscription evidence is not current'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_processing_subscription_operation_validate_insert
BEFORE INSERT ON agent_processing_subscription_operations
FOR EACH ROW
EXECUTE FUNCTION validate_agent_processing_subscription_operation_insert();

CREATE OR REPLACE FUNCTION reject_agent_processing_subscription_operation_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent processing subscription operations are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_processing_subscription_operation_reject_update_delete
BEFORE UPDATE OR DELETE ON agent_processing_subscription_operations
FOR EACH ROW
EXECUTE FUNCTION reject_agent_processing_subscription_operation_mutation();

CREATE TRIGGER agent_processing_subscription_operation_reject_truncate
BEFORE TRUNCATE ON agent_processing_subscription_operations
FOR EACH STATEMENT
EXECUTE FUNCTION reject_agent_processing_subscription_operation_mutation();
