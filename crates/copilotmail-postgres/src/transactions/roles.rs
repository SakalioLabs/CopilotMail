use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_role(&mut self, role: &Role) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO roles(id, organization_id, system_key, display_name, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(role.id().as_uuid())
        .bind(role.organization_id().as_uuid())
        .bind(role.system_role().as_str())
        .bind(role.display_name())
        .bind(role.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        for permission in role.permissions() {
            sqlx::query("INSERT INTO role_permissions(role_id, permission) VALUES ($1, $2)")
                .bind(role.id().as_uuid())
                .bind(permission.as_str())
                .execute(&mut **self.transaction()?)
                .await
                .map_err(map_store_error)?;
        }
        Ok(())
    }

    pub(crate) async fn tx_insert_role_assignment(
        &mut self,
        assignment: &RoleAssignment,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO role_assignments(
                id, organization_id, principal_id, role_id, created_by,
                valid_from, valid_until, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(assignment.id().as_uuid())
        .bind(assignment.organization_id().as_uuid())
        .bind(assignment.principal_id().as_uuid())
        .bind(assignment.role_id().as_uuid())
        .bind(assignment.created_by().as_uuid())
        .bind(assignment.valid_from())
        .bind(assignment.valid_until())
        .bind(assignment.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_insert_role_assignment_grant(
        &mut self,
        assignment: &RoleAssignment,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO role_assignment_grants(
                assignment_id, organization_id, approval_request_id,
                granted_by, granted_at
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(assignment.id().as_uuid())
        .bind(assignment.organization_id().as_uuid())
        .bind(approval_request_id.as_uuid())
        .bind(assignment.created_by().as_uuid())
        .bind(assignment.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_role_assignment_grant_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<RoleAssignmentGrantRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            assignment_id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            role_id: Uuid,
            approval_request_id: Uuid,
            granted_by: Uuid,
            valid_from: DateTime<Utc>,
            valid_until: DateTime<Utc>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT grant_record.assignment_id, grant_record.organization_id,
                   assignment.principal_id, assignment.role_id,
                   grant_record.approval_request_id, grant_record.granted_by,
                   assignment.valid_from, assignment.valid_until
              FROM role_assignment_grants AS grant_record
              JOIN role_assignments AS assignment
                ON assignment.id = grant_record.assignment_id
               AND assignment.organization_id = grant_record.organization_id
             WHERE grant_record.organization_id = $1
               AND grant_record.approval_request_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| RoleAssignmentGrantRecord {
            assignment_id: RoleAssignmentId::from_uuid(row.assignment_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            role_id: RoleId::from_uuid(row.role_id),
            approval_request_id: copilotmail_domain::ApprovalRequestId::from_uuid(
                row.approval_request_id,
            ),
            granted_by: PrincipalId::from_uuid(row.granted_by),
            valid_from: row.valid_from,
            valid_until: row.valid_until,
        }))
    }

    pub(crate) async fn tx_revoke_governed_role_assignment(
        &mut self,
        organization_id: OrganizationId,
        assignment_id: RoleAssignmentId,
        revoked_at: DateTime<Utc>,
    ) -> Result<RoleAssignmentRevocationResult, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 7))")
            .bind(organization_id.to_string())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct Row {
            system_key: String,
            valid_from: DateTime<Utc>,
            valid_until: Option<DateTime<Utc>>,
            revoked_at: Option<DateTime<Utc>>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT role.system_key, assignment.valid_from,
                   assignment.valid_until, assignment.revoked_at
              FROM role_assignments AS assignment
              JOIN roles AS role
                ON role.id = assignment.role_id
               AND role.organization_id = assignment.organization_id
             WHERE assignment.organization_id = $1
               AND assignment.id = $2
             FOR UPDATE OF assignment
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(RoleAssignmentRevocationResult::NotFound);
        };
        if row.revoked_at.is_some() {
            return Ok(RoleAssignmentRevocationResult::AlreadyRevoked);
        }
        if row.system_key == "org_owner"
            && row.valid_from <= revoked_at
            && row.valid_until.is_none_or(|until| until > revoked_at)
        {
            let other_owner_count = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT count(*)
                  FROM role_assignments AS assignment
                  JOIN roles AS role
                    ON role.id = assignment.role_id
                   AND role.organization_id = assignment.organization_id
                  JOIN principals AS principal
                    ON principal.id = assignment.principal_id
                   AND principal.organization_id = assignment.organization_id
                 WHERE assignment.organization_id = $1
                   AND assignment.id <> $2
                   AND role.system_key = 'org_owner'
                   AND assignment.revoked_at IS NULL
                   AND assignment.valid_from <= $3
                   AND (
                        assignment.valid_until IS NULL
                        OR assignment.valid_until > $3
                   )
                   AND principal.status = 'active'
                "#,
            )
            .bind(organization_id.as_uuid())
            .bind(assignment_id.as_uuid())
            .bind(revoked_at)
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
            if other_owner_count == 0 {
                return Ok(RoleAssignmentRevocationResult::LastActiveOwner);
            }
        }

        let changed = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE role_assignments
               SET revoked_at = $3
             WHERE organization_id = $1
               AND id = $2
               AND revoked_at IS NULL
            RETURNING 1
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .bind(revoked_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(if changed.is_some() {
            RoleAssignmentRevocationResult::Revoked
        } else {
            RoleAssignmentRevocationResult::AlreadyRevoked
        })
    }

    pub(crate) async fn tx_find_system_role(
        &mut self,
        organization_id: OrganizationId,
        system_role: SystemRole,
    ) -> Result<Option<RoleId>, StoreError> {
        let role_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM roles WHERE organization_id = $1 AND system_key = $2",
        )
        .bind(organization_id.as_uuid())
        .bind(system_role.as_str())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(role_id.map(RoleId::from_uuid))
    }
}
