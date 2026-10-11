use crate::*;

#[async_trait]
impl WebhookSubscriptionQueryStore for PgControlPlaneStore {
    async fn get_webhook_subscription(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscriptionSummary>, StoreError> {
        let row = sqlx::query_as::<_, WebhookSubscriptionRow>(
            r#"
            SELECT id AS subscription_id, organization_id, principal_id,
                   endpoint_url, event_types, signing_key_reference, enabled,
                   version, created_at, updated_at
              FROM webhook_subscriptions
             WHERE id = $1 AND organization_id = $2
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(webhook_subscription_summary_from_row).transpose()
    }

    async fn list_webhook_subscriptions(
        &self,
        organization_id: OrganizationId,
        principal_id: Option<PrincipalId>,
        after_id: Option<WebhookSubscriptionId>,
        limit: i64,
    ) -> Result<Vec<WebhookSubscriptionSummary>, StoreError> {
        if !(1..=200).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, WebhookSubscriptionRow>(
            r#"
            SELECT id AS subscription_id, organization_id, principal_id,
                   endpoint_url, event_types, signing_key_reference, enabled,
                   version, created_at, updated_at
              FROM webhook_subscriptions
             WHERE organization_id = $1
               AND ($2::uuid IS NULL OR principal_id = $2)
               AND ($3::uuid IS NULL OR id > $3)
             ORDER BY id
             LIMIT $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.map(PrincipalId::as_uuid))
        .bind(after_id.map(WebhookSubscriptionId::as_uuid))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(webhook_subscription_summary_from_row)
            .collect()
    }
}
