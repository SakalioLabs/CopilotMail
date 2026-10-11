use crate::*;

#[async_trait]
impl ComplianceQueryStore for PgControlPlaneStore {
    async fn active_human_compliance_actor_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn get_compliance_case(
        &self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            external_reference: String,
            title: String,
            purpose: String,
            status: String,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            closed_by: Option<Uuid>,
            closed_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, external_reference, title, purpose,
                   status, created_by, created_at, closed_by, closed_at
              FROM compliance_cases
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            ComplianceCase::from_persisted(
                ComplianceCaseId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                row.external_reference,
                row.title,
                row.purpose,
                parse_compliance_case_status(&row.status)?,
                PrincipalId::from_uuid(row.created_by),
                row.created_at,
                row.closed_by.map(PrincipalId::from_uuid),
                row.closed_at,
            )
            .map_err(|error| {
                tracing::error!(%error, "invalid compliance case persisted");
                StoreError::Unavailable
            })
        })
        .transpose()
    }

    async fn compliance_case_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<ComplianceCaseStatus>,
        case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM compliance_cases
                 WHERE organization_id = $1
                   AND id = $2
                   AND ($3::text IS NULL OR status = $3)
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .bind(status.map(compliance_case_status))
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_compliance_cases(
        &self,
        organization_id: OrganizationId,
        status: Option<ComplianceCaseStatus>,
        after_case_id: Option<ComplianceCaseId>,
        limit: i64,
    ) -> Result<Vec<ComplianceCase>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            external_reference: String,
            title: String,
            purpose: String,
            status: String,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            closed_by: Option<Uuid>,
            closed_at: Option<DateTime<Utc>>,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, external_reference, title, purpose,
                   status, created_by, created_at, closed_by, closed_at
              FROM compliance_cases
             WHERE organization_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND (
                   $3::uuid IS NULL
                   OR (created_at, id) < (
                       SELECT anchor.created_at, anchor.id
                         FROM compliance_cases AS anchor
                        WHERE anchor.organization_id = $1
                          AND anchor.id = $3
                          AND ($2::text IS NULL OR anchor.status = $2)
                   )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(status.map(compliance_case_status))
        .bind(after_case_id.map(ComplianceCaseId::as_uuid))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(|row| {
                ComplianceCase::from_persisted(
                    ComplianceCaseId::from_uuid(row.id),
                    OrganizationId::from_uuid(row.organization_id),
                    row.external_reference,
                    row.title,
                    row.purpose,
                    parse_compliance_case_status(&row.status)?,
                    PrincipalId::from_uuid(row.created_by),
                    row.created_at,
                    row.closed_by.map(PrincipalId::from_uuid),
                    row.closed_at,
                )
                .map_err(|error| {
                    tracing::error!(%error, "invalid compliance case persisted");
                    StoreError::Unavailable
                })
            })
            .collect()
    }

    async fn get_audit_archive(
        &self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            checkpoint_id: Uuid,
            provider: String,
            bucket: String,
            events_object_key: String,
            events_version_id: String,
            events_sha256: String,
            events_size_bytes: i64,
            manifest_object_key: String,
            manifest_version_id: String,
            manifest_sha256: String,
            manifest_size_bytes: i64,
            retention_mode: String,
            retain_until: DateTime<Utc>,
            verified_at: DateTime<Utc>,
            created_by: Uuid,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, checkpoint_id, provider, bucket,
                   events_object_key, events_version_id, events_sha256,
                   events_size_bytes, manifest_object_key, manifest_version_id,
                   manifest_sha256, manifest_size_bytes, retention_mode,
                   retain_until, verified_at, created_by, created_at
              FROM audit_archives
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(archive_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(AuditArchiveRecord {
                archive_id: AuditArchiveId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                checkpoint_id: AuditCheckpointId::from_uuid(row.checkpoint_id),
                provider: row.provider,
                bucket: row.bucket,
                events: ArchivedObjectRecord {
                    key: row.events_object_key,
                    version_id: row.events_version_id,
                    sha256_hex: row.events_sha256,
                    size_bytes: u64::try_from(row.events_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                manifest: ArchivedObjectRecord {
                    key: row.manifest_object_key,
                    version_id: row.manifest_version_id,
                    sha256_hex: row.manifest_sha256,
                    size_bytes: u64::try_from(row.manifest_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                retention_mode: row.retention_mode,
                retain_until: row.retain_until,
                verified_at: row.verified_at,
                created_by: PrincipalId::from_uuid(row.created_by),
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    async fn get_legal_hold(
        &self,
        organization_id: OrganizationId,
        hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            case_id: Uuid,
            archive_id: Uuid,
            reason: String,
            status: String,
            apply_approval_request_id: Uuid,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            release_approval_request_id: Option<Uuid>,
            released_by: Option<Uuid>,
            released_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, case_id, archive_id, reason, status,
                   apply_approval_request_id, created_by, created_at,
                   release_approval_request_id, released_by, released_at
              FROM legal_holds
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(hold_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            LegalHold::from_persisted(
                LegalHoldId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                ComplianceCaseId::from_uuid(row.case_id),
                AuditArchiveId::from_uuid(row.archive_id),
                row.reason,
                parse_legal_hold_status(&row.status)?,
                copilotmail_domain::ApprovalRequestId::from_uuid(row.apply_approval_request_id),
                PrincipalId::from_uuid(row.created_by),
                row.created_at,
                row.release_approval_request_id
                    .map(copilotmail_domain::ApprovalRequestId::from_uuid),
                row.released_by.map(PrincipalId::from_uuid),
                row.released_at,
            )
            .map_err(|error| {
                tracing::error!(%error, "invalid legal hold persisted");
                StoreError::Unavailable
            })
        })
        .transpose()
    }

    async fn legal_hold_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        hold_id: LegalHoldId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM legal_holds
                 WHERE organization_id = $1
                   AND id = $2
                   AND ($3::text IS NULL OR status = $3)
                   AND ($4::uuid IS NULL OR case_id = $4)
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(hold_id.as_uuid())
        .bind(status.map(legal_hold_status))
        .bind(case_id.map(ComplianceCaseId::as_uuid))
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_legal_holds(
        &self,
        organization_id: OrganizationId,
        status: Option<LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        after_hold_id: Option<LegalHoldId>,
        limit: i64,
    ) -> Result<Vec<LegalHold>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            case_id: Uuid,
            archive_id: Uuid,
            reason: String,
            status: String,
            apply_approval_request_id: Uuid,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            release_approval_request_id: Option<Uuid>,
            released_by: Option<Uuid>,
            released_at: Option<DateTime<Utc>>,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, case_id, archive_id, reason, status,
                   apply_approval_request_id, created_by, created_at,
                   release_approval_request_id, released_by, released_at
              FROM legal_holds
             WHERE organization_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::uuid IS NULL OR case_id = $3)
               AND (
                   $4::uuid IS NULL
                   OR (created_at, id) < (
                       SELECT anchor.created_at, anchor.id
                         FROM legal_holds AS anchor
                        WHERE anchor.organization_id = $1
                          AND anchor.id = $4
                          AND ($2::text IS NULL OR anchor.status = $2)
                          AND ($3::uuid IS NULL OR anchor.case_id = $3)
                   )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT $5
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(status.map(legal_hold_status))
        .bind(case_id.map(ComplianceCaseId::as_uuid))
        .bind(after_hold_id.map(LegalHoldId::as_uuid))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(|row| {
                LegalHold::from_persisted(
                    LegalHoldId::from_uuid(row.id),
                    OrganizationId::from_uuid(row.organization_id),
                    ComplianceCaseId::from_uuid(row.case_id),
                    AuditArchiveId::from_uuid(row.archive_id),
                    row.reason,
                    parse_legal_hold_status(&row.status)?,
                    copilotmail_domain::ApprovalRequestId::from_uuid(row.apply_approval_request_id),
                    PrincipalId::from_uuid(row.created_by),
                    row.created_at,
                    row.release_approval_request_id
                        .map(copilotmail_domain::ApprovalRequestId::from_uuid),
                    row.released_by.map(PrincipalId::from_uuid),
                    row.released_at,
                )
                .map_err(|error| {
                    tracing::error!(%error, "invalid legal hold persisted");
                    StoreError::Unavailable
                })
            })
            .collect()
    }

    async fn get_legal_hold_by_apply_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError> {
        let hold_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
              FROM legal_holds
             WHERE organization_id = $1
               AND apply_approval_request_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        match hold_id {
            Some(hold_id) => {
                self.get_legal_hold(organization_id, LegalHoldId::from_uuid(hold_id))
                    .await
            }
            None => Ok(None),
        }
    }
}
