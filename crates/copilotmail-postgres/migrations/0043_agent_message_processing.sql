INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('agent.processing_subscription.created', 'agent.processing_subscription', 'medium', true),
    ('agent.message.claimed', 'agent.message_processing', 'low', true),
    ('agent.message.lease_extended', 'agent.message_processing', 'low', true),
    ('agent.message.acknowledged', 'agent.message_processing', 'low', true),
    ('agent.message.nacked', 'agent.message_processing', 'medium', true),
    ('agent.message.dead_lettered', 'agent.message_processing', 'medium', true);

ALTER TABLE agent_subscriptions
    ADD CONSTRAINT agent_subscriptions_tenant_id_unique UNIQUE(id, organization_id),
    ADD CONSTRAINT agent_subscriptions_principal_tenant_fkey
        FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT agent_subscriptions_mailbox_tenant_fkey
        FOREIGN KEY(mailbox_id, organization_id)
        REFERENCES mailboxes(id, organization_id);

CREATE OR REPLACE FUNCTION validate_agent_processing_subscription_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.status <> 'active'
       OR NEW.cursor_value IS NOT NULL
       OR NEW.updated_at <> NEW.created_at
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
                  AND membership.valid_from <= NEW.created_at
                  AND (
                      membership.valid_until IS NULL
                      OR membership.valid_until > NEW.created_at
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
                  AND delegation.valid_from <= NEW.created_at
                  AND delegation.valid_until > NEW.created_at
           )
       )
    THEN
        RAISE EXCEPTION 'Agent processing subscription is not authorized'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_processing_subscription_validate_insert
BEFORE INSERT ON agent_subscriptions
FOR EACH ROW EXECUTE FUNCTION validate_agent_processing_subscription_insert();

CREATE OR REPLACE FUNCTION reject_agent_processing_subscription_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent processing subscriptions are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_processing_subscription_reject_update_delete
BEFORE UPDATE OR DELETE ON agent_subscriptions
FOR EACH ROW EXECUTE FUNCTION reject_agent_processing_subscription_mutation();

CREATE TRIGGER agent_processing_subscription_reject_truncate
BEFORE TRUNCATE ON agent_subscriptions
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_processing_subscription_mutation();

ALTER TABLE agent_message_states
    ADD COLUMN organization_id uuid,
    ADD COLUMN last_lease_id uuid,
    ADD COLUMN state_version bigint NOT NULL DEFAULT 0,
    ADD COLUMN acknowledged_at timestamptz,
    ADD COLUMN failed_at timestamptz;

UPDATE agent_message_states AS state
   SET organization_id = subscription.organization_id,
       principal_id = subscription.principal_id,
       last_lease_id = state.lease_id
  FROM agent_subscriptions AS subscription
 WHERE subscription.id = state.subscription_id;

ALTER TABLE agent_message_states
    ALTER COLUMN organization_id SET NOT NULL,
    ALTER COLUMN principal_id SET NOT NULL,
    DROP CONSTRAINT agent_message_states_check,
    ADD CONSTRAINT agent_message_states_tenant_identity_unique
        UNIQUE(subscription_id, message_id, organization_id),
    ADD CONSTRAINT agent_message_states_subscription_tenant_fkey
        FOREIGN KEY(subscription_id, organization_id)
        REFERENCES agent_subscriptions(id, organization_id),
    ADD CONSTRAINT agent_message_states_principal_tenant_fkey
        FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT agent_message_states_shape_check CHECK (
        (state = 'pending'
            AND lease_id IS NULL
            AND lease_expires_at IS NULL
            AND acknowledged_at IS NULL
            AND failed_at IS NULL)
        OR
        (state = 'leased'
            AND lease_id IS NOT NULL
            AND last_lease_id = lease_id
            AND lease_expires_at IS NOT NULL
            AND acknowledged_at IS NULL
            AND failed_at IS NULL)
        OR
        (state = 'acknowledged'
            AND lease_id IS NULL
            AND lease_expires_at IS NULL
            AND last_lease_id IS NOT NULL
            AND last_error IS NULL
            AND acknowledged_at IS NOT NULL
            AND failed_at IS NULL)
        OR
        (state = 'dead_lettered'
            AND lease_id IS NULL
            AND lease_expires_at IS NULL
            AND last_lease_id IS NOT NULL
            AND last_error IS NOT NULL
            AND acknowledged_at IS NULL
            AND failed_at IS NOT NULL)
    );

CREATE INDEX agent_message_claim_idx
    ON agent_message_states(
        organization_id, principal_id, subscription_id, state, updated_at, message_id
    );

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
              AND subscription.status = 'active'
       )
    THEN
        RAISE EXCEPTION 'invalid initial Agent message state'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION validate_agent_message_state_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.subscription_id <> OLD.subscription_id
       OR NEW.message_id <> OLD.message_id
       OR NEW.organization_id <> OLD.organization_id
       OR NEW.principal_id <> OLD.principal_id
       OR NEW.attempt_count < OLD.attempt_count
       OR NEW.state_version <> OLD.state_version + 1
       OR NEW.updated_at < OLD.updated_at
       OR NOT (
           (
               NEW.state = 'leased'
               AND NEW.attempt_count = OLD.attempt_count + 1
               AND (
                   OLD.state = 'pending'
                   OR (
                       OLD.state = 'leased'
                       AND OLD.lease_expires_at <= NEW.updated_at
                   )
               )
           )
           OR (
               OLD.state = 'leased'
               AND OLD.lease_id = NEW.last_lease_id
               AND OLD.lease_expires_at > NEW.updated_at
               AND NEW.attempt_count = OLD.attempt_count
               AND (
                   NEW.state IN ('pending', 'acknowledged', 'dead_lettered')
                   OR (
                       NEW.state = 'leased'
                       AND NEW.lease_id = OLD.lease_id
                       AND NEW.lease_expires_at > OLD.lease_expires_at
                   )
               )
           )
       )
    THEN
        RAISE EXCEPTION 'invalid Agent message processing transition'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_message_state_validate_insert
BEFORE INSERT ON agent_message_states
FOR EACH ROW EXECUTE FUNCTION validate_agent_message_state_insert();

CREATE TRIGGER agent_message_state_validate_update
BEFORE UPDATE ON agent_message_states
FOR EACH ROW EXECUTE FUNCTION validate_agent_message_state_update();

CREATE OR REPLACE FUNCTION reject_agent_message_state_removal()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent message state removal is forbidden'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_message_state_reject_delete
BEFORE DELETE ON agent_message_states
FOR EACH ROW EXECUTE FUNCTION reject_agent_message_state_removal();

CREATE TRIGGER agent_message_state_reject_truncate
BEFORE TRUNCATE ON agent_message_states
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_message_state_removal();

CREATE TABLE agent_message_processing_operations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    subscription_id uuid NOT NULL,
    message_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    lease_id uuid NOT NULL,
    operation text NOT NULL CHECK (
        operation IN ('claimed', 'lease_extended', 'acknowledged', 'nacked', 'dead_lettered')
    ),
    idempotency_key_sha256 bytea NOT NULL
        CHECK (octet_length(idempotency_key_sha256) = 32),
    request_sha256 bytea NOT NULL CHECK (octet_length(request_sha256) = 32),
    result_state text NOT NULL CHECK (
        result_state IN ('pending', 'leased', 'acknowledged', 'dead_lettered')
    ),
    attempt_count integer NOT NULL CHECK (attempt_count > 0),
    lease_expires_at timestamptz,
    reason_code text CHECK (
        reason_code IS NULL
        OR (
            char_length(reason_code) BETWEEN 1 AND 100
            AND reason_code ~ '^[a-z][a-z0-9_.-]*$'
        )
    ),
    occurred_at timestamptz NOT NULL,
    UNIQUE(organization_id, principal_id, operation, idempotency_key_sha256),
    FOREIGN KEY(subscription_id, message_id, organization_id)
        REFERENCES agent_message_states(subscription_id, message_id, organization_id),
    FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    CHECK (
        (result_state = 'leased' AND lease_expires_at IS NOT NULL)
        OR
        (result_state <> 'leased' AND lease_expires_at IS NULL)
    )
);

CREATE INDEX agent_message_processing_operations_lookup_idx
    ON agent_message_processing_operations(
        organization_id, principal_id, subscription_id, message_id, occurred_at
    );

CREATE OR REPLACE FUNCTION reject_agent_message_processing_operation_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'Agent message processing operations are immutable'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_message_processing_operation_reject_update_delete
BEFORE UPDATE OR DELETE ON agent_message_processing_operations
FOR EACH ROW EXECUTE FUNCTION reject_agent_message_processing_operation_mutation();

CREATE TRIGGER agent_message_processing_operation_reject_truncate
BEFORE TRUNCATE ON agent_message_processing_operations
FOR EACH STATEMENT EXECUTE FUNCTION reject_agent_message_processing_operation_mutation();
