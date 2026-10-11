use crate::{
    ApprovalCancellationWrite, ApprovalDecisionWrite, ApprovalProof, ApprovalQueryStore,
    ApprovalRequestDetail, ApprovalRequestListQuery, ApprovalRequestSummary, ApprovalRequestView,
    ControlPlaneStore, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    ApprovalAction, ApprovalDecision, ApprovalPolicyVersion, ApprovalRequest, ApprovalRequestId,
    ApprovalStatus, ApprovalStepId, AuditActor, AuditContext, AuditEvent, AuditOutcome,
    AuditResource, AuthorizationContext, DomainError, OrganizationId, Permission, PolicyId,
    PolicyVersionId,
};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct CreateApprovalPolicyCommand {
    pub organization_id: OrganizationId,
    pub policy_key: String,
    pub display_name: String,
    pub action: ApprovalAction,
    pub required_approvals: u8,
    pub rule: Value,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateApprovalPolicyResult {
    pub policy_id: PolicyId,
    pub policy_version_id: PolicyVersionId,
}

#[derive(Debug, Clone)]
pub struct AppendApprovalPolicyVersionCommand {
    pub organization_id: OrganizationId,
    pub policy_key: String,
    pub action: ApprovalAction,
    pub required_approvals: u8,
    pub rule: Value,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RequestApprovalCommand {
    pub context: AuthorizationContext,
    pub policy_key: String,
    pub action: ApprovalAction,
    pub resource_kind: String,
    pub resource_id: String,
    pub reason: String,
    pub ticket_reference: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DecideApprovalCommand {
    pub context: AuthorizationContext,
    pub request_id: ApprovalRequestId,
    pub decision: ApprovalDecision,
    pub reason: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecideApprovalResult {
    pub request_id: ApprovalRequestId,
    pub status: ApprovalStatus,
    pub approval_count: u8,
}

#[derive(Debug, Clone)]
pub struct GetApprovalRequestCommand {
    pub context: AuthorizationContext,
    pub request_id: ApprovalRequestId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ListApprovalRequestsCommand {
    pub context: AuthorizationContext,
    pub view: ApprovalRequestView,
    pub status: Option<ApprovalStatus>,
    pub after_request_id: Option<ApprovalRequestId>,
    pub limit: i64,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequestPage {
    pub items: Vec<ApprovalRequestSummary>,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct CancelApprovalRequestCommand {
    pub context: AuthorizationContext,
    pub request_id: ApprovalRequestId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApprovalWorkflowError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("approval policy or authorization context does not permit this operation")]
    Forbidden,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Operator-only bootstrap/maintenance service used by the local CLI, not an
/// HTTP route. The bound OIDC owner must still be a currently authorized Human.
pub struct ApprovalPolicyService<S> {
    store: S,
}

impl<S: ControlPlaneStore> ApprovalPolicyService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        command: CreateApprovalPolicyCommand,
    ) -> Result<CreateApprovalPolicyResult, ApprovalWorkflowError> {
        validate_policy_text(&command.policy_key, 100, true)?;
        validate_policy_text(&command.display_name, 200, false)?;
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        ensure_policy_owner(&mut transaction, command.organization_id, &owner).await?;
        let policy_id = PolicyId::new();
        let version = ApprovalPolicyVersion::new(
            policy_id,
            command.organization_id,
            1,
            command.action,
            command.required_approvals,
            command.rule,
            owner.principal_id,
            command.now,
        )?;
        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.policy_id = Some(policy_id);
        audit_context.policy_version_id = Some(version.id());
        audit_context.subject_principal_id = Some(owner.principal_id);
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: owner.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.approval_policy.created",
            AuditResource {
                kind: "authorization.approval_policy".into(),
                id: policy_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "policy_key": command.policy_key,
                "policy_version_id": version.id(),
                "action": version.action(),
                "required_approvals": version.required_approvals(),
                "rule": version.rule(),
            }),
            command.now,
        )?
        .with_context(audit_context)?;
        let event = PendingEvent::new(
            "authorization.approval_policy.created.v1",
            "authorization.approval_policy",
            policy_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "policy_version_id": version.id(),
            }),
            command.now,
        );
        transaction
            .insert_approval_policy(
                policy_id,
                command.organization_id,
                &command.policy_key,
                &command.display_name,
                owner.principal_id,
                command.now,
            )
            .await?;
        transaction.insert_approval_policy_version(&version).await?;
        if !transaction
            .activate_approval_policy_version(
                command.organization_id,
                policy_id,
                version.id(),
                None,
            )
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(CreateApprovalPolicyResult {
            policy_id,
            policy_version_id: version.id(),
        })
    }

    pub async fn append_version(
        &self,
        command: AppendApprovalPolicyVersionCommand,
    ) -> Result<CreateApprovalPolicyResult, ApprovalWorkflowError> {
        validate_policy_text(&command.policy_key, 100, true)?;
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        ensure_policy_owner(&mut transaction, command.organization_id, &owner).await?;
        let active = transaction
            .find_active_approval_policy(command.organization_id, &command.policy_key)
            .await?
            .ok_or(StoreError::MissingReference)?;
        let next_version = active
            .version
            .checked_add(1)
            .ok_or(DomainError::InvalidApproval)?;
        let version = ApprovalPolicyVersion::new(
            active.policy_id,
            command.organization_id,
            next_version,
            command.action,
            command.required_approvals,
            command.rule,
            owner.principal_id,
            command.now,
        )?;
        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.policy_id = Some(active.policy_id);
        audit_context.policy_version_id = Some(version.id());
        audit_context.subject_principal_id = Some(owner.principal_id);
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: owner.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.approval_policy.versioned",
            AuditResource {
                kind: "authorization.approval_policy".into(),
                id: active.policy_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "previous_policy_version_id": active.policy_version_id,
                "policy_version_id": version.id(),
                "version": version.version(),
                "action": version.action(),
                "required_approvals": version.required_approvals(),
                "rule": version.rule(),
            }),
            command.now,
        )?
        .with_context(audit_context)?;
        let event = PendingEvent::new(
            "authorization.approval_policy.versioned.v1",
            "authorization.approval_policy",
            active.policy_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "previous_policy_version_id": active.policy_version_id,
                "policy_version_id": version.id(),
            }),
            command.now,
        );
        transaction.insert_approval_policy_version(&version).await?;
        if !transaction
            .activate_approval_policy_version(
                command.organization_id,
                active.policy_id,
                version.id(),
                Some(active.policy_version_id),
            )
            .await?
        {
            return Err(StoreError::Conflict.into());
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(CreateApprovalPolicyResult {
            policy_id: active.policy_id,
            policy_version_id: version.id(),
        })
    }
}

pub struct ApprovalWorkflowService<S> {
    store: S,
}

impl<S: ControlPlaneStore> ApprovalWorkflowService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn request(
        &self,
        command: RequestApprovalCommand,
    ) -> Result<ApprovalRequestId, ApprovalWorkflowError> {
        let permission = approval_action_permission(command.action);
        if !command
            .context
            .permits(command.context.organization_id, permission)
        {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        validate_policy_text(&command.policy_key, 100, true)?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        if !transaction
            .authorize_principal_permission(
                command.context.organization_id,
                command.context.principal_id,
                permission,
            )
            .await?
        {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        let policy = transaction
            .find_active_approval_policy(command.context.organization_id, &command.policy_key)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if policy.action != command.action {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        let request = ApprovalRequest::new(
            command.context.organization_id,
            policy.policy_version_id,
            command.action,
            command.resource_kind,
            command.resource_id,
            command.context.principal_id,
            command.reason,
            command.ticket_reference,
            policy.required_approvals,
            command.now,
            command.expires_at,
        )?;
        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.policy_id = Some(policy.policy_id);
        audit_context.policy_version_id = Some(request.policy_version_id());
        audit_context.approval_request_id = Some(request.id());
        audit_context.ticket_reference = request.ticket_reference().map(str::to_owned);
        audit_context.subject_principal_id = Some(request.requested_by());
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.approval.requested",
            AuditResource {
                kind: "authorization.approval_request".into(),
                id: request.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "policy_version_id": request.policy_version_id(),
                "action": request.action(),
                "resource_kind": request.resource_kind(),
                "resource_id": request.resource_id(),
                "reason": request.reason(),
                "ticket_reference": request.ticket_reference(),
                "required_approvals": request.required_approvals(),
                "expires_at": request.expires_at(),
            }),
            command.now,
        )?
        .with_context(audit_context)?
        .correlate_human_session(command.context.authentication.as_ref())?;
        let event = PendingEvent::new(
            "authorization.approval.requested.v1",
            "authorization.approval_request",
            request.id().to_string(),
            json!({
                "organization_id": request.organization_id(),
                "requested_by": request.requested_by(),
                "action": request.action(),
            }),
            command.now,
        );
        transaction.insert_approval_request(&request).await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(request.id())
    }

    pub async fn decide(
        &self,
        command: DecideApprovalCommand,
    ) -> Result<DecideApprovalResult, ApprovalWorkflowError> {
        if !command.context.permits(
            command.context.organization_id,
            Permission::AuthorizationManage,
        ) {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        validate_policy_text(&command.reason, 1_000, false)?;
        let step_id = ApprovalStepId::new();
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        if !transaction
            .authorize_human_permission(
                command.context.organization_id,
                command.context.principal_id,
                Permission::AuthorizationManage,
            )
            .await?
        {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        let record = transaction
            .record_approval_decision(&ApprovalDecisionWrite {
                organization_id: command.context.organization_id,
                request_id: command.request_id,
                step_id,
                approver: command.context.principal_id,
                decision: command.decision,
                reason: command.reason.clone(),
                decided_at: command.now,
            })
            .await?
            .ok_or(StoreError::MissingReference)?;
        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.approval_request_id = Some(command.request_id);
        audit_context.subject_principal_id = Some(command.context.principal_id);
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.approval.decided",
            AuditResource {
                kind: "authorization.approval_request".into(),
                id: command.request_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "approval_step_id": step_id,
                "decision": command.decision,
                "reason": command.reason,
                "resulting_status": record.status,
                "approval_count": record.approval_count,
            }),
            command.now,
        )?
        .with_context(audit_context)?
        .correlate_human_session(command.context.authentication.as_ref())?;
        let event = PendingEvent::new(
            "authorization.approval.decided.v1",
            "authorization.approval_request",
            command.request_id.to_string(),
            json!({
                "organization_id": command.context.organization_id,
                "approval_step_id": step_id,
                "decision": command.decision,
                "resulting_status": record.status,
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(DecideApprovalResult {
            request_id: command.request_id,
            status: record.status,
            approval_count: record.approval_count,
        })
    }
}

impl<S: ControlPlaneStore + ApprovalQueryStore> ApprovalWorkflowService<S> {
    pub async fn get(
        &self,
        command: GetApprovalRequestCommand,
    ) -> Result<ApprovalRequestDetail, ApprovalWorkflowError> {
        let detail = self
            .store
            .get_approval_request(
                command.context.organization_id,
                command.request_id,
                command.now,
            )
            .await?
            .ok_or(StoreError::MissingReference)?;
        if detail.request.organization_id != command.context.organization_id
            || detail.request.request_id != command.request_id
        {
            return Err(StoreError::MissingReference.into());
        }
        let manages = command.context.permits(
            command.context.organization_id,
            Permission::AuthorizationManage,
        );
        let owns = detail.request.requested_by == command.context.principal_id;
        if !manages && !owns {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        let actor_active = if manages && !owns {
            transaction
                .authorize_human_permission(
                    command.context.organization_id,
                    command.context.principal_id,
                    Permission::AuthorizationManage,
                )
                .await?
        } else {
            transaction
                .active_principal_exists(
                    command.context.organization_id,
                    command.context.principal_id,
                )
                .await?
        };
        if !actor_active {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        append_approval_read_evidence(
            &mut transaction,
            &command.context,
            command.request_id.to_string(),
            json!({"operation": "get", "own_request": owns}),
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(detail)
    }

    pub async fn list(
        &self,
        command: ListApprovalRequestsCommand,
    ) -> Result<ApprovalRequestPage, ApprovalWorkflowError> {
        if !(1..=100).contains(&command.limit)
            || (command.view == ApprovalRequestView::Actionable
                && (command.status.is_some() && command.status != Some(ApprovalStatus::Pending)))
        {
            return Err(DomainError::InvalidApproval.into());
        }
        if command.view == ApprovalRequestView::Actionable
            && !command.context.permits(
                command.context.organization_id,
                Permission::AuthorizationManage,
            )
        {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        let mut items = self
            .store
            .list_approval_requests(ApprovalRequestListQuery {
                organization_id: command.context.organization_id,
                view: command.view,
                actor: command.context.principal_id,
                status: command.status,
                after_request_id: command.after_request_id,
                limit: command.limit + 1,
                now: command.now,
            })
            .await?;
        let has_more = items.len() > command.limit as usize;
        if has_more {
            items.truncate(command.limit as usize);
        }
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        let actor_active = if command.view == ApprovalRequestView::Actionable {
            transaction
                .authorize_human_permission(
                    command.context.organization_id,
                    command.context.principal_id,
                    Permission::AuthorizationManage,
                )
                .await?
        } else {
            transaction
                .active_principal_exists(
                    command.context.organization_id,
                    command.context.principal_id,
                )
                .await?
        };
        if !actor_active {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        append_approval_read_evidence(
            &mut transaction,
            &command.context,
            command.context.organization_id.to_string(),
            json!({
                "operation": "list",
                "view": approval_view_name(command.view),
                "status": command.status.map(approval_status_name),
                "cursor_present": command.after_request_id.is_some(),
                "result_count": items.len(),
                "has_more": has_more,
            }),
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(ApprovalRequestPage { items, has_more })
    }

    pub async fn cancel(
        &self,
        command: CancelApprovalRequestCommand,
    ) -> Result<DecideApprovalResult, ApprovalWorkflowError> {
        let detail = self
            .store
            .get_approval_request(
                command.context.organization_id,
                command.request_id,
                command.now,
            )
            .await?
            .ok_or(StoreError::MissingReference)?;
        if detail.request.organization_id != command.context.organization_id
            || detail.request.request_id != command.request_id
        {
            return Err(StoreError::MissingReference.into());
        }
        if detail.request.requested_by != command.context.principal_id
            || !command.context.permits(
                command.context.organization_id,
                approval_action_permission(detail.request.action),
            )
        {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        if !transaction
            .authorize_principal_permission(
                command.context.organization_id,
                command.context.principal_id,
                approval_action_permission(detail.request.action),
            )
            .await?
        {
            return Err(ApprovalWorkflowError::Forbidden);
        }
        let record = transaction
            .cancel_approval_request(&ApprovalCancellationWrite {
                organization_id: command.context.organization_id,
                request_id: command.request_id,
                requested_by: command.context.principal_id,
                cancelled_at: command.now,
            })
            .await?
            .ok_or(StoreError::MissingReference)?;
        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.approval_request_id = Some(command.request_id);
        audit_context.subject_principal_id = Some(command.context.principal_id);
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.approval.cancelled",
            AuditResource {
                kind: "authorization.approval_request".into(),
                id: command.request_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "action": detail.request.action,
                "resource_kind": detail.request.resource_kind,
                "resulting_status": record.status,
            }),
            command.now,
        )?
        .with_context(audit_context)?
        .correlate_human_session(command.context.authentication.as_ref())?;
        let event = PendingEvent::new(
            "authorization.approval.cancelled.v1",
            "authorization.approval_request",
            command.request_id.to_string(),
            json!({
                "organization_id": command.context.organization_id,
                "requested_by": command.context.principal_id,
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(DecideApprovalResult {
            request_id: command.request_id,
            status: record.status,
            approval_count: record.approval_count,
        })
    }
}

async fn ensure_policy_owner(
    transaction: &mut Box<dyn crate::ControlPlaneTransaction>,
    organization_id: OrganizationId,
    owner: &crate::ExistingOidcOwnerBinding,
) -> Result<(), ApprovalWorkflowError> {
    if owner.organization_id != organization_id
        || !transaction
            .authorize_human_permission(
                organization_id,
                owner.principal_id,
                Permission::AuthorizationManage,
            )
            .await?
    {
        return Err(ApprovalWorkflowError::Forbidden);
    }
    Ok(())
}

async fn append_approval_read_evidence(
    transaction: &mut Box<dyn crate::ControlPlaneTransaction>,
    context: &AuthorizationContext,
    resource_id: String,
    details: Value,
    trace_id: &str,
    now: DateTime<Utc>,
) -> Result<(), ApprovalWorkflowError> {
    let mut audit_context = AuditContext::for_request(trace_id.to_owned())?;
    audit_context.subject_principal_id = Some(context.principal_id);
    let audit = AuditEvent::new(
        context.organization_id,
        AuditActor {
            principal_id: context.principal_id,
            on_behalf_of: None,
            credential_id: None,
        },
        "authorization.approval.queried",
        AuditResource {
            kind: "authorization.approval_request_query".into(),
            id: resource_id.clone(),
        },
        AuditOutcome::Success,
        trace_id.to_owned(),
        details,
        now,
    )?
    .with_context(audit_context)?
    .correlate_human_session(context.authentication.as_ref())?;
    transaction.append_audit_event(&audit).await?;
    transaction
        .enqueue_event(&PendingEvent::new(
            "authorization.approval.queried.v1",
            "authorization.approval_request_query",
            resource_id,
            json!({"organization_id": context.organization_id}),
            now,
        ))
        .await?;
    Ok(())
}

const fn approval_action_permission(action: ApprovalAction) -> Permission {
    match action {
        ApprovalAction::SendAs | ApprovalAction::SendOnBehalfOf | ApprovalAction::BulkDelete => {
            Permission::MailboxUse
        }
        ApprovalAction::ContentReview => Permission::ContentReview,
        ApprovalAction::AuditExport => Permission::AuditExport,
        ApprovalAction::RetentionChange => Permission::RetentionManage,
        ApprovalAction::PrivilegeElevation => Permission::AuthorizationManage,
        ApprovalAction::ProductionAgentMail => Permission::AuthorizationManage,
        ApprovalAction::AgentFirstContact => Permission::AgentOperate,
        ApprovalAction::LegalHold => Permission::LegalHoldManage,
        ApprovalAction::BreakGlass => Permission::BreakGlass,
        ApprovalAction::SessionRevokeOthers => Permission::SessionRevokeOthers,
    }
}

const fn approval_view_name(view: ApprovalRequestView) -> &'static str {
    match view {
        ApprovalRequestView::Requested => "requested",
        ApprovalRequestView::Actionable => "actionable",
    }
}

const fn approval_status_name(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
        ApprovalStatus::Expired => "expired",
        ApprovalStatus::Cancelled => "cancelled",
    }
}

pub struct ApprovalGateService<S> {
    store: S,
}

impl<S: ApprovalQueryStore> ApprovalGateService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// An approval never creates authority: a false base decision is returned
    /// without querying the approval record.
    pub async fn permits(
        &self,
        base_authorized: bool,
        proof: &ApprovalProof,
    ) -> Result<bool, StoreError> {
        if !base_authorized {
            return Ok(false);
        }
        self.store.approval_permits(proof).await
    }
}

fn validate_policy_text(
    value: &str,
    max: usize,
    lowercase_key: bool,
) -> Result<(), ApprovalWorkflowError> {
    if value.is_empty()
        || value.trim() != value
        || value.chars().count() > max
        || value.chars().any(char::is_control)
        || (lowercase_key
            && (value != value.to_ascii_lowercase()
                || !value.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.'
                })))
    {
        return Err(DomainError::InvalidApproval.into());
    }
    Ok(())
}

#[cfg(test)]
mod workflow_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use copilotmail_domain::PrincipalId;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct CountingApprovalStore {
        calls: Arc<AtomicUsize>,
        result: bool,
    }

    #[async_trait]
    impl ApprovalQueryStore for CountingApprovalStore {
        async fn approval_permits(&self, _proof: &ApprovalProof) -> Result<bool, StoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.result)
        }
    }

    #[tokio::test]
    async fn approval_gate_never_broadens_a_denied_base_decision() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = ApprovalGateService::new(CountingApprovalStore {
            calls: Arc::clone(&calls),
            result: true,
        });
        assert!(
            !gate
                .permits(
                    false,
                    &ApprovalProof {
                        organization_id: OrganizationId::new(),
                        request_id: ApprovalRequestId::new(),
                        requested_by: PrincipalId::new(),
                        action: ApprovalAction::SendAs,
                        resource_kind: "mailbox".into(),
                        resource_id: "mailbox-1".into(),
                        now: Utc::now(),
                    },
                )
                .await
                .unwrap()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn approval_gate_requires_exact_persisted_proof_after_base_authorization() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = ApprovalGateService::new(CountingApprovalStore {
            calls: Arc::clone(&calls),
            result: false,
        });
        assert!(
            !gate
                .permits(
                    true,
                    &ApprovalProof {
                        organization_id: OrganizationId::new(),
                        request_id: ApprovalRequestId::new(),
                        requested_by: PrincipalId::new(),
                        action: ApprovalAction::AuditExport,
                        resource_kind: "audit_query".into(),
                        resource_id: "query-1".into(),
                        now: Utc::now(),
                    },
                )
                .await
                .unwrap()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
