use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_create_mail_submission_intent(
        &mut self,
        write: &MailSubmissionIntentWrite,
    ) -> Result<MailSubmissionIntentCreation, StoreError> {
        sqlx::query(
            r#"
            SELECT pg_advisory_xact_lock(
                hashtextextended(
                    $1::text || ':' || $2::text || ':' || encode($3::bytea, 'hex'),
                    0
                )
            )
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.idempotency_key_sha256.as_slice())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let existing = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id, draft_id,
                   draft_version, mail_data_plane_id, request_sha256, status,
                   state_version, created_at, updated_at
              FROM mail_submission_intents
             WHERE organization_id = $1
               AND requested_by = $2
               AND idempotency_key_sha256 = $3
             FOR UPDATE
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            let intent = mail_submission_intent_from_row(existing)?;
            if intent.request_sha256 != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            let production_admission = sqlx::query_as::<_, AgentMailAdmissionRow>(
                r#"
                SELECT id, organization_id, agent_id, submission_id,
                       policy_version_id, policy_version, recipient_count,
                       first_contact_count, admitted_at
                  FROM agent_mail_admissions
                 WHERE submission_id = $1
                "#,
            )
            .bind(intent.submission_id.as_uuid())
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?
            .map(agent_mail_admission_from_row)
            .transpose()?;
            return Ok(MailSubmissionIntentCreation {
                intent,
                created: false,
                production_admission,
            });
        }

        #[derive(sqlx::FromRow)]
        struct FrozenDraftRow {
            mail_data_plane_id: Uuid,
            engine_account_ref: String,
            engine_email_ref: String,
            draft_version: i64,
        }
        let frozen = sqlx::query_as::<_, FrozenDraftRow>(
            r#"
            UPDATE mail_drafts
               SET status = 'frozen',
                   version = version + 1,
                   updated_at = $6
             WHERE id = $1
               AND organization_id = $2
               AND mailbox_id = $3
               AND created_by = $4
               AND version = $5
               AND status = 'active'
            RETURNING mail_data_plane_id, engine_account_ref, engine_email_ref,
                      version AS draft_version
            "#,
        )
        .bind(write.draft_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.expected_draft_version)
        .bind(write.now)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;

        let row = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            INSERT INTO mail_submission_intents(
                id, organization_id, requested_by, mailbox_id, draft_id,
                draft_version, idempotency_key_sha256, request_sha256,
                mail_data_plane_id, engine_source, engine_account_ref,
                engine_email_ref, status,
                state_version, attempt_count, created_at, updated_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                $9, 'stalwart', $10, $11, 'pending', 1, 0,
                clock_timestamp(), clock_timestamp()
            )
            RETURNING id, organization_id, requested_by, mailbox_id, draft_id,
                      draft_version, mail_data_plane_id, request_sha256, status,
                      state_version, created_at, updated_at
            "#,
        )
        .bind(write.submission_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.draft_id.as_uuid())
        .bind(frozen.draft_version)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(frozen.mail_data_plane_id)
        .bind(&frozen.engine_account_ref)
        .bind(&frozen.engine_email_ref)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_agent_mail_admission_error)?;
        let production_admission = sqlx::query_as::<_, AgentMailAdmissionRow>(
            r#"
            INSERT INTO agent_mail_admissions(
                id, organization_id, agent_id, submission_id,
                policy_activation_id, policy_version_id, policy_version,
                draft_id, draft_version, thread_ref, recipient_count,
                first_contact_count, first_contact_approval_request_id,
                admitted_at, mail_data_plane_id
            )
            SELECT $1, $2, $3, $4,
                   '00000000-0000-0000-0000-000000000000'::uuid,
                   '00000000-0000-0000-0000-000000000000'::uuid,
                   1, $5, $6, decode(repeat('00', 32), 'hex'), 1, 0, $7, $8,
                   $9
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
             WHERE principal.id = $3
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
               AND profile.environment = 'production'
            RETURNING id, organization_id, agent_id, submission_id,
                      policy_version_id, policy_version, recipient_count,
                      first_contact_count, admitted_at
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.submission_id.as_uuid())
        .bind(write.draft_id.as_uuid())
        .bind(frozen.draft_version)
        .bind(
            write
                .first_contact_approval_request_id
                .map(copilotmail_domain::ApprovalRequestId::as_uuid),
        )
        .bind(write.now)
        .bind(frozen.mail_data_plane_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_agent_mail_admission_error)?
        .map(agent_mail_admission_from_row)
        .transpose()?;
        Ok(MailSubmissionIntentCreation {
            intent: mail_submission_intent_from_row(row)?,
            created: true,
            production_admission,
        })
    }

    pub(crate) async fn tx_acquire_mail_submission_lease(
        &mut self,
        submission_id: MailSubmissionId,
        expected_data_plane_id: MailDataPlaneId,
        lease_id: Uuid,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Option<MailSubmissionLease>, StoreError> {
        if lease_expires_at <= now || lease_expires_at > now + chrono::Duration::minutes(5) {
            return Err(StoreError::Conflict);
        }
        let row = sqlx::query_as::<_, MailSubmissionLeaseRow>(
            r#"
            UPDATE mail_submission_intents AS intent
               SET status = 'reconciling',
                   state_version = intent.state_version + 1,
                   lease_id = $3,
                   lease_expires_at = clock_timestamp() + ($5 - $4),
                   attempt_count = intent.attempt_count + 1,
                   updated_at = clock_timestamp()
              FROM mailboxes AS mailbox,
                   principals AS requester
             WHERE intent.id = $1
               AND intent.mail_data_plane_id = $2
               AND intent.status IN ('pending', 'reconciling')
               AND (
                    intent.lease_id IS NULL
                    OR intent.lease_expires_at <= clock_timestamp()
               )
               AND mailbox.id = intent.mailbox_id
               AND mailbox.organization_id = intent.organization_id
               AND requester.id = intent.requested_by
               AND requester.organization_id = intent.organization_id
               AND (
                   requester.kind = 'human'
                   OR (
                       requester.kind = 'agent'
                       AND EXISTS (
                           SELECT 1
                             FROM agent_profiles AS agent
                            WHERE agent.principal_id = requester.id
                              AND (
                                  agent.environment IN ('development', 'testing')
                                  OR (
                                      agent.environment = 'production'
                                      AND EXISTS (
                                          SELECT 1
                                            FROM agent_mail_admissions AS admission
                                           WHERE admission.organization_id = intent.organization_id
                                             AND admission.agent_id = intent.requested_by
                                             AND admission.submission_id = intent.id
                                      )
                                  )
                              )
                       )
                   )
               )
               AND requester.status = 'active'
            RETURNING intent.id, intent.organization_id, intent.requested_by,
                      intent.mailbox_id, intent.draft_id, intent.draft_version,
                      intent.mail_data_plane_id, intent.engine_account_ref,
                      intent.engine_email_ref, mailbox.primary_address,
                      intent.lease_id, intent.lease_expires_at,
                      intent.attempt_count
            "#,
        )
        .bind(submission_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .bind(lease_id)
        .bind(now)
        .bind(lease_expires_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(mail_submission_lease_from_row).transpose()
    }

    pub(crate) async fn tx_finalize_mail_submission_intent(
        &mut self,
        write: &MailSubmissionTerminalWrite,
    ) -> Result<Option<MailSubmissionTerminalTransition>, StoreError> {
        let (status, identity_ref, submission_ref, error_code) = match write.status {
            MailSubmissionStatus::Accepted
                if write.engine_identity_ref.is_some()
                    && write.engine_submission_ref.is_some()
                    && write.error_code.is_none() =>
            {
                (
                    "accepted",
                    write.engine_identity_ref.as_deref(),
                    write.engine_submission_ref.as_deref(),
                    None,
                )
            }
            MailSubmissionStatus::Rejected
                if write.engine_submission_ref.is_none() && write.error_code.is_some() =>
            {
                (
                    "rejected",
                    write.engine_identity_ref.as_deref(),
                    None,
                    write.error_code.as_deref(),
                )
            }
            MailSubmissionStatus::ManualReview
                if write.engine_identity_ref.is_none()
                    && write.engine_submission_ref.is_none()
                    && write.error_code.is_some() =>
            {
                ("manual_review", None, None, write.error_code.as_deref())
            }
            _ => return Err(StoreError::Conflict),
        };

        #[derive(sqlx::FromRow)]
        struct FinalizedIntentRow {
            draft_id: Uuid,
            draft_version: i64,
            terminal_at: DateTime<Utc>,
            accepted_at: Option<DateTime<Utc>>,
        }
        let finalized = sqlx::query_as::<_, FinalizedIntentRow>(
            r#"
            WITH terminal_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS terminal_at
            )
            UPDATE mail_submission_intents AS intent
               SET status = $4,
                   state_version = intent.state_version + 1,
                   engine_identity_ref = $5,
                   engine_submission_ref = $6,
                   error_code = $7,
                   lease_id = NULL,
                   lease_expires_at = NULL,
                   updated_at = terminal_clock.terminal_at,
                   accepted_at = CASE
                       WHEN $4 = 'accepted' THEN terminal_clock.terminal_at
                       ELSE NULL
                   END
              FROM terminal_clock
             WHERE intent.id = $1
               AND intent.organization_id = $2
               AND intent.status = 'reconciling'
               AND intent.lease_id = $3
               AND intent.lease_expires_at > terminal_clock.terminal_at
            RETURNING intent.draft_id,
                      intent.draft_version,
                      intent.updated_at AS terminal_at,
                      intent.accepted_at
            "#,
        )
        .bind(write.submission_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.lease_id)
        .bind(status)
        .bind(identity_ref)
        .bind(submission_ref)
        .bind(error_code)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(finalized) = finalized else {
            return Ok(None);
        };

        let accepted_at = match (write.status, finalized.accepted_at) {
            (MailSubmissionStatus::Accepted, Some(accepted_at))
                if accepted_at == finalized.terminal_at =>
            {
                Some(accepted_at)
            }
            (MailSubmissionStatus::Accepted, _)
            | (MailSubmissionStatus::Rejected | MailSubmissionStatus::ManualReview, Some(_)) => {
                return Err(StoreError::Unavailable);
            }
            (MailSubmissionStatus::Rejected | MailSubmissionStatus::ManualReview, None) => None,
            (MailSubmissionStatus::Pending | MailSubmissionStatus::Reconciling, _) => {
                return Err(StoreError::Unavailable);
            }
        };

        if write.status == MailSubmissionStatus::Accepted {
            let updated = sqlx::query_scalar::<_, i32>(
                r#"
                UPDATE mail_drafts
                   SET status = 'sent',
                       version = version + 1,
                       updated_at = GREATEST(updated_at, $4)
                 WHERE id = $1
                   AND organization_id = $2
                   AND version = $3
                   AND status = 'frozen'
                RETURNING 1
                "#,
            )
            .bind(finalized.draft_id)
            .bind(write.organization_id.as_uuid())
            .bind(finalized.draft_version)
            .bind(finalized.terminal_at)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
            if updated.is_none() {
                return Err(StoreError::Conflict);
            }
        }
        Ok(Some(MailSubmissionTerminalTransition {
            terminal_at: finalized.terminal_at,
            accepted_at,
        }))
    }
}
