use crate::*;

#[async_trait]
impl HumanMailContentQueryStore for PgControlPlaneStore {
    async fn resolve_human_mail_message_projections(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
        engine_account_ref: &str,
        document_refs: &[String],
    ) -> Result<Vec<HumanMailMessageProjection>, StoreError> {
        if document_refs.is_empty() {
            return Ok(Vec::new());
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            message_id: Uuid,
            document_ref: String,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT event.message_id, event.document_ref
              FROM unnest($5::text[]) WITH ORDINALITY
                   AS requested(document_ref, ordinal)
              JOIN inbound_mail_engine_events AS event
                ON event.document_ref = requested.document_ref
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $2
               AND event.mail_data_plane_id = $3
               AND event.account_ref = $4
             ORDER BY requested.ordinal
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .bind(engine_account_ref)
        .bind(document_refs)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;

        Ok(rows
            .into_iter()
            .map(|row| HumanMailMessageProjection {
                message_id: row.message_id,
                engine_document_ref: row.document_ref,
            })
            .collect())
    }

    async fn resolve_human_mail_content_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<HumanMailContentTarget>, StoreError> {
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

        Ok(row.map(|row| HumanMailContentTarget {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            message_id: row.message_id,
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            engine_account_ref: row.account_ref,
            engine_document_ref: row.document_ref,
        }))
    }
}
