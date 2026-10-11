use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_append_mail_delivery_snapshot(
        &mut self,
        write: &MailDeliverySnapshotWrite,
    ) -> Result<MailDeliverySnapshotInsert, StoreError> {
        if write.recipients.is_empty() || write.recipients.len() > 100 {
            return Err(StoreError::Conflict);
        }
        let mut delivered = 0_i32;
        let mut failed = 0_i32;
        let mut queued = 0_i32;
        let mut unknown = 0_i32;
        for recipient in &write.recipients {
            match recipient.state {
                MailDeliveryRecipientState::Delivered => delivered += 1,
                MailDeliveryRecipientState::PermanentFailure => failed += 1,
                MailDeliveryRecipientState::Queued => queued += 1,
                MailDeliveryRecipientState::Unknown => unknown += 1,
            }
        }
        let recipient_count =
            i32::try_from(write.recipients.len()).map_err(|_| StoreError::Conflict)?;
        let terminal = queued == 0 && unknown == 0;
        #[derive(sqlx::FromRow)]
        struct InsertedDeliverySnapshotRow {
            id: Uuid,
            observed_at: DateTime<Utc>,
        }
        let inserted = sqlx::query_as::<_, InsertedDeliverySnapshotRow>(
            r#"
            WITH snapshot_clock AS MATERIALIZED (
                SELECT GREATEST(submission.accepted_at, clock_timestamp()) AS observed_at
                  FROM mail_submission_intents AS submission
                 WHERE submission.id = $3
                   AND submission.organization_id = $2
                   AND submission.mail_data_plane_id = $4
                   AND submission.status = 'accepted'
                   AND submission.accepted_at IS NOT NULL
            )
            INSERT INTO mail_delivery_snapshots(
                id, organization_id, submission_id, mail_data_plane_id, source,
                source_state, source_sha256, recipient_count, delivered_count,
                failed_count, queued_count, unknown_count, terminal,
                observed_at, recorded_at
            )
            SELECT
                $1, $2, $3, $4, 'stalwart_jmap', $5, $6, $7, $8, $9, $10, $11, $12,
                snapshot_clock.observed_at,
                snapshot_clock.observed_at
              FROM snapshot_clock
            ON CONFLICT (
                organization_id, submission_id, mail_data_plane_id, source_sha256
            )
            DO NOTHING
            RETURNING id, observed_at
            "#,
        )
        .bind(write.snapshot_id)
        .bind(write.organization_id.as_uuid())
        .bind(write.submission_id.as_uuid())
        .bind(write.data_plane_id.as_uuid())
        .bind(&write.source_state)
        .bind(write.source_sha256.as_slice())
        .bind(recipient_count)
        .bind(delivered)
        .bind(failed)
        .bind(queued)
        .bind(unknown)
        .bind(terminal)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(inserted) = inserted else {
            // `INSERT .. SELECT` also returns no row when the accepted target
            // disappeared from the authoritative CTE. Do not mistake that
            // integrity failure for an idempotent source replay: only an exact
            // existing source fingerprint is a duplicate.
            let duplicate = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1
                      FROM mail_delivery_snapshots AS snapshot
                     WHERE snapshot.organization_id = $1
                       AND snapshot.submission_id = $2
                       AND snapshot.mail_data_plane_id = $3
                       AND snapshot.source_sha256 = $4
                )
                "#,
            )
            .bind(write.organization_id.as_uuid())
            .bind(write.submission_id.as_uuid())
            .bind(write.data_plane_id.as_uuid())
            .bind(write.source_sha256.as_slice())
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
            if duplicate {
                return Ok(MailDeliverySnapshotInsert::Duplicate);
            }
            return Err(StoreError::Conflict);
        };
        let snapshot_id = inserted.id;
        for recipient in &write.recipients {
            sqlx::query(
                r#"
                INSERT INTO mail_delivery_recipient_evidence(
                    snapshot_id, organization_id, submission_id,
                    recipient_ref, state, smtp_reply
                )
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(snapshot_id)
            .bind(write.organization_id.as_uuid())
            .bind(write.submission_id.as_uuid())
            .bind(recipient.recipient_ref.as_slice())
            .bind(mail_delivery_recipient_state(recipient.state))
            .bind(&recipient.smtp_reply)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        }
        Ok(MailDeliverySnapshotInsert::Inserted {
            observed_at: inserted.observed_at,
        })
    }
}
