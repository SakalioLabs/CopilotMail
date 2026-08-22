INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('mail.message.read', 'mail.message', 'high', true);

-- Human inbox list/detail reads project only already-authenticated inbound
-- events. This covering index keeps the tenant/mailbox/current-binding lookup
-- bounded without creating a second message identity source of truth.
CREATE INDEX inbound_mail_engine_events_human_content_projection_idx
    ON inbound_mail_engine_events(
        organization_id,
        mailbox_id,
        source,
        account_ref,
        document_ref
    )
    INCLUDE (message_id);
