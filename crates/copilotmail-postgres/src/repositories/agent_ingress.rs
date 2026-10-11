use crate::*;

#[async_trait]
impl AgentIngressRateLimitStore for PgControlPlaneStore {
    async fn consume_agent_ingress_capacity(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        tenant_requests_per_minute: u32,
        principal_requests_per_minute: u32,
    ) -> Result<AgentIngressRateLimitDecision, StoreError> {
        if tenant_requests_per_minute == 0
            || principal_requests_per_minute == 0
            || principal_requests_per_minute > tenant_requests_per_minute
        {
            return Err(StoreError::Conflict);
        }
        let tenant_limit =
            i32::try_from(tenant_requests_per_minute).map_err(|_| StoreError::Conflict)?;
        let principal_limit =
            i32::try_from(principal_requests_per_minute).map_err(|_| StoreError::Conflict)?;
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 30))")
            .bind(organization_id.as_uuid())
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        let (window_started_at, window_ends_at, observed_at, retry_after_seconds) =
            sqlx::query_as::<_, (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>, i32)>(
                r#"
                WITH snapshot AS (
                    SELECT clock_timestamp() AS observed_at
                )
                SELECT date_trunc('minute', observed_at),
                       date_trunc('minute', observed_at) + interval '1 minute',
                       observed_at,
                       GREATEST(
                           1,
                           CEIL(EXTRACT(
                               EPOCH FROM (
                                   date_trunc('minute', observed_at) + interval '1 minute'
                                   - observed_at
                               )
                           ))::integer
                       )
                  FROM snapshot
                "#,
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        sqlx::query(
            r#"
            DELETE FROM agent_ingress_rate_limit_windows
             WHERE organization_id = $1
               AND window_ends_at <= clock_timestamp()
            "#,
        )
        .bind(organization_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;

        if !increment_agent_ingress_window(
            &mut transaction,
            organization_id,
            None,
            window_started_at,
            window_ends_at,
            observed_at,
            tenant_limit,
        )
        .await
        .map_err(map_store_error)?
        {
            transaction.rollback().await.map_err(map_store_error)?;
            return Ok(AgentIngressRateLimitDecision::TenantLimited {
                retry_after_seconds: u32::try_from(retry_after_seconds).unwrap_or(1),
            });
        }
        if !increment_agent_ingress_window(
            &mut transaction,
            organization_id,
            Some(principal_id),
            window_started_at,
            window_ends_at,
            observed_at,
            principal_limit,
        )
        .await
        .map_err(map_store_error)?
        {
            transaction.rollback().await.map_err(map_store_error)?;
            return Ok(AgentIngressRateLimitDecision::PrincipalLimited {
                retry_after_seconds: u32::try_from(retry_after_seconds).unwrap_or(1),
            });
        }
        transaction.commit().await.map_err(map_store_error)?;
        Ok(AgentIngressRateLimitDecision::Allowed)
    }
}
