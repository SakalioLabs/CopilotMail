use super::*;
use crate::{ApprovedRequestEvidence, ControlPlaneTransaction, RetentionPolicyRecord};
use async_trait::async_trait;
use copilotmail_domain::{
    ApprovalRequestId, Organization, OrganizationId, OrganizationSlug, Principal, PrincipalId,
    Role, RoleAssignment,
};
use std::sync::{Arc, Mutex};

struct State {
    active_human: bool,
    authorized: bool,
    fail: Option<&'static str>,
    record: Option<RetentionPolicyRecord>,
    version: Option<RetentionPolicyVersion>,
    detail: Option<RetentionPolicyDetail>,
    activation: Option<RetentionPolicyActivationRecord>,
    approval: Option<ApprovedRequestEvidence>,
    cas: bool,
    begins: usize,
    reads: usize,
    commits: Vec<Vec<&'static str>>,
    activation_at: Option<DateTime<Utc>>,
    committed_times: Vec<Vec<DateTime<Utc>>>,
}

#[derive(Clone)]
struct Store(Arc<Mutex<State>>);

struct Tx {
    store: Store,
    pending: Vec<&'static str>,
    times: Vec<DateTime<Utc>>,
}

impl Tx {
    fn stage(&mut self, name: &'static str) -> Result<(), StoreError> {
        if self.store.0.lock().unwrap().fail == Some(name) {
            return Err(StoreError::Unavailable);
        }
        self.pending.push(name);
        Ok(())
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
            times: vec![],
        }))
    }
}

#[async_trait]
impl ApprovalQueryStore for Store {
    async fn approval_permits(&self, _: &ApprovalProof) -> Result<bool, StoreError> {
        Ok(false)
    }
    async fn approved_request_evidence(
        &self,
        _: &ApprovalProof,
    ) -> Result<Option<ApprovedRequestEvidence>, StoreError> {
        Ok(self.0.lock().unwrap().approval.clone())
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
    async fn active_human_principal_exists(
        &mut self,
        _: OrganizationId,
        _: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(self.store.0.lock().unwrap().active_human)
    }
    async fn authorize_human_permission(
        &mut self,
        _: OrganizationId,
        _: PrincipalId,
        permission: Permission,
    ) -> Result<bool, StoreError> {
        assert_eq!(permission, Permission::RetentionManage);
        let state = self.store.0.lock().unwrap();
        Ok(state.active_human && state.authorized)
    }
    async fn retention_policy_detail(
        &mut self,
        _: OrganizationId,
        _: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError> {
        let mut state = self.store.0.lock().unwrap();
        state.reads += 1;
        Ok(state.detail.clone())
    }
    async fn insert_retention_policy(&mut self, _: &RetentionPolicy) -> Result<(), StoreError> {
        self.stage("policy")
    }
    async fn insert_retention_policy_version(
        &mut self,
        _: &RetentionPolicyVersion,
    ) -> Result<(), StoreError> {
        self.stage("version")
    }
    async fn find_retention_policy_for_update(
        &mut self,
        _: OrganizationId,
        _: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyRecord>, StoreError> {
        Ok(self.store.0.lock().unwrap().record.clone())
    }
    async fn find_retention_policy_version(
        &mut self,
        _: OrganizationId,
        _: RetentionPolicyId,
        _: RetentionPolicyVersionId,
    ) -> Result<Option<RetentionPolicyVersion>, StoreError> {
        Ok(self.store.0.lock().unwrap().version.clone())
    }
    async fn find_retention_policy_activation_by_approval(
        &mut self,
        _: OrganizationId,
        _: ApprovalRequestId,
    ) -> Result<Option<RetentionPolicyActivationRecord>, StoreError> {
        Ok(self.store.0.lock().unwrap().activation.clone())
    }
    async fn insert_retention_policy_activation(
        &mut self,
        activation: &RetentionPolicyActivationRecord,
    ) -> Result<RetentionPolicyActivationRecord, StoreError> {
        self.stage("activation")?;
        let mut result = activation.clone();
        let state = self.store.0.lock().unwrap();
        if let Some(at) = state.activation_at {
            result.activated_at = at;
        }
        if state.fail == Some("bad_activation") {
            result.policy_id = RetentionPolicyId::new();
        }
        Ok(result)
    }
    async fn activate_retention_policy_version(
        &mut self,
        _: OrganizationId,
        _: RetentionPolicyId,
        _: RetentionPolicyVersionId,
        _: Option<RetentionPolicyVersionId>,
    ) -> Result<bool, StoreError> {
        self.stage("activate")?;
        Ok(self.store.0.lock().unwrap().cas)
    }
    async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
        self.times.push(event.occurred_at);
        self.stage("audit")
    }
    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
        self.times.push(event.occurred_at);
        self.stage("outbox")
    }
    async fn commit(self: Box<Self>) -> Result<(), StoreError> {
        let mut state = self.store.0.lock().unwrap();
        if state.fail == Some("commit") {
            return Err(StoreError::Unavailable);
        }
        state.commits.push(self.pending);
        state.committed_times.push(self.times);
        Ok(())
    }
}

struct Fixture {
    store: Store,
    context: AuthorizationContext,
    policy_id: RetentionPolicyId,
    version_id: RetentionPolicyVersionId,
    now: DateTime<Utc>,
}

fn fixture() -> Fixture {
    let now = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    let context = AuthorizationContext {
        organization_id: OrganizationId::new(),
        principal_id: PrincipalId::new(),
        permissions: [Permission::RetentionManage].into_iter().collect(),
        authentication: None,
    };
    let policy = RetentionPolicy::new(
        context.organization_id,
        "audit.standard".into(),
        "Standard".into(),
        RetentionResourceKind::AuditEvidence,
        context.principal_id,
        now,
    )
    .unwrap();
    let version = RetentionPolicyVersion::new(
        policy.id(),
        context.organization_id,
        2,
        365,
        Some(30),
        None,
        json!({}),
        context.principal_id,
        now,
    )
    .unwrap();
    let store = Store(Arc::new(Mutex::new(State {
        active_human: true,
        authorized: true,
        fail: None,
        record: Some(RetentionPolicyRecord {
            policy: policy.clone(),
            active_version_id: None,
            latest_version: 2,
        }),
        version: Some(version.clone()),
        detail: Some(RetentionPolicyDetail {
            policy: policy.clone(),
            active_version: None,
            latest_version: version.clone(),
        }),
        activation: None,
        approval: Some(ApprovedRequestEvidence {
            expires_at: now + chrono::Duration::minutes(5),
            required_approvals: 2,
            approval_count: 2,
        }),
        cas: true,
        begins: 0,
        reads: 0,
        commits: vec![],
        activation_at: None,
        committed_times: vec![],
    })));
    Fixture {
        store,
        context,
        policy_id: policy.id(),
        version_id: version.id(),
        now,
    }
}

impl Fixture {
    fn service(&self) -> RetentionPolicyService<Store> {
        RetentionPolicyService::new(self.store.clone())
    }
    fn create(&self) -> CreateRetentionPolicyCommand {
        CreateRetentionPolicyCommand {
            context: self.context.clone(),
            policy_key: "audit.standard".into(),
            display_name: "Standard".into(),
            resource_kind: RetentionResourceKind::AuditEvidence,
            minimum_retention_days: 365,
            archive_after_days: Some(30),
            delete_after_days: None,
            rule: json!({}),
            trace_id: "retention-create-test".into(),
            now: self.now,
        }
    }
    fn append(&self) -> AppendRetentionPolicyVersionCommand {
        AppendRetentionPolicyVersionCommand {
            context: self.context.clone(),
            policy_id: self.policy_id,
            minimum_retention_days: 365,
            archive_after_days: None,
            delete_after_days: None,
            rule: json!({}),
            trace_id: "retention-version-test".into(),
            now: self.now,
        }
    }
    fn activate(&self) -> ActivateRetentionPolicyVersionCommand {
        ActivateRetentionPolicyVersionCommand {
            context: self.context.clone(),
            policy_id: self.policy_id,
            policy_version_id: self.version_id,
            approval: ApprovalProof {
                organization_id: self.context.organization_id,
                requested_by: self.context.principal_id,
                request_id: ApprovalRequestId::new(),
                action: ApprovalAction::RetentionChange,
                resource_kind: "compliance.retention_policy_version".into(),
                resource_id: self.version_id.to_string(),
                now: self.now,
            },
            trace_id: "retention-activate-test".into(),
            now: self.now,
        }
    }
    fn get(&self) -> GetRetentionPolicyCommand {
        GetRetentionPolicyCommand {
            context: self.context.clone(),
            policy_id: self.policy_id,
            trace_id: "retention-read-test".into(),
            now: self.now,
        }
    }
    fn assert_uncommitted(&self) {
        assert!(self.store.0.lock().unwrap().commits.is_empty());
    }
}

#[tokio::test]
async fn creates_policy_and_version_with_one_atomic_audit_outbox_commit() {
    let f = fixture();
    let result = f.service().create(f.create()).await.unwrap();
    assert_eq!(result.version.version(), 1);
    assert_eq!(result.version.policy_id(), result.policy.id());
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["policy", "version", "audit", "outbox"]]
    );
}

#[tokio::test]
async fn every_create_failure_rolls_back_without_partial_policy() {
    for failure in ["begin", "policy", "version", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.service().create(f.create()).await.is_err());
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn current_permission_revocation_blocks_every_retention_operation() {
    let f = fixture();
    f.store.0.lock().unwrap().authorized = false;
    assert!(matches!(
        f.service().create(f.create()).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    assert!(matches!(
        f.service().append_version(f.append()).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    assert!(matches!(
        f.service().activate_version(f.activate()).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    assert!(matches!(
        f.service().get(f.get()).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    assert_eq!(f.store.0.lock().unwrap().reads, 0);
    f.assert_uncommitted();
}

#[tokio::test]
async fn invalid_policy_and_missing_permission_fail_before_storage() {
    let f = fixture();
    let mut command = f.create();
    command.policy_key = "Bad..Key".into();
    assert!(matches!(
        f.service().create(command).await,
        Err(RetentionPolicyError::Domain(_))
    ));
    let mut command = f.create();
    command.context.permissions.clear();
    assert!(matches!(
        f.service().create(command).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    assert_eq!(f.store.0.lock().unwrap().begins, 0);
}

#[tokio::test]
async fn inactive_or_nonhuman_actor_does_not_read_detail_or_write() {
    let f = fixture();
    f.store.0.lock().unwrap().active_human = false;
    assert!(matches!(
        f.service().create(f.create()).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    assert!(matches!(
        f.service().get(f.get()).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    assert_eq!(f.store.0.lock().unwrap().reads, 0);
    f.assert_uncommitted();
}

#[tokio::test]
async fn appends_only_a_new_version_and_rejects_overflow() {
    let f = fixture();
    let version = f.service().append_version(f.append()).await.unwrap();
    assert_eq!(version.version(), 3);
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["version", "audit", "outbox"]]
    );
    let f = fixture();
    f.store
        .0
        .lock()
        .unwrap()
        .record
        .as_mut()
        .unwrap()
        .latest_version = u32::MAX;
    assert!(matches!(
        f.service().append_version(f.append()).await,
        Err(RetentionPolicyError::Domain(_))
    ));
    f.assert_uncommitted();
}

#[tokio::test]
async fn activation_is_atomic_and_compare_and_swap_failure_rolls_back() {
    let f = fixture();
    let version = f.service().activate_version(f.activate()).await.unwrap();
    assert_eq!(version.id(), f.version_id);
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["activation", "activate", "audit", "outbox"]]
    );
    for failure in ["activation", "activate", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.service().activate_version(f.activate()).await.is_err());
        f.assert_uncommitted();
    }
    let f = fixture();
    f.store.0.lock().unwrap().cas = false;
    assert!(matches!(
        f.service().activate_version(f.activate()).await,
        Err(RetentionPolicyError::Conflict)
    ));
    f.assert_uncommitted();
}

#[tokio::test]
async fn expired_insufficient_or_transferred_approval_never_activates() {
    for invalid in 0..4 {
        let f = fixture();
        let mut command = f.activate();
        match invalid {
            0 => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .approval
                    .as_mut()
                    .unwrap()
                    .expires_at = f.now
            }
            1 => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .approval
                    .as_mut()
                    .unwrap()
                    .required_approvals = 1
            }
            2 => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .approval
                    .as_mut()
                    .unwrap()
                    .approval_count = 1
            }
            _ => command.approval.requested_by = PrincipalId::new(),
        }
        assert!(matches!(
            f.service().activate_version(command).await,
            Err(RetentionPolicyError::Forbidden)
        ));
        assert_eq!(f.store.0.lock().unwrap().begins, 0);
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn exact_activation_replay_is_readonly_and_cannot_target_another_actor() {
    let f = fixture();
    let command = f.activate();
    {
        let mut state = f.store.0.lock().unwrap();
        state.record.as_mut().unwrap().active_version_id = Some(f.version_id);
        state.activation = Some(RetentionPolicyActivationRecord {
            id: RetentionPolicyActivationId::new(),
            organization_id: f.context.organization_id,
            policy_id: f.policy_id,
            policy_version_id: f.version_id,
            approval_request_id: command.approval.request_id,
            activated_by: f.context.principal_id,
            activated_at: f.now,
        });
    }
    assert_eq!(
        f.service()
            .activate_version(command.clone())
            .await
            .unwrap()
            .id(),
        f.version_id
    );
    f.assert_uncommitted();
    f.store
        .0
        .lock()
        .unwrap()
        .activation
        .as_mut()
        .unwrap()
        .activated_by = PrincipalId::new();
    assert!(matches!(
        f.service().activate_version(command).await,
        Err(RetentionPolicyError::Conflict)
    ));
    f.assert_uncommitted();
}

#[tokio::test]
async fn detail_is_only_returned_after_audit_and_outbox_commit() {
    let f = fixture();
    assert_eq!(
        f.service().get(f.get()).await.unwrap().policy.id(),
        f.policy_id
    );
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["audit", "outbox"]]
    );
    for failure in ["audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.service().get(f.get()).await.is_err());
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn missing_or_cross_tenant_detail_never_produces_read_evidence() {
    let f = fixture();
    f.store.0.lock().unwrap().detail = None;
    assert!(matches!(
        f.service().get(f.get()).await,
        Err(RetentionPolicyError::NotFound)
    ));
    f.assert_uncommitted();
    let f = fixture();
    f.store.0.lock().unwrap().detail = fixture().store.0.lock().unwrap().detail.clone();
    assert!(f.service().get(f.get()).await.is_err());
    f.assert_uncommitted();
}

#[tokio::test]
async fn activation_uses_persisted_time_and_refuses_late_or_mismatched_proof() {
    let f = fixture();
    let stored_time = f.now + chrono::Duration::seconds(20);
    f.store.0.lock().unwrap().activation_at = Some(stored_time);
    f.service().activate_version(f.activate()).await.unwrap();
    assert_eq!(
        f.store.0.lock().unwrap().committed_times,
        vec![vec![stored_time, stored_time]]
    );

    let f = fixture();
    let expires_at = f
        .store
        .0
        .lock()
        .unwrap()
        .approval
        .as_ref()
        .unwrap()
        .expires_at;
    f.store.0.lock().unwrap().activation_at = Some(expires_at);
    assert!(matches!(
        f.service().activate_version(f.activate()).await,
        Err(RetentionPolicyError::Forbidden)
    ));
    f.assert_uncommitted();

    let f = fixture();
    f.store.0.lock().unwrap().fail = Some("bad_activation");
    assert!(matches!(
        f.service().activate_version(f.activate()).await,
        Err(RetentionPolicyError::Store(_))
    ));
    f.assert_uncommitted();
}
