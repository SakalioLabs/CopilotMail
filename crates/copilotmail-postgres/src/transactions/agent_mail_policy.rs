use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_find_agent_mail_policy_version(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        policy_version_id: Uuid,
    ) -> Result<Option<AgentMailPolicyVersionRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            agent_id: Uuid,
            version: i32,
            allowed_recipient_domains: Vec<String>,
            max_recipients_per_message: i32,
            max_messages_per_hour: i32,
            max_messages_per_recipient_per_day: i32,
            require_first_contact_approval: bool,
            max_auto_replies_per_thread_per_day: i32,
            created_by: Uuid,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, agent_id, version,
                   allowed_recipient_domains, max_recipients_per_message,
                   max_messages_per_hour, max_messages_per_recipient_per_day,
                   require_first_contact_approval,
                   max_auto_replies_per_thread_per_day, created_by, created_at
              FROM agent_mail_policy_versions
             WHERE organization_id = $1
               AND agent_id = $2
               AND id = $3
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(agent_id.as_uuid())
        .bind(policy_version_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let organization_id = OrganizationId::from_uuid(row.organization_id);
            let agent_id = PrincipalId::from_uuid(row.agent_id);
            Ok(AgentMailPolicyVersionRecord {
                id: row.id,
                policy: AgentMailPolicyVersion::new(
                    organization_id,
                    agent_id,
                    u32::try_from(row.version).map_err(|_| StoreError::Conflict)?,
                    row.allowed_recipient_domains,
                    u16::try_from(row.max_recipients_per_message)
                        .map_err(|_| StoreError::Conflict)?,
                    u32::try_from(row.max_messages_per_hour).map_err(|_| StoreError::Conflict)?,
                    u16::try_from(row.max_messages_per_recipient_per_day)
                        .map_err(|_| StoreError::Conflict)?,
                    row.require_first_contact_approval,
                    u8::try_from(row.max_auto_replies_per_thread_per_day)
                        .map_err(|_| StoreError::Conflict)?,
                )
                .map_err(|_| StoreError::Conflict)?,
                created_by: PrincipalId::from_uuid(row.created_by),
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    pub(crate) async fn tx_next_agent_mail_policy_version(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<u32, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("{organization_id}:{agent_id}"))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        let next = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(MAX(version), 0)::bigint + 1
              FROM agent_mail_policy_versions
             WHERE organization_id = $1
               AND agent_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(agent_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        u32::try_from(next).map_err(|_| StoreError::Conflict)
    }

    pub(crate) async fn tx_insert_agent_mail_policy_version(
        &mut self,
        record: &AgentMailPolicyVersionRecord,
    ) -> Result<(), StoreError> {
        let policy = &record.policy;
        sqlx::query(
            r#"
            INSERT INTO agent_mail_policy_versions(
                id, organization_id, agent_id, version,
                allowed_recipient_domains, max_recipients_per_message,
                max_messages_per_hour, max_messages_per_recipient_per_day,
                require_first_contact_approval,
                max_auto_replies_per_thread_per_day, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(record.id)
        .bind(policy.organization_id().as_uuid())
        .bind(policy.agent_id().as_uuid())
        .bind(i32::try_from(policy.version()).map_err(|_| StoreError::Conflict)?)
        .bind(
            policy
                .allowed_recipient_domains()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
        )
        .bind(i32::from(policy.max_recipients_per_message()))
        .bind(i32::try_from(policy.max_messages_per_hour()).map_err(|_| StoreError::Conflict)?)
        .bind(i32::from(policy.max_messages_per_recipient_per_day()))
        .bind(policy.require_first_contact_approval())
        .bind(i32::from(policy.max_auto_replies_per_thread_per_day()))
        .bind(record.created_by.as_uuid())
        .bind(record.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_agent_mail_policy_activation_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<AgentMailPolicyActivation>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            agent_id: Uuid,
            policy_version: i32,
            approval_request_id: Uuid,
            activated_by: Uuid,
            activated_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, agent_id, policy_version,
                   approval_request_id, activated_by, activated_at
              FROM agent_mail_policy_activations
             WHERE organization_id = $1
               AND approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            AgentMailPolicyActivation::restore(
                AgentMailPolicyActivationId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                PrincipalId::from_uuid(row.agent_id),
                u32::try_from(row.policy_version).map_err(|_| StoreError::Conflict)?,
                copilotmail_domain::ApprovalRequestId::from_uuid(row.approval_request_id),
                PrincipalId::from_uuid(row.activated_by),
                row.activated_at,
            )
            .map_err(|_| StoreError::Conflict)
        })
        .transpose()
    }

    pub(crate) async fn tx_insert_agent_mail_policy_activation(
        &mut self,
        activation: &AgentMailPolicyActivation,
        policy_version_id: Uuid,
    ) -> Result<(), StoreError> {
        let policy_version =
            i32::try_from(activation.policy_version()).map_err(|_| StoreError::Conflict)?;
        sqlx::query(
            r#"
            INSERT INTO agent_mail_policy_activations(
                id, organization_id, agent_id, policy_version_id,
                policy_version, approval_request_id, activated_by, activated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, clock_timestamp())
            "#,
        )
        .bind(activation.id().as_uuid())
        .bind(activation.organization_id().as_uuid())
        .bind(activation.agent_id().as_uuid())
        .bind(policy_version_id)
        .bind(policy_version)
        .bind(activation.approval_request_id().as_uuid())
        .bind(activation.activated_by().as_uuid())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }
}
