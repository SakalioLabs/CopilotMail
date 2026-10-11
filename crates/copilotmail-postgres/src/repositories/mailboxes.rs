use crate::*;

#[async_trait]
impl MailboxQueryStore for PgControlPlaneStore {
    async fn get_mailbox(
        &self,
        mailbox_id: MailboxId,
    ) -> Result<Option<MailboxSummary>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            kind: String,
            primary_address: String,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, kind, primary_address, created_at
              FROM mailboxes
             WHERE id = $1
            "#,
        )
        .bind(mailbox_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let kind = mailbox_kind_from_str(&row.kind).ok_or(StoreError::Unavailable)?;
            let primary_address = copilotmail_domain::MailboxAddress::parse(row.primary_address)
                .map_err(|_| StoreError::Unavailable)?;
            Ok(MailboxSummary {
                mailbox_id: MailboxId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                kind,
                primary_address,
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    async fn get_mail_engine_mailbox_binding(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            version: i64,
            updated_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT organization_id, mailbox_id, mail_data_plane_id, account_ref,
                   version, updated_at
              FROM mail_engine_mailbox_bindings
             WHERE organization_id = $1
               AND mailbox_id = $2
               AND mail_data_plane_id = $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailEngineMailboxBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            account_ref: row.account_ref,
            version: row.version,
            updated_at: row.updated_at,
        }))
    }
}
