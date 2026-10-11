//! Deterministic service tests; persistence races stay in the PG suite.
use super::*;
use crate::{
    ActiveApprovalPolicy, ApprovalDecisionRecord, ControlPlaneTransaction, ExistingOidcOwnerBinding,
};
use async_trait::async_trait;
use chrono::Duration;
use copilotmail_domain::{
    Organization, OrganizationSlug, Principal, PrincipalId, Role, RoleAssignment,
};
use std::sync::{Arc, Mutex};

struct State {
    authorized: bool,
    human: bool,
    owner: Option<ExistingOidcOwnerBinding>,
    policy: Option<ActiveApprovalPolicy>,
    detail: Option<ApprovalRequestDetail>,
    rows: Vec<ApprovalRequestSummary>,
    fail: Option<&'static str>,
    cas: bool,
    begins: usize,
    checked: Vec<(Permission, bool)>,
    commits: Vec<Vec<&'static str>>,
}

#[derive(Clone)]
struct Store(Arc<Mutex<State>>);
struct Tx {
    store: Store,
    pending: Vec<&'static str>,
}
impl Tx {
    fn stage(&mut self, operation: &'static str) -> Result<(), StoreError> {
        if self.store.0.lock().unwrap().fail == Some(operation) {
            return Err(StoreError::Unavailable);
        }
        self.pending.push(operation);
        Ok(())
    }
    fn permitted(&self, permission: Permission, human_only: bool) -> bool {
        let mut state = self.store.0.lock().unwrap();
        state.checked.push((permission, human_only));
        state.authorized && (!human_only || state.human)
    }
}

#[async_trait]
impl ControlPlaneStore for Store {
    async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
        let mut state = self.0.lock().unwrap();
        state.begins += 1;
        if state.fail == Some("begin") {
            return Err(StoreError::Unavailable);
        }
        Ok(Box::new(Tx {
            store: self.clone(),
            pending: vec![],
        }))
    }
}

#[async_trait]
impl ControlPlaneTransaction for Tx {
    async fn find_organization_bootstrap(
        &mut self,
        _: &OrganizationSlug,
    ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
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
        Ok(self.permitted(permission, true))
    }
    async fn authorize_principal_permission(
        &mut self,
        _: OrganizationId,
        _: PrincipalId,
        permission: Permission,
    ) -> Result<bool, StoreError> {
        Ok(self.permitted(permission, false))
    }
    async fn active_principal_exists(
        &mut self,
        _: OrganizationId,
        _: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(self.store.0.lock().unwrap().authorized)
    }
    async fn find_oidc_owner_binding(
        &mut self,
        _: OrganizationId,
    ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
        Ok(self.store.0.lock().unwrap().owner.clone())
    }
    async fn find_active_approval_policy(
        &mut self,
        _: OrganizationId,
        _: &str,
    ) -> Result<Option<ActiveApprovalPolicy>, StoreError> {
        Ok(self.store.0.lock().unwrap().policy.clone())
    }
    async fn insert_approval_policy(
        &mut self,
        _: PolicyId,
        _: OrganizationId,
        _: &str,
        _: &str,
        _: PrincipalId,
        _: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.stage("policy")
    }
    async fn insert_approval_policy_version(
        &mut self,
        _: &ApprovalPolicyVersion,
    ) -> Result<(), StoreError> {
        self.stage("version")
    }
    async fn activate_approval_policy_version(
        &mut self,
        _: OrganizationId,
        _: PolicyId,
        _: PolicyVersionId,
        _: Option<PolicyVersionId>,
    ) -> Result<bool, StoreError> {
        self.stage("activate")?;
        Ok(self.store.0.lock().unwrap().cas)
    }
    async fn insert_approval_request(&mut self, _: &ApprovalRequest) -> Result<(), StoreError> {
        self.stage("request")
    }
    async fn record_approval_decision(
        &mut self,
        _: &ApprovalDecisionWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        self.stage("decision")?;
        Ok(Some(ApprovalDecisionRecord {
            status: ApprovalStatus::Approved,
            approval_count: 2,
        }))
    }
    async fn cancel_approval_request(
        &mut self,
        _: &ApprovalCancellationWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        self.stage("cancel")?;
        Ok(Some(ApprovalDecisionRecord {
            status: ApprovalStatus::Cancelled,
            approval_count: 0,
        }))
    }
    async fn append_audit_event(&mut self, _: &AuditEvent) -> Result<(), StoreError> {
        self.stage("audit")
    }
    async fn enqueue_event(&mut self, _: &PendingEvent) -> Result<(), StoreError> {
        self.stage("outbox")
    }
    async fn commit(self: Box<Self>) -> Result<(), StoreError> {
        let mut state = self.store.0.lock().unwrap();
        if state.fail == Some("commit") {
            return Err(StoreError::Unavailable);
        }
        state.commits.push(self.pending);
        Ok(())
    }
}

#[async_trait]
impl ApprovalQueryStore for Store {
    async fn approval_permits(&self, _: &ApprovalProof) -> Result<bool, StoreError> {
        Ok(false)
    }
    async fn get_approval_request(
        &self,
        _: OrganizationId,
        _: ApprovalRequestId,
        _: DateTime<Utc>,
    ) -> Result<Option<ApprovalRequestDetail>, StoreError> {
        Ok(self.0.lock().unwrap().detail.clone())
    }
    async fn list_approval_requests(
        &self,
        _: ApprovalRequestListQuery,
    ) -> Result<Vec<ApprovalRequestSummary>, StoreError> {
        Ok(self.0.lock().unwrap().rows.clone())
    }
}

struct Fixture {
    store: Store,
    context: AuthorizationContext,
    request_id: ApprovalRequestId,
    now: DateTime<Utc>,
}
fn fixture() -> Fixture {
    let now = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    let context = AuthorizationContext {
        organization_id: OrganizationId::new(),
        principal_id: PrincipalId::new(),
        permissions: [Permission::AuthorizationManage, Permission::AuditExport]
            .into_iter()
            .collect(),
        authentication: None,
    };
    let request_id = ApprovalRequestId::new();
    let policy = ActiveApprovalPolicy {
        policy_id: PolicyId::new(),
        policy_version_id: PolicyVersionId::new(),
        version: 1,
        action: ApprovalAction::AuditExport,
        required_approvals: 2,
    };
    let detail = ApprovalRequestDetail {
        steps: vec![],
        request: ApprovalRequestSummary {
            request_id,
            organization_id: context.organization_id,
            policy_version_id: policy.policy_version_id,
            action: policy.action,
            resource_kind: "audit.archive".into(),
            resource_id: "archive".into(),
            requested_by: context.principal_id,
            reason: "Review evidence".into(),
            ticket_reference: None,
            required_approvals: 2,
            approval_count: 0,
            rejection_count: 0,
            status: ApprovalStatus::Pending,
            created_at: now,
            expires_at: now + Duration::hours(1),
            completed_at: None,
            cancelled_at: None,
        },
    };
    let owner = ExistingOidcOwnerBinding {
        organization_id: context.organization_id,
        principal_id: context.principal_id,
        issuer: "https://id.example.test".into(),
        subject: "owner".into(),
        display_name: "Owner".into(),
    };
    let store = Store(Arc::new(Mutex::new(State {
        authorized: true,
        human: true,
        owner: Some(owner),
        policy: Some(policy),
        rows: vec![detail.request.clone()],
        detail: Some(detail),
        fail: None,
        cas: true,
        begins: 0,
        checked: vec![],
        commits: vec![],
    })));
    Fixture {
        store,
        context,
        request_id,
        now,
    }
}

impl Fixture {
    fn policy(&self) -> ApprovalPolicyService<Store> {
        ApprovalPolicyService::new(self.store.clone())
    }
    fn workflow(&self) -> ApprovalWorkflowService<Store> {
        ApprovalWorkflowService::new(self.store.clone())
    }
    fn create(&self) -> CreateApprovalPolicyCommand {
        CreateApprovalPolicyCommand {
            organization_id: self.context.organization_id,
            policy_key: "audit.export".into(),
            display_name: "Audit export".into(),
            action: ApprovalAction::AuditExport,
            required_approvals: 2,
            rule: json!({}),
            trace_id: "approval-test".into(),
            now: self.now,
        }
    }
    fn append(&self) -> AppendApprovalPolicyVersionCommand {
        AppendApprovalPolicyVersionCommand {
            organization_id: self.context.organization_id,
            policy_key: "audit.export".into(),
            action: ApprovalAction::AuditExport,
            required_approvals: 2,
            rule: json!({}),
            trace_id: "approval-test".into(),
            now: self.now,
        }
    }
    fn request(&self) -> RequestApprovalCommand {
        RequestApprovalCommand {
            context: self.context.clone(),
            policy_key: "audit.export".into(),
            action: ApprovalAction::AuditExport,
            resource_kind: "audit.archive".into(),
            resource_id: "archive".into(),
            reason: "Review evidence".into(),
            ticket_reference: None,
            expires_at: self.now + Duration::hours(1),
            trace_id: "approval-test".into(),
            now: self.now,
        }
    }
    fn decide(&self) -> DecideApprovalCommand {
        DecideApprovalCommand {
            context: self.context.clone(),
            request_id: self.request_id,
            decision: ApprovalDecision::Approved,
            reason: "Reviewed".into(),
            trace_id: "approval-test".into(),
            now: self.now,
        }
    }
    fn cancel(&self) -> CancelApprovalRequestCommand {
        CancelApprovalRequestCommand {
            context: self.context.clone(),
            request_id: self.request_id,
            trace_id: "approval-test".into(),
            now: self.now,
        }
    }
    fn get(&self) -> GetApprovalRequestCommand {
        GetApprovalRequestCommand {
            context: self.context.clone(),
            request_id: self.request_id,
            trace_id: "approval-test".into(),
            now: self.now,
        }
    }
    fn list(&self) -> ListApprovalRequestsCommand {
        ListApprovalRequestsCommand {
            context: self.context.clone(),
            view: ApprovalRequestView::Requested,
            status: None,
            after_request_id: None,
            limit: 1,
            trace_id: "approval-test".into(),
            now: self.now,
        }
    }
    fn assert_uncommitted(&self) {
        assert!(self.store.0.lock().unwrap().commits.is_empty());
    }
}

#[test]
fn policy_text_checks_unicode_bounds_and_strict_keys() {
    assert!(validate_policy_text("审计导出", 4, false).is_ok());
    for value in ["", " space", "space ", "line\n", "ab\u{0}", "12345"] {
        assert!(validate_policy_text(value, 4, false).is_err(), "{value:?}");
    }
    assert!(validate_policy_text("audit.export2", 100, true).is_ok());
    for value in ["Audit.export", "audit-export", "策略", "audit/export"] {
        assert!(validate_policy_text(value, 100, true).is_err());
    }
}

#[test]
fn action_permissions_are_explicit_and_never_inferred_from_approval_alone() {
    for (action, permission) in [
        (ApprovalAction::SendAs, Permission::MailboxUse),
        (ApprovalAction::SendOnBehalfOf, Permission::MailboxUse),
        (ApprovalAction::BulkDelete, Permission::MailboxUse),
        (ApprovalAction::ContentReview, Permission::ContentReview),
        (ApprovalAction::AuditExport, Permission::AuditExport),
        (ApprovalAction::RetentionChange, Permission::RetentionManage),
        (
            ApprovalAction::PrivilegeElevation,
            Permission::AuthorizationManage,
        ),
        (
            ApprovalAction::ProductionAgentMail,
            Permission::AuthorizationManage,
        ),
        (ApprovalAction::AgentFirstContact, Permission::AgentOperate),
        (ApprovalAction::LegalHold, Permission::LegalHoldManage),
        (ApprovalAction::BreakGlass, Permission::BreakGlass),
    ] {
        assert_eq!(approval_action_permission(action), permission);
    }
}

#[tokio::test]
async fn policy_creation_and_versioning_are_atomic_and_cas_protected() {
    let f = fixture();
    f.policy().create(f.create()).await.unwrap();
    f.policy().append_version(f.append()).await.unwrap();
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![
            vec!["policy", "version", "activate", "audit", "outbox"],
            vec!["version", "activate", "audit", "outbox"]
        ]
    );
    for failure in [
        "begin", "policy", "version", "activate", "audit", "outbox", "commit",
    ] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.policy().create(f.create()).await.is_err(), "{failure}");
        f.assert_uncommitted();
    }
    let f = fixture();
    f.store.0.lock().unwrap().cas = false;
    assert!(f.policy().append_version(f.append()).await.is_err());
    f.assert_uncommitted();
    let f = fixture();
    f.store.0.lock().unwrap().policy.as_mut().unwrap().version = u32::MAX;
    assert!(f.policy().append_version(f.append()).await.is_err());
    f.assert_uncommitted();
}

#[tokio::test]
async fn operator_policy_service_requires_exact_current_human_owner() {
    for variant in 0..4 {
        let f = fixture();
        {
            let mut state = f.store.0.lock().unwrap();
            match variant {
                0 => state.owner = None,
                1 => state.owner.as_mut().unwrap().organization_id = OrganizationId::new(),
                2 => state.authorized = false,
                _ => state.human = false,
            }
        }
        assert!(f.policy().create(f.create()).await.is_err());
        assert!(f.policy().append_version(f.append()).await.is_err());
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn request_checks_base_and_current_permission_but_does_not_exclude_agents() {
    let f = fixture();
    let mut command = f.request();
    command.context.permissions.clear();
    assert!(matches!(
        f.workflow().request(command).await,
        Err(ApprovalWorkflowError::Forbidden)
    ));
    assert_eq!(f.store.0.lock().unwrap().begins, 0);
    let f = fixture();
    f.store.0.lock().unwrap().authorized = false;
    assert!(matches!(
        f.workflow().request(f.request()).await,
        Err(ApprovalWorkflowError::Forbidden)
    ));
    f.assert_uncommitted();
    let f = fixture();
    f.store.0.lock().unwrap().human = false;
    f.workflow().request(f.request()).await.unwrap();
    assert_eq!(
        f.store.0.lock().unwrap().checked,
        vec![(Permission::AuditExport, false)]
    );
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["request", "audit", "outbox"]]
    );
}

#[tokio::test]
async fn requests_reject_missing_policy_wrong_action_and_bad_lifetime() {
    for variant in 0..4 {
        let f = fixture();
        let mut command = f.request();
        match variant {
            0 => f.store.0.lock().unwrap().policy = None,
            1 => {
                f.store.0.lock().unwrap().policy.as_mut().unwrap().action =
                    ApprovalAction::LegalHold
            }
            2 => command.expires_at = command.now,
            _ => command.policy_key = "INVALID".into(),
        }
        assert!(f.workflow().request(command).await.is_err());
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn request_decision_and_cancellation_failures_roll_back_all_evidence() {
    for failure in ["begin", "request", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.workflow().request(f.request()).await.is_err());
        f.assert_uncommitted();
    }
    for failure in ["begin", "decision", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.workflow().decide(f.decide()).await.is_err());
        f.assert_uncommitted();
    }
    for failure in ["begin", "cancel", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.workflow().cancel(f.cancel()).await.is_err());
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn decisions_require_a_current_human_manager_and_commit_evidence() {
    for variant in 0..3 {
        let f = fixture();
        let mut command = f.decide();
        match variant {
            0 => command.context.permissions.clear(),
            1 => f.store.0.lock().unwrap().authorized = false,
            _ => f.store.0.lock().unwrap().human = false,
        }
        assert!(matches!(
            f.workflow().decide(command).await,
            Err(ApprovalWorkflowError::Forbidden)
        ));
        f.assert_uncommitted();
    }
    let f = fixture();
    assert_eq!(
        f.workflow().decide(f.decide()).await.unwrap().status,
        ApprovalStatus::Approved
    );
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["decision", "audit", "outbox"]]
    );
}

#[tokio::test]
async fn cancellation_requires_exact_requester_resource_and_live_action_permission() {
    for variant in 0..5 {
        let f = fixture();
        let mut command = f.cancel();
        match variant {
            0 => command.context.principal_id = PrincipalId::new(),
            1 => command.context.permissions.clear(),
            2 => f.store.0.lock().unwrap().authorized = false,
            3 => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .detail
                    .as_mut()
                    .unwrap()
                    .request
                    .organization_id = OrganizationId::new()
            }
            _ => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .detail
                    .as_mut()
                    .unwrap()
                    .request
                    .request_id = ApprovalRequestId::new()
            }
        }
        assert!(f.workflow().cancel(command).await.is_err());
        f.assert_uncommitted();
    }
    let f = fixture();
    f.store.0.lock().unwrap().human = false;
    assert_eq!(
        f.workflow().cancel(f.cancel()).await.unwrap().status,
        ApprovalStatus::Cancelled
    );
    assert_eq!(
        f.store.0.lock().unwrap().checked,
        vec![(Permission::AuditExport, false)]
    );
}

#[tokio::test]
async fn detail_disclosure_is_bound_and_fails_if_audit_commit_fails() {
    for failure in ["begin", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.workflow().get(f.get()).await.is_err());
        f.assert_uncommitted();
    }
    let f = fixture();
    f.store
        .0
        .lock()
        .unwrap()
        .detail
        .as_mut()
        .unwrap()
        .request
        .organization_id = OrganizationId::new();
    assert!(f.workflow().get(f.get()).await.is_err());
    f.assert_uncommitted();
    let f = fixture();
    f.workflow().get(f.get()).await.unwrap();
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["audit", "outbox"]]
    );
}

#[tokio::test]
async fn list_is_bounded_audited_and_actionable_view_requires_human_manager() {
    let f = fixture();
    let row = f.store.0.lock().unwrap().rows[0].clone();
    f.store.0.lock().unwrap().rows.push(row);
    let page = f.workflow().list(f.list()).await.unwrap();
    assert!(page.has_more);
    assert_eq!(page.items.len(), 1);
    for limit in [0, 101] {
        let mut command = f.list();
        command.limit = limit;
        assert!(f.workflow().list(command).await.is_err());
    }
    let f = fixture();
    f.store.0.lock().unwrap().human = false;
    let mut command = f.list();
    command.view = ApprovalRequestView::Actionable;
    assert!(matches!(
        f.workflow().list(command).await,
        Err(ApprovalWorkflowError::Forbidden)
    ));
    f.assert_uncommitted();
    for failure in ["begin", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.workflow().list(f.list()).await.is_err());
        f.assert_uncommitted();
    }
}
