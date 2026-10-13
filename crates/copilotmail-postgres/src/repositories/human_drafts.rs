use crate::*;

#[async_trait]
impl HumanDraftQueryStore for PgControlPlaneStore {
    async fn list_human_drafts(
        &self,
        query: &HumanDraftListQuery,
    ) -> Result<HumanDraftListBatch, StoreError> {
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
            return Ok(HumanDraftListBatch {
                active_human: false,
                cursor_valid: false,
                authorization: None,
                binding: None,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let Some((mailbox, binding, authorization)) = load_human_draft_access_snapshot(
            &mut transaction,
            query.organization_id,
            query.principal_id,
            query.mailbox_id,
            query.expected_data_plane_id,
            query.now,
        )
        .await?
        else {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftListBatch {
                active_human: true,
                cursor_valid: query.after_draft_id.is_none(),
                authorization: None,
                binding: None,
                candidates: Vec::new(),
                has_more: false,
            });
        };
        debug_assert_eq!(mailbox.mailbox_id, query.mailbox_id);

        if !human_draft_access_permits_read_and_send(
            &authorization,
            query.organization_id,
            query.mailbox_id,
        ) {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftListBatch {
                active_human: true,
                cursor_valid: query.after_draft_id.is_none(),
                authorization: Some(authorization),
                binding: Some(binding),
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let cursor_valid = if let Some(after_draft_id) = query.after_draft_id {
            sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                      FROM mail_drafts AS draft
                     WHERE draft.id = $1
                       AND draft.organization_id = $2
                       AND draft.mailbox_id = $3
                       AND draft.created_by = $4
                       AND draft.mail_data_plane_id = $5
                       AND draft.engine_account_ref = $6
                       AND draft.status = 'active'
                       AND draft.recipient_refs IS NOT NULL
                       AND draft.recipient_domains IS NOT NULL
                       AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
                       AND cardinality(draft.recipient_refs)
                           = cardinality(draft.recipient_domains)
                )
                "#,
            )
            .bind(after_draft_id.as_uuid())
            .bind(query.organization_id.as_uuid())
            .bind(query.mailbox_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(query.expected_data_plane_id.as_uuid())
            .bind(&binding.account_ref)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            true
        };
        if !cursor_valid {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftListBatch {
                active_human: true,
                cursor_valid: false,
                authorization: Some(authorization),
                binding: Some(binding),
                candidates: Vec::new(),
                has_more: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            mailbox_id: Uuid,
            created_by: Uuid,
            mail_data_plane_id: Uuid,
            version: i64,
            reply_to_message_id: Option<Uuid>,
            recipient_count: i32,
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
        }

        let mut rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT draft.id, draft.organization_id, draft.mailbox_id,
                   draft.created_by, draft.mail_data_plane_id, draft.version,
                   draft.reply_to_message_id,
                   cardinality(draft.recipient_refs) AS recipient_count,
                   draft.created_at, draft.updated_at
              FROM mail_drafts AS draft
             WHERE draft.organization_id = $1
               AND draft.mailbox_id = $2
               AND draft.created_by = $3
               AND draft.mail_data_plane_id = $4
               AND draft.engine_account_ref = $5
               AND draft.status = 'active'
               AND draft.recipient_refs IS NOT NULL
               AND draft.recipient_domains IS NOT NULL
               AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
               AND cardinality(draft.recipient_refs)
                   = cardinality(draft.recipient_domains)
               AND ($6::uuid IS NULL OR draft.id < $6)
             ORDER BY draft.id DESC
             LIMIT $7
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.mailbox_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.expected_data_plane_id.as_uuid())
        .bind(&binding.account_ref)
        .bind(query.after_draft_id.map(DraftId::as_uuid))
        .bind(i64::from(query.limit) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let has_more = rows.len() > usize::from(query.limit);
        rows.truncate(usize::from(query.limit));
        let candidates = rows
            .into_iter()
            .map(|row| {
                Ok(HumanDraftListCandidate {
                    draft_id: DraftId::from_uuid(row.id),
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    mailbox_id: MailboxId::from_uuid(row.mailbox_id),
                    created_by: PrincipalId::from_uuid(row.created_by),
                    data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
                    version: row.version,
                    status: MailDraftStatus::Active,
                    reply_to_message_id: row.reply_to_message_id,
                    recipient_count: u16::try_from(row.recipient_count)
                        .map_err(|_| StoreError::Unavailable)?,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanDraftListBatch {
            active_human: true,
            cursor_valid: true,
            authorization: Some(authorization),
            binding: Some(binding),
            candidates,
            has_more,
        })
    }

    async fn list_human_draft_attempts(
        &self,
        query: &HumanDraftAttemptListQuery,
    ) -> Result<HumanDraftAttemptListBatch, StoreError> {
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
            return Ok(HumanDraftAttemptListBatch {
                active_human: false,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let cursor_valid = if let Some(after_attempt_id) = query.after_attempt_id {
            sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                      FROM mail_draft_creation_attempts AS attempt
                     WHERE attempt.id = $1
                       AND attempt.organization_id = $2
                       AND attempt.created_by = $3
                       AND attempt.reconciled_at IS NULL
                )
                "#,
            )
            .bind(after_attempt_id.as_uuid())
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            true
        };
        if !cursor_valid {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftAttemptListBatch {
                active_human: true,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            mailbox_id: Uuid,
            created_by: Uuid,
            draft_id: Uuid,
            attempted_at: DateTime<Utc>,
        }

        let mut rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT attempt.id, attempt.organization_id, attempt.mailbox_id,
                   attempt.created_by, attempt.draft_id, attempt.attempted_at
              FROM mail_draft_creation_attempts AS attempt
             WHERE attempt.organization_id = $1
               AND attempt.created_by = $2
               AND attempt.reconciled_at IS NULL
               AND ($3::uuid IS NULL OR attempt.id < $3)
             ORDER BY attempt.id DESC
             LIMIT $4
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.after_attempt_id.map(DraftCreationAttemptId::as_uuid))
        .bind(i64::from(query.limit) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let has_more = rows.len() > usize::from(query.limit);
        rows.truncate(usize::from(query.limit));
        let candidates = rows
            .into_iter()
            .map(|row| {
                Ok(HumanDraftAttemptListCandidate {
                    attempt_id: DraftCreationAttemptId::from_uuid(row.id),
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    mailbox_id: MailboxId::from_uuid(row.mailbox_id),
                    created_by: PrincipalId::from_uuid(row.created_by),
                    draft_id: DraftId::from_uuid(row.draft_id),
                    attempted_at: row.attempted_at,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanDraftAttemptListBatch {
            active_human: true,
            cursor_valid: true,
            candidates,
            has_more,
        })
    }

    async fn resolve_human_draft_restore_target(
        &self,
        query: &HumanDraftRestoreQuery,
    ) -> Result<HumanDraftRestoreResolution, StoreError> {
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
            return Ok(HumanDraftRestoreResolution {
                active_human: false,
                target: None,
            });
        }

        let Some((mailbox, binding, authorization)) = load_human_draft_access_snapshot(
            &mut transaction,
            query.organization_id,
            query.principal_id,
            query.mailbox_id,
            query.expected_data_plane_id,
            query.now,
        )
        .await?
        else {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftRestoreResolution {
                active_human: true,
                target: None,
            });
        };
        if !human_draft_access_permits_read_and_send(
            &authorization,
            query.organization_id,
            query.mailbox_id,
        ) {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftRestoreResolution {
                active_human: true,
                target: None,
            });
        }

        let draft = sqlx::query_as::<_, MailDraftRow>(
            r#"
            SELECT draft.id, draft.organization_id, draft.mailbox_id,
                   draft.created_by, draft.mail_data_plane_id,
                   draft.engine_account_ref, draft.engine_email_ref,
                   draft.engine_thread_ref, draft.reply_to_message_id,
                   draft.reply_to_automatic, draft.body_ref,
                   draft.content_sha256, draft.recipient_refs,
                   draft.recipient_domains, draft.version, draft.status,
                   draft.created_at, draft.updated_at
              FROM mail_drafts AS draft
             WHERE draft.id = $1
               AND draft.organization_id = $2
               AND draft.mailbox_id = $3
               AND draft.created_by = $4
               AND draft.mail_data_plane_id = $5
               AND draft.engine_account_ref = $6
               AND draft.status = 'active'
               AND draft.recipient_refs IS NOT NULL
               AND draft.recipient_domains IS NOT NULL
               AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
               AND cardinality(draft.recipient_refs)
                   = cardinality(draft.recipient_domains)
            "#,
        )
        .bind(query.draft_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .bind(query.mailbox_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.expected_data_plane_id.as_uuid())
        .bind(&binding.account_ref)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?
        .map(mail_draft_from_row)
        .transpose()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanDraftRestoreResolution {
            active_human: true,
            target: draft.map(|draft| HumanDraftRestoreTarget {
                authorization,
                mailbox,
                binding,
                draft,
            }),
        })
    }
}
