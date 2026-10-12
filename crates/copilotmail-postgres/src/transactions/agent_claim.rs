use crate::*;

impl PgControlPlaneTransaction {
    /// Locks a matching mailbox delegation with `FOR SHARE` until commit.
    /// Revocation cannot land after this statement returns and before the
    /// caller writes the audit event.
    pub(crate) async fn tx_claim_next_agent_message(
        &mut self,
        write: &AgentMessageClaimWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        if write.lease_expires_at <= write.now {
            return Err(StoreError::Conflict);
        }
        let operation = AgentMessageProcessingOperation::Claimed;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 43))")
            .bind(format!(
                "{}:{}:{}:{}",
                write.organization_id,
                write.principal_id,
                operation.as_str(),
                format_args!("{:?}", write.idempotency_key_sha256)
            ))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        let existing = sqlx::query_as::<_, AgentMessageProcessingOperationRow>(
            r#"
            SELECT id, organization_id, subscription_id, message_id,
                   principal_id, lease_id, operation, request_sha256,
                   result_state, attempt_count, lease_expires_at,
                   reason_code, occurred_at
              FROM agent_message_processing_operations
             WHERE organization_id = $1
               AND principal_id = $2
               AND operation = $3
               AND idempotency_key_sha256 = $4
             FOR UPDATE
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(operation.as_str())
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            if existing.request_sha256 != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            return agent_message_processing_result_from_row(existing, true).map(Some);
        }

        let row = sqlx::query_as::<_, AgentMessageProcessingOperationRow>(
            r#"
            WITH candidate AS (
                SELECT state.subscription_id, state.message_id
                  FROM agent_message_states AS state
                  JOIN agent_subscriptions AS subscription
                    ON subscription.id = state.subscription_id
                   AND subscription.organization_id = state.organization_id
                  JOIN principals AS principal
                    ON principal.id = subscription.principal_id
                   AND principal.organization_id = subscription.organization_id
                 WHERE state.organization_id = $1
                   AND state.principal_id = $2
                   AND state.subscription_id = $3
                   AND subscription.principal_id = $2
                   AND subscription.status = 'active'
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = subscription.mailbox_id
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= $8
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $8
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = subscription.mailbox_id
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= $8
                              AND delegation.valid_until > $8
                         FOR SHARE OF delegation
                       )
                   )
                   AND (
                       state.state = 'pending'
                       OR (
                           state.state = 'leased'
                           AND state.lease_expires_at <= $8
                       )
                   )
                 ORDER BY state.updated_at, state.message_id
                 LIMIT 1
                 FOR UPDATE OF state SKIP LOCKED
            ),
            updated AS (
                UPDATE agent_message_states AS state
                   SET state = 'leased',
                       lease_id = $4,
                       last_lease_id = $4,
                       lease_expires_at = $5,
                       attempt_count = state.attempt_count + 1,
                       state_version = state.state_version + 1,
                       acknowledged_at = NULL,
                       failed_at = NULL,
                       updated_at = $8
                  FROM candidate
                 WHERE state.subscription_id = candidate.subscription_id
                   AND state.message_id = candidate.message_id
                RETURNING state.message_id, state.attempt_count
            )
            INSERT INTO agent_message_processing_operations(
                id, organization_id, subscription_id, message_id,
                principal_id, lease_id, operation, idempotency_key_sha256,
                request_sha256, result_state, attempt_count,
                lease_expires_at, reason_code, occurred_at
            )
            SELECT $9, $1, $3, updated.message_id, $2, $4, 'claimed',
                   $6, $7, 'leased', updated.attempt_count, $5, NULL, $8
              FROM updated
            RETURNING id, organization_id, subscription_id, message_id,
                      principal_id, lease_id, operation, request_sha256,
                      result_state, attempt_count, lease_expires_at,
                      reason_code, occurred_at
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(write.subscription_id.as_uuid())
        .bind(write.lease_id.as_uuid())
        .bind(write.lease_expires_at)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(write.now)
        .bind(Uuid::now_v7())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        row.map(|row| agent_message_processing_result_from_row(row, false))
            .transpose()
    }
}
