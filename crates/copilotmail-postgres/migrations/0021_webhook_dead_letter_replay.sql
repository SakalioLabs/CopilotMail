CREATE INDEX webhook_deliveries_dead_letter_cursor_idx
    ON webhook_deliveries(id)
    WHERE status = 'dead_lettered';

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('agent.webhook_delivery.replayed', 'agent.webhook_delivery', 'critical', true);
