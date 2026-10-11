use crate::*;

#[async_trait]
impl MailEventIngestionTransaction for PgControlPlaneTransaction {
    async fn resolve_mail_engine_mailbox_binding(
        &mut self,
        data_plane_id: MailDataPlaneId,
        account_ref: &str,
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
             WHERE mail_data_plane_id = $1
               AND account_ref = $2
             FOR SHARE
            "#,
        )
        .bind(data_plane_id.as_uuid())
        .bind(account_ref)
        .fetch_optional(&mut **self.transaction()?)
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

    async fn insert_inbound_mail_event(
        &mut self,
        record: &InboundMailEventRecord,
    ) -> Result<InboundMailEventInsert, StoreError> {
        let inserted = sqlx::query_scalar::<_, bool>(
            r#"
            INSERT INTO inbound_mail_engine_events(
                event_id, message_id, organization_id, mailbox_id,
                mail_data_plane_id, source, source_event_id, source_event_sha256,
                account_ref, document_ref, classification,
                source_occurred_at, received_at
            )
            VALUES (
                $1, $2, $3, $4, $5, 'stalwart', $6, $7, $8, $9, $10, $11, $12
            )
            ON CONFLICT DO NOTHING
            RETURNING true
            "#,
        )
        .bind(record.event_id)
        .bind(record.message_id)
        .bind(record.organization_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .bind(record.source_event_sha256.as_slice())
        .bind(&record.account_ref)
        .bind(&record.document_ref)
        .bind(&record.classification)
        .bind(record.source_occurred_at)
        .bind(record.received_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .unwrap_or(false);
        if inserted {
            return Ok(InboundMailEventInsert::Inserted);
        }

        #[derive(sqlx::FromRow)]
        struct ExistingRow {
            organization_id: Uuid,
            mailbox_id: Uuid,
            source_event_id: String,
            source_event_sha256: Vec<u8>,
            account_ref: String,
            document_ref: String,
            classification: String,
        }
        let existing = sqlx::query_as::<_, ExistingRow>(
            r#"
            SELECT organization_id, mailbox_id, source_event_id,
                   source_event_sha256, account_ref, document_ref, classification
              FROM inbound_mail_engine_events
             WHERE mail_data_plane_id = $1
               AND (
                   source_event_id = $2
                   OR (account_ref = $3 AND document_ref = $4)
               )
             FOR SHARE
            "#,
        )
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .bind(&record.account_ref)
        .bind(&record.document_ref)
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if existing.len() != 1 {
            return Err(StoreError::Conflict);
        }
        let existing = &existing[0];
        if existing.source_event_id == record.source_event_id {
            if existing.source_event_sha256.as_slice() == record.source_event_sha256 {
                return Ok(InboundMailEventInsert::Duplicate);
            }
            return Err(StoreError::Conflict);
        }
        let same_message = existing.organization_id == record.organization_id.as_uuid()
            && existing.mailbox_id == record.mailbox_id.as_uuid()
            && existing.account_ref == record.account_ref
            && existing.document_ref == record.document_ref
            && existing.classification == record.classification;
        if same_message {
            Ok(InboundMailEventInsert::Duplicate)
        } else {
            Err(StoreError::Conflict)
        }
    }

    async fn fanout_agent_message_states(
        &mut self,
        record: &InboundMailEventRecord,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            INSERT INTO agent_message_states(
                subscription_id, message_id, organization_id, principal_id,
                state, attempt_count, state_version, updated_at
            )
            SELECT subscription.id, $1, subscription.organization_id,
                   subscription.principal_id, 'pending', 0, 0, $4
              FROM agent_subscriptions AS subscription
             WHERE subscription.organization_id = $2
               AND subscription.mailbox_id = $3
               AND subscription.status IN ('active', 'paused')
            ON CONFLICT(subscription_id, message_id) DO NOTHING
            "#,
        )
        .bind(record.message_id)
        .bind(record.organization_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.received_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected())
    }

    async fn resolve_mail_engine_mailbox_by_primary_address(
        &mut self,
        data_plane_id: MailDataPlaneId,
        primary_address: &copilotmail_domain::MailboxAddress,
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
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT binding.organization_id, binding.mailbox_id,
                   binding.mail_data_plane_id, binding.account_ref,
                   binding.version, binding.updated_at
              FROM mail_engine_mailbox_bindings AS binding
              JOIN mailboxes AS mailbox
                ON mailbox.id = binding.mailbox_id
               AND mailbox.organization_id = binding.organization_id
             WHERE binding.mail_data_plane_id = $1
               AND mailbox.primary_address = $2
             ORDER BY binding.organization_id, binding.mailbox_id
             LIMIT 2
             FOR SHARE OF binding, mailbox
            "#,
        )
        .bind(data_plane_id.as_uuid())
        .bind(primary_address.as_str())
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if rows.len() > 1 {
            return Err(StoreError::Conflict);
        }
        Ok(rows.into_iter().next().map(|row| MailEngineMailboxBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            account_ref: row.account_ref,
            version: row.version,
            updated_at: row.updated_at,
        }))
    }

    async fn insert_outbound_mail_event(
        &mut self,
        record: &OutboundMailEventRecord,
    ) -> Result<OutboundMailEventInsert, StoreError> {
        let inserted = sqlx::query_scalar::<_, bool>(
            r#"
            INSERT INTO outbound_mail_engine_events(
                event_id, organization_id, mailbox_id, mail_data_plane_id, source,
                source_event_id, source_event_sha256, queue_ref, event_kind,
                recipient_ref, smtp_code, next_retry_at,
                source_occurred_at, received_at
            )
            VALUES (
                $1, $2, $3, $4, 'stalwart', $5, $6, $7, $8,
                $9, $10, $11, $12, $13
            )
            ON CONFLICT (mail_data_plane_id, source_event_id) DO NOTHING
            RETURNING true
            "#,
        )
        .bind(record.event_id)
        .bind(record.organization_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .bind(record.source_event_sha256.as_slice())
        .bind(&record.queue_ref)
        .bind(outbound_mail_event_kind(record.kind))
        .bind(record.recipient_ref.as_ref().map(<[u8; 32]>::as_slice))
        .bind(
            record
                .smtp_code
                .map(|value| i16::try_from(value).map_err(|_| StoreError::Conflict))
                .transpose()?,
        )
        .bind(record.next_retry_at)
        .bind(record.source_occurred_at)
        .bind(record.received_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .unwrap_or(false);
        if inserted {
            return Ok(OutboundMailEventInsert::Inserted);
        }
        let existing = sqlx::query_scalar::<_, Vec<u8>>(
            r#"
            SELECT source_event_sha256
              FROM outbound_mail_engine_events
             WHERE mail_data_plane_id = $1
               AND source_event_id = $2
             FOR SHARE
            "#,
        )
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;
        if existing.as_slice() == record.source_event_sha256 {
            Ok(OutboundMailEventInsert::Duplicate)
        } else {
            Err(StoreError::Conflict)
        }
    }

    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO outbox_events(
                id, event_type, aggregate_type, aggregate_id,
                schema_version, payload, mail_data_plane_id, occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(event.id)
        .bind(&event.event_type)
        .bind(&event.aggregate_type)
        .bind(&event.aggregate_id)
        .bind(i32::try_from(event.schema_version).map_err(|_| StoreError::Conflict)?)
        .bind(&event.payload)
        .bind(event.data_plane_id.map(MailDataPlaneId::as_uuid))
        .bind(event.occurred_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn commit(mut self: Box<Self>) -> Result<(), StoreError> {
        self.transaction
            .take()
            .ok_or(StoreError::Unavailable)?
            .commit()
            .await
            .map_err(map_store_error)
    }
}
