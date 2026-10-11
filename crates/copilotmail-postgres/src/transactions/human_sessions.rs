use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_human_session(
        &mut self,
        write: &HumanSessionWrite,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO human_browser_sessions(
                id, organization_id, principal_id, login_transaction_id,
                client_key, token_sha256, oidc_token_sha256, oidc_jti,
                upstream_expires_at, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(write.session_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(write.login_transaction_id.as_uuid())
        .bind(human_session_client(write.client_key))
        .bind(write.token_sha256.as_slice())
        .bind(write.oidc_token_sha256.as_slice())
        .bind(write.oidc_jti.as_deref())
        .bind(write.upstream_expires_at)
        .bind(write.created_at)
        .bind(write.expires_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_revoke_human_session_by_token_sha256(
        &mut self,
        token_sha256: &[u8; 32],
        revocation_id: HumanSessionRevocationId,
        cause: HumanSessionRevocationCause,
        revoked_at: DateTime<Utc>,
    ) -> Result<Option<HumanSessionRevocationResult>, StoreError> {
        let session = sqlx::query_as::<_, HumanSessionRow>(
            r#"
            SELECT id, organization_id, principal_id, login_transaction_id,
                   client_key, oidc_jti, upstream_expires_at,
                   created_at, expires_at
              FROM human_browser_sessions
             WHERE token_sha256 = $1
             FOR UPDATE
            "#,
        )
        .bind(token_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(session) = session else {
            return Ok(None);
        };
        let session = human_session_from_row(session)?;

        #[derive(sqlx::FromRow)]
        struct RevocationRow {
            id: Uuid,
            cause: String,
            revoked_at: DateTime<Utc>,
        }

        if let Some(existing) = sqlx::query_as::<_, RevocationRow>(
            r#"
            SELECT id, cause, revoked_at
              FROM human_browser_session_revocations
             WHERE session_id = $1
            "#,
        )
        .bind(session.session_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        {
            return Ok(Some(HumanSessionRevocationResult {
                session,
                revocation_id: HumanSessionRevocationId::from_uuid(existing.id),
                cause: human_session_revocation_cause_from_str(&existing.cause)?,
                revoked_at: existing.revoked_at,
                newly_revoked: false,
            }));
        }

        sqlx::query(
            r#"
            INSERT INTO human_browser_session_revocations(
                id, session_id, organization_id, principal_id, cause, revoked_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(revocation_id.as_uuid())
        .bind(session.session_id.as_uuid())
        .bind(session.organization_id.as_uuid())
        .bind(session.principal_id.as_uuid())
        .bind(human_session_revocation_cause(cause))
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(Some(HumanSessionRevocationResult {
            session,
            revocation_id,
            cause,
            revoked_at,
            newly_revoked: true,
        }))
    }
}
