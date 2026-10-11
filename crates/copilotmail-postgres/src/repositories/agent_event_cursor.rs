use crate::*;

#[async_trait]
impl AgentEventCursorQueryStore for PgControlPlaneStore {
    async fn webhook_cursor_event_exists(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        event_id: Uuid,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM webhook_subscriptions AS subscription
                  JOIN principals AS principal
                    ON principal.id = subscription.principal_id
                   AND principal.organization_id = subscription.organization_id
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                  JOIN outbox_events AS event
                    ON event.id = $4
                   AND event.payload ->> 'organization_id' =
                       subscription.organization_id::text
                   AND event.event_type = ANY(subscription.event_types)
                   AND event.event_type IN (
                       'message.received.v1',
                       'message.delivery_status_changed.v1',
                       'agent.message.dead_lettered.v1'
                   )
                   AND event.occurred_at >= subscription.created_at
                 WHERE subscription.organization_id = $1
                   AND subscription.id = $2
                   AND subscription.principal_id = $3
                   AND subscription.enabled
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(subscription_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(event_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_webhook_cursor_events(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        after_event_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<AgentCursorEvent>, StoreError> {
        if !(1..=101).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, AgentCursorEventRow>(
            r#"
            SELECT event.id AS event_id,
                   subscription.organization_id,
                   event.event_type,
                   event.aggregate_type,
                   event.aggregate_id,
                   event.schema_version,
                   event.payload,
                   event.occurred_at
              FROM webhook_subscriptions AS subscription
              JOIN principals AS principal
                ON principal.id = subscription.principal_id
               AND principal.organization_id = subscription.organization_id
               AND principal.kind = 'agent'
               AND principal.status = 'active'
              JOIN outbox_events AS event
                ON event.payload ->> 'organization_id' =
                   subscription.organization_id::text
               AND event.event_type = ANY(subscription.event_types)
               AND event.event_type IN (
                   'message.received.v1',
                   'message.delivery_status_changed.v1',
                   'agent.message.dead_lettered.v1'
               )
               AND event.occurred_at >= subscription.created_at
             WHERE subscription.organization_id = $1
               AND subscription.id = $2
               AND subscription.principal_id = $3
               AND subscription.enabled
               AND event.id > COALESCE(
                   $4,
                   '00000000-0000-0000-0000-000000000000'::uuid
               )
             ORDER BY event.id
             LIMIT $5
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(subscription_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(after_event_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(|row| {
                if !row.payload.is_object() {
                    return Err(StoreError::Conflict);
                }
                Ok(AgentCursorEvent {
                    event_id: row.event_id,
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    event_type: WebhookEventType::parse(&row.event_type)
                        .map_err(|_| StoreError::Conflict)?,
                    aggregate_type: row.aggregate_type,
                    aggregate_id: row.aggregate_id,
                    schema_version: row.schema_version,
                    payload: row.payload,
                    occurred_at: row.occurred_at,
                })
            })
            .collect()
    }

    async fn release_agent_event_stream_lease(
        &self,
        lease: &AgentEventStreamLease,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM agent_event_stream_leases
             WHERE id = $1
               AND organization_id = $2
               AND principal_id = $3
               AND subscription_id = $4
            "#,
        )
        .bind(lease.lease_id)
        .bind(lease.organization_id.as_uuid())
        .bind(lease.principal_id.as_uuid())
        .bind(lease.subscription_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }
}
