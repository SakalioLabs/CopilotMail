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
        // The session row is append-only. Revocation is a separate unique row.
        // FOR KEY SHARE does not conflict with the FOR SHARE held by an
        // in-flight begin_authorized request, so this revocation can commit
        // and fail that request at its commit recheck. FOR UPDATE cannot:
        // it waits until the request ends. Two revocations of one session
        // serialize on the unique session_id constraint below.
        let session = sqlx::query_as::<_, HumanSessionRow>(
            r#"
            SELECT id, organization_id, principal_id, login_transaction_id,
                   client_key, oidc_jti, upstream_expires_at,
                   created_at, expires_at
              FROM human_browser_sessions
             WHERE token_sha256 = $1
             FOR KEY SHARE
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

        let inserted = sqlx::query(
            r#"
            INSERT INTO human_browser_session_revocations(
                id, session_id, organization_id, principal_id, cause, revoked_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (session_id) DO NOTHING
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
        if inserted.rows_affected() == 0 {
            let Some(existing) = sqlx::query_as::<_, RevocationRow>(
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
            else {
                return Err(StoreError::Unavailable);
            };
            return Ok(Some(HumanSessionRevocationResult {
                session,
                revocation_id: HumanSessionRevocationId::from_uuid(existing.id),
                cause: human_session_revocation_cause_from_str(&existing.cause)?,
                revoked_at: existing.revoked_at,
                newly_revoked: false,
            }));
        }
        Ok(Some(HumanSessionRevocationResult {
            session,
            revocation_id,
            cause,
            revoked_at,
            newly_revoked: true,
        }))
    }

    pub(crate) async fn tx_revoke_active_human_browser_sessions(
        &mut self,
        write: &RevokeActiveHumanSessions,
    ) -> Result<Vec<HumanSessionRevocationResult>, StoreError> {
        let rows = sqlx::query_as::<_, HumanSessionRow>(
            r#"
            SELECT session.id, session.organization_id, session.principal_id,
                   session.login_transaction_id, session.client_key, session.oidc_jti,
                   session.upstream_expires_at, session.created_at, session.expires_at
              FROM human_browser_sessions AS session
             WHERE session.organization_id = $1
               AND session.principal_id = $2
               AND session.expires_at > clock_timestamp()
               AND ($3::uuid IS NULL OR session.id <> $3)
               AND NOT EXISTS (
                   SELECT 1
                     FROM human_browser_session_revocations AS revocation
                    WHERE revocation.session_id = session.id
               )
             ORDER BY session.id
               FOR SHARE OF session
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(write.excluded_session_id.map(HumanSessionId::as_uuid))
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let mut revoked = Vec::with_capacity(rows.len());
        for row in rows {
            let session = human_session_from_row(row)?;
            let revocation_id = HumanSessionRevocationId::new();
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
            .bind(human_session_revocation_cause(write.cause))
            .bind(write.revoked_at)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
            revoked.push(HumanSessionRevocationResult {
                session,
                revocation_id,
                cause: write.cause,
                revoked_at: write.revoked_at,
                newly_revoked: true,
            });
        }
        Ok(revoked)
    }

    pub(crate) async fn tx_human_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        let found = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT principal.id
              FROM principals AS principal
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'human'
             FOR SHARE OF principal
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(found.is_some())
    }

    pub(crate) async fn tx_lock_exact_dual_approval(
        &mut self,
        proof: &ApprovalProof,
    ) -> Result<bool, StoreError> {
        let locked = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT request.id
              FROM approval_requests AS request
              JOIN policy_versions AS version
                ON version.id = request.policy_version_id
               AND version.organization_id = request.organization_id
             WHERE request.id = $1
               AND request.organization_id = $2
               AND request.requested_by = $3
               AND request.action = $4
               AND version.action = request.action
               AND request.resource_kind = $5
               AND request.resource_id = $6
               AND request.status = 'approved'
               AND request.required_approvals >= 2
               AND request.expires_at > clock_timestamp()
               AND NOT EXISTS (
                   SELECT 1
                     FROM approval_steps AS step
                    WHERE step.approval_request_id = request.id
                      AND step.organization_id = request.organization_id
                      AND step.decision = 'rejected'
               )
               AND (
                   SELECT count(DISTINCT step.approver_principal_id)
                     FROM approval_steps AS step
                    WHERE step.approval_request_id = request.id
                      AND step.organization_id = request.organization_id
                      AND step.decision = 'approved'
                      AND step.approver_principal_id <> request.requested_by
               ) >= request.required_approvals
             FOR UPDATE OF request
            "#,
        )
        .bind(proof.request_id.as_uuid())
        .bind(proof.organization_id.as_uuid())
        .bind(proof.requested_by.as_uuid())
        .bind(proof.action.as_str())
        .bind(&proof.resource_kind)
        .bind(&proof.resource_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(locked.is_some())
    }

    pub(crate) async fn tx_insert_human_session_governance_execution(
        &mut self,
        write: &HumanSessionGovernanceExecutionWrite,
    ) -> Result<HumanSessionGovernanceExecution, StoreError> {
        let inserted = sqlx::query(
            r#"
            INSERT INTO human_session_governance_executions(
                approval_request_id, organization_id, target_principal_id,
                requested_by, excluded_session_id, executed_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (approval_request_id) DO NOTHING
            "#,
        )
        .bind(write.approval_request_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.target_principal_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.excluded_session_id.map(HumanSessionId::as_uuid))
        .bind(write.executed_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if inserted.rows_affected() == 1 {
            return Ok(HumanSessionGovernanceExecution::Inserted);
        }

        #[derive(sqlx::FromRow)]
        struct ExistingExecution {
            target_principal_id: Uuid,
            requested_by: Uuid,
        }

        let existing = sqlx::query_as::<_, ExistingExecution>(
            r#"
            SELECT target_principal_id, requested_by
              FROM human_session_governance_executions
             WHERE approval_request_id = $1
             FOR SHARE
            "#,
        )
        .bind(write.approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(existing) = existing else {
            return Err(StoreError::Unavailable);
        };
        Ok(HumanSessionGovernanceExecution::AlreadyExecuted {
            target_principal_id: PrincipalId::from_uuid(existing.target_principal_id),
            requested_by: PrincipalId::from_uuid(existing.requested_by),
        })
    }
}
