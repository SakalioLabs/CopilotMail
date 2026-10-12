use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_mailbox(&mut self, mailbox: &Mailbox) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mailboxes(id, organization_id, kind, primary_address, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(mailbox.id.as_uuid())
        .bind(mailbox.organization_id.as_uuid())
        .bind(mailbox_kind(mailbox.kind))
        .bind(mailbox.primary_address.as_str())
        .bind(mailbox.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_upsert_mail_engine_mailbox_binding(
        &mut self,
        write: &MailEngineMailboxBindingWrite,
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
            WITH updated AS (
                UPDATE mail_engine_mailbox_bindings
                   SET account_ref = $4,
                       version = version + 1,
                       updated_by = $6,
                       updated_at = $7
                 WHERE mailbox_id = $1
                   AND organization_id = $2
                   AND mail_data_plane_id = $3
                   AND version = $5
                   AND $5 > 0
                   AND $7 >= updated_at
                RETURNING organization_id, mailbox_id, mail_data_plane_id,
                          account_ref, version, updated_at
            ), inserted AS (
                INSERT INTO mail_engine_mailbox_bindings(
                    mailbox_id, organization_id, mail_data_plane_id, source, account_ref,
                    version, updated_by, created_at, updated_at
                )
                SELECT $1, $2, $3, 'stalwart', $4, 1, $6, $7, $7
                 WHERE $5 = 0
                   AND EXISTS (
                       SELECT 1
                         FROM mailboxes
                        WHERE id = $1
                          AND organization_id = $2
                   )
                   AND NOT EXISTS (SELECT 1 FROM updated)
                ON CONFLICT(mailbox_id) DO NOTHING
                RETURNING organization_id, mailbox_id, mail_data_plane_id,
                          account_ref, version, updated_at
            )
            SELECT organization_id, mailbox_id, mail_data_plane_id,
                   account_ref, version, updated_at
              FROM updated
            UNION ALL
            SELECT organization_id, mailbox_id, mail_data_plane_id,
                   account_ref, version, updated_at
              FROM inserted
            "#,
        )
        .bind(write.mailbox_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.data_plane_id.as_uuid())
        .bind(&write.account_ref)
        .bind(write.expected_version)
        .bind(write.updated_by.as_uuid())
        .bind(write.updated_at)
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

    pub(crate) async fn tx_tenant_mailbox_exists(
        &mut self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM mailboxes
                 WHERE id = $1
                   AND organization_id = $2
            )
            "#,
        )
        .bind(mailbox_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_insert_mailbox_membership(
        &mut self,
        membership: &MailboxMembership,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mailbox_memberships(
                organization_id, mailbox_id, principal_id, role,
                valid_from, valid_until, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(membership.organization_id().as_uuid())
        .bind(membership.mailbox_id().as_uuid())
        .bind(membership.principal_id().as_uuid())
        .bind(mailbox_membership_role(membership.role()))
        .bind(membership.valid_from())
        .bind(membership.valid_until())
        .bind(membership.created_by().as_uuid())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_mailbox_owned_by(
        &mut self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM mailbox_memberships AS membership
                  JOIN principals AS principal
                    ON principal.id = membership.principal_id
                   AND principal.organization_id = membership.organization_id
                   AND principal.status = 'active'
                 WHERE membership.organization_id = $1
                   AND membership.mailbox_id = $2
                   AND membership.principal_id = $3
                   AND membership.role = 'owner'
                   AND membership.valid_from <= $4
                   AND (
                       membership.valid_until IS NULL
                       OR membership.valid_until > $4
                   )
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_insert_mailbox_delegation(
        &mut self,
        delegation: &MailboxDelegation,
    ) -> Result<(), StoreError> {
        let scopes = delegation
            .capabilities()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>();
        sqlx::query(
            r#"
            INSERT INTO delegations(
                id, organization_id, grantor_principal_id, grantee_principal_id,
                mailbox_id, scopes, reason, valid_from, valid_until, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(delegation.id().as_uuid())
        .bind(delegation.organization_id().as_uuid())
        .bind(delegation.grantor_principal_id().as_uuid())
        .bind(delegation.grantee_principal_id().as_uuid())
        .bind(delegation.mailbox_id().as_uuid())
        .bind(scopes)
        .bind(delegation.reason())
        .bind(delegation.valid_from())
        .bind(delegation.valid_until())
        .bind(delegation.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_revoke_mailbox_delegation(
        &mut self,
        organization_id: OrganizationId,
        delegation_id: DelegationId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE delegations
               SET revoked_at = $3
             WHERE id = $1
               AND organization_id = $2
               AND revoked_at IS NULL
            "#,
        )
        .bind(delegation_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn tx_mailbox_delegation_managed_by(
        &mut self,
        organization_id: OrganizationId,
        delegation_id: DelegationId,
        actor: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM delegations AS delegation
                  JOIN mailbox_memberships AS membership
                    ON membership.organization_id = delegation.organization_id
                   AND membership.mailbox_id = delegation.mailbox_id
                   AND membership.principal_id = $3
                   AND membership.role = 'owner'
                   AND membership.valid_from <= $4
                   AND (
                       membership.valid_until IS NULL
                       OR membership.valid_until > $4
                   )
                  JOIN principals AS principal
                    ON principal.id = membership.principal_id
                   AND principal.organization_id = membership.organization_id
                   AND principal.status = 'active'
                 WHERE delegation.id = $1
                   AND delegation.organization_id = $2
                   AND delegation.revoked_at IS NULL
            )
            "#,
        )
        .bind(delegation_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(actor.as_uuid())
        .bind(now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_mailbox_send_capability_held(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        mailbox_send_capability_held(
            self.transaction()?,
            organization_id,
            principal_id,
            mailbox_id,
            now,
        )
        .await
    }
}

/// Locks the membership or active delegation that still grants `mailbox.send`.
///
/// The share lock is held until the caller commits, so a revocation `UPDATE`
/// cannot land between this check and the send it authorizes. `valid_until`
/// is compared to the database clock. `valid_from` also accepts the caller
/// timestamp already enforced by the earlier authorization check, so a clock
/// that trails the application does not reject a grant that has just started.
pub(crate) async fn mailbox_send_capability_held(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    mailbox_id: MailboxId,
    now: DateTime<Utc>,
) -> Result<bool, StoreError> {
    sqlx::query_scalar(
        r#"
        SELECT
            EXISTS (
                SELECT 1
                  FROM mailbox_memberships AS membership
                  JOIN principals AS principal
                    ON principal.id = membership.principal_id
                   AND principal.organization_id = membership.organization_id
                   AND principal.status = 'active'
                 WHERE membership.organization_id = $1
                   AND membership.mailbox_id = $2
                   AND membership.principal_id = $3
                   AND membership.role IN ('owner', 'member')
                   AND membership.valid_from <= GREATEST(clock_timestamp(), $4::timestamptz)
                   AND (
                       membership.valid_until IS NULL
                       OR membership.valid_until > clock_timestamp()
                   )
                 FOR SHARE OF membership
            )
            OR EXISTS (
                SELECT 1
                  FROM delegations AS delegation
                  JOIN principals AS principal
                    ON principal.id = delegation.grantee_principal_id
                   AND principal.organization_id = delegation.organization_id
                   AND principal.status = 'active'
                 WHERE delegation.organization_id = $1
                   AND delegation.mailbox_id = $2
                   AND delegation.grantee_principal_id = $3
                   AND 'mailbox.send' = ANY(delegation.scopes)
                   AND delegation.revoked_at IS NULL
                   AND delegation.valid_from <= GREATEST(clock_timestamp(), $4::timestamptz)
                   AND delegation.valid_until > clock_timestamp()
                 FOR SHARE OF delegation
            )
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(mailbox_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(now)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_store_error)
}
