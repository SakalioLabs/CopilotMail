use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_insert_approval_policy(
        &mut self,
        policy_id: PolicyId,
        organization_id: OrganizationId,
        policy_key: &str,
        display_name: &str,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO policies(
                id, organization_id, policy_key, display_name, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(policy_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(policy_key)
        .bind(display_name)
        .bind(created_by.as_uuid())
        .bind(created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_insert_approval_policy_version(
        &mut self,
        version: &ApprovalPolicyVersion,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO policy_versions(
                id, organization_id, policy_id, version, action,
                required_approvals, rule, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(version.id().as_uuid())
        .bind(version.organization_id().as_uuid())
        .bind(version.policy_id().as_uuid())
        .bind(i64::from(version.version()))
        .bind(version.action().as_str())
        .bind(i16::from(version.required_approvals()))
        .bind(version.rule())
        .bind(version.created_by().as_uuid())
        .bind(version.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_activate_approval_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: PolicyId,
        policy_version_id: PolicyVersionId,
        expected_active_version_id: Option<PolicyVersionId>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE policies
               SET active_version_id = $3
             WHERE id = $1
               AND organization_id = $2
               AND retired_at IS NULL
               AND active_version_id IS NOT DISTINCT FROM $4
            "#,
        )
        .bind(policy_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(policy_version_id.as_uuid())
        .bind(expected_active_version_id.map(PolicyVersionId::as_uuid))
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn tx_find_active_approval_policy(
        &mut self,
        organization_id: OrganizationId,
        policy_key: &str,
    ) -> Result<Option<ActiveApprovalPolicy>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            policy_id: Uuid,
            policy_version_id: Uuid,
            version: i32,
            action: String,
            required_approvals: i16,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT policy.id AS policy_id,
                   version.id AS policy_version_id,
                   version.version,
                   version.action,
                   version.required_approvals
              FROM policies AS policy
              JOIN policy_versions AS version
                ON version.id = policy.active_version_id
               AND version.organization_id = policy.organization_id
               AND version.policy_id = policy.id
             WHERE policy.organization_id = $1
               AND policy.policy_key = $2
               AND policy.retired_at IS NULL
             FOR UPDATE OF policy
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_key)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let action = approval_action_from_str(&row.action).ok_or(StoreError::Unavailable)?;
            let required_approvals =
                u8::try_from(row.required_approvals).map_err(|_| StoreError::Unavailable)?;
            let version = u32::try_from(row.version).map_err(|_| StoreError::Unavailable)?;
            Ok(ActiveApprovalPolicy {
                policy_id: PolicyId::from_uuid(row.policy_id),
                policy_version_id: PolicyVersionId::from_uuid(row.policy_version_id),
                version,
                action,
                required_approvals,
            })
        })
        .transpose()
    }

    pub(crate) async fn tx_insert_approval_request(
        &mut self,
        request: &ApprovalRequest,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            WITH runtime_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS created_at
            )
            INSERT INTO approval_requests(
                id, organization_id, policy_version_id, action,
                resource_kind, resource_id, requested_by, reason,
                ticket_reference, required_approvals, status, created_at, expires_at
            )
            SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                   runtime_clock.created_at,
                   runtime_clock.created_at
                       + ($13::timestamptz - $12::timestamptz)
              FROM runtime_clock
            "#,
        )
        .bind(request.id().as_uuid())
        .bind(request.organization_id().as_uuid())
        .bind(request.policy_version_id().as_uuid())
        .bind(request.action().as_str())
        .bind(request.resource_kind())
        .bind(request.resource_id())
        .bind(request.requested_by().as_uuid())
        .bind(request.reason())
        .bind(request.ticket_reference())
        .bind(i16::from(request.required_approvals()))
        .bind(approval_status(request.status()))
        .bind(request.created_at())
        .bind(request.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_record_approval_decision(
        &mut self,
        decision_write: &ApprovalDecisionWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct RequestRow {
            requested_by: Uuid,
            required_approvals: i16,
            status: String,
            is_active: bool,
        }
        let row = sqlx::query_as::<_, RequestRow>(
            r#"
            SELECT requested_by, required_approvals, status,
                   expires_at > clock_timestamp() AS is_active
              FROM approval_requests
             WHERE id = $1
               AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.requested_by == decision_write.approver.as_uuid()
            || row.status != "pending"
            || !row.is_active
        {
            return Err(StoreError::Conflict);
        }
        let decided_at = sqlx::query_scalar::<_, DateTime<Utc>>(
            r#"
            WITH runtime_clock AS MATERIALIZED (SELECT clock_timestamp() AS now)
            INSERT INTO approval_steps(
                id, organization_id, approval_request_id,
                approver_principal_id, decision, reason, decided_at
            )
            SELECT $1, $2, $3, $4, $5, $6, runtime_clock.now
              FROM runtime_clock
             WHERE EXISTS (
                 SELECT 1 FROM approval_requests
                  WHERE id = $3 AND organization_id = $2
                    AND status = 'pending' AND expires_at > runtime_clock.now
             )
            RETURNING decided_at
            "#,
        )
        .bind(decision_write.step_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.approver.as_uuid())
        .bind(approval_decision(decision_write.decision))
        .bind(&decision_write.reason)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;

        let approval_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
              FROM approval_steps
             WHERE approval_request_id = $1
               AND organization_id = $2
               AND decision = 'approved'
            "#,
        )
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let status = if decision_write.decision == ApprovalDecision::Rejected {
            ApprovalStatus::Rejected
        } else if approval_count >= i64::from(row.required_approvals) {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        };
        sqlx::query(
            r#"
            UPDATE approval_requests
               SET status = $3,
                   completed_at = CASE
                       WHEN $3 = 'pending' THEN NULL
                       ELSE GREATEST(clock_timestamp(), $4)
                   END
             WHERE id = $1
               AND organization_id = $2
            "#,
        )
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .bind(approval_status(status))
        .bind(decided_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(Some(ApprovalDecisionRecord {
            status,
            approval_count: u8::try_from(approval_count).map_err(|_| StoreError::Unavailable)?,
        }))
    }

    pub(crate) async fn tx_cancel_approval_request(
        &mut self,
        cancellation: &ApprovalCancellationWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct RequestRow {
            requested_by: Uuid,
            status: String,
            expires_at: DateTime<Utc>,
            approval_count: i64,
        }
        let row = sqlx::query_as::<_, RequestRow>(
            r#"
            SELECT request.requested_by, request.status, request.expires_at,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.organization_id = request.organization_id
                          AND step.approval_request_id = request.id
                          AND step.decision = 'approved'
                   ) AS approval_count
              FROM approval_requests AS request
             WHERE request.id = $1
               AND request.organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(cancellation.request_id.as_uuid())
        .bind(cancellation.organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.requested_by != cancellation.requested_by.as_uuid()
            || row.status != "pending"
            || row.expires_at <= cancellation.cancelled_at
        {
            return Err(StoreError::Conflict);
        }
        let updated = sqlx::query(
            r#"
            UPDATE approval_requests
               SET status = 'cancelled',
                   cancelled_at = $3
             WHERE id = $1
               AND organization_id = $2
               AND status = 'pending'
               AND expires_at > clock_timestamp()
            "#,
        )
        .bind(cancellation.request_id.as_uuid())
        .bind(cancellation.organization_id.as_uuid())
        .bind(cancellation.cancelled_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(Some(ApprovalDecisionRecord {
            status: ApprovalStatus::Cancelled,
            approval_count: u8::try_from(row.approval_count)
                .map_err(|_| StoreError::Unavailable)?,
        }))
    }
}
