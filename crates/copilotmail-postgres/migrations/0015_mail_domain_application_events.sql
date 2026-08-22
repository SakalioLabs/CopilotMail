INSERT INTO audit_event_catalog(action, resource_type, risk, privileged) VALUES
    ('mail_domain.registered', 'mail_domain', 'medium', true),
    ('mail_domain.dns_observed', 'mail_domain', 'low', true),
    ('mail_domain.verified', 'mail_domain', 'high', true),
    ('mail_domain.activated', 'mail_domain', 'critical', true),
    ('mail_domain.suspended', 'mail_domain', 'critical', true),
    ('mail_domain.route_updated', 'mail_domain', 'critical', true);
