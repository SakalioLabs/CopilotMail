use crate::*;

impl PgControlPlaneTransaction {
    pub(crate) async fn tx_retention_policy_detail(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError> {
        repositories::retention::load_detail(&mut **self.transaction()?, organization_id, policy_id)
            .await
    }

    pub(crate) async fn tx_insert_retention_policy(
        &mut self,
        policy: &RetentionPolicy,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO retention_policies(
                id, organization_id, policy_key, display_name, resource_kind,
                active_version_id, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)
            "#,
        )
        .bind(policy.id().as_uuid())
        .bind(policy.organization_id().as_uuid())
        .bind(policy.policy_key())
        .bind(policy.display_name())
        .bind(policy.resource_kind().as_str())
        .bind(policy.created_by().as_uuid())
        .bind(policy.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_insert_retention_policy_version(
        &mut self,
        version: &RetentionPolicyVersion,
    ) -> Result<(), StoreError> {
        let version_number = i32::try_from(version.version()).map_err(|_| StoreError::Conflict)?;
        let minimum_retention_days =
            i32::try_from(version.minimum_retention_days()).map_err(|_| StoreError::Conflict)?;
        let archive_after_days = version
            .archive_after_days()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| StoreError::Conflict)?;
        let delete_after_days = version
            .delete_after_days()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| StoreError::Conflict)?;
        sqlx::query(
            r#"
            INSERT INTO retention_policy_versions(
                id, organization_id, policy_id, version,
                minimum_retention_days, archive_after_days, delete_after_days,
                rule, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(version.id().as_uuid())
        .bind(version.organization_id().as_uuid())
        .bind(version.policy_id().as_uuid())
        .bind(version_number)
        .bind(minimum_retention_days)
        .bind(archive_after_days)
        .bind(delete_after_days)
        .bind(version.rule())
        .bind(version.created_by().as_uuid())
        .bind(version.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    pub(crate) async fn tx_find_retention_policy_for_update(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyRecord>, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyRow>(
            r#"
            SELECT policy.id, policy.organization_id, policy.policy_key,
                   policy.display_name, policy.resource_kind,
                   policy.active_version_id, policy.created_by,
                   policy.created_at,
                   COALESCE((
                       SELECT MAX(version.version)
                         FROM retention_policy_versions AS version
                        WHERE version.organization_id = policy.organization_id
                          AND version.policy_id = policy.id
                   ), 0)::integer AS latest_version
              FROM retention_policies AS policy
             WHERE policy.organization_id = $1
               AND policy.id = $2
             FOR UPDATE OF policy
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(retention_policy_record_from_row).transpose()
    }

    pub(crate) async fn tx_find_retention_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
        version_id: RetentionPolicyVersionId,
    ) -> Result<Option<RetentionPolicyVersion>, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyVersionRow>(
            r#"
            SELECT id, policy_id, organization_id, version,
                   minimum_retention_days, archive_after_days,
                   delete_after_days, rule, created_by, created_at
              FROM retention_policy_versions
             WHERE organization_id = $1
               AND policy_id = $2
               AND id = $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_id.as_uuid())
        .bind(version_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(retention_policy_version_from_row).transpose()
    }

    pub(crate) async fn tx_insert_retention_policy_activation(
        &mut self,
        activation: &RetentionPolicyActivationRecord,
    ) -> Result<RetentionPolicyActivationRecord, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyActivationRow>(
            r#"
            INSERT INTO retention_policy_activations(
                id, organization_id, policy_id, policy_version_id,
                approval_request_id, activated_by, activated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, clock_timestamp())
            RETURNING id, organization_id, policy_id, policy_version_id,
                      approval_request_id, activated_by, activated_at
            "#,
        )
        .bind(activation.id.as_uuid())
        .bind(activation.organization_id.as_uuid())
        .bind(activation.policy_id.as_uuid())
        .bind(activation.policy_version_id.as_uuid())
        .bind(activation.approval_request_id.as_uuid())
        .bind(activation.activated_by.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(retention_policy_activation_from_row(row))
    }

    pub(crate) async fn tx_find_retention_policy_activation_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<RetentionPolicyActivationRecord>, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyActivationRow>(
            r#"
            SELECT id, organization_id, policy_id, policy_version_id,
                   approval_request_id, activated_by, activated_at
              FROM retention_policy_activations
             WHERE organization_id = $1
               AND approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(retention_policy_activation_from_row))
    }

    pub(crate) async fn tx_activate_retention_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
        version_id: RetentionPolicyVersionId,
        expected_active_version_id: Option<RetentionPolicyVersionId>,
    ) -> Result<bool, StoreError> {
        let changed = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE retention_policies
               SET active_version_id = $3
             WHERE organization_id = $1
               AND id = $2
               AND active_version_id IS NOT DISTINCT FROM $4
            RETURNING 1
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_id.as_uuid())
        .bind(version_id.as_uuid())
        .bind(expected_active_version_id.map(RetentionPolicyVersionId::as_uuid))
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(changed.is_some())
    }
}
