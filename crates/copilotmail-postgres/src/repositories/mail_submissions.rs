use crate::*;

#[async_trait]
impl MailSubmissionQueryStore for PgControlPlaneStore {
    async fn get_mail_submission_intent(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id, draft_id,
                   draft_version, mail_data_plane_id, request_sha256, status,
                   state_version, created_at, updated_at
              FROM mail_submission_intents
             WHERE id = $1
               AND organization_id = $2
               AND requested_by = $3
            "#,
        )
        .bind(submission_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(requested_by.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_submission_intent_from_row).transpose()
    }

    async fn get_mail_submission_intent_for_execution(
        &self,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id, draft_id,
                   draft_version, mail_data_plane_id, request_sha256, status,
                   state_version, created_at, updated_at
              FROM mail_submission_intents
             WHERE id = $1
            "#,
        )
        .bind(submission_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_submission_intent_from_row).transpose()
    }
}
