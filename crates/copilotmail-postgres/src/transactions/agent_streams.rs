use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_acquire_agent_event_stream_lease(
        &mut self,
        lease: &AgentEventStreamLease,
        tenant_limit: u32,
        principal_limit: u32,
    ) -> Result<AgentEventStreamLeaseAcquisition, StoreError> {
        if tenant_limit == 0 || principal_limit == 0 || principal_limit > tenant_limit {
            return Err(StoreError::Conflict);
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 29))")
            .bind(lease.organization_id.as_uuid())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        sqlx::query(
            r#"
            DELETE FROM agent_event_stream_leases
             WHERE organization_id = $1
               AND expires_at <= $2
            "#,
        )
        .bind(lease.organization_id.as_uuid())
        .bind(lease.opened_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let (tenant_count, principal_count) = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT count(*)::bigint,
                   count(*) FILTER (WHERE principal_id = $2)::bigint
              FROM agent_event_stream_leases
             WHERE organization_id = $1
               AND expires_at > $3
            "#,
        )
        .bind(lease.organization_id.as_uuid())
        .bind(lease.principal_id.as_uuid())
        .bind(lease.opened_at)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if tenant_count >= i64::from(tenant_limit) {
            return Ok(AgentEventStreamLeaseAcquisition::TenantCapacityReached);
        }
        if principal_count >= i64::from(principal_limit) {
            return Ok(AgentEventStreamLeaseAcquisition::PrincipalCapacityReached);
        }
        sqlx::query(
            r#"
            INSERT INTO agent_event_stream_leases(
                id, organization_id, principal_id, subscription_id,
                opened_at, expires_at
            )
            VALUES($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(lease.lease_id)
        .bind(lease.organization_id.as_uuid())
        .bind(lease.principal_id.as_uuid())
        .bind(lease.subscription_id.as_uuid())
        .bind(lease.opened_at)
        .bind(lease.expires_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(AgentEventStreamLeaseAcquisition::Acquired(lease.clone()))
    }
}
