//! Transaction-local authorization. Lock ownership stays with the unit of work.

use crate::*;

impl PgControlPlaneTransaction {
    pub(super) async fn lock_human_permission(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        permission: Permission,
    ) -> Result<bool, StoreError> {
        self.lock_principal_permission(organization_id, principal_id, permission, true)
            .await
    }

    pub(super) async fn lock_principal_permission(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        permission: Permission,
        human_only: bool,
    ) -> Result<bool, StoreError> {
        // Match governed role revocation's tenant lock BEFORE locking actor
        // assignments. Otherwise two administrators revoking each other can
        // each hold the row the other needs (or invert the advisory/row order).
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 7))")
            .bind(organization_id.to_string())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        let assignments = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT assignment.id
              FROM principals AS principal
              JOIN role_assignments AS assignment
                ON assignment.organization_id = principal.organization_id
               AND assignment.principal_id = principal.id
              JOIN roles AS role
                ON role.id = assignment.role_id
               AND role.organization_id = assignment.organization_id
              JOIN role_permissions AS capability ON capability.role_id = role.id
             WHERE principal.organization_id = $1 AND principal.id = $2
               AND (NOT $4::boolean OR principal.kind = 'human')
               AND principal.status = 'active'
               AND assignment.revoked_at IS NULL
               AND assignment.valid_from <= clock_timestamp()
               AND (assignment.valid_until IS NULL OR assignment.valid_until > clock_timestamp())
               AND capability.permission = $3
             ORDER BY assignment.id
             FOR UPDATE OF principal, assignment, role, capability
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(permission.as_str())
        .bind(human_only)
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if assignments.is_empty() {
            return Ok(false);
        }
        // Lock waits can cross an assignment expiry. Only recheck rows that
        // this transaction actually locked, never a concurrently added grant.
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM principals AS principal
                JOIN role_assignments AS assignment
                  ON assignment.principal_id = principal.id
                 AND assignment.organization_id = principal.organization_id
                JOIN roles AS role ON role.id = assignment.role_id
                 AND role.organization_id = assignment.organization_id
                JOIN role_permissions AS capability ON capability.role_id = role.id
                WHERE principal.organization_id = $1 AND principal.id = $2
                  AND (NOT $5::boolean OR principal.kind = 'human')
                  AND principal.status = 'active'
                  AND assignment.id = ANY($4)
                  AND assignment.revoked_at IS NULL
                  AND assignment.valid_from <= clock_timestamp()
                  AND (assignment.valid_until IS NULL OR assignment.valid_until > clock_timestamp())
                  AND capability.permission = $3
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(permission.as_str())
        .bind(assignments)
        .bind(human_only)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }
}
