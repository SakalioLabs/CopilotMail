use crate::*;

#[async_trait]
impl ExternalIdentityResolver for PgControlPlaneStore {
    async fn resolve_human_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<PrincipalId>, StoreError> {
        let principal_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT principal.id
              FROM human_profiles AS profile
              JOIN principals AS principal ON principal.id = profile.principal_id
             WHERE profile.identity_provider = $1
               AND profile.identity_subject = $2
               AND principal.kind = 'human'
               AND principal.status = 'active'
            "#,
        )
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(principal_id.map(PrincipalId::from_uuid))
    }
}
