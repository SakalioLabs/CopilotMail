use crate::*;

#[async_trait]
impl AuthorizationStore for PgControlPlaneStore {
    async fn resolve_authorization(
        &self,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<Option<AuthorizationContext>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct AuthorizationRow {
            organization_id: Uuid,
            permissions: Vec<String>,
        }

        let row = sqlx::query_as::<_, AuthorizationRow>(
            r#"
            SELECT principal.organization_id,
                   COALESCE(
                       array_agg(DISTINCT permission.permission)
                           FILTER (WHERE permission.permission IS NOT NULL),
                       ARRAY[]::text[]
                   ) AS permissions
              FROM principals AS principal
              LEFT JOIN role_assignments AS assignment
                ON assignment.principal_id = principal.id
               AND assignment.organization_id = principal.organization_id
               AND assignment.revoked_at IS NULL
               AND assignment.valid_from <= $2
               AND (assignment.valid_until IS NULL OR assignment.valid_until > $2)
              LEFT JOIN role_permissions AS permission
                ON permission.role_id = assignment.role_id
             WHERE principal.id = $1
               AND principal.status = 'active'
             GROUP BY principal.organization_id
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;

        Ok(row.map(|row| AuthorizationContext {
            principal_id,
            organization_id: OrganizationId::from_uuid(row.organization_id),
            permissions: row
                .permissions
                .into_iter()
                .filter_map(|value| permission_from_str(&value))
                .collect::<BTreeSet<_>>(),
            authentication: None,
        }))
    }
}
