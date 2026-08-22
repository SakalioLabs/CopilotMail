ALTER TABLE webhook_subscriptions
    DROP CONSTRAINT webhook_subscriptions_principal_id_fkey,
    ADD CONSTRAINT webhook_subscriptions_tenant_principal_fkey
        FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    ADD CONSTRAINT webhook_subscriptions_external_event_allowlist
        CHECK (
            event_types <@ ARRAY[
                'message.received.v1',
                'message.delivery_status_changed.v1',
                'agent.message.dead_lettered.v1'
            ]::text[]
        ),
    ADD CONSTRAINT webhook_subscriptions_id_organization_unique
        UNIQUE(id, organization_id);

CREATE OR REPLACE FUNCTION enforce_webhook_subscription_agent_owner()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM principals
         WHERE id = NEW.principal_id
           AND organization_id = NEW.organization_id
           AND kind = 'agent'
    ) THEN
        RAISE EXCEPTION 'webhook subscription owner must be a tenant Agent'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER webhook_subscriptions_require_agent_owner
BEFORE INSERT OR UPDATE OF principal_id, organization_id
ON webhook_subscriptions
FOR EACH ROW EXECUTE FUNCTION enforce_webhook_subscription_agent_owner();

CREATE OR REPLACE FUNCTION reject_webhook_delivery_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'webhook_deliveries is append-only'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER webhook_deliveries_reject_update_delete
BEFORE UPDATE OR DELETE ON webhook_deliveries
FOR EACH ROW EXECUTE FUNCTION reject_webhook_delivery_mutation();

CREATE TRIGGER webhook_deliveries_reject_truncate
BEFORE TRUNCATE ON webhook_deliveries
FOR EACH STATEMENT EXECUTE FUNCTION reject_webhook_delivery_mutation();

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('agent.webhook_subscription.created', 'agent.webhook_subscription', 'high', true),
    ('agent.webhook_subscription.updated', 'agent.webhook_subscription', 'high', true),
    ('agent.webhook_subscription.disabled', 'agent.webhook_subscription', 'high', true);
