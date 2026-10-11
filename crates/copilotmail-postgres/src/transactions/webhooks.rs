use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_webhook_subscription(
        &mut self,
        subscription: &WebhookSubscription,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO webhook_subscriptions(
                id, organization_id, principal_id, endpoint_url, event_types,
                signing_key_reference, enabled, version, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
            "#,
        )
        .bind(subscription.id.as_uuid())
        .bind(subscription.organization_id.as_uuid())
        .bind(subscription.principal_id.as_uuid())
        .bind(&subscription.endpoint_url)
        .bind(webhook_event_type_names(&subscription.event_types))
        .bind(&subscription.signing_key_reference)
        .bind(subscription.enabled)
        .bind(subscription.version)
        .bind(subscription.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_webhook_subscription_for_update(
        &mut self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscription>, StoreError> {
        let row = sqlx::query_as::<_, WebhookSubscriptionRow>(
            r#"
            SELECT id AS subscription_id, organization_id, principal_id,
                   endpoint_url, event_types, signing_key_reference, enabled,
                   version, created_at, updated_at
              FROM webhook_subscriptions
             WHERE id = $1 AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(webhook_subscription_from_row).transpose()
    }

    pub(crate) async fn tx_update_webhook_subscription(
        &mut self,
        subscription: &WebhookSubscription,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE webhook_subscriptions
               SET endpoint_url = $3,
                   event_types = $4,
                   signing_key_reference = $5,
                   enabled = $6,
                   version = $7,
                   updated_at = $8
             WHERE id = $1
               AND organization_id = $2
               AND version = $9
            "#,
        )
        .bind(subscription.id.as_uuid())
        .bind(subscription.organization_id.as_uuid())
        .bind(&subscription.endpoint_url)
        .bind(webhook_event_type_names(&subscription.event_types))
        .bind(&subscription.signing_key_reference)
        .bind(subscription.enabled)
        .bind(subscription.version)
        .bind(subscription.updated_at)
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn tx_replay_dead_lettered_webhook_delivery(
        &mut self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        event_id: Uuid,
        requested_at: DateTime<Utc>,
    ) -> Result<Option<i32>, StoreError> {
        let transaction = self.transaction()?;
        let latest_attempt = sqlx::query_scalar::<_, i32>(
            r#"
            SELECT delivery.attempt
              FROM webhook_subscriptions AS subscription
              JOIN outbox_events AS outbox
                ON outbox.id = $3
               AND outbox.payload ->> 'organization_id' =
                   subscription.organization_id::text
              JOIN LATERAL (
                    SELECT attempt, status
                      FROM webhook_deliveries
                     WHERE subscription_id = subscription.id
                       AND event_id = $3
                     ORDER BY attempt DESC
                     LIMIT 1
              ) AS delivery ON true
             WHERE subscription.id = $1
               AND subscription.organization_id = $2
               AND subscription.enabled
               AND delivery.status = 'dead_lettered'
             FOR UPDATE OF subscription, outbox
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(event_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(map_store_error)?;
        let Some(latest_attempt) = latest_attempt else {
            return Ok(None);
        };
        let replay_attempt = latest_attempt.checked_add(1).ok_or(StoreError::Conflict)?;
        sqlx::query(
            r#"
            INSERT INTO webhook_deliveries(
                id, subscription_id, event_id, attempt, status,
                response_status, next_attempt_at, error_code, delivered_at,
                created_at, updated_at
            )
            VALUES(
                $1, $2, $3, $4, 'retrying',
                NULL, $5, 'manual_replay_requested', NULL, $5, $5
            )
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(subscription_id.as_uuid())
        .bind(event_id)
        .bind(replay_attempt)
        .bind(requested_at)
        .execute(&mut **transaction)
        .await
        .map_err(map_store_error)?;
        let outbox = sqlx::query(
            r#"
            UPDATE outbox_events
               SET published_at = NULL,
                   locked_at = NULL,
                   locked_by = NULL,
                   next_attempt_at = $2,
                   last_error = NULL
             WHERE id = $1
            "#,
        )
        .bind(event_id)
        .bind(requested_at)
        .execute(&mut **transaction)
        .await
        .map_err(map_store_error)?;
        if outbox.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(Some(replay_attempt))
    }
}
