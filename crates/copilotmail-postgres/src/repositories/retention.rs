use crate::*;

pub(crate) async fn load_detail(
    connection: &mut sqlx::PgConnection,
    organization_id: OrganizationId,
    policy_id: RetentionPolicyId,
) -> Result<Option<RetentionPolicyDetail>, StoreError> {
    let row = sqlx::query_as::<_, RetentionPolicyDetailRow>(
        r#"
        SELECT
               policy.id AS policy_id,
               policy.organization_id,
               policy.policy_key,
               policy.display_name,
               policy.resource_kind,
               policy.created_by,
               policy.created_at AS policy_created_at,
               latest.id AS latest_id,
               latest.version AS latest_version,
               latest.minimum_retention_days AS latest_minimum_retention_days,
               latest.archive_after_days AS latest_archive_after_days,
               latest.delete_after_days AS latest_delete_after_days,
               latest.rule AS latest_rule,
               latest.created_by AS latest_created_by,
               latest.created_at AS latest_created_at,
               active.id AS active_id,
               active.version AS active_version,
               active.minimum_retention_days AS active_minimum_retention_days,
               active.archive_after_days AS active_archive_after_days,
               active.delete_after_days AS active_delete_after_days,
               active.rule AS active_rule,
               active.created_by AS active_created_by,
               active.created_at AS active_created_at
          FROM retention_policies AS policy
          JOIN LATERAL (
              SELECT version.*
                FROM retention_policy_versions AS version
               WHERE version.organization_id = policy.organization_id
                 AND version.policy_id = policy.id
               ORDER BY version.version DESC
               LIMIT 1
          ) AS latest ON true
          LEFT JOIN retention_policy_versions AS active
            ON active.organization_id = policy.organization_id
           AND active.policy_id = policy.id
           AND active.id = policy.active_version_id
         WHERE policy.organization_id = $1
           AND policy.id = $2
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(policy_id.as_uuid())
    .fetch_optional(connection)
    .await
    .map_err(map_store_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    retention_policy_detail_from_row(row).map(Some)
}

#[async_trait]
impl RetentionPolicyQueryStore for PgControlPlaneStore {
    async fn get_retention_policy(
        &self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError> {
        let mut connection = self.pool.acquire().await.map_err(map_store_error)?;
        load_detail(&mut connection, organization_id, policy_id).await
    }
}
