use crate::*;

#[async_trait]
impl AuditCheckpointQueryStore for PgControlPlaneStore {
    async fn audit_checkpoint_candidate(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<AuditCheckpointCandidate>, StoreError> {
        let report = self
            .verify_audit_chain(organization_id)
            .await
            .map_err(|error| {
                tracing::error!(%organization_id, %error, "audit chain is not checkpointable");
                StoreError::Unavailable
            })?;
        let (Some(first_event_id), Some(last_event_id), Some(root_hash), Some(hash_version)) = (
            report.first_event_id,
            report.last_event_id,
            report.head_hash,
            report.hash_version,
        ) else {
            return Ok(None);
        };
        let chain_sequence_end =
            i64::try_from(report.event_count).map_err(|_| StoreError::Unavailable)?;
        let (period_start, period_end): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            r#"
            SELECT min(occurred_at), max(occurred_at)
              FROM audit_events
             WHERE organization_id = $1
               AND chain_sequence <= $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(chain_sequence_end)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(Some(AuditCheckpointCandidate {
            organization_id,
            chain_sequence_end,
            event_count: report.event_count,
            first_event_id,
            last_event_id,
            period_start,
            period_end,
            root_hash,
            hash_version,
        }))
    }
}
