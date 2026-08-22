-- Registers the two content-minimized Human draft disclosure actions.
-- Draft metadata and body content remain in their existing authoritative
-- stores; this migration deliberately adds no mutable projection table.
INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('mail.drafts.queried', 'mailbox', 'medium', true),
    ('mail.draft.read', 'mail.draft', 'high', true);
