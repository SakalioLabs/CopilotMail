use crate::*;

#[async_trait]
impl AgentMailPolicyQueryStore for PgControlPlaneStore {
    async fn get_agent_mail_policy_runtime(
        &self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyRuntimeSnapshot>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct RuntimeRow {
            environment: String,
            policy_version: Option<i32>,
            allowed_recipient_domains: Option<Vec<String>>,
            max_recipients_per_message: Option<i32>,
            max_messages_per_hour: Option<i32>,
            max_messages_per_recipient_per_day: Option<i32>,
            require_first_contact_approval: Option<bool>,
            max_auto_replies_per_thread_per_day: Option<i32>,
            observed_at: DateTime<Utc>,
            hourly_window_started_at: DateTime<Utc>,
            admitted_messages_in_hour: i64,
        }

        let row = sqlx::query_as::<_, RuntimeRow>(
            r#"
            WITH runtime_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS observed_at
            )
            SELECT profile.environment,
                   policy.version AS policy_version,
                   policy.allowed_recipient_domains,
                   policy.max_recipients_per_message,
                   policy.max_messages_per_hour,
                   policy.max_messages_per_recipient_per_day,
                   policy.require_first_contact_approval,
                   policy.max_auto_replies_per_thread_per_day,
                   runtime_clock.observed_at,
                   runtime_clock.observed_at - interval '1 hour'
                       AS hourly_window_started_at,
                   (
                       SELECT count(*)
                         FROM agent_mail_admissions AS admission
                        WHERE admission.organization_id = principal.organization_id
                          AND admission.agent_id = principal.id
                          AND admission.admitted_at
                              > runtime_clock.observed_at - interval '1 hour'
                          AND admission.admitted_at <= runtime_clock.observed_at
                   ) AS admitted_messages_in_hour
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
              CROSS JOIN runtime_clock
              LEFT JOIN LATERAL (
                  SELECT activation.policy_version_id
                    FROM agent_mail_policy_activations AS activation
                   WHERE activation.organization_id = principal.organization_id
                     AND activation.agent_id = principal.id
                     AND profile.environment = 'production'
                   ORDER BY activation.policy_version DESC
                   LIMIT 1
              ) AS activation ON true
              LEFT JOIN agent_mail_policy_versions AS policy
                ON policy.id = activation.policy_version_id
               AND policy.organization_id = principal.organization_id
               AND policy.agent_id = principal.id
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(agent_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;

        row.map(|row| {
            let environment = match row.environment.as_str() {
                "development" => AgentEnvironment::Development,
                "testing" => AgentEnvironment::Testing,
                "production" => AgentEnvironment::Production,
                _ => return Err(StoreError::Unavailable),
            };
            let active_policy = match (
                row.policy_version,
                row.allowed_recipient_domains,
                row.max_recipients_per_message,
                row.max_messages_per_hour,
                row.max_messages_per_recipient_per_day,
                row.require_first_contact_approval,
                row.max_auto_replies_per_thread_per_day,
            ) {
                (None, None, None, None, None, None, None) => None,
                (
                    Some(version),
                    Some(allowed_recipient_domains),
                    Some(max_recipients_per_message),
                    Some(max_messages_per_hour),
                    Some(max_messages_per_recipient_per_day),
                    Some(require_first_contact_approval),
                    Some(max_auto_replies_per_thread_per_day),
                ) => Some(
                    AgentMailPolicyVersion::new(
                        organization_id,
                        agent_id,
                        u32::try_from(version).map_err(|_| StoreError::Unavailable)?,
                        allowed_recipient_domains,
                        u16::try_from(max_recipients_per_message)
                            .map_err(|_| StoreError::Unavailable)?,
                        u32::try_from(max_messages_per_hour)
                            .map_err(|_| StoreError::Unavailable)?,
                        u16::try_from(max_messages_per_recipient_per_day)
                            .map_err(|_| StoreError::Unavailable)?,
                        require_first_contact_approval,
                        u8::try_from(max_auto_replies_per_thread_per_day)
                            .map_err(|_| StoreError::Unavailable)?,
                    )
                    .map_err(|_| StoreError::Unavailable)?,
                ),
                _ => return Err(StoreError::Unavailable),
            };
            Ok(AgentMailPolicyRuntimeSnapshot {
                organization_id,
                agent_id,
                environment,
                active_policy,
                observed_at: row.observed_at,
                hourly_window_started_at: row.hourly_window_started_at,
                admitted_messages_in_hour: u32::try_from(row.admitted_messages_in_hour)
                    .map_err(|_| StoreError::Unavailable)?,
            })
        })
        .transpose()
    }
}
