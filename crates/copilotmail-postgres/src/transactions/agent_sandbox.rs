use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_create_agent_mail_sandbox_submission(
        &mut self,
        write: &AgentMailSandboxSubmissionWrite,
    ) -> Result<AgentMailSandboxSubmissionCreation, StoreError> {
        sqlx::query(
            r#"
            SELECT pg_advisory_xact_lock(
                hashtextextended($1::text || ':' || $2::text, 0)
            )
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let existing = sqlx::query_as::<_, AgentMailSandboxSubmissionRow>(
            r#"
            SELECT id, organization_id, agent_id, mailbox_id, draft_id,
                   draft_version, request_sha256, recipient_count, captured_at
              FROM agent_mail_sandbox_submissions
             WHERE organization_id = $1
               AND agent_id = $2
               AND idempotency_key_sha256 = $3
             FOR UPDATE
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            if existing.request_sha256.as_slice() != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            return Ok(AgentMailSandboxSubmissionCreation {
                record: agent_mail_sandbox_submission_from_row(existing)?,
                created: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct FrozenSandboxDraftRow {
            draft_version: i64,
            body_ref: Vec<u8>,
            recipient_count: i32,
        }
        let frozen = sqlx::query_as::<_, FrozenSandboxDraftRow>(
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
            RETURNING version AS draft_version,
                      body_ref,
                      cardinality(recipient_refs)::integer AS recipient_count
            "#,
        )
        .bind(write.draft_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .bind(write.expected_draft_version)
        .bind(write.now)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;

        let row = sqlx::query_as::<_, AgentMailSandboxSubmissionRow>(
            r#"
            INSERT INTO agent_mail_sandbox_submissions(
                id, organization_id, agent_id, mailbox_id, draft_id,
                draft_version, idempotency_key_sha256, request_sha256,
                body_ref, recipient_count, captured_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id, organization_id, agent_id, mailbox_id, draft_id,
                      draft_version, request_sha256, recipient_count, captured_at
            "#,
        )
        .bind(write.submission_id)
        .bind(write.organization_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.draft_id.as_uuid())
        .bind(frozen.draft_version)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(frozen.body_ref)
        .bind(frozen.recipient_count)
        .bind(write.now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(AgentMailSandboxSubmissionCreation {
            record: agent_mail_sandbox_submission_from_row(row)?,
            created: true,
        })
    }
}
