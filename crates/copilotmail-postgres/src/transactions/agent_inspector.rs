use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_authorize_agent_inspector_reader(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        // Keep the actor, every live assignment that currently conveys
        // `identity.manage`, and its immutable role catalog rows locked through
        // commit. A concurrent suspension or authorization-catalog mutation
        // therefore blocks until the successful read's Audit and Outbox
        // evidence commit, and the service repeats this proof just before it
        // writes that evidence.
        let authorization_locks = sqlx::query_scalar::<_, Uuid>(
            r#"
            WITH runtime_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS observed_at
            )
            SELECT principal.id
              FROM principals AS principal
              CROSS JOIN runtime_clock
              JOIN role_assignments AS assignment
                ON assignment.organization_id = principal.organization_id
               AND assignment.principal_id = principal.id
               AND assignment.revoked_at IS NULL
               AND assignment.valid_from <= runtime_clock.observed_at
               AND (
                   assignment.valid_until IS NULL
                   OR assignment.valid_until > runtime_clock.observed_at
               )
              JOIN roles AS role
                ON role.id = assignment.role_id
               AND role.organization_id = assignment.organization_id
              JOIN role_permissions AS role_permission
                ON role_permission.role_id = role.id
               AND role_permission.permission = 'identity.manage'
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'human'
               AND principal.status = 'active'
             FOR UPDATE OF principal, assignment, role, role_permission
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(!authorization_locks.is_empty())
    }

    pub(crate) async fn tx_get_agent_inspector_snapshot(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<Option<AgentInspectorSnapshotCandidate>, StoreError> {
        let row = sqlx::query_as::<_, AgentInspectorSnapshotRow>(
            r#"
            WITH runtime_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS observed_at
            ),
            target AS (
                SELECT principal.organization_id,
                       principal.id AS agent_id,
                       principal.kind AS agent_kind,
                       principal.status AS agent_status,
                       principal.display_name AS agent_display_name,
                       principal.created_at AS agent_created_at,
                       profile.owner_principal_id AS profile_owner_principal_id,
                       profile.environment,
                       profile.risk_level,
                       profile.purpose,
                       profile.review_due_at,
                       runtime_clock.observed_at
                  FROM principals AS principal
                  JOIN agent_profiles AS profile
                    ON profile.principal_id = principal.id
                  CROSS JOIN runtime_clock
                 WHERE principal.id = $2
                   AND principal.organization_id = $1
                   AND principal.kind = 'agent'
            ),
            credential_candidates AS (
                SELECT credential.id,
                       credential.kind,
                       credential.status,
                       credential.created_at,
                       credential.expires_at,
                       credential.revoked_at,
                       credential.last_used_at,
                       target.observed_at,
                       private_key.credential_id IS NOT NULL
                           AS has_private_key_metadata,
                       mtls.credential_id IS NOT NULL AS has_mtls_metadata,
                       CASE credential.kind
                           WHEN 'private_key_jwt' THEN private_key.allowed_scopes
                           WHEN 'mtls' THEN mtls.allowed_scopes
                           ELSE NULL
                       END AS allowed_scopes
                  FROM target
                  JOIN credentials AS credential
                    ON credential.principal_id = target.agent_id
                  LEFT JOIN agent_private_key_credentials AS private_key
                    ON private_key.credential_id = credential.id
                  LEFT JOIN agent_mtls_credentials AS mtls
                    ON mtls.credential_id = credential.id
            ),
            valid_credentials AS (
                SELECT *
                  FROM credential_candidates AS credential
                 WHERE credential.kind IN ('private_key_jwt', 'mtls')
                   AND credential.status = 'active'
                   AND credential.revoked_at IS NULL
                   AND credential.created_at <= credential.observed_at
                   AND credential.expires_at > credential.observed_at
            ),
            credential_integrity AS (
                SELECT COALESCE(
                    bool_or(
                        credential.kind IN ('private_key_jwt', 'mtls')
                        AND credential.status = 'active'
                        AND (
                            credential.expires_at IS NULL
                            OR credential.revoked_at IS NOT NULL
                            OR credential.created_at
                                > credential.observed_at + interval '30 seconds'
                            OR (
                                credential.expires_at > credential.observed_at
                                AND (
                                    credential.expires_at <= credential.created_at
                                    OR (
                                        credential.kind = 'private_key_jwt'
                                        AND (
                                            NOT credential.has_private_key_metadata
                                            OR credential.has_mtls_metadata
                                        )
                                    )
                                    OR (
                                        credential.kind = 'mtls'
                                        AND (
                                            NOT credential.has_mtls_metadata
                                            OR credential.has_private_key_metadata
                                        )
                                    )
                                    OR credential.allowed_scopes IS NULL
                                    OR cardinality(credential.allowed_scopes) = 0
                                    OR NOT (
                                        credential.allowed_scopes <@ ARRAY[
                                            'organization.read',
                                            'agent.operate',
                                            'agent.mail.read',
                                            'mailbox.use'
                                        ]::text[]
                                    )
                                    OR cardinality(credential.allowed_scopes) <> (
                                        SELECT count(DISTINCT scope.value)::integer
                                          FROM unnest(credential.allowed_scopes)
                                            AS scope(value)
                                    )
                                    OR (
                                        credential.last_used_at IS NOT NULL
                                        AND (
                                            credential.last_used_at < credential.created_at
                                            OR credential.last_used_at
                                                > credential.observed_at
                                                    + interval '30 seconds'
                                        )
                                    )
                                )
                            )
                        )
                    ),
                    false
                ) AS credential_integrity_error
                  FROM credential_candidates AS credential
            ),
            credential_summary AS (
                SELECT count(*)::bigint AS active_credential_count,
                       min(credential.expires_at) AS next_credential_expiry_at,
                       max(credential.last_used_at) AS last_authenticated_at
                  FROM valid_credentials AS credential
            ),
            valid_credential_scope_values AS (
                SELECT DISTINCT scope.value AS scope
                  FROM valid_credentials AS credential
                  CROSS JOIN LATERAL unnest(credential.allowed_scopes) AS scope(value)
            ),
            current_agent_permissions AS (
                SELECT DISTINCT role_permission.permission
                  FROM target
                  JOIN role_assignments AS assignment
                    ON assignment.organization_id = target.organization_id
                   AND assignment.principal_id = target.agent_id
                   AND assignment.revoked_at IS NULL
                   AND assignment.valid_from <= target.observed_at
                   AND (
                       assignment.valid_until IS NULL
                       OR assignment.valid_until > target.observed_at
                   )
                  JOIN roles AS role
                    ON role.id = assignment.role_id
                   AND role.organization_id = assignment.organization_id
                  JOIN role_permissions AS role_permission
                    ON role_permission.role_id = role.id
                 WHERE target.agent_status = 'active'
                   AND role_permission.permission IN (
                       'organization.read',
                       'agent.operate',
                       'agent.mail.read',
                       'mailbox.use'
                   )
            ),
            effective_scope_values AS (
                SELECT scope
                  FROM valid_credential_scope_values
                INTERSECT
                SELECT permission
                  FROM current_agent_permissions
            ),
            effective_scope_summary AS (
                SELECT COALESCE(
                    array_agg(scope ORDER BY scope),
                    ARRAY[]::text[]
                ) AS effective_scopes
                  FROM effective_scope_values
            ),
            latest_production_policy_activation AS (
                SELECT activation.organization_id AS activation_organization_id,
                       activation.agent_id AS activation_agent_id,
                       activation.policy_version_id AS activation_policy_version_id,
                       activation.policy_version AS activation_policy_version,
                       policy.id AS policy_id,
                       policy.organization_id AS policy_organization_id,
                       policy.agent_id AS policy_agent_id,
                       policy.version AS policy_version
                  FROM target
                  JOIN agent_mail_policy_activations AS activation
                    ON activation.organization_id = target.organization_id
                   AND activation.agent_id = target.agent_id
                  LEFT JOIN agent_mail_policy_versions AS policy
                    ON policy.id = activation.policy_version_id
                 WHERE target.environment = 'production'
                 ORDER BY activation.policy_version DESC,
                          activation.activated_at DESC,
                          activation.id DESC
                 LIMIT 1
            ),
            policy_summary AS (
                SELECT EXISTS(
                           SELECT 1
                             FROM latest_production_policy_activation
                       ) AS has_active_production_mail_policy,
                       (
                           EXISTS(
                               SELECT 1
                                 FROM latest_production_policy_activation AS activation
                                WHERE activation.policy_id IS NULL
                                   OR activation.policy_organization_id
                                      IS DISTINCT FROM activation.activation_organization_id
                                   OR activation.policy_agent_id
                                      IS DISTINCT FROM activation.activation_agent_id
                                   OR activation.policy_version
                                      IS DISTINCT FROM activation.activation_policy_version
                           )
                           OR EXISTS(
                               SELECT 1
                                 FROM target
                                 JOIN agent_mail_policy_activations AS activation
                                   ON activation.organization_id = target.organization_id
                                  AND activation.agent_id = target.agent_id
                                WHERE target.environment <> 'production'
                           )
                       ) AS policy_integrity_error
            ),
            subscription_summary AS (
                SELECT (count(*) FILTER (
                            WHERE subscription.status = 'active'
                        ))::bigint AS active_processing_subscription_count,
                       (count(*) FILTER (
                            WHERE subscription.status = 'paused'
                        ))::bigint AS paused_processing_subscription_count,
                       (count(*) FILTER (
                            WHERE subscription.status = 'retired'
                        ))::bigint AS retired_processing_subscription_count,
                       COALESCE(
                           bool_or(
                               subscription.status NOT IN ('active', 'paused', 'retired')
                           ),
                           false
                       ) AS subscription_integrity_error
                  FROM target
                  LEFT JOIN agent_subscriptions AS subscription
                    ON subscription.organization_id = target.organization_id
                   AND subscription.principal_id = target.agent_id
            ),
            processing_activity AS (
                SELECT max(activity.activity_at) AS last_processing_activity_at
                  FROM (
                      SELECT operation.occurred_at AS activity_at
                        FROM target
                        JOIN agent_message_processing_operations AS operation
                          ON operation.organization_id = target.organization_id
                         AND operation.principal_id = target.agent_id
                      UNION ALL
                      SELECT operation.occurred_at AS activity_at
                        FROM target
                        JOIN agent_processing_subscription_operations AS operation
                          ON operation.organization_id = target.organization_id
                         AND operation.principal_id = target.agent_id
                  ) AS activity
            )
            SELECT target.organization_id,
                   target.agent_id,
                   target.agent_kind,
                   target.agent_status,
                   target.agent_display_name,
                   target.agent_created_at,
                   target.profile_owner_principal_id,
                   owner.id AS owner_id,
                   owner.organization_id AS owner_organization_id,
                   owner.kind AS owner_kind,
                   owner.status AS owner_status,
                   owner.display_name AS owner_display_name,
                   target.environment,
                   target.risk_level,
                   target.purpose,
                   target.review_due_at,
                   credential_summary.active_credential_count,
                   credential_summary.next_credential_expiry_at,
                   credential_summary.last_authenticated_at,
                   effective_scope_summary.effective_scopes,
                   credential_integrity.credential_integrity_error,
                   policy_summary.has_active_production_mail_policy,
                   policy_summary.policy_integrity_error,
                   subscription_summary.active_processing_subscription_count,
                   subscription_summary.paused_processing_subscription_count,
                   subscription_summary.retired_processing_subscription_count,
                   subscription_summary.subscription_integrity_error,
                   processing_activity.last_processing_activity_at,
                   target.observed_at
              FROM target
              LEFT JOIN principals AS owner
                ON owner.id = target.profile_owner_principal_id
              CROSS JOIN credential_integrity
              CROSS JOIN credential_summary
              CROSS JOIN effective_scope_summary
              CROSS JOIN policy_summary
              CROSS JOIN subscription_summary
              CROSS JOIN processing_activity
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(agent_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        row.map(agent_inspector_snapshot_from_row).transpose()
    }
}
