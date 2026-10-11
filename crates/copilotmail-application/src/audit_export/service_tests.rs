use super::*;
use crate::{ApprovedRequestEvidence, ControlPlaneTransaction};
use async_trait::async_trait;
use copilotmail_domain::{
    ApprovalRequestId, AuditCheckpointId, ComplianceCase, ComplianceCaseId, ComplianceCaseStatus,
    LegalHold, LegalHoldId, LegalHoldStatus, Organization, OrganizationId, OrganizationSlug,
    Principal, PrincipalId, Role, RoleAssignment,
};
use std::sync::{Arc, Mutex};

struct State {
    authorized: bool,
    fail: Option<&'static str>,
    grant: Option<AuditExportGrant>,
    archive: Option<AuditArchiveRecord>,
    approval: Option<ApprovedRequestEvidence>,
    begins: usize,
    commits: Vec<Vec<&'static str>>,
    audits: Vec<AuditEvent>,
    stored_time: Option<DateTime<Utc>>,
    corrupt_download: bool,
}

#[derive(Clone)]
struct Store(Arc<Mutex<State>>);

struct Tx {
    store: Store,
    pending: Vec<&'static str>,
    audits: Vec<AuditEvent>,
}

impl Tx {
    fn stage(&mut self, operation: &'static str) -> Result<(), StoreError> {
        if self.store.0.lock().unwrap().fail == Some(operation) {
            return Err(StoreError::Unavailable);
        }
        self.pending.push(operation);
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
            audits: vec![],
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
        assert_eq!(permission, Permission::AuditExport);
        Ok(self.store.0.lock().unwrap().authorized)
    }
    async fn authorize_audit_export_grant(
        &mut self,
        grant: &AuditExportGrant,
    ) -> Result<AuditExportGrant, StoreError> {
        self.stage("grant")?;
        Ok(grant.clone())
    }
    async fn record_audit_export_download(
        &mut self,
        download: &AuditExportDownloadRecord,
    ) -> Result<AuditExportDownloadRecord, StoreError> {
        self.stage("download")?;
        let state = self.store.0.lock().unwrap();
        let mut stored = download.clone();
        stored.downloaded_at = state.stored_time.unwrap_or(download.downloaded_at);
        if state.corrupt_download {
            stored.archive_id = AuditArchiveId::new();
        }
        Ok(stored)
    }
    async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
        self.stage("audit")?;
        self.audits.push(event.clone());
        Ok(())
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
        state.audits.extend(self.audits);
        Ok(())
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
impl AuditExportQueryStore for Store {
    async fn get_audit_export_grant(
        &self,
        _: OrganizationId,
        _: AuditExportId,
    ) -> Result<Option<AuditExportGrant>, StoreError> {
        Ok(self.0.lock().unwrap().grant.clone())
    }
    async fn get_audit_export_grant_by_approval(
        &self,
        _: OrganizationId,
        _: ApprovalRequestId,
    ) -> Result<Option<AuditExportGrant>, StoreError> {
        Ok(self.0.lock().unwrap().grant.clone())
    }
    async fn audit_export_grant_is_active(
        &self,
        _: OrganizationId,
        _: AuditExportId,
    ) -> Result<bool, StoreError> {
        let state = self.0.lock().unwrap();
        let database_now = state.stored_time.unwrap_or_else(Utc::now);
        Ok(state
            .grant
            .as_ref()
            .is_some_and(|grant| grant.is_active(database_now)))
    }
}

#[async_trait]
impl ComplianceQueryStore for Store {
    async fn active_human_compliance_actor_exists(
        &self,
        _: OrganizationId,
        _: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(self.0.lock().unwrap().authorized)
    }
    async fn get_audit_archive(
        &self,
        _: OrganizationId,
        _: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        Ok(self.0.lock().unwrap().archive.clone())
    }
    async fn get_compliance_case(
        &self,
        _: OrganizationId,
        _: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn compliance_case_cursor_exists(
        &self,
        _: OrganizationId,
        _: Option<ComplianceCaseStatus>,
        _: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn list_compliance_cases(
        &self,
        _: OrganizationId,
        _: Option<ComplianceCaseStatus>,
        _: Option<ComplianceCaseId>,
        _: i64,
    ) -> Result<Vec<ComplianceCase>, StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn get_legal_hold(
        &self,
        _: OrganizationId,
        _: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn legal_hold_cursor_exists(
        &self,
        _: OrganizationId,
        _: Option<LegalHoldStatus>,
        _: Option<ComplianceCaseId>,
        _: LegalHoldId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn list_legal_holds(
        &self,
        _: OrganizationId,
        _: Option<LegalHoldStatus>,
        _: Option<ComplianceCaseId>,
        _: Option<LegalHoldId>,
        _: i64,
    ) -> Result<Vec<LegalHold>, StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn get_legal_hold_by_apply_approval(
        &self,
        _: OrganizationId,
        _: ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError> {
        Err(StoreError::Unavailable)
    }
}

struct Fixture {
    store: Store,
    context: AuthorizationContext,
    grant: AuditExportGrant,
    archive: AuditArchiveRecord,
    now: DateTime<Utc>,
}

fn fixture() -> Fixture {
    let now = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    let context = AuthorizationContext {
        organization_id: OrganizationId::new(),
        principal_id: PrincipalId::new(),
        permissions: [Permission::AuditExport].into_iter().collect(),
        authentication: None,
    };
    let archive = AuditArchiveRecord {
        archive_id: AuditArchiveId::new(),
        organization_id: context.organization_id,
        checkpoint_id: AuditCheckpointId::new(),
        provider: "s3".into(),
        bucket: "audit".into(),
        events: ArchivedObjectRecord {
            key: "events.jsonl".into(),
            version_id: "events-v1".into(),
            sha256_hex: "a".repeat(64),
            size_bytes: 200,
        },
        manifest: ArchivedObjectRecord {
            key: "manifest.json".into(),
            version_id: "manifest-v1".into(),
            sha256_hex: "b".repeat(64),
            size_bytes: 100,
        },
        retention_mode: "COMPLIANCE".into(),
        retain_until: now + Duration::days(365),
        verified_at: now,
        created_by: context.principal_id,
        created_at: now,
    };
    let grant = AuditExportGrant::new(
        context.organization_id,
        archive.archive_id,
        ApprovalRequestId::new(),
        context.principal_id,
        now,
        now + Duration::minutes(15),
    )
    .unwrap();
    let store = Store(Arc::new(Mutex::new(State {
        authorized: true,
        fail: None,
        grant: None,
        archive: Some(archive.clone()),
        approval: Some(ApprovedRequestEvidence {
            expires_at: now + Duration::hours(1),
            required_approvals: 2,
            approval_count: 2,
        }),
        begins: 0,
        commits: vec![],
        audits: vec![],
        // The fake database clock defaults to the fixture's time axis; tests
        // move it explicitly to simulate grant expiry or a future download.
        stored_time: Some(now),
        corrupt_download: false,
    })));
    Fixture {
        store,
        context,
        grant,
        archive,
        now,
    }
}

impl Fixture {
    fn authorization(&self) -> AuditExportAuthorizationService<Store> {
        AuditExportAuthorizationService::new(self.store.clone())
    }
    fn downloads(&self) -> AuditExportDownloadService<Store> {
        AuditExportDownloadService::new(self.store.clone())
    }
    fn authorize(&self) -> AuthorizeAuditExportCommand {
        AuthorizeAuditExportCommand {
            context: self.context.clone(),
            archive_id: self.archive.archive_id,
            approval: ApprovalProof {
                organization_id: self.context.organization_id,
                request_id: self.grant.approval_request_id(),
                requested_by: self.context.principal_id,
                action: ApprovalAction::AuditExport,
                resource_kind: "audit.archive".into(),
                resource_id: self.archive.archive_id.to_string(),
                now: self.now,
            },
            trace_id: "export-test".into(),
            now: self.now,
        }
    }
    fn resolve(&self, artifact: AuditExportArtifact) -> ResolveAuditExportObjectCommand {
        ResolveAuditExportObjectCommand {
            context: self.context.clone(),
            grant_id: self.grant.id(),
            artifact,
            now: self.now,
        }
    }
    fn record(&self) -> RecordAuditExportDownloadCommand {
        // A download can only be recorded for a grant persisted by authorize.
        self.store.0.lock().unwrap().grant = Some(self.grant.clone());
        RecordAuditExportDownloadCommand {
            context: self.context.clone(),
            authorized: AuthorizedAuditExportObject {
                grant: self.grant.clone(),
                archive_id: self.archive.archive_id,
                artifact: AuditExportArtifact::Events,
                object: self.archive.events.clone(),
                bucket: self.archive.bucket.clone(),
            },
            retrieved: self.archive.events.clone(),
            trace_id: "export-download-test".into(),
            now: self.now,
        }
    }
    fn assert_uncommitted(&self) {
        assert!(self.store.0.lock().unwrap().commits.is_empty());
    }
}

#[tokio::test]
async fn authorization_clamps_lifetime_and_commits_grant_audit_outbox_together() {
    for lifetime in [Duration::minutes(2), Duration::hours(1)] {
        let f = fixture();
        f.store
            .0
            .lock()
            .unwrap()
            .approval
            .as_mut()
            .unwrap()
            .expires_at = f.now + lifetime;
        let grant = f.authorization().authorize(f.authorize()).await.unwrap();
        assert_eq!(
            grant.expires_at(),
            f.now + lifetime.min(Duration::minutes(15))
        );
        assert_eq!(
            f.store.0.lock().unwrap().commits,
            vec![vec!["grant", "audit", "outbox"]]
        );
    }
}

#[tokio::test]
async fn authorization_and_download_failures_never_commit_partial_evidence() {
    for failure in ["begin", "grant", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(
            f.authorization().authorize(f.authorize()).await.is_err(),
            "{failure}"
        );
        f.assert_uncommitted();
    }
    for failure in ["begin", "download", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(f.downloads().record(f.record()).await.is_err(), "{failure}");
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn replay_never_bypasses_current_human_permission_or_tenant_binding() {
    let f = fixture();
    {
        let mut state = f.store.0.lock().unwrap();
        state.grant = Some(f.grant.clone());
        state.authorized = false;
    }
    assert!(matches!(
        f.authorization().authorize(f.authorize()).await,
        Err(AuditExportAuthorizationError::Forbidden)
    ));
    f.assert_uncommitted();
    let f = fixture();
    f.store.0.lock().unwrap().grant = Some(
        AuditExportGrant::restore(
            f.grant.id(),
            OrganizationId::new(),
            f.grant.archive_id(),
            f.grant.approval_request_id(),
            f.grant.requested_by(),
            f.grant.created_at(),
            f.grant.expires_at(),
        )
        .unwrap(),
    );
    assert!(f.authorization().authorize(f.authorize()).await.is_err());
    f.assert_uncommitted();
}

#[tokio::test]
async fn exact_replay_preserves_the_original_expiration_and_adds_no_audit() {
    let f = fixture();
    f.store.0.lock().unwrap().grant = Some(f.grant.clone());
    let mut command = f.authorize();
    command.now += Duration::hours(1);
    command.approval.now = command.now;
    assert_eq!(f.authorization().authorize(command).await.unwrap(), f.grant);
    assert!(f.store.0.lock().unwrap().audits.is_empty());
}

#[tokio::test]
async fn invalid_scope_proof_and_expired_or_insufficient_approval_fail_closed() {
    for variant in 0..8 {
        let f = fixture();
        let mut command = f.authorize();
        match variant {
            0 => command.context.permissions.clear(),
            1 => command.approval.requested_by = PrincipalId::new(),
            2 => command.approval.resource_id = AuditArchiveId::new().to_string(),
            3 => command.approval.now += Duration::seconds(1),
            4 => f.store.0.lock().unwrap().approval = None,
            5 => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .approval
                    .as_mut()
                    .unwrap()
                    .expires_at = f.now
            }
            6 => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .approval
                    .as_mut()
                    .unwrap()
                    .approval_count = 1
            }
            _ => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .approval
                    .as_mut()
                    .unwrap()
                    .required_approvals = 1
            }
        }
        assert!(matches!(
            f.authorization().authorize(command).await,
            Err(AuditExportAuthorizationError::Forbidden)
        ));
        assert_eq!(f.store.0.lock().unwrap().begins, 0);
    }
}

#[tokio::test]
async fn resolution_selects_one_exact_artifact_and_rejects_wrong_ids_or_expiry() {
    for artifact in [AuditExportArtifact::Events, AuditExportArtifact::Manifest] {
        let f = fixture();
        f.store.0.lock().unwrap().grant = Some(f.grant.clone());
        let resolved = f.downloads().resolve(f.resolve(artifact)).await.unwrap();
        assert_eq!(&resolved.object, select_artifact(&f.archive, artifact));
        assert_eq!(resolved.artifact, artifact);
    }
    for variant in 0..5 {
        let f = fixture();
        f.store.0.lock().unwrap().grant = Some(f.grant.clone());
        let mut command = f.resolve(AuditExportArtifact::Events);
        match variant {
            0 => command.grant_id = AuditExportId::new(),
            1 => {
                f.store
                    .0
                    .lock()
                    .unwrap()
                    .archive
                    .as_mut()
                    .unwrap()
                    .archive_id = AuditArchiveId::new()
            }
            // Grant activity is evaluated on the database clock, so expiry is
            // simulated by moving the stored clock past the grant window.
            2 => {
                f.store.0.lock().unwrap().stored_time = Some(f.grant.expires_at());
            }
            3 => {
                f.store.0.lock().unwrap().stored_time =
                    Some(f.grant.created_at() - Duration::seconds(1));
            }
            _ => command.context.principal_id = PrincipalId::new(),
        }
        assert!(
            f.downloads().resolve(command).await.is_err(),
            "variant {variant}"
        );
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn download_checks_archive_binding_exact_object_and_current_permission() {
    for variant in 0..6 {
        let f = fixture();
        let mut command = f.record();
        match variant {
            0 => command.authorized.archive_id = AuditArchiveId::new(),
            1 => command.retrieved.version_id = "wrong".into(),
            2 => command.retrieved.sha256_hex = "c".repeat(64),
            3 => command.retrieved.size_bytes += 1,
            4 => {
                f.store.0.lock().unwrap().stored_time = Some(f.grant.expires_at());
            }
            _ => f.store.0.lock().unwrap().authorized = false,
        }
        assert!(
            f.downloads().record(command).await.is_err(),
            "variant {variant}"
        );
        f.assert_uncommitted();
    }
    let f = fixture();
    let result = f.downloads().record(f.record()).await.unwrap();
    assert_eq!(result.object, f.archive.events);
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["download", "audit", "outbox"]]
    );
}

#[tokio::test]
async fn download_uses_database_time_and_refuses_late_or_substituted_evidence() {
    let f = fixture();
    let stored_time = f.now + Duration::seconds(3);
    f.store.0.lock().unwrap().stored_time = Some(stored_time);
    let result = f.downloads().record(f.record()).await.unwrap();
    assert_eq!(result.downloaded_at, stored_time);
    assert_eq!(f.store.0.lock().unwrap().audits[0].occurred_at, stored_time);
    for corrupted in [false, true] {
        let f = fixture();
        {
            let mut state = f.store.0.lock().unwrap();
            state.stored_time = Some(f.grant.expires_at());
            state.corrupt_download = corrupted;
        }
        assert!(f.downloads().record(f.record()).await.is_err());
        f.assert_uncommitted();
    }
}

#[tokio::test]
async fn malformed_trace_and_overflowing_time_return_errors_not_partial_writes() {
    let f = fixture();
    let mut command = f.authorize();
    command.trace_id = String::new();
    assert!(f.authorization().authorize(command).await.is_err());
    f.assert_uncommitted();
    let mut command = f.authorize();
    command.now = DateTime::<Utc>::MAX_UTC;
    command.approval.now = command.now;
    assert!(f.authorization().authorize(command).await.is_err());
    f.assert_uncommitted();
}
