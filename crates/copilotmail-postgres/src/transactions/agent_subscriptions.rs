use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_ensure_agent_processing_subscription(
        &mut self,
        record: &AgentProcessingSubscriptionRecord,
    ) -> Result<(AgentProcessingSubscriptionRecord, bool), StoreError> {
        let inserted = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO agent_subscriptions(
                id, organization_id, principal_id, mailbox_id, name,
                status, cursor_value, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, 'default', 'active', NULL, $5, $5)
            ON CONFLICT (principal_id, mailbox_id, name) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(record.subscription_id.as_uuid())
        .bind(record.organization_id.as_uuid())
        .bind(record.principal_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.created_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            mailbox_id: Uuid,
            status: String,
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
        }

        let stored = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, principal_id, mailbox_id,
                   status, created_at, updated_at
              FROM agent_subscriptions
             WHERE organization_id = $1
               AND principal_id = $2
               AND mailbox_id = $3
               AND name = 'default'
             FOR UPDATE
            "#,
        )
        .bind(record.organization_id.as_uuid())
        .bind(record.principal_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;

        Ok((
            AgentProcessingSubscriptionRecord {
                subscription_id: SubscriptionId::from_uuid(stored.id),
                organization_id: OrganizationId::from_uuid(stored.organization_id),
                principal_id: PrincipalId::from_uuid(stored.principal_id),
                mailbox_id: MailboxId::from_uuid(stored.mailbox_id),
                status: agent_processing_subscription_status(&stored.status)?,
                created_at: stored.created_at,
                updated_at: stored.updated_at,
            },
            inserted.is_some(),
        ))
    }

    /// Locks a matching mailbox delegation with `FOR SHARE` until commit.
    pub(crate) async fn tx_change_agent_processing_subscription(
        &mut self,
        write: &AgentProcessingSubscriptionChangeWrite,
    ) -> Result<Option<AgentProcessingSubscriptionChangeResult>, StoreError> {
        let operation = match write.target_status {
            AgentProcessingSubscriptionStatus::Active => "resumed",
            AgentProcessingSubscriptionStatus::Paused => "paused",
            AgentProcessingSubscriptionStatus::Retired => "retired",
        };
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 45))")
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

        #[derive(sqlx::FromRow)]
        struct Row {
            operation_id: Uuid,
            organization_id: Uuid,
            subscription_id: Uuid,
            principal_id: Uuid,
            mailbox_id: Uuid,
            previous_status: String,
            result_status: String,
            request_sha256: Vec<u8>,
            created_at: DateTime<Utc>,
            occurred_at: DateTime<Utc>,
        }

        let existing = sqlx::query_as::<_, Row>(
            r#"
            SELECT operation.id AS operation_id,
                   operation.organization_id,
                   operation.subscription_id,
                   operation.principal_id,
                   subscription.mailbox_id,
                   operation.previous_status,
                   operation.result_status,
                   operation.request_sha256,
                   subscription.created_at,
                   operation.occurred_at
              FROM agent_processing_subscription_operations AS operation
              JOIN agent_subscriptions AS subscription
                ON subscription.id = operation.subscription_id
               AND subscription.organization_id = operation.organization_id
             WHERE operation.organization_id = $1
               AND operation.principal_id = $2
               AND operation.operation = $3
               AND operation.idempotency_key_sha256 = $4
             FOR UPDATE OF operation
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
            return Ok(Some(agent_processing_subscription_change_result(
                existing.operation_id,
                existing.organization_id,
                existing.subscription_id,
                existing.principal_id,
                existing.mailbox_id,
                &existing.previous_status,
                &existing.result_status,
                existing.created_at,
                existing.occurred_at,
                true,
            )?));
        }

        let transitioned = sqlx::query_as::<_, Row>(
            r#"
            WITH changed AS (
                UPDATE agent_subscriptions AS subscription
                   SET status = $5,
                       updated_at = $6
                 WHERE subscription.id = $1
                   AND subscription.organization_id = $2
                   AND subscription.principal_id = $3
                   AND subscription.status = $4
                   AND EXISTS (
                       SELECT 1
                         FROM principals AS principal
                        WHERE principal.id = $3
                          AND principal.organization_id = $2
                          AND principal.kind = 'agent'
                          AND principal.status = 'active'
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $2
                              AND membership.mailbox_id = subscription.mailbox_id
                              AND membership.principal_id = $3
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= $6
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $6
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $2
                              AND delegation.mailbox_id = subscription.mailbox_id
                              AND delegation.grantee_principal_id = $3
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= $6
                              AND delegation.valid_until > $6
                         FOR SHARE OF delegation
                       )
                   )
                   AND (
                       $5 <> 'retired'
                       OR NOT EXISTS (
                           SELECT 1
                             FROM agent_message_states AS state
                            WHERE state.organization_id = $2
                              AND state.subscription_id = $1
                              AND state.state = 'leased'
                              AND state.lease_expires_at > $6
                       )
                   )
                RETURNING subscription.organization_id,
                          subscription.id AS subscription_id,
                          subscription.principal_id,
                          subscription.mailbox_id,
                          subscription.created_at
            )
            INSERT INTO agent_processing_subscription_operations(
                id, organization_id, subscription_id, principal_id,
                operation, previous_status, result_status,
                idempotency_key_sha256, request_sha256, occurred_at
            )
            SELECT $7, changed.organization_id, changed.subscription_id,
                   changed.principal_id, $8, $4, $5, $9, $10, $6
              FROM changed
            RETURNING id AS operation_id, organization_id, subscription_id,
                      principal_id,
                      (SELECT mailbox_id FROM changed) AS mailbox_id,
                      previous_status, result_status, request_sha256,
                      (SELECT created_at FROM changed) AS created_at,
                      occurred_at
            "#,
        )
        .bind(write.subscription_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(write.expected_status.as_str())
        .bind(write.target_status.as_str())
        .bind(write.now)
        .bind(write.operation_id)
        .bind(operation)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        transitioned
            .map(|row| {
                agent_processing_subscription_change_result(
                    row.operation_id,
                    row.organization_id,
                    row.subscription_id,
                    row.principal_id,
                    row.mailbox_id,
                    &row.previous_status,
                    &row.result_status,
                    row.created_at,
                    row.occurred_at,
                    false,
                )
            })
            .transpose()
    }
}
