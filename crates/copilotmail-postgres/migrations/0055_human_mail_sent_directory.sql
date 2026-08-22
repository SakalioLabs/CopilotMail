-- Registers the governed, content-free Human accepted-submission directory.
-- The directory reads existing durable submission and delivery evidence; it
-- creates no mutable projection and never needs recipient/SMTP evidence.
--
-- The supporting index is deliberately a separate 0056 non-transactional
-- migration so PostgreSQL can build it without blocking submission writes.
INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES ('mail.submissions.queried', 'mailbox', 'medium', true);
