CREATE INDEX outbox_agent_cursor_feed_idx
    ON outbox_events(id, event_type)
    WHERE event_type IN (
        'message.received.v1',
        'message.delivery_status_changed.v1',
        'agent.message.dead_lettered.v1'
    );

INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('agent.event_cursor.read', 'agent.event_cursor', 'medium', true);
