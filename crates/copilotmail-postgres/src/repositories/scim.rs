use crate::*;

#[async_trait]
impl ScimDirectoryQueryStore for PgControlPlaneStore {
    async fn authenticate_scim_token(
        &self,
        token_hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<ScimCredential>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct CredentialRow {
            id: Uuid,
            organization_id: Uuid,
            actor_principal_id: Uuid,
            label: String,
            expires_at: Option<DateTime<Utc>>,
        }
        if token_hash.len() != 32 {
            return Ok(None);
        }
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            UPDATE scim_tokens AS token
               SET last_used_at = $2
              FROM principals AS actor
             WHERE token.token_hash = $1
               AND token.status = 'active'
               AND (token.expires_at IS NULL OR token.expires_at > $2)
               AND actor.id = token.actor_principal_id
               AND actor.organization_id = token.organization_id
               AND actor.status = 'active'
            RETURNING token.id, token.organization_id, token.actor_principal_id,
                      token.label, token.expires_at
            "#,
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ScimCredential {
            token_id: ScimTokenId::from_uuid(row.id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            actor_principal_id: PrincipalId::from_uuid(row.actor_principal_id),
            label: row.label,
            expires_at: row.expires_at,
        }))
    }

    async fn get_scim_user(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<ExistingScimUser>, StoreError> {
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
               AND directory.principal_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_user_from_row))
    }

    async fn list_scim_users(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        user_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimUser>, i64), StoreError> {
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
              FROM scim_users
             WHERE organization_id = $1
               AND ($2::text IS NULL OR external_id = $2)
               AND ($3::text IS NULL OR lower(user_name) = lower($3))
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(user_name)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)?;
        let rows = sqlx::query_as::<_, ScimUserRow>(
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
               AND ($2::text IS NULL OR directory.external_id = $2)
               AND ($3::text IS NULL OR lower(directory.user_name) = lower($3))
             ORDER BY directory.created_at, directory.principal_id
             OFFSET $4 LIMIT $5
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(user_name)
        .bind(offset)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok((rows.into_iter().map(scim_user_from_row).collect(), total))
    }

    async fn get_scim_group(
        &self,
        organization_id: OrganizationId,
        group_id: ScimGroupId,
    ) -> Result<Option<ExistingScimGroup>, StoreError> {
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
               AND directory.id = $2
             GROUP BY directory.id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(group_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_group_from_row))
    }

    async fn list_scim_groups(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        display_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimGroup>, i64), StoreError> {
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
              FROM scim_groups
             WHERE organization_id = $1
               AND ($2::text IS NULL OR external_id = $2)
               AND ($3::text IS NULL OR lower(display_name) = lower($3))
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(display_name)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)?;
        let rows = sqlx::query_as::<_, ScimGroupRow>(
            r#"
            WITH page AS (
                SELECT *
                  FROM scim_groups
                 WHERE organization_id = $1
                   AND ($2::text IS NULL OR external_id = $2)
                   AND ($3::text IS NULL OR lower(display_name) = lower($3))
                 ORDER BY created_at, id
                 OFFSET $4 LIMIT $5
            )
            SELECT page.organization_id,
                   page.id AS group_id,
                   page.external_id,
                   page.display_name,
                   COALESCE(
                     array_agg(member.principal_id ORDER BY member.principal_id)
                       FILTER (WHERE member.principal_id IS NOT NULL),
                     ARRAY[]::uuid[]
                   ) AS members,
                   page.version,
                   page.created_at,
                   page.updated_at
              FROM page
              LEFT JOIN scim_group_members AS member ON member.group_id = page.id
             GROUP BY page.id, page.organization_id, page.external_id,
                      page.display_name, page.version, page.created_at, page.updated_at
             ORDER BY page.created_at, page.id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(display_name)
        .bind(offset)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok((rows.into_iter().map(scim_group_from_row).collect(), total))
    }
}
