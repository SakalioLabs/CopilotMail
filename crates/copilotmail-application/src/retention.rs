use crate::{
    ApprovalProof, ApprovalQueryStore, ControlPlaneStore, PendingEvent,
    RetentionPolicyActivationRecord, RetentionPolicyDetail, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    ApprovalAction, AuditActor, AuditContext, AuditEvent, AuditOutcome, AuditResource,
    AuthorizationContext, DomainError, Permission, RetentionPolicy, RetentionPolicyActivationId,
    RetentionPolicyId, RetentionPolicyVersion, RetentionPolicyVersionId, RetentionResourceKind,
};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct CreateRetentionPolicyCommand {
    pub context: AuthorizationContext,
    pub policy_key: String,
    pub display_name: String,
    pub resource_kind: RetentionResourceKind,
    pub minimum_retention_days: u32,
    pub archive_after_days: Option<u32>,
    pub delete_after_days: Option<u32>,
    pub rule: Value,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AppendRetentionPolicyVersionCommand {
    pub context: AuthorizationContext,
    pub policy_id: RetentionPolicyId,
    pub minimum_retention_days: u32,
    pub archive_after_days: Option<u32>,
    pub delete_after_days: Option<u32>,
    pub rule: Value,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ActivateRetentionPolicyVersionCommand {
    pub context: AuthorizationContext,
    pub policy_id: RetentionPolicyId,
    pub policy_version_id: RetentionPolicyVersionId,
    pub approval: ApprovalProof,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct GetRetentionPolicyCommand {
    pub context: AuthorizationContext,
    pub policy_id: RetentionPolicyId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicyDraft {
    pub policy: RetentionPolicy,
    pub version: RetentionPolicyVersion,
}

#[derive(Debug, thiserror::Error)]
pub enum RetentionPolicyError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("retention policy operation is forbidden")]
    Forbidden,
    #[error("retention policy or version is not available")]
    NotFound,
    #[error("retention policy state conflicts with the request")]
    Conflict,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Governs immutable retention-policy drafts and approval-bound activation.
///
/// Creating or editing a policy can never alter production behavior. Only
/// [`Self::activate_version`] advances the active pointer, and the database
/// independently reproves the Active Human and exact dual approval.
pub struct RetentionPolicyService<S> {
    store: S,
}

impl<S> RetentionPolicyService<S>
where
    S: ControlPlaneStore + ApprovalQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        command: CreateRetentionPolicyCommand,
    ) -> Result<RetentionPolicyDraft, RetentionPolicyError> {
        ensure_base_authorized(&command.context)?;
        let policy = RetentionPolicy::new(
            command.context.organization_id,
            command.policy_key,
            command.display_name,
            command.resource_kind,
            command.context.principal_id,
            command.now,
        )?;
        let version = RetentionPolicyVersion::new(
            policy.id(),
            policy.organization_id(),
            1,
            command.minimum_retention_days,
            command.archive_after_days,
            command.delete_after_days,
            command.rule,
            command.context.principal_id,
            command.now,
        )?;

        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_retention_manager(&mut transaction, &command.context).await?;
        transaction.insert_retention_policy(&policy).await?;
        transaction
            .insert_retention_policy_version(&version)
            .await?;
        append_governance_evidence(
            &mut transaction,
            &command.context,
            "compliance.retention_policy.created",
            policy.id(),
            &version,
            None,
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(RetentionPolicyDraft { policy, version })
    }

    pub async fn append_version(
        &self,
        command: AppendRetentionPolicyVersionCommand,
    ) -> Result<RetentionPolicyVersion, RetentionPolicyError> {
        ensure_base_authorized(&command.context)?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_retention_manager(&mut transaction, &command.context).await?;
        let record = transaction
            .find_retention_policy_for_update(command.context.organization_id, command.policy_id)
            .await?
            .ok_or(RetentionPolicyError::NotFound)?;
        ensure_policy_scope(&record.policy, &command.context, command.policy_id)?;
        let version_number = record
            .latest_version
            .checked_add(1)
            .ok_or(DomainError::InvalidRetentionPolicy)?;
        let version = RetentionPolicyVersion::new(
            record.policy.id(),
            command.context.organization_id,
            version_number,
            command.minimum_retention_days,
            command.archive_after_days,
            command.delete_after_days,
            command.rule,
            command.context.principal_id,
            command.now,
        )?;
        transaction
            .insert_retention_policy_version(&version)
            .await?;
        append_governance_evidence(
            &mut transaction,
            &command.context,
            "compliance.retention_policy.versioned",
            record.policy.id(),
            &version,
            None,
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(version)
    }

    pub async fn activate_version(
        &self,
        command: ActivateRetentionPolicyVersionCommand,
    ) -> Result<RetentionPolicyVersion, RetentionPolicyError> {
        ensure_base_authorized(&command.context)?;
        if command.approval.organization_id != command.context.organization_id
            || command.approval.requested_by != command.context.principal_id
            || command.approval.action != ApprovalAction::RetentionChange
            || command.approval.resource_kind != "compliance.retention_policy_version"
            || command.approval.resource_id != command.policy_version_id.to_string()
            || command.approval.now != command.now
        {
            return Err(RetentionPolicyError::Forbidden);
        }
        let approval = self
            .store
            .approved_request_evidence(&command.approval)
            .await?
            .ok_or(RetentionPolicyError::Forbidden)?;
        if approval.required_approvals < 2
            || approval.approval_count < approval.required_approvals
            || approval.expires_at <= command.now
        {
            return Err(RetentionPolicyError::Forbidden);
        }

        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_retention_manager(&mut transaction, &command.context).await?;
        let record = transaction
            .find_retention_policy_for_update(command.context.organization_id, command.policy_id)
            .await?
            .ok_or(RetentionPolicyError::NotFound)?;
        ensure_policy_scope(&record.policy, &command.context, command.policy_id)?;
        let version = transaction
            .find_retention_policy_version(
                command.context.organization_id,
                command.policy_id,
                command.policy_version_id,
            )
            .await?
            .ok_or(RetentionPolicyError::NotFound)?;

        if version.id() != command.policy_version_id
            || version.policy_id() != command.policy_id
            || version.organization_id() != command.context.organization_id
        {
            return Err(StoreError::Unavailable.into());
        }

        if let Some(existing) = transaction
            .find_retention_policy_activation_by_approval(
                command.context.organization_id,
                command.approval.request_id,
            )
            .await?
        {
            if existing.policy_id == command.policy_id
                && existing.policy_version_id == command.policy_version_id
                && existing.activated_by == command.context.principal_id
                && record.active_version_id == Some(command.policy_version_id)
            {
                return Ok(version);
            }
            return Err(RetentionPolicyError::Conflict);
        }
        if record.active_version_id == Some(command.policy_version_id) {
            return Err(RetentionPolicyError::Conflict);
        }

        let activation = RetentionPolicyActivationRecord {
            id: RetentionPolicyActivationId::new(),
            organization_id: command.context.organization_id,
            policy_id: command.policy_id,
            policy_version_id: command.policy_version_id,
            approval_request_id: command.approval.request_id,
            activated_by: command.context.principal_id,
            activated_at: command.now,
        };
        let stored_activation = transaction
            .insert_retention_policy_activation(&activation)
            .await?;
        if stored_activation.id != activation.id
            || stored_activation.organization_id != activation.organization_id
            || stored_activation.policy_id != activation.policy_id
            || stored_activation.policy_version_id != activation.policy_version_id
            || stored_activation.approval_request_id != activation.approval_request_id
            || stored_activation.activated_by != activation.activated_by
        {
            return Err(StoreError::Unavailable.into());
        }
        if stored_activation.activated_at >= approval.expires_at {
            return Err(RetentionPolicyError::Forbidden);
        }
        if !transaction
            .activate_retention_policy_version(
                command.context.organization_id,
                command.policy_id,
                command.policy_version_id,
                record.active_version_id,
            )
            .await?
        {
            return Err(RetentionPolicyError::Conflict);
        }
        append_governance_evidence(
            &mut transaction,
            &command.context,
            "compliance.retention_policy.activated",
            command.policy_id,
            &version,
            Some(command.approval.request_id),
            &command.trace_id,
            stored_activation.activated_at,
        )
        .await?;
        transaction.commit().await?;
        Ok(version)
    }

    /// Returns one tenant policy only after its privileged read evidence has
    /// committed. A failed audit write therefore fails the read closed.
    pub async fn get(
        &self,
        command: GetRetentionPolicyCommand,
    ) -> Result<RetentionPolicyDetail, RetentionPolicyError> {
        ensure_base_authorized(&command.context)?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_retention_manager(&mut transaction, &command.context).await?;
        let detail = transaction
            .retention_policy_detail(command.context.organization_id, command.policy_id)
            .await?
            .ok_or(RetentionPolicyError::NotFound)?;
        ensure_policy_scope(&detail.policy, &command.context, command.policy_id)?;
        ensure_detail_versions(&detail)?;
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "compliance.retention_policy.queried",
            AuditResource {
                kind: "compliance.retention_policy_query".into(),
                id: command.policy_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "policy_id": command.policy_id,
                "active_version_id": detail.active_version.as_ref().map(RetentionPolicyVersion::id),
                "latest_version_id": detail.latest_version.id(),
            }),
            command.now,
        )?
        .with_context({
            let mut context = AuditContext::for_request(command.trace_id)?;
            context.subject_principal_id = Some(command.context.principal_id);
            context
        })?;
        ensure_retention_manager(&mut transaction, &command.context).await?;
        transaction.append_audit_event(&audit).await?;
        let event = PendingEvent::new(
            "compliance.retention_policy.queried.v1",
            "compliance.retention_policy_query",
            command.policy_id.to_string(),
            json!({"organization_id": command.context.organization_id}),
            command.now,
        );
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(detail)
    }
}

fn ensure_base_authorized(context: &AuthorizationContext) -> Result<(), RetentionPolicyError> {
    if !context.permits(context.organization_id, Permission::RetentionManage) {
        return Err(RetentionPolicyError::Forbidden);
    }
    Ok(())
}

fn ensure_policy_scope(
    policy: &RetentionPolicy,
    context: &AuthorizationContext,
    policy_id: RetentionPolicyId,
) -> Result<(), RetentionPolicyError> {
    if policy.id() != policy_id || policy.organization_id() != context.organization_id {
        return Err(StoreError::Unavailable.into());
    }
    Ok(())
}

fn ensure_detail_versions(detail: &RetentionPolicyDetail) -> Result<(), RetentionPolicyError> {
    let belongs = |version: &RetentionPolicyVersion| {
        version.policy_id() == detail.policy.id()
            && version.organization_id() == detail.policy.organization_id()
    };
    if !belongs(&detail.latest_version)
        || detail.active_version.as_ref().is_some_and(|active| {
            !belongs(active)
                || active.version() > detail.latest_version.version()
                || (active.version() == detail.latest_version.version()
                    && active.id() != detail.latest_version.id())
        })
    {
        return Err(StoreError::Unavailable.into());
    }
    Ok(())
}

async fn ensure_retention_manager(
    transaction: &mut Box<dyn crate::ControlPlaneTransaction>,
    context: &AuthorizationContext,
) -> Result<(), RetentionPolicyError> {
    if !transaction
        .authorize_human_permission(
            context.organization_id,
            context.principal_id,
            Permission::RetentionManage,
        )
        .await?
    {
        return Err(RetentionPolicyError::Forbidden);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn append_governance_evidence(
    transaction: &mut Box<dyn crate::ControlPlaneTransaction>,
    context: &AuthorizationContext,
    action: &str,
    policy_id: RetentionPolicyId,
    version: &RetentionPolicyVersion,
    approval_request_id: Option<copilotmail_domain::ApprovalRequestId>,
    trace_id: &str,
    now: DateTime<Utc>,
) -> Result<(), RetentionPolicyError> {
    ensure_retention_manager(transaction, context).await?;
    let mut audit_context = AuditContext::for_request(trace_id.to_owned())?;
    audit_context.subject_principal_id = Some(context.principal_id);
    audit_context.approval_request_id = approval_request_id;
    let audit = AuditEvent::new(
        context.organization_id,
        AuditActor {
            principal_id: context.principal_id,
            on_behalf_of: None,
            credential_id: None,
        },
        action,
        AuditResource {
            kind: "compliance.retention_policy".into(),
            id: policy_id.to_string(),
        },
        AuditOutcome::Success,
        trace_id.to_owned(),
        json!({
            "policy_version_id": version.id(),
            "version": version.version(),
            "minimum_retention_days": version.minimum_retention_days(),
            "archive_after_days": version.archive_after_days(),
            "delete_after_days": version.delete_after_days(),
            "legal_hold_override": true,
        }),
        now,
    )?
    .with_context(audit_context)?;
    let event = PendingEvent::new(
        format!("{action}.v1"),
        "compliance.retention_policy",
        policy_id.to_string(),
        json!({
            "organization_id": context.organization_id,
            "policy_version_id": version.id(),
        }),
        now,
    );
    transaction.append_audit_event(&audit).await?;
    transaction.enqueue_event(&event).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
