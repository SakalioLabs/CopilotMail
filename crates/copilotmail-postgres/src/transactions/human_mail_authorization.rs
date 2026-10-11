use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_human_draft_list_still_authorized(
        &mut self,
        proof: &HumanDraftListRecheck,
    ) -> Result<bool, StoreError> {
        if proof.expected_drafts.len() > 50
            || proof
                .expected_drafts
                .iter()
                .any(|draft| draft.draft_id.as_uuid().is_nil() || draft.expected_version <= 0)
        {
            return Ok(false);
        }
        let mut unique_draft_ids = BTreeSet::new();
        if proof
            .expected_drafts
            .iter()
            .any(|draft| !unique_draft_ids.insert(draft.draft_id))
        {
            return Ok(false);
        }
        let expected_draft_ids = proof
            .expected_drafts
            .iter()
            .map(|draft| draft.draft_id.as_uuid())
            .collect::<Vec<_>>();
        let expected_versions = proof
            .expected_drafts
            .iter()
            .map(|draft| draft.expected_version)
            .collect::<Vec<_>>();

        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals AS principal
                  JOIN mailboxes AS mailbox
                    ON mailbox.organization_id = principal.organization_id
                   AND mailbox.id = $3
                  JOIN mail_engine_mailbox_bindings AS binding
                    ON binding.organization_id = mailbox.organization_id
                   AND binding.mailbox_id = mailbox.id
                   AND binding.mail_data_plane_id = $4
                   AND binding.account_ref = $5
                 WHERE principal.id = $2
                   AND principal.organization_id = $1
                   AND principal.kind = 'human'
                   AND principal.status = 'active'
                   AND EXISTS (
                       SELECT 1
                         FROM role_assignments AS assignment
                         JOIN roles AS role
                           ON role.id = assignment.role_id
                          AND role.organization_id = assignment.organization_id
                         JOIN role_permissions AS permission
                           ON permission.role_id = role.id
                          AND permission.permission = 'mailbox.use'
                        WHERE assignment.organization_id = $1
                          AND assignment.principal_id = $2
                          AND assignment.revoked_at IS NULL
                          AND assignment.valid_from <= CURRENT_TIMESTAMP
                          AND (
                              assignment.valid_until IS NULL
                              OR assignment.valid_until > CURRENT_TIMESTAMP
                          )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.send' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND NOT EXISTS (
                       SELECT 1
                         FROM unnest($6::uuid[], $7::bigint[])
                              AS expected(draft_id, expected_version)
                         LEFT JOIN mail_drafts AS draft
                           ON draft.id = expected.draft_id
                          AND draft.organization_id = $1
                          AND draft.mailbox_id = $3
                          AND draft.created_by = $2
                          AND draft.mail_data_plane_id = $4
                          AND draft.engine_account_ref = $5
                          AND draft.status = 'active'
                          AND draft.version = expected.expected_version
                        WHERE draft.id IS NULL
                   )
            )
            "#,
        )
        .bind(proof.organization_id.as_uuid())
        .bind(proof.principal_id.as_uuid())
        .bind(proof.mailbox_id.as_uuid())
        .bind(proof.expected_data_plane_id.as_uuid())
        .bind(&proof.expected_engine_account_ref)
        .bind(expected_draft_ids.as_slice())
        .bind(expected_versions.as_slice())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_human_sent_list_still_authorized(
        &mut self,
        proof: &HumanSentListRecheck,
    ) -> Result<bool, StoreError> {
        if proof.expected_submissions.len() > 50
            || proof.expected_submissions.iter().any(|submission| {
                submission.submission_id.as_uuid().is_nil()
                    || !(1..=100).contains(&submission.expected_recipient_count)
                    || submission
                        .expected_delivery_snapshot_id
                        .is_some_and(|snapshot_id| snapshot_id.is_nil())
            })
        {
            return Ok(false);
        }
        let mut unique_submission_ids = BTreeSet::new();
        if proof
            .expected_submissions
            .iter()
            .any(|submission| !unique_submission_ids.insert(submission.submission_id))
        {
            return Ok(false);
        }
        let expected_submission_ids = proof
            .expected_submissions
            .iter()
            .map(|submission| submission.submission_id.as_uuid())
            .collect::<Vec<_>>();
        let expected_accepted_at = proof
            .expected_submissions
            .iter()
            .map(|submission| submission.expected_accepted_at)
            .collect::<Vec<_>>();
        let expected_recipient_counts = proof
            .expected_submissions
            .iter()
            .map(|submission| i32::from(submission.expected_recipient_count))
            .collect::<Vec<_>>();
        // PostgreSQL arrays cannot represent a portable typed NULL element in
        // every sqlx version we support. A nil UUID is impossible for an
        // actual snapshot (and rejected above when caller-supplied), so it is
        // a safe in-transaction sentinel for an expected absent snapshot.
        let expected_snapshot_ids = proof
            .expected_submissions
            .iter()
            .map(|submission| {
                submission
                    .expected_delivery_snapshot_id
                    .unwrap_or_else(Uuid::nil)
            })
            .collect::<Vec<_>>();

        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals AS principal
                  JOIN mailboxes AS mailbox
                    ON mailbox.organization_id = principal.organization_id
                   AND mailbox.id = $3
                 WHERE principal.id = $2
                   AND principal.organization_id = $1
                   AND principal.kind = 'human'
                   AND principal.status = 'active'
                   AND EXISTS (
                       SELECT 1
                         FROM role_assignments AS assignment
                         JOIN roles AS role
                           ON role.id = assignment.role_id
                          AND role.organization_id = assignment.organization_id
                         JOIN role_permissions AS permission
                           ON permission.role_id = role.id
                          AND permission.permission = 'mailbox.use'
                        WHERE assignment.organization_id = $1
                          AND assignment.principal_id = $2
                          AND assignment.revoked_at IS NULL
                          AND assignment.valid_from <= CURRENT_TIMESTAMP
                          AND (
                              assignment.valid_until IS NULL
                              OR assignment.valid_until > CURRENT_TIMESTAMP
                          )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.send' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND NOT EXISTS (
                       SELECT 1
                         FROM unnest(
                                  $4::uuid[],
                                  $5::timestamptz[],
                                  $6::integer[],
                                  $7::uuid[]
                              ) AS expected(
                                  submission_id,
                                  accepted_at,
                                  recipient_count,
                                  snapshot_id
                              )
                         LEFT JOIN mail_submission_intents AS intent
                           ON intent.id = expected.submission_id
                          AND intent.organization_id = $1
                          AND intent.mailbox_id = $3
                          AND intent.requested_by = $2
                          AND intent.status = 'accepted'
                          AND intent.accepted_at = expected.accepted_at
                         LEFT JOIN mail_drafts AS draft
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
                             SELECT id
                               FROM mail_delivery_snapshots
                              WHERE organization_id = intent.organization_id
                                AND submission_id = intent.id
                              ORDER BY observed_at DESC, id DESC
                              LIMIT 1
                         ) AS snapshot ON true
                        WHERE intent.id IS NULL
                           OR draft.id IS NULL
                           OR cardinality(draft.recipient_refs) <> expected.recipient_count
                           OR COALESCE(
                                  snapshot.id,
                                  '00000000-0000-0000-0000-000000000000'::uuid
                              ) <> expected.snapshot_id
                   )
            )
            "#,
        )
        .bind(proof.organization_id.as_uuid())
        .bind(proof.principal_id.as_uuid())
        .bind(proof.mailbox_id.as_uuid())
        .bind(expected_submission_ids.as_slice())
        .bind(expected_accepted_at.as_slice())
        .bind(expected_recipient_counts.as_slice())
        .bind(expected_snapshot_ids.as_slice())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_human_draft_read_still_authorized(
        &mut self,
        proof: &HumanDraftReadRecheck,
    ) -> Result<bool, StoreError> {
        if proof.expected_version <= 0 || !(1..=100).contains(&proof.expected_recipients.len()) {
            return Ok(false);
        }
        let recipient_refs = proof
            .expected_recipients
            .iter()
            .map(|recipient| recipient.recipient_ref.to_vec())
            .collect::<Vec<_>>();
        let recipient_domains = proof
            .expected_recipients
            .iter()
            .map(|recipient| recipient.domain.clone())
            .collect::<Vec<_>>();
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals AS principal
                  JOIN mailboxes AS mailbox
                    ON mailbox.organization_id = principal.organization_id
                   AND mailbox.id = $3
                   AND mailbox.primary_address = $10
                  JOIN mail_engine_mailbox_bindings AS binding
                    ON binding.organization_id = mailbox.organization_id
                   AND binding.mailbox_id = mailbox.id
                   AND binding.mail_data_plane_id = $6
                   AND binding.account_ref = $7
                  JOIN mail_drafts AS draft
                    ON draft.organization_id = mailbox.organization_id
                   AND draft.mailbox_id = mailbox.id
                   AND draft.id = $4
                   AND draft.created_by = principal.id
                   AND draft.mail_data_plane_id = binding.mail_data_plane_id
                   AND draft.engine_account_ref = binding.account_ref
                 WHERE principal.id = $2
                   AND principal.organization_id = $1
                   AND principal.kind = 'human'
                   AND principal.status = 'active'
                   AND draft.status = 'active'
                   AND draft.version = $5
                   AND draft.engine_email_ref = $8
                   AND draft.engine_thread_ref = $9
                   AND draft.body_ref = $11
                   AND draft.content_sha256 = $12
                   AND draft.recipient_refs = $13
                   AND draft.recipient_domains = $14
                   AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
                   AND cardinality(draft.recipient_refs)
                       = cardinality(draft.recipient_domains)
                   AND draft.reply_to_message_id IS NOT DISTINCT FROM $15
                   AND EXISTS (
                       SELECT 1
                         FROM role_assignments AS assignment
                         JOIN roles AS role
                           ON role.id = assignment.role_id
                          AND role.organization_id = assignment.organization_id
                         JOIN role_permissions AS permission
                           ON permission.role_id = role.id
                          AND permission.permission = 'mailbox.use'
                        WHERE assignment.organization_id = $1
                          AND assignment.principal_id = $2
                          AND assignment.revoked_at IS NULL
                          AND assignment.valid_from <= CURRENT_TIMESTAMP
                          AND (
                              assignment.valid_until IS NULL
                              OR assignment.valid_until > CURRENT_TIMESTAMP
                          )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.send' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
            )
            "#,
        )
        .bind(proof.organization_id.as_uuid())
        .bind(proof.principal_id.as_uuid())
        .bind(proof.mailbox_id.as_uuid())
        .bind(proof.draft_id.as_uuid())
        .bind(proof.expected_version)
        .bind(proof.expected_data_plane_id.as_uuid())
        .bind(&proof.expected_engine_account_ref)
        .bind(&proof.expected_engine_email_ref)
        .bind(&proof.expected_engine_thread_ref)
        .bind(proof.expected_primary_address.as_str())
        .bind(proof.expected_body_ref.as_slice())
        .bind(proof.expected_content_sha256.as_slice())
        .bind(&recipient_refs)
        .bind(&recipient_domains)
        .bind(proof.expected_reply_to_message_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }
}
