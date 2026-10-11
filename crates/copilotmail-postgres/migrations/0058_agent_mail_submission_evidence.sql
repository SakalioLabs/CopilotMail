-- Registers the successful, privileged, content-free exact Agent production
-- mail evidence read. The read projects existing immutable admission,
-- approval, delivery, and audit records; it adds no mutable evidence table.
INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('agent.mail_submission_evidence.queried', 'agent.mail_submission_evidence', 'high', true);
