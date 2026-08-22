INSERT INTO audit_event_catalog(action, resource_type, risk, privileged)
VALUES
    ('mail.submission.accepted', 'mail.submission', 'high', true),
    ('mail.submission.rejected', 'mail.submission', 'high', true),
    ('mail.submission.review_required', 'mail.submission', 'critical', true);
