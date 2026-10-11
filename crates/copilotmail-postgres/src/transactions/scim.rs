use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_scim_token(
        &mut self,
        credential: &ScimCredential,
        token_hash: &[u8],
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO scim_tokens(
                id, organization_id, actor_principal_id, token_hash,
                label, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(credential.token_id.as_uuid())
        .bind(credential.organization_id.as_uuid())
        .bind(credential.actor_principal_id.as_uuid())
        .bind(token_hash)
        .bind(&credential.label)
        .bind(created_at)
        .bind(credential.expires_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_revoke_scim_token(
        &mut self,
        organization_id: OrganizationId,
        token_id: ScimTokenId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE scim_tokens
               SET status = 'revoked',
                   revoked_at = $3
             WHERE id = $1
               AND organization_id = $2
               AND status = 'active'
            "#,
        )
        .bind(token_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn tx_find_scim_user_by_external_id(
        &mut self,
        organization_id: OrganizationId,
        external_id: &str,
    ) -> Result<Option<ExistingScimUser>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 2))")
            .bind(format!("{organization_id}:{external_id}"))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        let row = sqlx::query_as::<_, ScimUserRow>(
            r#"
            SELECT directory.organization_id,
                   directory.principal_id,
                   directory.external_id,
                   directory.user_name,
                   principal.display_name,
                   principal.status = 'active' AS active,
                   directory.version,
                   directory.created_at,
                   directory.updated_at
              FROM scim_users AS directory
              JOIN principals AS principal ON principal.id = directory.principal_id
             WHERE directory.organization_id = $1
               AND directory.external_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_user_from_row))
    }

    pub(crate) async fn tx_insert_scim_user(
        &mut self,
        user: &ExistingScimUser,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO scim_users(
                principal_id, organization_id, external_id, user_name,
                version, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.external_id)
        .bind(&user.user_name)
        .bind(user.version)
        .bind(user.created_at)
        .bind(user.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_adopt_scim_user(
        &mut self,
        user: &ExistingScimUser,
    ) -> Result<(), StoreError> {
        let status = if user.active { "active" } else { "suspended" };
        let result = sqlx::query(
            r#"
            UPDATE principals
               SET display_name = $3,
                   status = $4,
                   updated_at = $5
             WHERE id = $1
               AND organization_id = $2
               AND kind = 'human'
               AND status <> 'retired'
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.display_name)
        .bind(status)
        .bind(user.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if result.rows_affected() != 1 {
            return Err(StoreError::MissingReference);
        }
        self.insert_scim_user(user).await
    }

    pub(crate) async fn tx_replace_scim_user(
        &mut self,
        user: &ExistingScimUser,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        let status = if user.active { "active" } else { "suspended" };
        let updated = sqlx::query(
            r#"
            UPDATE scim_users
               SET user_name = $3,
                   version = $4,
                   updated_at = $5
             WHERE principal_id = $1
               AND organization_id = $2
               AND version = $6
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.user_name)
        .bind(user.version)
        .bind(user.updated_at)
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if updated.rows_affected() != 1 {
            return Ok(false);
        }
        let principal = sqlx::query(
            r#"
            UPDATE principals
               SET display_name = $3,
                   status = $4,
                   updated_at = $5
             WHERE id = $1
               AND organization_id = $2
               AND kind = 'human'
               AND status <> 'retired'
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.display_name)
        .bind(status)
        .bind(user.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if principal.rows_affected() != 1 {
            return Err(StoreError::MissingReference);
        }
        Ok(true)
    }

    pub(crate) async fn tx_find_scim_group_by_external_id(
        &mut self,
        organization_id: OrganizationId,
        external_id: &str,
    ) -> Result<Option<ExistingScimGroup>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 3))")
            .bind(format!("{organization_id}:{external_id}"))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        let row = sqlx::query_as::<_, ScimGroupRow>(
            r#"
            SELECT directory.organization_id,
                   directory.id AS group_id,
                   directory.external_id,
                   directory.display_name,
                   COALESCE(
                     array_agg(member.principal_id ORDER BY member.principal_id)
                       FILTER (WHERE member.principal_id IS NOT NULL),
                     ARRAY[]::uuid[]
                   ) AS members,
                   directory.version,
                   directory.created_at,
                   directory.updated_at
              FROM scim_groups AS directory
              LEFT JOIN scim_group_members AS member ON member.group_id = directory.id
             WHERE directory.organization_id = $1
               AND directory.external_id = $2
             GROUP BY directory.id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_group_from_row))
    }

    pub(crate) async fn tx_insert_scim_group(
        &mut self,
        group: &ExistingScimGroup,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO scim_groups(
                id, organization_id, external_id, display_name,
                version, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(group.group_id.as_uuid())
        .bind(group.organization_id.as_uuid())
        .bind(&group.external_id)
        .bind(&group.display_name)
        .bind(group.version)
        .bind(group.created_at)
        .bind(group.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        insert_scim_group_members(self.transaction()?, group).await
    }

    pub(crate) async fn tx_replace_scim_group(
        &mut self,
        group: &ExistingScimGroup,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE scim_groups
               SET display_name = $3,
                   version = $4,
                   updated_at = $5
             WHERE id = $1
               AND organization_id = $2
               AND version = $6
            "#,
        )
        .bind(group.group_id.as_uuid())
        .bind(group.organization_id.as_uuid())
        .bind(&group.display_name)
        .bind(group.version)
        .bind(group.updated_at)
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if result.rows_affected() != 1 {
            return Ok(false);
        }
        sqlx::query("DELETE FROM scim_group_members WHERE group_id = $1")
            .bind(group.group_id.as_uuid())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        insert_scim_group_members(self.transaction()?, group).await?;
        Ok(true)
    }

    pub(crate) async fn tx_delete_scim_group(
        &mut self,
        organization_id: OrganizationId,
        group_id: ScimGroupId,
        expected_version: Option<i64>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM scim_groups
             WHERE id = $1
               AND organization_id = $2
               AND ($3::bigint IS NULL OR version = $3)
            "#,
        )
        .bind(group_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }
}
