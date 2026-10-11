use crate::*;

#[async_trait]
impl MailboxAuthorizationStore for PgControlPlaneStore {
    async fn principal_can_submit_mail(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals AS principal
                  LEFT JOIN agent_profiles AS agent
                    ON agent.principal_id = principal.id
                 WHERE principal.id = $1
                   AND principal.organization_id = $2
                   AND principal.status = 'active'
                   AND (
                       principal.kind = 'human'
                       OR (
                           principal.kind = 'agent'
                           AND (
                               agent.environment IN ('development', 'testing')
                               OR (
                                   agent.environment = 'production'
                                   AND EXISTS (
                                       SELECT 1
                                         FROM agent_mail_policy_activations AS activation
                                        WHERE activation.organization_id = principal.organization_id
                                          AND activation.agent_id = principal.id
                                   )
                               )
                           )
                       )
                   )
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn active_production_agent_mail_policy(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyVersion>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct ActivePolicyRow {
            version: i32,
            allowed_recipient_domains: Vec<String>,
            max_recipients_per_message: i32,
            max_messages_per_hour: i32,
            max_messages_per_recipient_per_day: i32,
            require_first_contact_approval: bool,
            max_auto_replies_per_thread_per_day: i32,
        }
        let row = sqlx::query_as::<_, ActivePolicyRow>(
            r#"
            SELECT policy.version, policy.allowed_recipient_domains,
                   policy.max_recipients_per_message,
                   policy.max_messages_per_hour,
                   policy.max_messages_per_recipient_per_day,
                   policy.require_first_contact_approval,
                   policy.max_auto_replies_per_thread_per_day
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
              JOIN LATERAL (
                  SELECT activation.policy_version_id
                    FROM agent_mail_policy_activations AS activation
                   WHERE activation.organization_id = principal.organization_id
                     AND activation.agent_id = principal.id
                   ORDER BY activation.policy_version DESC
                   LIMIT 1
              ) AS activation ON true
              JOIN agent_mail_policy_versions AS policy
                ON policy.id = activation.policy_version_id
               AND policy.organization_id = principal.organization_id
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
               AND profile.environment = 'production'
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            AgentMailPolicyVersion::new(
                organization_id,
                principal_id,
                u32::try_from(row.version).map_err(|_| StoreError::Unavailable)?,
                row.allowed_recipient_domains,
                u16::try_from(row.max_recipients_per_message)
                    .map_err(|_| StoreError::Unavailable)?,
                u32::try_from(row.max_messages_per_hour).map_err(|_| StoreError::Unavailable)?,
                u16::try_from(row.max_messages_per_recipient_per_day)
                    .map_err(|_| StoreError::Unavailable)?,
                row.require_first_contact_approval,
                u8::try_from(row.max_auto_replies_per_thread_per_day)
                    .map_err(|_| StoreError::Unavailable)?,
            )
            .map_err(|_| StoreError::Unavailable)
        })
        .transpose()
    }

    async fn active_agent_environment(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<copilotmail_domain::AgentEnvironment>, StoreError> {
        let environment = sqlx::query_scalar::<_, String>(
            r#"
            SELECT profile.environment
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        environment
            .map(|value| match value.as_str() {
                "development" => Ok(copilotmail_domain::AgentEnvironment::Development),
                "testing" => Ok(copilotmail_domain::AgentEnvironment::Testing),
                "production" => Ok(copilotmail_domain::AgentEnvironment::Production),
                unknown => {
                    tracing::error!(environment = unknown, "unknown Agent environment persisted");
                    Err(StoreError::Unavailable)
                }
            })
            .transpose()
    }

    async fn resolve_mailbox_access(
        &self,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        now: DateTime<Utc>,
    ) -> Result<Option<MailboxAccessSnapshot>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct MailboxAccessRow {
            organization_id: Uuid,
            membership: Option<String>,
        }
        #[derive(sqlx::FromRow)]
        struct DelegationRow {
            id: Uuid,
            grantor_principal_id: Uuid,
            scopes: Vec<String>,
        }

        let mailbox = sqlx::query_as::<_, MailboxAccessRow>(
            r#"
            SELECT mailbox.organization_id,
                   (
                       SELECT membership.role
                         FROM mailbox_memberships AS membership
                        WHERE membership.organization_id = mailbox.organization_id
                          AND membership.mailbox_id = mailbox.id
                          AND membership.principal_id = principal.id
                          AND membership.valid_from <= $3
                          AND (
                              membership.valid_until IS NULL
                              OR membership.valid_until > $3
                          )
                        ORDER BY CASE membership.role
                            WHEN 'owner' THEN 1
                            WHEN 'member' THEN 2
                            ELSE 3
                        END
                        LIMIT 1
                   ) AS membership
              FROM mailboxes AS mailbox
              JOIN principals AS principal
                ON principal.id = $1
               AND principal.organization_id = mailbox.organization_id
               AND principal.status = 'active'
             WHERE mailbox.id = $2
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        let Some(mailbox) = mailbox else {
            return Ok(None);
        };

        let delegations = sqlx::query_as::<_, DelegationRow>(
            r#"
            SELECT id, grantor_principal_id, scopes
              FROM delegations
             WHERE organization_id = $1
               AND mailbox_id = $2
               AND grantee_principal_id = $3
               AND revoked_at IS NULL
               AND valid_from <= $4
               AND valid_until > $4
            "#,
        )
        .bind(mailbox.organization_id)
        .bind(mailbox_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;

        let membership = mailbox
            .membership
            .as_deref()
            .and_then(mailbox_membership_role_from_str);
        let delegations = delegations
            .into_iter()
            .map(|row| ResolvedMailboxDelegation {
                delegation_id: DelegationId::from_uuid(row.id),
                grantor_principal_id: PrincipalId::from_uuid(row.grantor_principal_id),
                capabilities: row
                    .scopes
                    .into_iter()
                    .filter_map(|scope| mailbox_capability_from_str(&scope))
                    .collect(),
            })
            .collect();
        Ok(Some(MailboxAccessSnapshot {
            organization_id: OrganizationId::from_uuid(mailbox.organization_id),
            mailbox_id,
            membership,
            delegations,
        }))
    }
}
