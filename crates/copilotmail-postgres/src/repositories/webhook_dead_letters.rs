use crate::*;

#[async_trait]
impl WebhookDeadLetterQueryStore for PgControlPlaneStore {
    async fn list_webhook_dead_letters(
        &self,
        organization_id: OrganizationId,
        after_delivery_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<WebhookDeadLetterSummary>, StoreError> {
        if !(1..=200).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, WebhookDeadLetterRow>(
            r#"
            SELECT delivery.id AS delivery_id,
                   delivery.subscription_id,
                   subscription.organization_id,
                   subscription.principal_id,
                   delivery.event_id,
                   outbox.event_type,
                   delivery.attempt,
                   delivery.response_status,
                   delivery.error_code,
                   delivery.created_at AS failed_at
              FROM webhook_deliveries AS delivery
              JOIN webhook_subscriptions AS subscription
                ON subscription.id = delivery.subscription_id
              JOIN outbox_events AS outbox
                ON outbox.id = delivery.event_id
               AND outbox.payload ->> 'organization_id' =
                   subscription.organization_id::text
             WHERE subscription.organization_id = $1
               AND delivery.status = 'dead_lettered'
               AND delivery.id > COALESCE($2, '00000000-0000-0000-0000-000000000000'::uuid)
               AND NOT EXISTS (
                   SELECT 1
                     FROM webhook_deliveries AS later
                    WHERE later.subscription_id = delivery.subscription_id
                      AND later.event_id = delivery.event_id
                      AND later.attempt > delivery.attempt
               )
             ORDER BY delivery.id
             LIMIT $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(after_delivery_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(rows
            .into_iter()
            .map(|row| WebhookDeadLetterSummary {
                delivery_id: row.delivery_id,
                subscription_id: WebhookSubscriptionId::from_uuid(row.subscription_id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                principal_id: PrincipalId::from_uuid(row.principal_id),
                event_id: row.event_id,
                event_type: row.event_type,
                attempt: row.attempt,
                response_status: row.response_status,
                error_code: row.error_code,
                failed_at: row.failed_at,
            })
            .collect())
    }
}
