use crate::*;

#[async_trait]
impl ApprovalQueryStore for PgControlPlaneStore {
    async fn approval_permits(&self, proof: &ApprovalProof) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM approval_requests AS request
                  JOIN policy_versions AS version
                    ON version.id = request.policy_version_id
                   AND version.organization_id = request.organization_id
                 WHERE request.id = $1
                   AND request.organization_id = $2
                   AND request.action = $3
                   AND request.requested_by = $6
                   AND version.action = request.action
                   AND request.resource_kind = $4
                   AND request.resource_id = $5
                   AND request.status = 'approved'
                   AND request.expires_at > clock_timestamp()
                   AND (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.approval_request_id = request.id
                          AND step.organization_id = request.organization_id
                          AND step.decision = 'approved'
                   ) >= request.required_approvals
                   AND NOT EXISTS (
                       SELECT 1
                         FROM approval_steps AS step
                        WHERE step.approval_request_id = request.id
                          AND step.organization_id = request.organization_id
                          AND step.decision = 'rejected'
                   )
            )
            "#,
        )
        .bind(proof.request_id.as_uuid())
        .bind(proof.organization_id.as_uuid())
        .bind(proof.action.as_str())
        .bind(&proof.resource_kind)
        .bind(&proof.resource_id)
        .bind(proof.requested_by.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn approved_request_evidence(
        &self,
        proof: &ApprovalProof,
    ) -> Result<Option<ApprovedRequestEvidence>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            expires_at: DateTime<Utc>,
            required_approvals: i16,
            approval_count: i64,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT request.expires_at,
                   request.required_approvals,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.approval_request_id = request.id
                          AND step.organization_id = request.organization_id
                          AND step.decision = 'approved'
                   ) AS approval_count
              FROM approval_requests AS request
              JOIN policy_versions AS version
                ON version.id = request.policy_version_id
               AND version.organization_id = request.organization_id
             WHERE request.id = $1
               AND request.organization_id = $2
               AND request.requested_by = $3
               AND request.action = $4
               AND version.action = request.action
               AND request.resource_kind = $5
               AND request.resource_id = $6
               AND request.status = 'approved'
               AND request.expires_at > clock_timestamp()
               AND NOT EXISTS (
                   SELECT 1
                     FROM approval_steps AS step
                    WHERE step.approval_request_id = request.id
                      AND step.organization_id = request.organization_id
                      AND step.decision = 'rejected'
               )
            "#,
        )
        .bind(proof.request_id.as_uuid())
        .bind(proof.organization_id.as_uuid())
        .bind(proof.requested_by.as_uuid())
        .bind(proof.action.as_str())
        .bind(&proof.resource_kind)
        .bind(&proof.resource_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(ApprovedRequestEvidence {
                expires_at: row.expires_at,
                required_approvals: u8::try_from(row.required_approvals)
                    .map_err(|_| StoreError::Unavailable)?,
                approval_count: u8::try_from(row.approval_count)
                    .map_err(|_| StoreError::Unavailable)?,
            })
        })
        .transpose()
    }

    async fn get_approval_request(
        &self,
        organization_id: OrganizationId,
        request_id: copilotmail_domain::ApprovalRequestId,
        now: DateTime<Utc>,
    ) -> Result<Option<ApprovalRequestDetail>, StoreError> {
        let row = sqlx::query_as::<_, ApprovalRequestSummaryRow>(
            r#"
            SELECT request.id, request.organization_id, request.policy_version_id,
                   request.action, request.resource_kind, request.resource_id,
                   request.requested_by, request.reason, request.ticket_reference,
                   request.required_approvals,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.organization_id = request.organization_id
                          AND step.approval_request_id = request.id
                          AND step.decision = 'approved'
                   ) AS approval_count,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.organization_id = request.organization_id
                          AND step.approval_request_id = request.id
                          AND step.decision = 'rejected'
                   ) AS rejection_count,
                   CASE
                       WHEN request.status IN ('pending', 'approved')
                            AND request.expires_at <= $3
                       THEN 'expired'
                       ELSE request.status
                   END AS effective_status,
                   request.created_at, request.expires_at,
                   request.completed_at, request.cancelled_at
              FROM approval_requests AS request
             WHERE request.organization_id = $1
               AND request.id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(request_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let request = approval_request_summary_from_row(row)?;
        let step_rows = sqlx::query_as::<_, ApprovalStepSummaryRow>(
            r#"
            SELECT id, approver_principal_id, decision, reason, decided_at
              FROM approval_steps
             WHERE organization_id = $1
               AND approval_request_id = $2
             ORDER BY decided_at, id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(request_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        let steps = step_rows
            .into_iter()
            .map(approval_step_summary_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ApprovalRequestDetail { request, steps }))
    }

    async fn approval_request_cursor_exists(
        &self,
        organization_id: OrganizationId,
        view: ApprovalRequestView,
        actor: PrincipalId,
        status: Option<ApprovalStatus>,
        request_id: copilotmail_domain::ApprovalRequestId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM approval_requests AS request
                 WHERE request.organization_id = $1
                   AND request.id = $5
                   AND (
                       ($2 = 'requested' AND request.requested_by = $3)
                       OR (
                           $2 = 'actionable'
                           AND request.status = 'pending'
                           AND request.expires_at > $6
                           AND request.requested_by <> $3
                           AND NOT EXISTS (
                               SELECT 1
                                 FROM approval_steps AS own_step
                                WHERE own_step.organization_id =
                                      request.organization_id
                                  AND own_step.approval_request_id = request.id
                                  AND own_step.approver_principal_id = $3
                           )
                       )
                   )
                   AND (
                       $4::text IS NULL
                       OR CASE
                           WHEN request.status IN ('pending', 'approved')
                                AND request.expires_at <= $6
                           THEN 'expired'
                           ELSE request.status
                          END = $4
                   )
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_view(view))
        .bind(actor.as_uuid())
        .bind(status.map(approval_status))
        .bind(request_id.as_uuid())
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_approval_requests(
        &self,
        query: ApprovalRequestListQuery,
    ) -> Result<Vec<ApprovalRequestSummary>, StoreError> {
        let ApprovalRequestListQuery {
            organization_id,
            view,
            actor,
            status,
            after_request_id,
            limit,
            now,
        } = query;
        let rows = sqlx::query_as::<_, ApprovalRequestSummaryRow>(
            r#"
            WITH eligible AS (
                SELECT request.id, request.organization_id,
                       request.policy_version_id, request.action,
                       request.resource_kind, request.resource_id,
                       request.requested_by, request.reason,
                       request.ticket_reference, request.required_approvals,
                       (
                           SELECT count(*)
                             FROM approval_steps AS step
                            WHERE step.organization_id =
                                  request.organization_id
                              AND step.approval_request_id = request.id
                              AND step.decision = 'approved'
                       ) AS approval_count,
                       (
                           SELECT count(*)
                             FROM approval_steps AS step
                            WHERE step.organization_id =
                                  request.organization_id
                              AND step.approval_request_id = request.id
                              AND step.decision = 'rejected'
                       ) AS rejection_count,
                       CASE
                           WHEN request.status IN ('pending', 'approved')
                                AND request.expires_at <= $6
                           THEN 'expired'
                           ELSE request.status
                       END AS effective_status,
                       request.created_at, request.expires_at,
                       request.completed_at, request.cancelled_at
                  FROM approval_requests AS request
                 WHERE request.organization_id = $1
                   AND (
                       ($2 = 'requested' AND request.requested_by = $3)
                       OR (
                           $2 = 'actionable'
                           AND request.status = 'pending'
                           AND request.expires_at > $6
                           AND request.requested_by <> $3
                           AND NOT EXISTS (
                               SELECT 1
                                 FROM approval_steps AS own_step
                                WHERE own_step.organization_id =
                                      request.organization_id
                                  AND own_step.approval_request_id = request.id
                                  AND own_step.approver_principal_id = $3
                           )
                       )
                   )
            )
            SELECT *
              FROM eligible
             WHERE ($4::text IS NULL OR effective_status = $4)
               AND (
                   $5::uuid IS NULL
                   OR (created_at, id) < (
                       SELECT anchor.created_at, anchor.id
                         FROM eligible AS anchor
                        WHERE anchor.id = $5
                          AND ($4::text IS NULL OR anchor.effective_status = $4)
                   )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT $7
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_view(view))
        .bind(actor.as_uuid())
        .bind(status.map(approval_status))
        .bind(after_request_id.map(|id| id.as_uuid()))
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(approval_request_summary_from_row)
            .collect()
    }
}
