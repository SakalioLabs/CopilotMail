use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_agent_profile(
        &mut self,
        profile: &AgentProfile,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO agent_profiles(
                principal_id, owner_principal_id, environment, risk_level,
                purpose, review_due_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(profile.principal_id().as_uuid())
        .bind(profile.owner_principal_id().as_uuid())
        .bind(agent_environment(profile.environment()))
        .bind(agent_risk_level(profile.risk_level()))
        .bind(profile.purpose())
        .bind(profile.review_due_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_insert_agent_credential(
        &mut self,
        credential: &AgentCredential,
        public_key_pem: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO credentials(
                id, principal_id, kind, public_fingerprint, status,
                created_at, expires_at
            )
            VALUES ($1, $2, 'private_key_jwt', $3, 'active', $4, $5)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(credential.principal_id().as_uuid())
        .bind(credential.fingerprint())
        .bind(credential.created_at())
        .bind(credential.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let scopes = credential
            .allowed_scopes()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>();
        sqlx::query(
            r#"
            INSERT INTO agent_private_key_credentials(
                credential_id, public_key_pem, allowed_scopes
            )
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(public_key_pem)
        .bind(scopes)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_insert_agent_mtls_credential(
        &mut self,
        credential: &AgentMtlsCredential,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO credentials(
                id, principal_id, kind, public_fingerprint, status,
                created_at, expires_at
            )
            VALUES ($1, $2, 'mtls', $3, 'active', $4, $5)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(credential.principal_id().as_uuid())
        .bind(credential.certificate_fingerprint())
        .bind(credential.created_at())
        .bind(credential.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let scopes = credential
            .allowed_scopes()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>();
        sqlx::query(
            r#"
            INSERT INTO agent_mtls_credentials(credential_id, allowed_scopes)
            VALUES ($1, $2)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(scopes)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_active_agent_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'agent'
                   AND status = 'active'
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_revoke_agent_credential(
        &mut self,
        organization_id: OrganizationId,
        credential_id: CredentialId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE credentials AS credential
               SET status = 'revoked',
                   revoked_at = $3
              FROM principals AS principal
             WHERE credential.id = $1
               AND credential.status = 'active'
               AND principal.id = credential.principal_id
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }
}
