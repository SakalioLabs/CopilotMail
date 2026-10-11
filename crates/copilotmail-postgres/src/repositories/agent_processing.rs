use crate::*;

#[async_trait]
impl AgentProcessingQueryStore for PgControlPlaneStore {
    async fn get_agent_processing_subscription(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        subscription_id: SubscriptionId,
    ) -> Result<Option<AgentProcessingSubscriptionRecord>, StoreError> {
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
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT subscription.id, subscription.organization_id,
                   subscription.principal_id, subscription.mailbox_id,
                   subscription.status, subscription.created_at,
                   subscription.updated_at
              FROM agent_subscriptions AS subscription
              JOIN principals AS principal
                ON principal.id = subscription.principal_id
               AND principal.organization_id = subscription.organization_id
             WHERE subscription.id = $1
               AND subscription.organization_id = $2
               AND subscription.principal_id = $3
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(AgentProcessingSubscriptionRecord {
                subscription_id: SubscriptionId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                principal_id: PrincipalId::from_uuid(row.principal_id),
                mailbox_id: MailboxId::from_uuid(row.mailbox_id),
                status: agent_processing_subscription_status(&row.status)?,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        })
        .transpose()
    }

    async fn list_agent_processing_dead_letters(
        &self,
        query: &AgentProcessingDeadLetterQuery,
    ) -> Result<AgentProcessingDeadLetterBatch, StoreError> {
        if !(1..=101).contains(&query.limit) {
            return Err(StoreError::Conflict);
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        let authorized = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM agent_subscriptions AS subscription
                  JOIN principals AS principal
                    ON principal.id = subscription.principal_id
                   AND principal.organization_id = subscription.organization_id
                 WHERE subscription.id = $1
                   AND subscription.organization_id = $2
                   AND subscription.principal_id = $3
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $2
                              AND membership.mailbox_id = subscription.mailbox_id
                              AND membership.principal_id = $3
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= $4
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $4
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
                              AND delegation.valid_from <= $4
                              AND delegation.valid_until > $4
                       )
                   )
            )
            "#,
        )
        .bind(query.subscription_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.observed_at)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !authorized {
            return Err(StoreError::MissingReference);
        }

        let anchor_failed_at = if let Some(before_message_id) = query.before_message_id {
            sqlx::query_scalar::<_, DateTime<Utc>>(
                r#"
                SELECT state.failed_at
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
                   AND state.message_id = $4
                   AND state.state = 'dead_lettered'
                   AND subscription.principal_id = $2
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
                              AND membership.valid_from <= $5
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $5
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
                              AND delegation.valid_from <= $5
                              AND delegation.valid_until > $5
                       )
                   )
                "#,
            )
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(query.subscription_id.as_uuid())
            .bind(before_message_id)
            .bind(query.observed_at)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            None
        };
        if query.before_message_id.is_some() && anchor_failed_at.is_none() {
            return Ok(AgentProcessingDeadLetterBatch {
                items: Vec::new(),
                cursor_valid: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            subscription_id: Uuid,
            message_id: Uuid,
            principal_id: Uuid,
            attempt_count: i32,
            reason_code: String,
            failed_at: DateTime<Utc>,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT state.organization_id, state.subscription_id, state.message_id,
                   state.principal_id, state.attempt_count,
                   state.last_error AS reason_code, state.failed_at
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
               AND state.state = 'dead_lettered'
               AND subscription.principal_id = $2
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
                          AND membership.valid_from <= $4
                          AND (
                              membership.valid_until IS NULL
                              OR membership.valid_until > $4
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
                          AND delegation.valid_from <= $4
                          AND delegation.valid_until > $4
                   )
               )
               AND (
                   $5::timestamptz IS NULL
                   OR (state.failed_at, state.message_id) < ($5, $6)
               )
             ORDER BY state.failed_at DESC, state.message_id DESC
             LIMIT $7
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.subscription_id.as_uuid())
        .bind(query.observed_at)
        .bind(anchor_failed_at)
        .bind(query.before_message_id)
        .bind(query.limit)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let items = rows
            .into_iter()
            .map(|row| {
                Ok(AgentProcessingDeadLetterRecord {
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    subscription_id: SubscriptionId::from_uuid(row.subscription_id),
                    message_id: row.message_id,
                    principal_id: PrincipalId::from_uuid(row.principal_id),
                    attempt_count: u32::try_from(row.attempt_count)
                        .map_err(|_| StoreError::Conflict)?,
                    reason_code: row.reason_code,
                    failed_at: row.failed_at,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(AgentProcessingDeadLetterBatch {
            items,
            cursor_valid: true,
        })
    }
}
