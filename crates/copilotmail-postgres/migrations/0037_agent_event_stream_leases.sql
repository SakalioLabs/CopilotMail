ALTER TABLE webhook_subscriptions
    ADD CONSTRAINT webhook_subscriptions_id_tenant_principal_unique
        UNIQUE(id, organization_id, principal_id);

ALTER TABLE principals
    ADD CONSTRAINT principals_id_organization_unique
        UNIQUE(id, organization_id);

CREATE TABLE agent_event_stream_leases (
    id UUID PRIMARY KEY,
    organization_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    subscription_id UUID NOT NULL,
    opened_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT agent_event_stream_leases_window
        CHECK (
            expires_at > opened_at
            AND expires_at <= opened_at + INTERVAL '5 minutes'
        ),
    CONSTRAINT agent_event_stream_leases_tenant_principal_fkey
        FOREIGN KEY(principal_id, organization_id)
        REFERENCES principals(id, organization_id),
    CONSTRAINT agent_event_stream_leases_subscription_owner_fkey
        FOREIGN KEY(subscription_id, organization_id, principal_id)
        REFERENCES webhook_subscriptions(id, organization_id, principal_id)
);

CREATE INDEX agent_event_stream_leases_tenant_expiry_idx
    ON agent_event_stream_leases(organization_id, expires_at);

CREATE INDEX agent_event_stream_leases_principal_expiry_idx
    ON agent_event_stream_leases(organization_id, principal_id, expires_at);

CREATE INDEX agent_event_stream_leases_subscription_expiry_idx
    ON agent_event_stream_leases(organization_id, subscription_id, expires_at);

CREATE OR REPLACE FUNCTION validate_agent_event_stream_lease_insert()
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
           AND status = 'active'
    ) THEN
        RAISE EXCEPTION 'event stream lease requires an active tenant Agent'
            USING ERRCODE = '23514';
    END IF;

    IF NOT EXISTS (
        SELECT 1
          FROM webhook_subscriptions
         WHERE id = NEW.subscription_id
           AND organization_id = NEW.organization_id
           AND principal_id = NEW.principal_id
           AND enabled
    ) THEN
        RAISE EXCEPTION 'event stream lease requires an enabled owned subscription'
            USING ERRCODE = '23514';
    END IF;

    RETURN NEW;
END;
$$;

CREATE TRIGGER agent_event_stream_leases_validate_insert
BEFORE INSERT ON agent_event_stream_leases
FOR EACH ROW EXECUTE FUNCTION validate_agent_event_stream_lease_insert();

CREATE OR REPLACE FUNCTION validate_agent_event_stream_lease_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'event stream leases cannot be updated'
        USING ERRCODE = '55000';
END;
$$;

CREATE TRIGGER agent_event_stream_leases_reject_update
BEFORE UPDATE ON agent_event_stream_leases
FOR EACH ROW EXECUTE FUNCTION validate_agent_event_stream_lease_update();
