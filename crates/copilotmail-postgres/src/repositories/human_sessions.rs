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

    async fn list_human_sessions_for_principal(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<Vec<HumanSessionListItem>, StoreError> {
        // Bounded at the database so a session-accumulation anomaly cannot
        // amplify the self-service device list beyond the service bound.
        let rows = sqlx::query_as::<_, HumanSessionListItemRow>(
            r#"
            SELECT session.id, session.client_key,
                   session.created_at, session.expires_at
              FROM human_browser_sessions AS session
              JOIN principals AS principal
                ON principal.id = session.principal_id
               AND principal.organization_id = session.organization_id
             WHERE session.organization_id = $1
               AND session.principal_id = $2
               AND session.created_at <= $3
               AND session.expires_at > $3
               AND session.upstream_expires_at > $3
               AND principal.kind = 'human'
               AND principal.status = 'active'
               AND NOT EXISTS (
                   SELECT 1
                     FROM human_browser_session_revocations AS revocation
                    WHERE revocation.session_id = session.id
               )
             ORDER BY session.created_at DESC, session.id DESC
             LIMIT 64
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.iter()
            .map(|row| {
                Ok(HumanSessionListItem {
                    session_id: HumanSessionId::from_uuid(row.id),
                    client_key: human_session_client_from_str(&row.client_key)?,
                    created_at: row.created_at,
                    expires_at: row.expires_at,
                })
            })
            .collect()
    }
}

#[derive(sqlx::FromRow)]
struct HumanSessionListItemRow {
    id: Uuid,
    client_key: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}
