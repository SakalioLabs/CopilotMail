use crate::*;

#[async_trait]
impl RoleGovernanceQueryStore for PgControlPlaneStore {
    async fn list_roles(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<RoleSummary>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            system_key: String,
            display_name: String,
            permissions: Vec<String>,
            created_at: DateTime<Utc>,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT role.id, role.organization_id, role.system_key,
                   role.display_name,
                   array_agg(permission.permission ORDER BY permission.permission)
                       AS permissions,
                   role.created_at
              FROM roles AS role
              JOIN role_permissions AS permission
                ON permission.role_id = role.id
             WHERE role.organization_id = $1
               AND role.system_key IS NOT NULL
             GROUP BY role.id
             ORDER BY role.system_key
            "#,
        )
        .bind(organization_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;

        rows.into_iter()
            .map(|row| {
                let permissions = row
                    .permissions
                    .iter()
                    .map(|permission| {
                        permission_from_str(permission).ok_or(StoreError::Unavailable)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(RoleSummary {
                    role_id: RoleId::from_uuid(row.id),
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    system_role: system_role_from_str(&row.system_key)?,
                    display_name: row.display_name,
                    permissions,
                    created_at: row.created_at,
                })
            })
            .collect()
    }

    async fn get_role_assignment(
        &self,
        organization_id: OrganizationId,
        assignment_id: RoleAssignmentId,
    ) -> Result<Option<RoleAssignmentSummary>, StoreError> {
        let row = sqlx::query_as::<_, RoleAssignmentSummaryRow>(
            r#"
            SELECT assignment.id, assignment.organization_id,
                   assignment.principal_id, principal.kind AS principal_kind,
                   principal.status AS principal_status,
                   principal.display_name AS principal_display_name,
                   assignment.role_id, role.system_key,
                   role.display_name AS role_display_name,
                   assignment.created_by, grant_record.approval_request_id,
                   assignment.valid_from, assignment.valid_until,
                   assignment.revoked_at, assignment.created_at
              FROM role_assignments AS assignment
              JOIN principals AS principal
                ON principal.id = assignment.principal_id
               AND principal.organization_id = assignment.organization_id
              JOIN roles AS role
                ON role.id = assignment.role_id
               AND role.organization_id = assignment.organization_id
         LEFT JOIN role_assignment_grants AS grant_record
                ON grant_record.assignment_id = assignment.id
               AND grant_record.organization_id = assignment.organization_id
             WHERE assignment.organization_id = $1
               AND assignment.id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(role_assignment_summary_from_row).transpose()
    }

    async fn role_assignment_cursor_exists(
        &self,
        query: RoleAssignmentListQuery,
        assignment_id: RoleAssignmentId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM role_assignments AS assignment
                  JOIN principals AS principal
                    ON principal.id = assignment.principal_id
                   AND principal.organization_id = assignment.organization_id
                 WHERE assignment.organization_id = $1
                   AND assignment.id = $2
                   AND ($3::uuid IS NULL OR assignment.principal_id = $3)
                   AND (
                        $4
                        OR (
                            assignment.revoked_at IS NULL
                            AND assignment.valid_from <= $5
                            AND (
                                assignment.valid_until IS NULL
                                OR assignment.valid_until > $5
                            )
                            AND principal.status = 'active'
                        )
                   )
            )
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .bind(query.principal_id.map(PrincipalId::as_uuid))
        .bind(query.include_inactive)
        .bind(query.now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_role_assignments(
        &self,
        query: RoleAssignmentListQuery,
    ) -> Result<Vec<RoleAssignmentSummary>, StoreError> {
        let rows = sqlx::query_as::<_, RoleAssignmentSummaryRow>(
            r#"
            WITH eligible AS (
                SELECT assignment.id, assignment.organization_id,
                       assignment.principal_id,
                       principal.kind AS principal_kind,
                       principal.status AS principal_status,
                       principal.display_name AS principal_display_name,
                       assignment.role_id, role.system_key,
                       role.display_name AS role_display_name,
                       assignment.created_by,
                       grant_record.approval_request_id,
                       assignment.valid_from, assignment.valid_until,
                       assignment.revoked_at, assignment.created_at
                  FROM role_assignments AS assignment
                  JOIN principals AS principal
                    ON principal.id = assignment.principal_id
                   AND principal.organization_id = assignment.organization_id
                  JOIN roles AS role
                    ON role.id = assignment.role_id
                   AND role.organization_id = assignment.organization_id
             LEFT JOIN role_assignment_grants AS grant_record
                    ON grant_record.assignment_id = assignment.id
                   AND grant_record.organization_id = assignment.organization_id
                 WHERE assignment.organization_id = $1
                   AND ($2::uuid IS NULL OR assignment.principal_id = $2)
                   AND (
                        $3
                        OR (
                            assignment.revoked_at IS NULL
                            AND assignment.valid_from <= $5
                            AND (
                                assignment.valid_until IS NULL
                                OR assignment.valid_until > $5
                            )
                            AND principal.status = 'active'
                        )
                   )
            )
            SELECT *
              FROM eligible
             WHERE (
                $4::uuid IS NULL
                OR (created_at, id) < (
                    SELECT anchor.created_at, anchor.id
                      FROM eligible AS anchor
                     WHERE anchor.id = $4
                )
             )
             ORDER BY created_at DESC, id DESC
             LIMIT $6
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.map(PrincipalId::as_uuid))
        .bind(query.include_inactive)
        .bind(query.after_assignment_id.map(RoleAssignmentId::as_uuid))
        .bind(query.now)
        .bind(query.limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(role_assignment_summary_from_row)
            .collect()
    }
}
