use crate::*;

impl PgControlPlaneTransaction {
    /// Locks the request credential row for the transaction lifetime and keeps
    /// the binding so `tx_commit` can re-validate expiry and revocation against
    /// the database clock immediately before commit.
    pub(crate) async fn tx_bind_request_authentication(
        &mut self,
        context: &AuthorizationContext,
    ) -> Result<(), StoreError> {
        let Some(binding) = context.authentication.clone() else {
            return Ok(());
        };
        match &binding {
            AuthenticationBinding::HumanSession { session_id } => {
                let locked = sqlx::query_scalar::<_, Uuid>(
                    r#"
                    SELECT session.id
                      FROM human_browser_sessions AS session
                      JOIN principals AS principal
                        ON principal.id = session.principal_id
                       AND principal.organization_id = session.organization_id
                     WHERE session.id = $1
                       AND session.organization_id = $2
                       AND session.principal_id = $3
                       AND session.expires_at > clock_timestamp()
                       AND principal.kind = 'human'
                       AND principal.status = 'active'
                       AND NOT EXISTS (
                           SELECT 1
                             FROM human_browser_session_revocations AS revocation
                            WHERE revocation.session_id = session.id
                       )
                     FOR SHARE OF session
                    "#,
                )
                .bind(session_id.as_uuid())
                .bind(context.organization_id.as_uuid())
                .bind(context.principal_id.as_uuid())
                .fetch_optional(&mut **self.transaction()?)
                .await
                .map_err(map_store_error)?;
                if locked.is_none() {
                    return Err(StoreError::AuthenticationInvalid);
                }
            }
            AuthenticationBinding::AgentCredential {
                credential_id,
                expires_at,
                ..
            } => {
                let locked = sqlx::query_scalar::<_, Uuid>(
                    r#"
                    SELECT credential.id
                      FROM credentials AS credential
                      JOIN principals AS principal
                        ON principal.id = credential.principal_id
                       AND principal.organization_id = $3
                     WHERE credential.id = $1
                       AND credential.principal_id = $2
                       AND credential.kind IN ('private_key_jwt', 'mtls')
                       AND credential.status = 'active'
                       AND credential.revoked_at IS NULL
                       AND principal.kind = 'agent'
                       AND principal.status = 'active'
                       AND (
                           credential.expires_at IS NULL
                           OR credential.expires_at > clock_timestamp()
                       )
                     FOR UPDATE OF principal, credential
                    "#,
                )
                .bind(credential_id.as_uuid())
                .bind(context.principal_id.as_uuid())
                .bind(context.organization_id.as_uuid())
                .fetch_optional(&mut **self.transaction()?)
                .await
                .map_err(map_store_error)?;
                if locked.is_none()
                    || !Self::tx_still_unexpired(self.transaction()?, *expires_at).await?
                {
                    return Err(StoreError::AuthenticationInvalid);
                }
            }
            AuthenticationBinding::ScimToken { token_id } => {
                let locked = sqlx::query_scalar::<_, Uuid>(
                    r#"
                    SELECT token.id
                      FROM scim_tokens AS token
                      JOIN principals AS principal
                        ON principal.id = token.actor_principal_id
                       AND principal.organization_id = $2
                     WHERE token.id = $1
                       AND token.organization_id = $2
                       AND token.actor_principal_id = $3
                       AND token.status = 'active'
                       AND token.revoked_at IS NULL
                       AND principal.kind = 'human'
                       AND principal.status = 'active'
                       AND (
                           token.expires_at IS NULL
                           OR token.expires_at > clock_timestamp()
                       )
                     FOR UPDATE OF principal, token
                    "#,
                )
                .bind(token_id.as_uuid())
                .bind(context.organization_id.as_uuid())
                .bind(context.principal_id.as_uuid())
                .fetch_optional(&mut **self.transaction()?)
                .await
                .map_err(map_store_error)?;
                if locked.is_none() {
                    return Err(StoreError::AuthenticationInvalid);
                }
            }
            AuthenticationBinding::HumanBearer { expires_at } => {
                if !Self::tx_still_unexpired(self.transaction()?, *expires_at).await? {
                    return Err(StoreError::AuthenticationInvalid);
                }
            }
        }
        self.bound_authentication = Some(binding);
        Ok(())
    }

    async fn tx_still_unexpired(
        transaction: &mut Transaction<'static, Postgres>,
        expires_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>("SELECT $1::timestamptz > clock_timestamp()")
            .bind(expires_at)
            .fetch_one(&mut **transaction)
            .await
            .map_err(map_store_error)
    }

    /// Rechecks the bound credential against the database clock immediately
    /// before commit. Human sessions are share-locked, so a concurrent
    /// revocation or principal suspension can commit and fail this check.
    /// The principal row itself is not locked here: an update of principal
    /// status must not deadlock with the session fence.
    pub(crate) async fn tx_bound_authentication_still_valid(&mut self) -> Result<bool, StoreError> {
        let Some(binding) = self.bound_authentication.clone() else {
            return Ok(true);
        };
        match binding {
            AuthenticationBinding::HumanSession { session_id } => sqlx::query_scalar::<_, bool>(
                r#"
                    SELECT EXISTS (
                        SELECT 1
                          FROM human_browser_sessions AS session
                          JOIN principals AS principal
                            ON principal.id = session.principal_id
                           AND principal.organization_id = session.organization_id
                         WHERE session.id = $1
                           AND session.expires_at > clock_timestamp()
                           AND principal.kind = 'human'
                           AND principal.status = 'active'
                           AND NOT EXISTS (
                               SELECT 1
                                 FROM human_browser_session_revocations AS revocation
                                WHERE revocation.session_id = session.id
                           )
                    )
                    "#,
            )
            .bind(session_id.as_uuid())
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(map_store_error),
            AuthenticationBinding::AgentCredential {
                credential_id,
                expires_at,
                ..
            } => {
                let credential_valid = sqlx::query_scalar::<_, bool>(
                    r#"
                    SELECT EXISTS (
                        SELECT 1
                          FROM credentials AS credential
                          JOIN principals AS principal
                            ON principal.id = credential.principal_id
                         WHERE credential.id = $1
                           AND credential.status = 'active'
                           AND credential.revoked_at IS NULL
                           AND principal.kind = 'agent'
                           AND principal.status = 'active'
                           AND (
                               credential.expires_at IS NULL
                               OR credential.expires_at > clock_timestamp()
                           )
                    )
                    "#,
                )
                .bind(credential_id.as_uuid())
                .fetch_one(&mut **self.transaction()?)
                .await
                .map_err(map_store_error)?;
                if !credential_valid {
                    return Ok(false);
                }
                Self::tx_still_unexpired(self.transaction()?, expires_at).await
            }
            AuthenticationBinding::ScimToken { token_id } => sqlx::query_scalar::<_, bool>(
                r#"
                    SELECT EXISTS (
                        SELECT 1
                          FROM scim_tokens AS token
                          JOIN principals AS principal
                            ON principal.id = token.actor_principal_id
                         WHERE token.id = $1
                           AND token.status = 'active'
                           AND token.revoked_at IS NULL
                           AND principal.kind = 'human'
                           AND principal.status = 'active'
                           AND (
                               token.expires_at IS NULL
                               OR token.expires_at > clock_timestamp()
                           )
                    )
                    "#,
            )
            .bind(token_id.as_uuid())
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(map_store_error),
            AuthenticationBinding::HumanBearer { expires_at } => {
                Self::tx_still_unexpired(self.transaction()?, expires_at).await
            }
        }
    }

    pub(crate) async fn tx_authorize_human_permission(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        permission: Permission,
    ) -> Result<bool, StoreError> {
        self.lock_human_permission(organization_id, principal_id, permission)
            .await
    }

    pub(crate) async fn tx_active_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
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

    pub(crate) async fn tx_active_agent_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
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

    pub(crate) async fn tx_active_human_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
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
}
