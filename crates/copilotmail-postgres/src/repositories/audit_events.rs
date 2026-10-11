use crate::*;

#[async_trait]
impl AuditEventQueryStore for PgControlPlaneStore {
    async fn active_human_audit_reader_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE organization_id = $1
                   AND id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn audit_event_search_anchor_exists(
        &self,
        search: &AuditEventSearch,
        chain_sequence: i64,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM audit_events
                 WHERE organization_id = $1
                   AND occurred_at >= $2
                   AND occurred_at <= $3
                   AND ($4::text IS NULL OR action = $4)
                   AND ($5::uuid IS NULL OR actor_principal_id = $5)
                   AND ($6::text IS NULL OR resource_type = $6)
                   AND ($7::text IS NULL OR resource_id = $7)
                   AND ($8::text IS NULL OR outcome = $8)
                   AND ($9::text IS NULL OR request_id = $9)
                   AND chain_sequence = $10
            )
            "#,
        )
        .bind(search.organization_id.as_uuid())
        .bind(search.occurred_from)
        .bind(search.occurred_to)
        .bind(search.action.as_deref())
        .bind(search.actor_principal_id.map(|id| id.as_uuid()))
        .bind(search.resource_type.as_deref())
        .bind(search.resource_id.as_deref())
        .bind(search.outcome.map(audit_outcome))
        .bind(search.request_id.as_deref())
        .bind(chain_sequence)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn search_audit_events(
        &self,
        search: &AuditEventSearch,
    ) -> Result<Vec<AuditEventSummary>, StoreError> {
        if !(1..=101).contains(&search.limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, AuditEventSummaryRow>(
            r#"
            SELECT event.id AS event_id,
                   event.organization_id,
                   event.actor_principal_id,
                   actor.kind AS actor_kind,
                   event.on_behalf_of_principal_id,
                   event.credential_id,
                   event.subject_principal_id,
                   event.action,
                   event.resource_type,
                   event.resource_id,
                   event.outcome,
                   event.trace_id,
                   event.request_id,
                   CASE
                       WHEN event.source_ip IS NULL THEN NULL
                       ELSE host(event.source_ip)
                   END AS source_ip,
                   event.client_id,
                   event.session_id,
                   event.token_jti,
                   event.policy_id,
                   event.policy_version_id,
                   event.approval_request_id,
                   event.ticket_reference,
                   event.environment,
                   event.region,
                   event.details,
                   event.occurred_at,
                   event.received_at,
                   event.chain_sequence,
                   event.hash_version,
                   encode(event.previous_hash, 'hex') AS previous_hash_hex,
                   encode(event.event_hash, 'hex') AS event_hash_hex,
                   event.signature_batch_id
              FROM audit_events AS event
              JOIN principals AS actor
                ON actor.id = event.actor_principal_id
               AND actor.organization_id = event.organization_id
             WHERE event.organization_id = $1
               AND event.occurred_at >= $2
               AND event.occurred_at <= $3
               AND ($4::text IS NULL OR event.action = $4)
               AND ($5::uuid IS NULL OR event.actor_principal_id = $5)
               AND ($6::text IS NULL OR event.resource_type = $6)
               AND ($7::text IS NULL OR event.resource_id = $7)
               AND ($8::text IS NULL OR event.outcome = $8)
               AND ($9::text IS NULL OR event.request_id = $9)
               AND ($10::bigint IS NULL OR event.chain_sequence < $10)
             ORDER BY event.chain_sequence DESC
             LIMIT $11
            "#,
        )
        .bind(search.organization_id.as_uuid())
        .bind(search.occurred_from)
        .bind(search.occurred_to)
        .bind(search.action.as_deref())
        .bind(search.actor_principal_id.map(|id| id.as_uuid()))
        .bind(search.resource_type.as_deref())
        .bind(search.resource_id.as_deref())
        .bind(search.outcome.map(audit_outcome))
        .bind(search.request_id.as_deref())
        .bind(search.before_chain_sequence)
        .bind(search.limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter().map(audit_event_summary_from_row).collect()
    }
}
