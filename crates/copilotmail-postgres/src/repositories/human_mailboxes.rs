use crate::*;

#[async_trait]
impl HumanMailboxDirectoryStore for PgControlPlaneStore {
    async fn list_human_mailbox_directory(
        &self,
        query: &HumanMailboxDirectoryQuery,
    ) -> Result<HumanMailboxDirectoryBatch, StoreError> {
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
            return Ok(HumanMailboxDirectoryBatch {
                active_human: false,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let cursor_valid = if let Some(after_mailbox_id) = query.after_mailbox_id {
            sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                      FROM mailboxes AS mailbox
                      JOIN mail_engine_mailbox_bindings AS binding
                        ON binding.organization_id = mailbox.organization_id
                       AND binding.mailbox_id = mailbox.id
                     WHERE mailbox.id = $1
                       AND mailbox.organization_id = $2
                       AND (
                           EXISTS (
                               SELECT 1
                                 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = mailbox.organization_id
                                  AND membership.mailbox_id = mailbox.id
                                  AND membership.principal_id = $3
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $4
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $4
                                  )
                           )
                           OR EXISTS (
                               SELECT 1
                                 FROM delegations AS delegation
                                WHERE delegation.organization_id = mailbox.organization_id
                                  AND delegation.mailbox_id = mailbox.id
                                  AND delegation.grantee_principal_id = $3
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $4
                                  AND delegation.valid_until > $4
                           )
                       )
                )
                "#,
            )
            .bind(after_mailbox_id.as_uuid())
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(query.now)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            true
        };
        if !cursor_valid {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct MailboxRow {
            id: Uuid,
            organization_id: Uuid,
            kind: String,
            primary_address: String,
            created_at: DateTime<Utc>,
            mail_data_plane_id: Uuid,
        }
        let mut rows = sqlx::query_as::<_, MailboxRow>(
            r#"
            SELECT mailbox.id, mailbox.organization_id, mailbox.kind,
                   mailbox.primary_address, mailbox.created_at,
                   binding.mail_data_plane_id
              FROM mailboxes AS mailbox
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = mailbox.organization_id
               AND binding.mailbox_id = mailbox.id
              JOIN principals AS principal
                ON principal.id = $2
               AND principal.organization_id = mailbox.organization_id
               AND principal.kind = 'human'
               AND principal.status = 'active'
             WHERE mailbox.organization_id = $1
               AND ($3::uuid IS NULL OR mailbox.id > $3)
               AND (
                   EXISTS (
                       SELECT 1
                         FROM mailbox_memberships AS membership
                        WHERE membership.organization_id = mailbox.organization_id
                          AND membership.mailbox_id = mailbox.id
                          AND membership.principal_id = principal.id
                          AND membership.role IN ('owner', 'member')
                          AND membership.valid_from <= $4
                          AND (
                              membership.valid_until IS NULL
                              OR membership.valid_until > $4
                          )
                   )
                   OR EXISTS (
                       SELECT 1
                         FROM delegations AS delegation
                        WHERE delegation.organization_id = mailbox.organization_id
                          AND delegation.mailbox_id = mailbox.id
                          AND delegation.grantee_principal_id = principal.id
                          AND 'mailbox.read' = ANY(delegation.scopes)
                          AND delegation.revoked_at IS NULL
                          AND delegation.valid_from <= $4
                          AND delegation.valid_until > $4
                   )
               )
             ORDER BY mailbox.id
             LIMIT $5
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.after_mailbox_id.map(MailboxId::as_uuid))
        .bind(query.now)
        .bind(i64::from(query.limit) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let has_more = rows.len() > usize::from(query.limit);
        rows.truncate(usize::from(query.limit));

        let mailbox_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
        #[derive(sqlx::FromRow)]
        struct MembershipRow {
            mailbox_id: Uuid,
            role: String,
        }
        let membership_rows = if mailbox_ids.is_empty() {
            Vec::new()
        } else {
            sqlx::query_as::<_, MembershipRow>(
                r#"
                SELECT DISTINCT ON (membership.mailbox_id)
                       membership.mailbox_id, membership.role
                  FROM mailbox_memberships AS membership
                 WHERE membership.organization_id = $1
                   AND membership.principal_id = $2
                   AND membership.mailbox_id = ANY($3)
                   AND membership.valid_from <= $4
                   AND (
                       membership.valid_until IS NULL
                       OR membership.valid_until > $4
                   )
                 ORDER BY membership.mailbox_id,
                          CASE membership.role
                              WHEN 'owner' THEN 1
                              WHEN 'member' THEN 2
                              ELSE 3
                          END
                "#,
            )
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(&mailbox_ids)
            .bind(query.now)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_store_error)?
        };
        let mut memberships = BTreeMap::new();
        for row in membership_rows {
            let role =
                mailbox_membership_role_from_str(&row.role).ok_or(StoreError::Unavailable)?;
            memberships.insert(row.mailbox_id, role);
        }

        #[derive(sqlx::FromRow)]
        struct DelegationRow {
            mailbox_id: Uuid,
            id: Uuid,
            grantor_principal_id: Uuid,
            scopes: Vec<String>,
        }
        let delegation_rows = if mailbox_ids.is_empty() {
            Vec::new()
        } else {
            sqlx::query_as::<_, DelegationRow>(
                r#"
                SELECT mailbox_id, id, grantor_principal_id, scopes
                  FROM delegations
                 WHERE organization_id = $1
                   AND grantee_principal_id = $2
                   AND mailbox_id = ANY($3)
                   AND revoked_at IS NULL
                   AND valid_from <= $4
                   AND valid_until > $4
                 ORDER BY mailbox_id, id
                "#,
            )
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(&mailbox_ids)
            .bind(query.now)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_store_error)?
        };
        let mut delegations = BTreeMap::<Uuid, Vec<ResolvedMailboxDelegation>>::new();
        for row in delegation_rows {
            let capabilities = row
                .scopes
                .into_iter()
                .map(|scope| mailbox_capability_from_str(&scope).ok_or(StoreError::Unavailable))
                .collect::<Result<BTreeSet<_>, StoreError>>()?;
            delegations
                .entry(row.mailbox_id)
                .or_default()
                .push(ResolvedMailboxDelegation {
                    delegation_id: DelegationId::from_uuid(row.id),
                    grantor_principal_id: PrincipalId::from_uuid(row.grantor_principal_id),
                    capabilities,
                });
        }

        let candidates = rows
            .into_iter()
            .map(|row| {
                let kind = mailbox_kind_from_str(&row.kind).ok_or(StoreError::Unavailable)?;
                let primary_address =
                    copilotmail_domain::MailboxAddress::parse(row.primary_address)
                        .map_err(|_| StoreError::Unavailable)?;
                let mailbox_id = MailboxId::from_uuid(row.id);
                let organization_id = OrganizationId::from_uuid(row.organization_id);
                Ok(HumanMailboxDirectoryCandidate {
                    mailbox: MailboxSummary {
                        mailbox_id,
                        organization_id,
                        kind,
                        primary_address,
                        created_at: row.created_at,
                    },
                    data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
                    authorization: MailboxAccessSnapshot {
                        organization_id,
                        mailbox_id,
                        membership: memberships.remove(&row.id),
                        delegations: delegations.remove(&row.id).unwrap_or_default(),
                    },
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanMailboxDirectoryBatch {
            active_human: true,
            cursor_valid: true,
            candidates,
            has_more,
        })
    }
}
