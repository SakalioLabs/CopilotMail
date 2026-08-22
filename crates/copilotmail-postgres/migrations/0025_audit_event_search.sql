CREATE INDEX audit_events_actor_search_idx
    ON audit_events(organization_id, actor_principal_id, chain_sequence DESC);

ALTER TABLE audit_events
    ADD CONSTRAINT audit_events_details_object
        CHECK (jsonb_typeof(details) = 'object');

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('audit.events.queried', 'audit.query', 'high', true);
