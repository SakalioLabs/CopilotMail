use crate::*;

#[async_trait]
impl HumanSessionQueryStore for PgControlPlaneStore {
    async fn resolve_human_session_by_token_sha256(
        &self,
        token_sha256: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<Option<HumanSessionRecord>, StoreError> {
        let row = sqlx::query_as::<_, HumanSessionRow>(
            r#"
            SELECT session.id, session.organization_id, session.principal_id,
                   session.login_transaction_id, session.client_key,
                   session.oidc_jti, session.upstream_expires_at,
                   session.created_at, session.expires_at
              FROM human_browser_sessions AS session
              JOIN principals AS principal
                ON principal.id = session.principal_id
               AND principal.organization_id = session.organization_id
             WHERE session.token_sha256 = $1
               AND session.created_at <= $2
               AND session.expires_at > $2
               AND session.upstream_expires_at > $2
               AND principal.kind = 'human'
               AND principal.status = 'active'
               AND NOT EXISTS (
                   SELECT 1
                     FROM human_browser_session_revocations AS revocation
                    WHERE revocation.session_id = session.id
               )
            "#,
        )
        .bind(token_sha256.as_slice())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(human_session_from_row).transpose()
    }
}
