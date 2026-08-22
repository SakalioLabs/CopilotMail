ALTER TABLE webhook_subscriptions
    ADD COLUMN version BIGINT NOT NULL DEFAULT 1,
    ADD CONSTRAINT webhook_subscriptions_version_positive
        CHECK (version > 0),
    ADD CONSTRAINT webhook_subscriptions_endpoint_bounded
        CHECK (
            length(endpoint_url) BETWEEN 8 AND 2048
            AND endpoint_url !~ '[[:cntrl:]]'
        ),
    ADD CONSTRAINT webhook_subscriptions_event_types_bounded
        CHECK (
            cardinality(event_types) BETWEEN 1 AND 100
            AND array_position(event_types, NULL) IS NULL
        ),
    ADD CONSTRAINT webhook_subscriptions_signing_key_reference_bounded
        CHECK (
            length(signing_key_reference) BETWEEN 1 AND 100
            AND signing_key_reference ~ '^[A-Za-z0-9._-]+$'
        );

ALTER TABLE webhook_deliveries
    ADD COLUMN error_code TEXT,
    ADD COLUMN delivered_at TIMESTAMPTZ,
    ADD CONSTRAINT webhook_deliveries_response_status_valid
        CHECK (response_status IS NULL OR response_status BETWEEN 100 AND 599),
    ADD CONSTRAINT webhook_deliveries_error_code_stable
        CHECK (
            error_code IS NULL
            OR (
                length(error_code) BETWEEN 1 AND 100
                AND error_code ~ '^[a-z0-9_.-]+$'
            )
        ),
    ADD CONSTRAINT webhook_deliveries_terminal_shape
        CHECK (
            (status = 'delivered' AND response_status IS NOT NULL
                AND response_status BETWEEN 200 AND 299
                AND error_code IS NULL AND delivered_at IS NOT NULL
                AND next_attempt_at IS NULL)
            OR
            (status IN ('retrying', 'dead_lettered') AND error_code IS NOT NULL
                AND delivered_at IS NULL
                AND (
                    (status = 'retrying' AND next_attempt_at IS NOT NULL)
                    OR (status = 'dead_lettered' AND next_attempt_at IS NULL)
                ))
            OR
            (status = 'pending' AND response_status IS NULL
                AND error_code IS NULL AND delivered_at IS NULL)
        );

CREATE INDEX webhook_subscriptions_enabled_events_idx
    ON webhook_subscriptions(organization_id, enabled, created_at, id);

CREATE INDEX webhook_deliveries_event_terminal_idx
    ON webhook_deliveries(event_id, subscription_id, attempt DESC);
