use crate::*;

#[async_trait]
impl AuditExportQueryStore for PgControlPlaneStore {
    async fn get_audit_export_grant(
        &self,
        organization_id: OrganizationId,
        grant_id: AuditExportId,
    ) -> Result<Option<AuditExportGrant>, StoreError> {
        let row = sqlx::query_as::<_, AuditExportGrantRow>(
            r#"
            SELECT id, organization_id, archive_id, approval_request_id,
                   requested_by, created_at, expires_at
              FROM audit_export_grants
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(grant_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(audit_export_grant_from_row).transpose()
    }

    async fn get_audit_export_grant_by_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<AuditExportGrant>, StoreError> {
        let row = sqlx::query_as::<_, AuditExportGrantRow>(
            r#"
            SELECT id, organization_id, archive_id, approval_request_id,
                   requested_by, created_at, expires_at
              FROM audit_export_grants
             WHERE organization_id = $1 AND approval_request_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(audit_export_grant_from_row).transpose()
    }

    async fn audit_export_grant_is_active(
        &self,
        organization_id: OrganizationId,
        grant_id: AuditExportId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM audit_export_grants
                 WHERE organization_id = $1
                   AND id = $2
                   AND created_at <= clock_timestamp()
                   AND expires_at > clock_timestamp()
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(grant_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }
}
