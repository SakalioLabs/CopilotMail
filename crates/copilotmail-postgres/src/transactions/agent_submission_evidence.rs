use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_authorize_agent_mail_submission_evidence_reader(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        // Lock the active Human, all live role assignments that contribute the
        // two required permissions, and their role catalog rows until this
        // exact evidence read commits its Audit and Outbox records. The service
        // invokes this proof both before the read and immediately before those
        // durable disclosure records.
        sqlx::query_scalar::<_, bool>(
            r#"
            WITH runtime_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS observed_at
            ),
            locked_permissions AS MATERIALIZED (
                SELECT role_permission.permission
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
                   AND role_permission.permission IN ('identity.manage', 'audit.read')
                 WHERE principal.id = $1
                   AND principal.organization_id = $2
                   AND principal.kind = 'human'
                   AND principal.status = 'active'
                 FOR UPDATE OF principal, assignment, role, role_permission
            )
            SELECT count(DISTINCT permission) = 2
              FROM locked_permissions
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_get_agent_mail_submission_evidence(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<AgentMailSubmissionEvidenceCandidate>, StoreError> {
        let row = sqlx::query_as::<_, AgentMailSubmissionEvidenceRow>(
            r#"
            WITH runtime_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS observed_at
            ),
            target AS MATERIALIZED (
                SELECT admission.organization_id,
                       admission.agent_id,
                       submission.id AS submission_id,
                       submission.status AS submission_status,
                       submission.created_at AS submission_created_at,
                       submission.updated_at AS submission_updated_at,
                       submission.accepted_at AS submission_accepted_at,
                       admission.id AS admission_id,
                       admission.admitted_at,
                       admission.policy_activation_id,
                       activation.activated_at AS policy_activated_at,
                       activation.activated_at <= admission.admitted_at
                           AS policy_activation_timing_valid,
                       admission.policy_version_id,
                       admission.policy_version,
                       activation.approval_request_id
                           AS policy_activation_approval_request_id,
                       admission.recipient_count,
                       admission.first_contact_count,
                       admission.first_contact_approval_request_id,
                       admission.draft_id,
                       admission.draft_version,
                       admission.mail_data_plane_id,
                       activation.activated_by,
                       runtime_clock.observed_at
                  FROM agent_mail_admissions AS admission
                  JOIN mail_submission_intents AS submission
                    ON submission.id = admission.submission_id
                   AND submission.organization_id = admission.organization_id
                   AND submission.requested_by = admission.agent_id
                   AND submission.mail_data_plane_id = admission.mail_data_plane_id
                  JOIN principals AS agent
                    ON agent.id = admission.agent_id
                   AND agent.organization_id = admission.organization_id
                   AND agent.kind = 'agent'
                  JOIN agent_mail_policy_activations AS activation
                    ON activation.id = admission.policy_activation_id
                   AND activation.organization_id = admission.organization_id
                   AND activation.agent_id = admission.agent_id
                   AND activation.policy_version_id = admission.policy_version_id
                   AND activation.policy_version = admission.policy_version
                  JOIN agent_mail_policy_versions AS policy
                    ON policy.id = admission.policy_version_id
                   AND policy.organization_id = admission.organization_id
                   AND policy.agent_id = admission.agent_id
                   AND policy.version = admission.policy_version
                  CROSS JOIN runtime_clock
                 WHERE admission.organization_id = $1
                   AND admission.agent_id = $2
                   AND admission.submission_id = $3
                 FOR SHARE OF admission, submission, agent
            ),
            approval_proofs AS (
                SELECT target.*,
                       EXISTS (
                           SELECT 1
                             FROM approval_requests AS request
                            WHERE request.id = target.policy_activation_approval_request_id
                              AND request.organization_id = target.organization_id
                              AND request.action = 'agent.production_mail'
                              AND request.resource_kind = 'agent.mail_policy_version'
                              AND request.resource_id = target.policy_version_id::text
                              AND request.requested_by = target.activated_by
                              AND request.status = 'approved'
                              AND request.required_approvals >= 2
                              AND request.created_at <= target.policy_activated_at
                              AND request.completed_at IS NOT NULL
                              AND request.completed_at <= target.policy_activated_at
                              AND request.expires_at > target.policy_activated_at
                              AND (
                                  SELECT count(*)
                                    FROM approval_steps AS step
                                   WHERE step.organization_id = target.organization_id
                                     AND step.approval_request_id = request.id
                                     AND step.decision = 'approved'
                                     AND step.decided_at <= request.completed_at
                                     AND step.decided_at <= target.policy_activated_at
                              ) >= request.required_approvals
                              AND NOT EXISTS (
                                  SELECT 1
                                    FROM approval_steps AS step
                                   WHERE step.organization_id = target.organization_id
                                     AND step.approval_request_id = request.id
                                     AND step.decision = 'rejected'
                              )
                       ) AS policy_activation_approval_verified,
                       CASE
                           WHEN target.first_contact_count = 0
                                AND target.first_contact_approval_request_id IS NULL THEN true
                           WHEN target.first_contact_count > 0
                                AND target.first_contact_approval_request_id IS NOT NULL THEN EXISTS (
                               SELECT 1
                                 FROM approval_requests AS request
                                WHERE request.id = target.first_contact_approval_request_id
                                  AND request.organization_id = target.organization_id
                                  AND request.action = 'agent.first_contact'
                                  AND request.resource_kind = 'agent.mail_draft'
                                  AND request.resource_id = (
                                      target.draft_id::text || ':'
                                          || (target.draft_version - 1)::text
                                  )
                                  AND request.requested_by = target.agent_id
                                  AND request.status = 'approved'
                                  AND request.required_approvals >= 1
                                  AND request.created_at <= target.admitted_at
                                  AND request.completed_at IS NOT NULL
                                  AND request.completed_at <= target.admitted_at
                                  AND request.expires_at > target.admitted_at
                                  AND (
                                      SELECT count(*)
                                        FROM approval_steps AS step
                                       WHERE step.organization_id = target.organization_id
                                         AND step.approval_request_id = request.id
                                         AND step.decision = 'approved'
                                         AND step.decided_at <= request.completed_at
                                         AND step.decided_at <= target.admitted_at
                                  ) >= request.required_approvals
                                  AND NOT EXISTS (
                                      SELECT 1
                                        FROM approval_steps AS step
                                       WHERE step.organization_id = target.organization_id
                                         AND step.approval_request_id = request.id
                                         AND step.decision = 'rejected'
                                  )
                           )
                           ELSE false
                       END AS first_contact_approval_verified
                  FROM target
            )
            SELECT target.organization_id,
                   target.agent_id,
                   target.submission_id,
                   target.submission_status,
                   target.submission_created_at,
                   target.submission_updated_at,
                   target.submission_accepted_at,
                   target.admission_id,
                   target.admitted_at,
                   target.policy_activation_id,
                   target.policy_activated_at,
                   target.policy_activation_timing_valid,
                   target.policy_version_id,
                   target.policy_version,
                   target.policy_activation_approval_request_id,
                   target.policy_activation_approval_verified,
                   target.recipient_count,
                   target.first_contact_count,
                   target.first_contact_approval_request_id,
                   target.first_contact_approval_verified,
                   delivery.id AS delivery_snapshot_id,
                   delivery.observed_at AS delivery_observed_at,
                   delivery.terminal AS delivery_terminal,
                   delivery.recipient_count AS delivery_recipient_count,
                   delivery.delivered_count AS delivery_delivered_count,
                   delivery.failed_count AS delivery_failed_count,
                   delivery.queued_count AS delivery_queued_count,
                   delivery.unknown_count AS delivery_unknown_count,
                   requested_audit.id AS requested_audit_event_id,
                   requested_audit.occurred_at AS requested_audit_occurred_at,
                   requested_audit.event_count AS requested_audit_event_count,
                   admitted_audit.id AS admitted_audit_event_id,
                   admitted_audit.occurred_at AS admitted_audit_occurred_at,
                   admitted_audit.event_count AS admitted_audit_event_count,
                   terminal_audit.id AS terminal_audit_event_id,
                   terminal_audit.action AS terminal_audit_action,
                   terminal_audit.outcome AS terminal_audit_outcome,
                   terminal_audit.occurred_at AS terminal_audit_occurred_at,
                   COALESCE(terminal_audit.event_count, 0)::bigint
                       AS terminal_audit_event_count,
                   target.observed_at
              FROM approval_proofs AS target
              LEFT JOIN LATERAL (
                  SELECT snapshot.id,
                         snapshot.observed_at,
                         snapshot.terminal,
                         snapshot.recipient_count,
                         snapshot.delivered_count,
                         snapshot.failed_count,
                         snapshot.queued_count,
                         snapshot.unknown_count
                    FROM mail_delivery_snapshots AS snapshot
                   WHERE snapshot.organization_id = target.organization_id
                     AND snapshot.submission_id = target.submission_id
                     AND snapshot.mail_data_plane_id = target.mail_data_plane_id
                   ORDER BY snapshot.observed_at DESC, snapshot.id DESC
                   LIMIT 1
              ) AS delivery ON true
              LEFT JOIN LATERAL (
                  SELECT event.id,
                         event.occurred_at,
                         count(*) OVER ()::bigint AS event_count
                    FROM audit_events AS event
                   WHERE event.organization_id = target.organization_id
                     AND event.actor_principal_id = target.agent_id
                     AND event.on_behalf_of_principal_id IS NULL
                     AND event.action = 'mail.submission.requested'
                     AND event.resource_type = 'mail.submission'
                     AND event.resource_id = target.submission_id::text
                     AND event.outcome = 'success'
                     AND event.occurred_at = target.submission_created_at
                   ORDER BY event.occurred_at, event.id
                   LIMIT 1
              ) AS requested_audit ON true
              LEFT JOIN LATERAL (
                  SELECT event.id,
                         event.occurred_at,
                         count(*) OVER ()::bigint AS event_count
                    FROM audit_events AS event
                   WHERE event.organization_id = target.organization_id
                     AND event.actor_principal_id = target.agent_id
                     AND event.on_behalf_of_principal_id IS NULL
                     AND event.action = 'agent.mail.admitted'
                     AND event.resource_type = 'agent.mail_admission'
                     AND event.resource_id = target.admission_id::text
                     AND event.outcome = 'success'
                     AND event.occurred_at = target.admitted_at
                   ORDER BY event.occurred_at, event.id
                   LIMIT 1
              ) AS admitted_audit ON true
              LEFT JOIN LATERAL (
                  SELECT event.id,
                         event.action,
                         event.outcome,
                         event.occurred_at,
                         count(*) OVER ()::bigint AS event_count
                    FROM audit_events AS event
                   WHERE event.organization_id = target.organization_id
                     AND event.actor_principal_id = target.agent_id
                     AND event.on_behalf_of_principal_id IS NULL
                     AND event.action IN (
                         'mail.submission.accepted',
                         'mail.submission.rejected',
                         'mail.submission.review_required'
                     )
                     AND event.resource_type = 'mail.submission'
                     AND event.resource_id = target.submission_id::text
                     AND event.occurred_at >= target.admitted_at
                     AND event.occurred_at <= target.submission_updated_at
                   ORDER BY event.occurred_at, event.id
                   LIMIT 1
              ) AS terminal_audit ON true
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(agent_id.as_uuid())
        .bind(submission_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        row.map(agent_mail_submission_evidence_from_row).transpose()
    }
}
