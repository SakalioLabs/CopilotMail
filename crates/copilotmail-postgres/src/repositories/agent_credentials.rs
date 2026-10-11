use crate::*;

#[async_trait]
impl AgentCredentialStore for PgControlPlaneStore {
    async fn get_agent_credential(
        &self,
        client_id: PrincipalId,
        credential_id: CredentialId,
        now: DateTime<Utc>,
    ) -> Result<Option<ExistingAgentCredential>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct CredentialRow {
            credential_id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            public_key_pem: String,
            allowed_scopes: Vec<String>,
            expires_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            SELECT credential.id AS credential_id,
                   principal.organization_id,
                   principal.id AS principal_id,
                   key.public_key_pem,
                   key.allowed_scopes,
                   credential.expires_at
              FROM credentials AS credential
              JOIN agent_private_key_credentials AS key
                ON key.credential_id = credential.id
              JOIN principals AS principal ON principal.id = credential.principal_id
             WHERE credential.id = $1
               AND credential.principal_id = $2
               AND credential.kind = 'private_key_jwt'
               AND credential.status = 'active'
               AND credential.expires_at > $3
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(client_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingAgentCredential {
            credential_id: CredentialId::from_uuid(row.credential_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            public_key_pem: row.public_key_pem,
            allowed_scopes: row
                .allowed_scopes
                .into_iter()
                .filter_map(|scope| permission_from_str(&scope))
                .collect(),
            expires_at: row.expires_at,
        }))
    }

    async fn consume_agent_assertion(
        &self,
        credential_id: CredentialId,
        jti_hash: &[u8],
        assertion_expires_at: DateTime<Utc>,
        source: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        if jti_hash.len() != 32
            || source.is_some_and(|source| {
                source.is_empty() || source.len() > 200 || source.chars().any(char::is_control)
            })
            || assertion_expires_at <= now
        {
            return Ok(false);
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        let active: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT true
              FROM credentials
             WHERE id = $1
               AND status = 'active'
               AND expires_at > $2
             FOR UPDATE
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if active.is_none() {
            return Ok(false);
        }
        let inserted = sqlx::query(
            r#"
            INSERT INTO oauth_client_assertion_jtis(
                credential_id, jti_hash, expires_at, created_at
            )
            VALUES ($1, $2, $3, $4)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(jti_hash)
        .bind(assertion_expires_at)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if inserted.rows_affected() != 1 {
            return Ok(false);
        }
        sqlx::query(
            r#"
            UPDATE credentials
               SET last_used_at = $2,
                   last_used_source = $3
             WHERE id = $1
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(now)
        .bind(source)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(true)
    }

    async fn authenticate_agent_mtls(
        &self,
        client_id: PrincipalId,
        certificate_fingerprint: &str,
        source: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<ExistingAgentMtlsCredential>, StoreError> {
        if certificate_fingerprint.len() != 64
            || !certificate_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || source.is_some_and(|source| {
                source.is_empty() || source.len() > 200 || source.chars().any(char::is_control)
            })
        {
            return Ok(None);
        }
        #[derive(sqlx::FromRow)]
        struct CredentialRow {
            credential_id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            certificate_fingerprint: String,
            allowed_scopes: Vec<String>,
            expires_at: DateTime<Utc>,
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            SELECT credential.id AS credential_id,
                   principal.organization_id,
                   principal.id AS principal_id,
                   credential.public_fingerprint AS certificate_fingerprint,
                   mtls.allowed_scopes,
                   credential.expires_at
              FROM credentials AS credential
              JOIN agent_mtls_credentials AS mtls
                ON mtls.credential_id = credential.id
              JOIN principals AS principal ON principal.id = credential.principal_id
             WHERE credential.principal_id = $1
               AND credential.public_fingerprint = $2
               AND credential.kind = 'mtls'
               AND credential.status = 'active'
               AND credential.expires_at > $3
               AND principal.kind = 'agent'
               AND principal.status = 'active'
             FOR UPDATE OF credential
            "#,
        )
        .bind(client_id.as_uuid())
        .bind(certificate_fingerprint.to_ascii_lowercase())
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        sqlx::query(
            r#"
            UPDATE credentials
               SET last_used_at = $2,
                   last_used_source = $3
             WHERE id = $1
            "#,
        )
        .bind(row.credential_id)
        .bind(now)
        .bind(source)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(Some(ExistingAgentMtlsCredential {
            credential_id: CredentialId::from_uuid(row.credential_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            certificate_fingerprint: row.certificate_fingerprint,
            allowed_scopes: row
                .allowed_scopes
                .into_iter()
                .filter_map(|scope| permission_from_str(&scope))
                .collect(),
            expires_at: row.expires_at,
        }))
    }

    async fn validate_agent_token_binding(
        &self,
        principal_id: PrincipalId,
        credential_id: CredentialId,
        scopes: &BTreeSet<Permission>,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        if scopes.is_empty()
            || !scopes
                .iter()
                .all(|scope| Permission::AGENT_TOKEN.contains(scope))
        {
            return Ok(false);
        }
        let requested = scopes
            .iter()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>();
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM credentials AS credential
                  JOIN principals AS principal ON principal.id = credential.principal_id
                 WHERE credential.id = $1
                   AND credential.principal_id = $2
                   AND credential.kind IN ('private_key_jwt', 'mtls')
                   AND credential.status = 'active'
                   AND credential.expires_at > $4
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM agent_private_key_credentials AS key
                            WHERE key.credential_id = credential.id
                              AND key.allowed_scopes @> $3::text[]
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM agent_mtls_credentials AS mtls
                            WHERE mtls.credential_id = credential.id
                              AND mtls.allowed_scopes @> $3::text[]
                       )
                   )
            )
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(requested)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }
}
