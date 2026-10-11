-- Registers the successful, privileged, content-free Agent Inspector read.
-- The Inspector itself remains a transactional projection of existing
-- control-plane records; it intentionally adds no mutable snapshot table.
INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('identity.agent.queried', 'identity.agent', 'high', true);
