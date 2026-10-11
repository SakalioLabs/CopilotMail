use crate::*;

#[async_trait]
impl HumanSentQueryStore for PgControlPlaneStore {
    async fn list_human_sent_submissions(
        &self,
        query: &HumanSentListQuery,
    ) -> Result<HumanSentListBatch, StoreError> {
        if !(1..=50).contains(&query.limit) {
            return Err(StoreError::Conflict);
        }

        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;

        let active_human = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(query.principal_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !active_human {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanSentListBatch {
                active_human: false,
                cursor_valid: false,
                authorization: None,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let Some(authorization) = load_human_sent_access_snapshot(
            &mut transaction,
            query.organization_id,
            query.principal_id,
            query.mailbox_id,
            query.now,
        )
        .await?
        else {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanSentListBatch {
                active_human: true,
                cursor_valid: query.after_submission_id.is_none(),
                authorization: None,
                candidates: Vec::new(),
                has_more: false,
            });
        };
        if !human_draft_access_permits_read_and_send(
            &authorization,
            query.organization_id,
            query.mailbox_id,
        ) {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanSentListBatch {
                active_human: true,
                cursor_valid: query.after_submission_id.is_none(),
                authorization: Some(authorization),
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let cursor_accepted_at = if let Some(after_submission_id) = query.after_submission_id {
            sqlx::query_scalar::<_, DateTime<Utc>>(
                r#"
                SELECT intent.accepted_at
                  FROM mail_submission_intents AS intent
                  JOIN mail_drafts AS draft
                    ON draft.id = intent.draft_id
                   AND draft.organization_id = intent.organization_id
                   AND draft.mailbox_id = intent.mailbox_id
                   AND draft.created_by = intent.requested_by
                   AND draft.mail_data_plane_id = intent.mail_data_plane_id
                   AND draft.status = 'sent'
                   AND draft.version = intent.draft_version + 1
                   AND draft.recipient_refs IS NOT NULL
                   AND draft.recipient_domains IS NOT NULL
                   AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
                   AND cardinality(draft.recipient_refs)
                       = cardinality(draft.recipient_domains)
                 WHERE intent.id = $1
                   AND intent.organization_id = $2
                   AND intent.mailbox_id = $3
                   AND intent.requested_by = $4
                   AND intent.status = 'accepted'
                   AND intent.accepted_at IS NOT NULL
                "#,
            )
            .bind(after_submission_id.as_uuid())
            .bind(query.organization_id.as_uuid())
            .bind(query.mailbox_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            Some(query.now)
        };
        if query.after_submission_id.is_some() && cursor_accepted_at.is_none() {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanSentListBatch {
                active_human: true,
                cursor_valid: false,
                authorization: Some(authorization),
                candidates: Vec::new(),
                has_more: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            mailbox_id: Uuid,
            requested_by: Uuid,
            accepted_at: DateTime<Utc>,
            recipient_count: i32,
            snapshot_id: Option<Uuid>,
            observed_at: Option<DateTime<Utc>>,
            terminal: Option<bool>,
            delivery_recipient_count: Option<i32>,
            delivered_count: Option<i32>,
            failed_count: Option<i32>,
            queued_count: Option<i32>,
            unknown_count: Option<i32>,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT intent.id, intent.organization_id, intent.mailbox_id,
                   intent.requested_by, intent.accepted_at,
                   cardinality(draft.recipient_refs) AS recipient_count,
                   snapshot.id AS snapshot_id,
                   snapshot.observed_at,
                   snapshot.terminal,
                   snapshot.recipient_count AS delivery_recipient_count,
                   snapshot.delivered_count,
                   snapshot.failed_count,
                   snapshot.queued_count,
                   snapshot.unknown_count
              FROM mail_submission_intents AS intent
              JOIN mail_drafts AS draft
                ON draft.id = intent.draft_id
               AND draft.organization_id = intent.organization_id
               AND draft.mailbox_id = intent.mailbox_id
               AND draft.created_by = intent.requested_by
               AND draft.mail_data_plane_id = intent.mail_data_plane_id
               AND draft.status = 'sent'
               AND draft.version = intent.draft_version + 1
               AND draft.recipient_refs IS NOT NULL
               AND draft.recipient_domains IS NOT NULL
               AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
               AND cardinality(draft.recipient_refs)
                   = cardinality(draft.recipient_domains)
              LEFT JOIN LATERAL (
                  SELECT id, observed_at, terminal, recipient_count,
                         delivered_count, failed_count, queued_count, unknown_count
                    FROM mail_delivery_snapshots
                   WHERE organization_id = intent.organization_id
                     AND submission_id = intent.id
                   ORDER BY observed_at DESC, id DESC
                   LIMIT 1
              ) AS snapshot ON true
             WHERE intent.organization_id = $1
               AND intent.mailbox_id = $2
               AND intent.requested_by = $3
               AND intent.status = 'accepted'
               AND intent.accepted_at IS NOT NULL
               AND (
                   $4::timestamptz IS NULL
                   OR (intent.accepted_at, intent.id) < ($4, $5)
               )
             ORDER BY intent.accepted_at DESC, intent.id DESC
             LIMIT $6
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.mailbox_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.after_submission_id.and(cursor_accepted_at))
        .bind(query.after_submission_id.map(MailSubmissionId::as_uuid))
        .bind(i64::from(query.limit) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let has_more = rows.len() > usize::from(query.limit);
        let candidates = rows
            .into_iter()
            .take(usize::from(query.limit))
            .map(|row| {
                let delivery = match (
                    row.snapshot_id,
                    row.observed_at,
                    row.terminal,
                    row.delivery_recipient_count,
                    row.delivered_count,
                    row.failed_count,
                    row.queued_count,
                    row.unknown_count,
                ) {
                    (None, None, None, None, None, None, None, None) => None,
                    (
                        Some(snapshot_id),
                        Some(observed_at),
                        Some(terminal),
                        Some(recipient_count),
                        Some(delivered_count),
                        Some(failed_count),
                        Some(queued_count),
                        Some(unknown_count),
                    ) => Some(HumanSentDeliveryEvidence {
                        snapshot_id,
                        observed_at,
                        terminal,
                        recipient_count: u16::try_from(recipient_count)
                            .map_err(|_| StoreError::Unavailable)?,
                        delivered_count: u16::try_from(delivered_count)
                            .map_err(|_| StoreError::Unavailable)?,
                        failed_count: u16::try_from(failed_count)
                            .map_err(|_| StoreError::Unavailable)?,
                        queued_count: u16::try_from(queued_count)
                            .map_err(|_| StoreError::Unavailable)?,
                        unknown_count: u16::try_from(unknown_count)
                            .map_err(|_| StoreError::Unavailable)?,
                    }),
                    _ => return Err(StoreError::Unavailable),
                };
                Ok(HumanSentListCandidate {
                    submission_id: MailSubmissionId::from_uuid(row.id),
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    mailbox_id: MailboxId::from_uuid(row.mailbox_id),
                    requested_by: PrincipalId::from_uuid(row.requested_by),
                    accepted_at: row.accepted_at,
                    recipient_count: u16::try_from(row.recipient_count)
                        .map_err(|_| StoreError::Unavailable)?,
                    delivery,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanSentListBatch {
            active_human: true,
            cursor_valid: true,
            authorization: Some(authorization),
            candidates,
            has_more,
        })
    }
}
