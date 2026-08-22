INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('agent.message_dead_letters.queried', 'agent.message_dead_letter_query', 'medium', true);

CREATE INDEX agent_message_dead_letter_query_idx
    ON agent_message_states(
        organization_id,
        principal_id,
        subscription_id,
        failed_at DESC,
        message_id DESC
    )
    WHERE state = 'dead_lettered';
