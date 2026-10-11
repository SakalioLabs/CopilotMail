use crate::*;

#[async_trait]
impl MailReplyTargetQueryStore for PgControlPlaneStore {
    async fn resolve_mail_reply_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailReplyTarget>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            message_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            document_ref: String,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT event.organization_id, event.mailbox_id, event.message_id,
                   event.mail_data_plane_id, event.account_ref, event.document_ref
              FROM inbound_mail_engine_events AS event
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $2
               AND event.message_id = $3
               AND event.mail_data_plane_id = $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(message_id)
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailReplyTarget {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            message_id: row.message_id,
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            engine_account_ref: row.account_ref,
            engine_document_ref: row.document_ref,
        }))
    }

    async fn resolve_mail_reply_message_id_by_engine_document(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
        engine_document_ref: &str,
    ) -> Result<Option<Uuid>, StoreError> {
        let message_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT event.message_id
              FROM inbound_mail_engine_events AS event
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $2
               AND event.mail_data_plane_id = $3
               AND event.document_ref = $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .bind(engine_document_ref)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(message_id)
    }
}
