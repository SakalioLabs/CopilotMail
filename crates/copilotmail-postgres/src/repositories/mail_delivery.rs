use crate::*;

#[async_trait]
impl MailDeliveryQueryStore for PgControlPlaneStore {
    async fn get_mail_delivery_reconciliation_target(
        &self,
        submission_id: MailSubmissionId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDeliveryReconciliationTarget>, StoreError> {
        let row = sqlx::query_as::<_, MailDeliveryTargetRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id,
                   mail_data_plane_id, engine_account_ref, engine_email_ref,
                   engine_identity_ref, engine_submission_ref
              FROM mail_submission_intents
             WHERE id = $1
               AND mail_data_plane_id = $2
               AND status = 'accepted'
               AND engine_identity_ref IS NOT NULL
               AND engine_submission_ref IS NOT NULL
            "#,
        )
        .bind(submission_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailDeliveryReconciliationTarget {
            submission_id: MailSubmissionId::from_uuid(row.id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            requested_by: PrincipalId::from_uuid(row.requested_by),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            engine_account_ref: row.engine_account_ref,
            engine_email_ref: row.engine_email_ref,
            engine_identity_ref: row.engine_identity_ref,
            engine_submission_ref: row.engine_submission_ref,
        }))
    }

    async fn get_latest_mail_delivery_snapshot(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailDeliverySnapshotRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailDeliverySnapshotRow>(
            r#"
            SELECT snapshot.id, snapshot.organization_id,
                   snapshot.submission_id, snapshot.source_state,
                   snapshot.source_sha256, snapshot.terminal,
                   snapshot.observed_at
              FROM mail_delivery_snapshots AS snapshot
              JOIN mail_submission_intents AS submission
                ON submission.id = snapshot.submission_id
               AND submission.organization_id = snapshot.organization_id
             WHERE snapshot.organization_id = $1
               AND snapshot.submission_id = $2
               AND submission.requested_by = $3
             ORDER BY snapshot.observed_at DESC, snapshot.id DESC
             LIMIT 1
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(submission_id.as_uuid())
        .bind(requested_by.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let recipients = sqlx::query_as::<_, MailDeliveryRecipientRow>(
            r#"
            SELECT recipient_ref, state, smtp_reply
              FROM mail_delivery_recipient_evidence
             WHERE snapshot_id = $1
               AND organization_id = $2
               AND submission_id = $3
             ORDER BY recipient_ref
            "#,
        )
        .bind(row.id)
        .bind(row.organization_id)
        .bind(row.submission_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?
        .into_iter()
        .map(mail_delivery_recipient_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        let source_sha256 = digest_from_bytes(row.source_sha256, "mail delivery source")?;
        Ok(Some(MailDeliverySnapshotRecord {
            snapshot_id: row.id,
            organization_id: OrganizationId::from_uuid(row.organization_id),
            submission_id: MailSubmissionId::from_uuid(row.submission_id),
            source_state: row.source_state,
            source_sha256,
            recipients,
            terminal: row.terminal,
            observed_at: row.observed_at,
        }))
    }
}
