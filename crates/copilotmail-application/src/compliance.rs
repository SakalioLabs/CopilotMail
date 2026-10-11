use crate::{
    ApprovalProof, ApprovalQueryStore, AuditArchiveRecord, ControlPlaneStore,
    LegalHoldObjectEvidence, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    ApprovalAction, ApprovalRequestId, AuditActor, AuditArchiveId, AuditContext, AuditEvent,
    AuditOutcome, AuditResource, AuthorizationContext, ComplianceCase, ComplianceCaseId,
    ComplianceCaseStatus, DomainError, LegalHold, LegalHoldId, LegalHoldStatus, OrganizationId,
    Permission,
};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct CreateComplianceCaseCommand {
    pub context: AuthorizationContext,
    pub external_reference: String,
    pub title: String,
    pub purpose: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CloseComplianceCaseCommand {
    pub context: AuthorizationContext,
    pub case_id: ComplianceCaseId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApplyLegalHoldCommand {
    pub context: AuthorizationContext,
    pub case_id: ComplianceCaseId,
    pub archive_id: AuditArchiveId,
    pub reason: String,
    pub approval: ApprovalProof,
    pub evidence: LegalHoldObjectEvidence,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ReleaseLegalHoldCommand {
    pub context: AuthorizationContext,
    pub hold_id: LegalHoldId,
    pub approval: ApprovalProof,
    pub evidence: LegalHoldObjectEvidence,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplianceReadKind {
    Case,
    LegalHold,
}

#[derive(Debug, Clone)]
pub struct RecordComplianceReadCommand {
    pub context: AuthorizationContext,
    pub kind: ComplianceReadKind,
    pub resource_id: String,
    pub details: serde_json::Value,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum ComplianceGovernanceError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("compliance operation is forbidden")]
    Forbidden,
    #[error("compliance resource is not available")]
    NotFound,
    #[error("compliance resource state conflicts with the request")]
    Conflict,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct ComplianceGovernanceService<S> {
    store: S,
}

impl<S> ComplianceGovernanceService<S>
where
    S: ControlPlaneStore + ApprovalQueryStore + Clone,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create_case(
        &self,
        command: CreateComplianceCaseCommand,
    ) -> Result<ComplianceCase, ComplianceGovernanceError> {
        ensure_base_authorized(&command.context)?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_active_human(&mut transaction, &command.context).await?;
        let case = ComplianceCase::new(
            command.context.organization_id,
            command.external_reference,
            command.title,
            command.purpose,
            command.context.principal_id,
            command.now,
        )?;
        transaction.insert_compliance_case(&case).await?;
        append_governance_evidence(
            &mut transaction,
            &command.context,
            "compliance.case.created",
            "compliance.case",
            case.id().to_string(),
            json!({
                "external_reference": case.external_reference(),
                "status": case.status(),
            }),
            None,
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(case)
    }

    pub async fn close_case(
        &self,
        command: CloseComplianceCaseCommand,
    ) -> Result<ComplianceCase, ComplianceGovernanceError> {
        ensure_base_authorized(&command.context)?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_active_human(&mut transaction, &command.context).await?;
        let mut case = transaction
            .find_compliance_case_for_update(command.context.organization_id, command.case_id)
            .await?
            .ok_or(ComplianceGovernanceError::NotFound)?;
        if transaction
            .active_legal_hold_exists_for_case(command.context.organization_id, command.case_id)
            .await?
        {
            return Err(ComplianceGovernanceError::Conflict);
        }
        case.close(command.context.principal_id, command.now)?;
        if !transaction.update_compliance_case(&case).await? {
            return Err(ComplianceGovernanceError::Conflict);
        }
        append_governance_evidence(
            &mut transaction,
            &command.context,
            "compliance.case.closed",
            "compliance.case",
            case.id().to_string(),
            json!({"status": case.status()}),
            None,
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(case)
    }

    pub async fn apply_hold(
        &self,
        command: ApplyLegalHoldCommand,
    ) -> Result<LegalHold, ComplianceGovernanceError> {
        ensure_base_authorized(&command.context)?;
        ensure_approval(
            &self.store,
            &command.approval,
            command.context.organization_id,
            command.context.principal_id,
            "audit.archive",
            &command.archive_id.to_string(),
        )
        .await?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_active_human(&mut transaction, &command.context).await?;
        if let Some(existing) = transaction
            .find_legal_hold_by_apply_approval_for_update(
                command.context.organization_id,
                command.approval.request_id,
            )
            .await?
        {
            if existing.case_id() == command.case_id
                && existing.archive_id() == command.archive_id
                && existing.reason() == command.reason
                && existing.status() == LegalHoldStatus::Active
            {
                return Ok(existing);
            }
            return Err(ComplianceGovernanceError::Conflict);
        }
        let case = transaction
            .find_compliance_case_for_update(command.context.organization_id, command.case_id)
            .await?
            .ok_or(ComplianceGovernanceError::NotFound)?;
        if case.status() != ComplianceCaseStatus::Open {
            return Err(ComplianceGovernanceError::Conflict);
        }
        let archive = transaction
            .find_audit_archive(command.context.organization_id, command.archive_id)
            .await?
            .ok_or(ComplianceGovernanceError::NotFound)?;
        ensure_archive_scope(&archive, command.context.organization_id)?;
        ensure_object_evidence(&archive, &command.evidence, true, command.now)?;
        let hold = LegalHold::new(
            command.context.organization_id,
            command.case_id,
            command.archive_id,
            command.reason,
            command.approval.request_id,
            command.context.principal_id,
            command.now,
        )?;
        transaction
            .insert_legal_hold(&hold, &command.evidence)
            .await?;
        append_governance_evidence(
            &mut transaction,
            &command.context,
            "compliance.legal_hold.applied",
            "compliance.legal_hold",
            hold.id().to_string(),
            json!({
                "case_id": hold.case_id(),
                "archive_id": hold.archive_id(),
                "approval_request_id": hold.apply_approval_request_id(),
                "events_version_id": command.evidence.events_version_id,
                "manifest_version_id": command.evidence.manifest_version_id,
                "verified_at": command.evidence.verified_at,
                "status": hold.status(),
            }),
            Some(command.approval.request_id),
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(hold)
    }

    pub async fn release_hold(
        &self,
        command: ReleaseLegalHoldCommand,
    ) -> Result<LegalHold, ComplianceGovernanceError> {
        ensure_base_authorized(&command.context)?;
        ensure_approval(
            &self.store,
            &command.approval,
            command.context.organization_id,
            command.context.principal_id,
            "compliance.legal_hold",
            &command.hold_id.to_string(),
        )
        .await?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_active_human(&mut transaction, &command.context).await?;
        let mut hold = transaction
            .find_legal_hold_for_update(command.context.organization_id, command.hold_id)
            .await?
            .ok_or(ComplianceGovernanceError::NotFound)?;
        let archive = transaction
            .find_audit_archive(command.context.organization_id, hold.archive_id())
            .await?
            .ok_or(ComplianceGovernanceError::NotFound)?;
        ensure_object_evidence(&archive, &command.evidence, false, command.now)?;
        if hold.status() == LegalHoldStatus::Released {
            if hold.release_approval_request_id() == Some(command.approval.request_id) {
                return Ok(hold);
            }
            return Err(ComplianceGovernanceError::Conflict);
        }
        hold.release(
            command.approval.request_id,
            command.context.principal_id,
            command.now,
        )?;
        if !transaction
            .update_legal_hold(&hold, &command.evidence)
            .await?
        {
            return Err(ComplianceGovernanceError::Conflict);
        }
        append_governance_evidence(
            &mut transaction,
            &command.context,
            "compliance.legal_hold.released",
            "compliance.legal_hold",
            hold.id().to_string(),
            json!({
                "case_id": hold.case_id(),
                "archive_id": hold.archive_id(),
                "approval_request_id": hold.release_approval_request_id(),
                "events_version_id": command.evidence.events_version_id,
                "manifest_version_id": command.evidence.manifest_version_id,
                "verified_at": command.evidence.verified_at,
                "status": LegalHoldStatus::Released,
            }),
            Some(command.approval.request_id),
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(hold)
    }

    /// Records privileged compliance reads before their result is returned.
    /// The caller must fail closed if this transaction cannot commit.
    pub async fn record_read(
        &self,
        command: RecordComplianceReadCommand,
    ) -> Result<(), ComplianceGovernanceError> {
        ensure_base_authorized(&command.context)?;
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        ensure_active_human(&mut transaction, &command.context).await?;
        let (action, resource_kind) = match command.kind {
            ComplianceReadKind::Case => ("compliance.case.queried", "compliance.case_query"),
            ComplianceReadKind::LegalHold => (
                "compliance.legal_hold.queried",
                "compliance.legal_hold_query",
            ),
        };
        append_governance_evidence(
            &mut transaction,
            &command.context,
            action,
            resource_kind,
            command.resource_id,
            command.details,
            None,
            &command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn ensure_base_authorized(context: &AuthorizationContext) -> Result<(), ComplianceGovernanceError> {
    if !context.permits(context.organization_id, Permission::LegalHoldManage) {
        return Err(ComplianceGovernanceError::Forbidden);
    }
    Ok(())
}

async fn ensure_active_human(
    transaction: &mut Box<dyn crate::ControlPlaneTransaction>,
    context: &AuthorizationContext,
) -> Result<(), ComplianceGovernanceError> {
    if !transaction
        .authorize_human_permission(
            context.organization_id,
            context.principal_id,
            Permission::LegalHoldManage,
        )
        .await?
    {
        return Err(ComplianceGovernanceError::Forbidden);
    }
    Ok(())
}

async fn ensure_approval<S: ApprovalQueryStore>(
    store: &S,
    proof: &ApprovalProof,
    organization_id: OrganizationId,
    requested_by: copilotmail_domain::PrincipalId,
    resource_kind: &str,
    resource_id: &str,
) -> Result<(), ComplianceGovernanceError> {
    if proof.organization_id != organization_id
        || proof.requested_by != requested_by
        || proof.action != ApprovalAction::LegalHold
        || proof.resource_kind != resource_kind
        || proof.resource_id != resource_id
        || !store.approval_permits(proof).await?
    {
        return Err(ComplianceGovernanceError::Forbidden);
    }
    Ok(())
}

fn ensure_archive_scope(
    archive: &AuditArchiveRecord,
    organization_id: OrganizationId,
) -> Result<(), ComplianceGovernanceError> {
    if archive.organization_id != organization_id {
        return Err(ComplianceGovernanceError::NotFound);
    }
    Ok(())
}

fn ensure_object_evidence(
    archive: &AuditArchiveRecord,
    evidence: &LegalHoldObjectEvidence,
    expected_enabled: bool,
    now: DateTime<Utc>,
) -> Result<(), ComplianceGovernanceError> {
    if evidence.enabled != expected_enabled
        || evidence.events_version_id != archive.events.version_id
        || evidence.manifest_version_id != archive.manifest.version_id
        || evidence.verified_at < archive.verified_at
        || evidence.verified_at > now
    {
        return Err(ComplianceGovernanceError::Conflict);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn append_governance_evidence(
    transaction: &mut Box<dyn crate::ControlPlaneTransaction>,
    context: &AuthorizationContext,
    action: &str,
    resource_kind: &str,
    resource_id: String,
    details: serde_json::Value,
    approval_request_id: Option<ApprovalRequestId>,
    trace_id: &str,
    now: DateTime<Utc>,
) -> Result<(), ComplianceGovernanceError> {
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
            kind: resource_kind.into(),
            id: resource_id.clone(),
        },
        AuditOutcome::Success,
        trace_id.to_owned(),
        details,
        now,
    )?
    .with_context(audit_context)?
    .correlate_human_session(context.authentication.as_ref())?;
    let event = PendingEvent::new(
        format!("{action}.v1"),
        resource_kind,
        resource_id,
        json!({"organization_id": context.organization_id}),
        now,
    );
    transaction.append_audit_event(&audit).await?;
    transaction.enqueue_event(&event).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlPlaneTransaction, ExistingOrganizationBootstrap, PendingEvent};
    use async_trait::async_trait;
    use copilotmail_domain::{
        AuditCheckpointId, Organization, OrganizationSlug, Principal, PrincipalId, Role,
        RoleAssignment,
    };
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    struct CaptureTransaction {
        audit: Arc<Mutex<Option<AuditEvent>>>,
    }

    #[derive(Clone)]
    struct CaptureStore {
        audit: Arc<Mutex<Option<AuditEvent>>>,
    }

    #[async_trait]
    impl ControlPlaneStore for CaptureStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(CaptureTransaction {
                audit: self.audit.clone(),
            }))
        }
    }

    #[async_trait]
    impl ApprovalQueryStore for CaptureStore {
        async fn approval_permits(&self, _: &ApprovalProof) -> Result<bool, StoreError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for CaptureTransaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_organization(&mut self, _: &Organization) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_principal(&mut self, _: &Principal) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role(&mut self, _: &Role) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role_assignment(&mut self, _: &RoleAssignment) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn record_organization_bootstrap(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn authorize_human_permission(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            permission: Permission,
        ) -> Result<bool, StoreError> {
            assert_eq!(permission, Permission::LegalHoldManage);
            Ok(true)
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            *self.audit.lock().expect("audit capture lock") = Some(event.clone());
            Ok(())
        }

        async fn enqueue_event(&mut self, _: &PendingEvent) -> Result<(), StoreError> {
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            Ok(())
        }
    }

    fn context(permission: bool) -> AuthorizationContext {
        AuthorizationContext {
            principal_id: PrincipalId::new(),
            organization_id: OrganizationId::new(),
            permissions: if permission {
                [Permission::LegalHoldManage].into_iter().collect()
            } else {
                BTreeSet::new()
            },
            authentication: None,
        }
    }

    fn archive(context: &AuthorizationContext, now: DateTime<Utc>) -> AuditArchiveRecord {
        AuditArchiveRecord {
            archive_id: AuditArchiveId::new(),
            organization_id: context.organization_id,
            checkpoint_id: AuditCheckpointId::new(),
            provider: "s3".into(),
            bucket: "audit".into(),
            events: crate::ArchivedObjectRecord {
                key: "events".into(),
                version_id: "events-v1".into(),
                sha256_hex: "a".repeat(64),
                size_bytes: 1,
            },
            manifest: crate::ArchivedObjectRecord {
                key: "manifest".into(),
                version_id: "manifest-v1".into(),
                sha256_hex: "b".repeat(64),
                size_bytes: 1,
            },
            retention_mode: "compliance".into(),
            retain_until: now + chrono::Duration::days(1),
            verified_at: now,
            created_by: context.principal_id,
            created_at: now,
        }
    }

    #[test]
    fn legal_hold_permission_is_default_deny_and_tenant_bound() {
        assert!(matches!(
            ensure_base_authorized(&context(false)),
            Err(ComplianceGovernanceError::Forbidden)
        ));
        assert!(ensure_base_authorized(&context(true)).is_ok());
        let mut cross_tenant = context(true);
        cross_tenant.organization_id = OrganizationId::new();
        assert!(!cross_tenant.permits(OrganizationId::new(), Permission::LegalHoldManage));
    }

    #[test]
    fn object_evidence_requires_both_exact_versions_and_bounded_readback_time() {
        let context = context(true);
        let now = Utc::now();
        let archive = archive(&context, now);
        let exact = LegalHoldObjectEvidence {
            events_version_id: archive.events.version_id.clone(),
            manifest_version_id: archive.manifest.version_id.clone(),
            enabled: true,
            verified_at: now,
        };
        assert!(ensure_object_evidence(&archive, &exact, true, now).is_ok());
        let mut wrong = exact.clone();
        wrong.manifest_version_id = "another-version".into();
        assert!(matches!(
            ensure_object_evidence(&archive, &wrong, true, now),
            Err(ComplianceGovernanceError::Conflict)
        ));
        let mut future = exact;
        future.verified_at = now + chrono::Duration::seconds(1);
        assert!(matches!(
            ensure_object_evidence(&archive, &future, true, now),
            Err(ComplianceGovernanceError::Conflict)
        ));
    }

    #[tokio::test]
    async fn legal_hold_audit_context_carries_the_exact_approval_request() {
        let context = context(true);
        let approval_request_id = ApprovalRequestId::new();
        let captured = Arc::new(Mutex::new(None));
        let mut transaction: Box<dyn ControlPlaneTransaction> = Box::new(CaptureTransaction {
            audit: captured.clone(),
        });
        append_governance_evidence(
            &mut transaction,
            &context,
            "compliance.legal_hold.applied",
            "compliance.legal_hold",
            LegalHoldId::new().to_string(),
            json!({"safe": true}),
            Some(approval_request_id),
            "trace-legal-hold",
            Utc::now(),
        )
        .await
        .expect("capture legal hold audit evidence");
        let audit = captured
            .lock()
            .expect("audit capture lock")
            .clone()
            .expect("captured audit event");
        assert_eq!(audit.context.approval_request_id, Some(approval_request_id));
        assert_eq!(
            audit.context.subject_principal_id,
            Some(context.principal_id)
        );
    }

    #[tokio::test]
    async fn privileged_compliance_read_is_audited_before_return() {
        let context = context(true);
        let organization_id = context.organization_id;
        let captured = Arc::new(Mutex::new(None));
        ComplianceGovernanceService::new(CaptureStore {
            audit: captured.clone(),
        })
        .record_read(RecordComplianceReadCommand {
            context,
            kind: ComplianceReadKind::Case,
            resource_id: organization_id.to_string(),
            details: json!({
                "operation": "list",
                "result_count": 0,
                "has_more": false,
            }),
            trace_id: "trace-compliance-read".into(),
            now: Utc::now(),
        })
        .await
        .expect("record compliance read");
        let audit = captured
            .lock()
            .expect("audit capture lock")
            .clone()
            .expect("captured compliance read audit");
        assert_eq!(audit.action, "compliance.case.queried");
        assert_eq!(audit.resource.kind, "compliance.case_query");
    }
}
