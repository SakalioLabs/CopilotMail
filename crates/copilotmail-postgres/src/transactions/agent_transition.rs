use crate::*;

impl PgControlPlaneTransaction {
    /// Locks a matching mailbox delegation with `FOR SHARE` until commit.
    /// Lease extension, acknowledgement, and nack share that lock.
    pub(crate) async fn tx_transition_agent_message(
        &mut self,
        write: &AgentMessageTransitionWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        if matches!(
            write.operation,
            AgentMessageProcessingOperation::Claimed
                | AgentMessageProcessingOperation::DeadLettered
        ) || (write.operation == AgentMessageProcessingOperation::LeaseExtended
            && write
                .lease_expires_at
                .is_none_or(|expires_at| expires_at <= write.now))
            || (write.operation == AgentMessageProcessingOperation::Nacked
                && (write.max_attempts == 0 || write.reason_code.is_none()))
        {
            return Err(StoreError::Conflict);
        }
        let operation = write.operation.as_str();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 43))")
            .bind(format!(
                "{}:{}:{}:{}",
                write.organization_id,
                write.principal_id,
                operation,
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
        .bind(operation)
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

        #[derive(sqlx::FromRow)]
        struct TransitionRow {
            message_id: Uuid,
            attempt_count: i32,
            result_state: String,
            lease_expires_at: Option<DateTime<Utc>>,
            reason_code: Option<String>,
        }

        let transitioned = match write.operation {
            AgentMessageProcessingOperation::LeaseExtended => sqlx::query_as::<_, TransitionRow>(
                r#"
                    UPDATE agent_message_states AS state
                       SET lease_expires_at = $6,
                           state_version = state.state_version + 1,
                           updated_at = $7
                      FROM agent_subscriptions AS subscription
                     WHERE state.subscription_id = subscription.id
                       AND state.organization_id = subscription.organization_id
                       AND state.organization_id = $1
                       AND state.principal_id = $2
                       AND state.subscription_id = $3
                       AND state.message_id = $4
                       AND state.state = 'leased'
                       AND state.lease_id = $5
                       AND state.lease_expires_at > $7
                       AND subscription.principal_id = $2
                       AND subscription.status IN ('active', 'paused')
                       AND EXISTS (
                           SELECT 1 FROM principals AS principal
                            WHERE principal.id = $2
                              AND principal.organization_id = $1
                              AND principal.kind = 'agent'
                              AND principal.status = 'active'
                       )
                       AND (
                           EXISTS (
                               SELECT 1 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = $1
                                  AND membership.mailbox_id = subscription.mailbox_id
                                  AND membership.principal_id = $2
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $7
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $7
                                  )
                           )
                           OR EXISTS (
                               SELECT 1 FROM delegations AS delegation
                                WHERE delegation.organization_id = $1
                                  AND delegation.mailbox_id = subscription.mailbox_id
                                  AND delegation.grantee_principal_id = $2
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $7
                                  AND delegation.valid_until > $7
                             FOR SHARE OF delegation
                           )
                       )
                    RETURNING state.message_id, state.attempt_count,
                              'leased'::text AS result_state,
                              state.lease_expires_at, NULL::text AS reason_code
                    "#,
            )
            .bind(write.organization_id.as_uuid())
            .bind(write.principal_id.as_uuid())
            .bind(write.subscription_id.as_uuid())
            .bind(write.message_id)
            .bind(write.lease_id.as_uuid())
            .bind(write.lease_expires_at)
            .bind(write.now)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?,
            AgentMessageProcessingOperation::Acknowledged => sqlx::query_as::<_, TransitionRow>(
                r#"
                    UPDATE agent_message_states AS state
                       SET state = 'acknowledged',
                           lease_id = NULL,
                           lease_expires_at = NULL,
                           last_error = NULL,
                           state_version = state.state_version + 1,
                           acknowledged_at = $6,
                           failed_at = NULL,
                           updated_at = $6
                      FROM agent_subscriptions AS subscription
                     WHERE state.subscription_id = subscription.id
                       AND state.organization_id = subscription.organization_id
                       AND state.organization_id = $1
                       AND state.principal_id = $2
                       AND state.subscription_id = $3
                       AND state.message_id = $4
                       AND state.state = 'leased'
                       AND state.lease_id = $5
                       AND state.lease_expires_at > $6
                       AND subscription.principal_id = $2
                       AND subscription.status IN ('active', 'paused')
                       AND EXISTS (
                           SELECT 1 FROM principals AS principal
                            WHERE principal.id = $2
                              AND principal.organization_id = $1
                              AND principal.kind = 'agent'
                              AND principal.status = 'active'
                       )
                       AND (
                           EXISTS (
                               SELECT 1 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = $1
                                  AND membership.mailbox_id = subscription.mailbox_id
                                  AND membership.principal_id = $2
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $6
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $6
                                  )
                           )
                           OR EXISTS (
                               SELECT 1 FROM delegations AS delegation
                                WHERE delegation.organization_id = $1
                                  AND delegation.mailbox_id = subscription.mailbox_id
                                  AND delegation.grantee_principal_id = $2
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $6
                                  AND delegation.valid_until > $6
                             FOR SHARE OF delegation
                           )
                       )
                    RETURNING state.message_id, state.attempt_count,
                              'acknowledged'::text AS result_state,
                              NULL::timestamptz AS lease_expires_at,
                              NULL::text AS reason_code
                    "#,
            )
            .bind(write.organization_id.as_uuid())
            .bind(write.principal_id.as_uuid())
            .bind(write.subscription_id.as_uuid())
            .bind(write.message_id)
            .bind(write.lease_id.as_uuid())
            .bind(write.now)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?,
            AgentMessageProcessingOperation::Nacked => sqlx::query_as::<_, TransitionRow>(
                r#"
                    UPDATE agent_message_states AS state
                       SET state = CASE
                               WHEN state.attempt_count >= $6
                               THEN 'dead_lettered'
                               ELSE 'pending'
                           END,
                           lease_id = NULL,
                           lease_expires_at = NULL,
                           last_error = $8,
                           state_version = state.state_version + 1,
                           acknowledged_at = NULL,
                           failed_at = CASE
                               WHEN state.attempt_count >= $6 THEN $7
                               ELSE NULL
                           END,
                           updated_at = $7
                      FROM agent_subscriptions AS subscription
                     WHERE state.subscription_id = subscription.id
                       AND state.organization_id = subscription.organization_id
                       AND state.organization_id = $1
                       AND state.principal_id = $2
                       AND state.subscription_id = $3
                       AND state.message_id = $4
                       AND state.state = 'leased'
                       AND state.lease_id = $5
                       AND state.lease_expires_at > $7
                       AND subscription.principal_id = $2
                       AND subscription.status IN ('active', 'paused')
                       AND EXISTS (
                           SELECT 1 FROM principals AS principal
                            WHERE principal.id = $2
                              AND principal.organization_id = $1
                              AND principal.kind = 'agent'
                              AND principal.status = 'active'
                       )
                       AND (
                           EXISTS (
                               SELECT 1 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = $1
                                  AND membership.mailbox_id = subscription.mailbox_id
                                  AND membership.principal_id = $2
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $7
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $7
                                  )
                           )
                           OR EXISTS (
                               SELECT 1 FROM delegations AS delegation
                                WHERE delegation.organization_id = $1
                                  AND delegation.mailbox_id = subscription.mailbox_id
                                  AND delegation.grantee_principal_id = $2
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $7
                                  AND delegation.valid_until > $7
                             FOR SHARE OF delegation
                           )
                       )
                    RETURNING state.message_id, state.attempt_count,
                              state.state AS result_state,
                              NULL::timestamptz AS lease_expires_at,
                              state.last_error AS reason_code
                    "#,
            )
            .bind(write.organization_id.as_uuid())
            .bind(write.principal_id.as_uuid())
            .bind(write.subscription_id.as_uuid())
            .bind(write.message_id)
            .bind(write.lease_id.as_uuid())
            .bind(i64::from(write.max_attempts))
            .bind(write.now)
            .bind(write.reason_code.as_deref())
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?,
            AgentMessageProcessingOperation::Claimed
            | AgentMessageProcessingOperation::DeadLettered => unreachable!(),
        };

        let Some(transitioned) = transitioned else {
            return Ok(None);
        };
        let row = sqlx::query_as::<_, AgentMessageProcessingOperationRow>(
            r#"
            INSERT INTO agent_message_processing_operations(
                id, organization_id, subscription_id, message_id,
                principal_id, lease_id, operation, idempotency_key_sha256,
                request_sha256, result_state, attempt_count,
                lease_expires_at, reason_code, occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                    $10, $11, $12, $13, $14)
            RETURNING id, organization_id, subscription_id, message_id,
                      principal_id, lease_id, operation, request_sha256,
                      result_state, attempt_count, lease_expires_at,
                      reason_code, occurred_at
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(write.organization_id.as_uuid())
        .bind(write.subscription_id.as_uuid())
        .bind(transitioned.message_id)
        .bind(write.principal_id.as_uuid())
        .bind(write.lease_id.as_uuid())
        .bind(operation)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(&transitioned.result_state)
        .bind(transitioned.attempt_count)
        .bind(transitioned.lease_expires_at)
        .bind(transitioned.reason_code.as_deref())
        .bind(write.now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        agent_message_processing_result_from_row(row, false).map(Some)
    }
}
