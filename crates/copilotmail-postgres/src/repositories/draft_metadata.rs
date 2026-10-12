use crate::*;

#[async_trait]
impl MailDraftMetadataStore for PgControlPlaneStore {
    async fn get_mail_draft(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        created_by: PrincipalId,
        draft_id: DraftId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDraftRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailDraftRow>(
            r#"
            SELECT id, organization_id, mailbox_id, created_by, mail_data_plane_id,
                   engine_account_ref, engine_email_ref, engine_thread_ref,
                   reply_to_message_id, reply_to_automatic, body_ref, content_sha256,
                   recipient_refs, recipient_domains,
                   version, status, created_at, updated_at
              FROM mail_drafts
             WHERE id = $1
               AND organization_id = $2
               AND mailbox_id = $3
               AND created_by = $4
               AND mail_data_plane_id = $5
            "#,
        )
        .bind(draft_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(created_by.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_draft_from_row).transpose()
    }

    async fn record_mail_draft(
        &self,
        write: MailDraftWrite,
    ) -> Result<MailDraftRecord, StoreError> {
        let recipient_refs = write
            .recipients
            .iter()
            .map(|recipient| recipient.recipient_ref.to_vec())
            .collect::<Vec<_>>();
        let recipient_domains = write
            .recipients
            .iter()
            .map(|recipient| recipient.domain.clone())
            .collect::<Vec<_>>();
        // The engine write already happened. Hold the grant until this
        // metadata insert commits so a revocation in that gap cannot persist.
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        if !crate::transactions::mailbox_send_capability_held(
            &mut transaction,
            write.organization_id,
            write.created_by,
            write.mailbox_id,
            write.now,
        )
        .await?
        {
            return Err(StoreError::MissingReference);
        }
        let row = if let Some(expected_version) = write.expected_version {
            sqlx::query_as::<_, MailDraftRow>(
                r#"
                UPDATE mail_drafts
                   SET engine_email_ref = $5,
                       engine_thread_ref = $6,
                       content_sha256 = $7,
                       body_ref = $8,
                       recipient_refs = $9,
                       recipient_domains = $10,
                       version = version + 1,
                       updated_at = $11
                 WHERE id = $1
                   AND organization_id = $2
                   AND mailbox_id = $3
                   AND created_by = $4
                   AND mail_data_plane_id = $12
                   AND engine_account_ref = $13
                   AND version = $14
                   AND status = 'active'
                   AND reply_to_message_id IS NOT DISTINCT FROM $15
                   AND reply_to_automatic = $16
                RETURNING id, organization_id, mailbox_id, created_by,
                          mail_data_plane_id, engine_account_ref, engine_email_ref,
                          engine_thread_ref, reply_to_message_id, reply_to_automatic,
                          body_ref, content_sha256, recipient_refs, recipient_domains,
                          version, status, created_at, updated_at
                "#,
            )
            .bind(write.draft_id.as_uuid())
            .bind(write.organization_id.as_uuid())
            .bind(write.mailbox_id.as_uuid())
            .bind(write.created_by.as_uuid())
            .bind(&write.engine_email_ref)
            .bind(&write.engine_thread_ref)
            .bind(write.content_sha256.as_slice())
            .bind(write.body_ref.as_slice())
            .bind(&recipient_refs)
            .bind(&recipient_domains)
            .bind(write.now)
            .bind(write.data_plane_id.as_uuid())
            .bind(&write.engine_account_ref)
            .bind(expected_version)
            .bind(write.reply_to_message_id)
            .bind(write.reply_to_automatic)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(map_store_error)?
            .ok_or(StoreError::Conflict)?
        } else {
            sqlx::query_as::<_, MailDraftRow>(
                r#"
                INSERT INTO mail_drafts(
                    id, organization_id, mailbox_id, created_by, mail_data_plane_id,
                    engine_source, engine_account_ref, engine_email_ref, engine_thread_ref,
                    reply_to_message_id, reply_to_automatic, body_ref, content_sha256,
                    recipient_refs, recipient_domains,
                    version, status, created_at, updated_at
                )
                VALUES (
                    $1, $2, $3, $4, $5, 'stalwart', $6, $7, $8, $9, $10, $11, $12,
                    $13, $14, 1, 'active', $15, $15
                )
                RETURNING id, organization_id, mailbox_id, created_by,
                          mail_data_plane_id, engine_account_ref, engine_email_ref,
                          engine_thread_ref, reply_to_message_id, reply_to_automatic,
                          body_ref, content_sha256, recipient_refs, recipient_domains,
                          version, status, created_at, updated_at
                "#,
            )
            .bind(write.draft_id.as_uuid())
            .bind(write.organization_id.as_uuid())
            .bind(write.mailbox_id.as_uuid())
            .bind(write.created_by.as_uuid())
            .bind(write.data_plane_id.as_uuid())
            .bind(&write.engine_account_ref)
            .bind(&write.engine_email_ref)
            .bind(&write.engine_thread_ref)
            .bind(write.reply_to_message_id)
            .bind(write.reply_to_automatic)
            .bind(write.body_ref.as_slice())
            .bind(write.content_sha256.as_slice())
            .bind(&recipient_refs)
            .bind(&recipient_domains)
            .bind(write.now)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?
        };
        transaction.commit().await.map_err(map_store_error)?;
        mail_draft_from_row(row)
    }
}
