use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_compliance_case(
        &mut self,
        case: &ComplianceCase,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO compliance_cases(
                id, organization_id, external_reference, title, purpose,
                status, created_by, created_at, closed_by, closed_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(case.id().as_uuid())
        .bind(case.organization_id().as_uuid())
        .bind(case.external_reference())
        .bind(case.title())
        .bind(case.purpose())
        .bind(compliance_case_status(case.status()))
        .bind(case.created_by().as_uuid())
        .bind(case.created_at())
        .bind(case.closed_by().map(PrincipalId::as_uuid))
        .bind(case.closed_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_compliance_case_for_update(
        &mut self,
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
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
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

    pub(crate) async fn tx_update_compliance_case(
        &mut self,
        case: &ComplianceCase,
    ) -> Result<bool, StoreError> {
        let updated = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE compliance_cases
               SET status = $3, closed_by = $4, closed_at = $5
             WHERE id = $1 AND organization_id = $2 AND status = 'open'
            RETURNING 1
            "#,
        )
        .bind(case.id().as_uuid())
        .bind(case.organization_id().as_uuid())
        .bind(compliance_case_status(case.status()))
        .bind(case.closed_by().map(PrincipalId::as_uuid))
        .bind(case.closed_at())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(updated.is_some())
    }

    pub(crate) async fn tx_active_legal_hold_exists_for_case(
        &mut self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM legal_holds
                 WHERE organization_id = $1
                   AND case_id = $2
                   AND status = 'active'
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    pub(crate) async fn tx_insert_legal_hold(
        &mut self,
        hold: &LegalHold,
        evidence: &LegalHoldObjectEvidence,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO legal_holds(
                id, organization_id, case_id, archive_id, reason, status,
                apply_approval_request_id, applied_events_version_id,
                applied_manifest_version_id, applied_verified_at,
                created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(hold.id().as_uuid())
        .bind(hold.organization_id().as_uuid())
        .bind(hold.case_id().as_uuid())
        .bind(hold.archive_id().as_uuid())
        .bind(hold.reason())
        .bind(legal_hold_status(hold.status()))
        .bind(hold.apply_approval_request_id().as_uuid())
        .bind(&evidence.events_version_id)
        .bind(&evidence.manifest_version_id)
        .bind(evidence.verified_at)
        .bind(hold.created_by().as_uuid())
        .bind(hold.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_legal_hold_by_apply_approval_for_update(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
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
             WHERE organization_id = $1
               AND apply_approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
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

    pub(crate) async fn tx_find_legal_hold_for_update(
        &mut self,
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
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(hold_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
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

    pub(crate) async fn tx_update_legal_hold(
        &mut self,
        hold: &LegalHold,
        evidence: &LegalHoldObjectEvidence,
    ) -> Result<bool, StoreError> {
        let updated = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE legal_holds
               SET status = $3,
                   release_approval_request_id = $4,
                   released_events_version_id = $5,
                   released_manifest_version_id = $6,
                   released_verified_at = $7,
                   released_by = $8,
                   released_at = $9
             WHERE id = $1 AND organization_id = $2 AND status = 'active'
            RETURNING 1
            "#,
        )
        .bind(hold.id().as_uuid())
        .bind(hold.organization_id().as_uuid())
        .bind(legal_hold_status(hold.status()))
        .bind(
            hold.release_approval_request_id()
                .map(copilotmail_domain::ApprovalRequestId::as_uuid),
        )
        .bind(&evidence.events_version_id)
        .bind(&evidence.manifest_version_id)
        .bind(evidence.verified_at)
        .bind(hold.released_by().map(PrincipalId::as_uuid))
        .bind(hold.released_at())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(updated.is_some())
    }
}
