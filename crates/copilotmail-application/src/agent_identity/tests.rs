use super::*;
use crate::{ControlPlaneTransaction, ExistingOidcOwnerBinding};
use async_trait::async_trait;
use copilotmail_domain::{Organization, OrganizationSlug, Role, RoleId};
use std::sync::{Arc, Mutex};

struct State {
    owner: Option<ExistingOidcOwnerBinding>,
    role: Option<RoleId>,
    active: bool,
    revoked: bool,
    fail: Option<&'static str>,
    begins: usize,
    commits: Vec<Vec<&'static str>>,
    audits: Vec<AuditEvent>,
}

#[derive(Clone)]
struct Store(Arc<Mutex<State>>);
struct Tx {
    state: Store,
    pending: Vec<&'static str>,
    audits: Vec<AuditEvent>,
}

impl Tx {
    fn stage(&mut self, operation: &'static str) -> Result<(), StoreError> {
        if self.state.0.lock().unwrap().fail == Some(operation) {
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
            state: self.clone(),
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
    async fn insert_principal(&mut self, principal: &Principal) -> Result<(), StoreError> {
        assert_eq!(principal.kind(), PrincipalKind::Agent);
        self.stage("principal")
    }
    async fn insert_role(&mut self, _: &Role) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn insert_role_assignment(&mut self, _: &RoleAssignment) -> Result<(), StoreError> {
        self.stage("assignment")
    }
    async fn record_organization_bootstrap(
        &mut self,
        _: OrganizationId,
        _: PrincipalId,
        _: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }
    async fn find_oidc_owner_binding(
        &mut self,
        _: OrganizationId,
    ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
        Ok(self.state.0.lock().unwrap().owner.clone())
    }
    async fn find_system_role(
        &mut self,
        _: OrganizationId,
        role: SystemRole,
    ) -> Result<Option<RoleId>, StoreError> {
        assert_eq!(role, SystemRole::AgentOperator);
        Ok(self.state.0.lock().unwrap().role)
    }
    async fn insert_agent_profile(&mut self, _: &AgentProfile) -> Result<(), StoreError> {
        self.stage("profile")
    }
    async fn insert_agent_credential(
        &mut self,
        _: &AgentCredential,
        _: &str,
    ) -> Result<(), StoreError> {
        self.stage("credential")
    }
    async fn insert_agent_mtls_credential(
        &mut self,
        _: &AgentMtlsCredential,
    ) -> Result<(), StoreError> {
        self.stage("mtls")
    }
    async fn active_agent_exists(
        &mut self,
        _: OrganizationId,
        _: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(self.state.0.lock().unwrap().active)
    }
    async fn revoke_agent_credential(
        &mut self,
        _: OrganizationId,
        _: CredentialId,
        _: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.stage("revoke")?;
        Ok(self.state.0.lock().unwrap().revoked)
    }
    async fn append_audit_event(&mut self, audit: &AuditEvent) -> Result<(), StoreError> {
        self.stage("audit")?;
        self.audits.push(audit.clone());
        Ok(())
    }
    async fn enqueue_event(&mut self, _: &PendingEvent) -> Result<(), StoreError> {
        self.stage("outbox")
    }
    async fn commit(self: Box<Self>) -> Result<(), StoreError> {
        let mut state = self.state.0.lock().unwrap();
        if state.fail == Some("commit") {
            return Err(StoreError::Unavailable);
        }
        state.commits.push(self.pending);
        state.audits.extend(self.audits);
        Ok(())
    }
}

struct Fixture {
    store: Store,
    command: RegisterAgentCommand,
    agent: PrincipalId,
}

fn fixture() -> Fixture {
    let now = DateTime::from_timestamp(1_790_000_000, 0).unwrap();
    let organization_id = OrganizationId::new();
    let store = Store(Arc::new(Mutex::new(State {
        owner: Some(ExistingOidcOwnerBinding {
            organization_id,
            principal_id: PrincipalId::new(),
            issuer: "https://idp.example.test".into(),
            subject: "owner".into(),
            display_name: "Owner".into(),
        }),
        role: Some(RoleId::new()),
        active: true,
        revoked: true,
        fail: None,
        begins: 0,
        commits: vec![],
        audits: vec![],
    })));
    Fixture {
        store,
        agent: PrincipalId::new(),
        command: RegisterAgentCommand {
            organization_id,
            display_name: "Bound agent".into(),
            environment: AgentEnvironment::Development,
            risk_level: AgentRiskLevel::Low,
            purpose: "Controlled work".into(),
            review_due_at: now + Duration::days(30),
            // Cryptographic PEM validation belongs to the trusted boundary adapter;
            // this unit fixture exercises only the application's shape/policy gate.
            public_key_pem: format!(
                "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----",
                "A".repeat(300)
            ),
            public_key_fingerprint: "a".repeat(64),
            allowed_scopes: [Permission::AgentOperate].into_iter().collect(),
            credential_expires_at: now + Duration::days(30),
            trace_id: "agent-identity-test".into(),
            now,
        },
    }
}

impl Fixture {
    fn add(&self) -> AddAgentCredentialCommand {
        AddAgentCredentialCommand {
            organization_id: self.command.organization_id,
            principal_id: self.agent,
            public_key_pem: self.command.public_key_pem.clone(),
            public_key_fingerprint: "b".repeat(64),
            allowed_scopes: self.command.allowed_scopes.clone(),
            credential_expires_at: self.command.credential_expires_at,
            trace_id: "agent-key-add-test".into(),
            now: self.command.now,
        }
    }
    fn mtls(&self) -> AddAgentMtlsCredentialCommand {
        AddAgentMtlsCredentialCommand {
            organization_id: self.command.organization_id,
            principal_id: self.agent,
            certificate_fingerprint: "c".repeat(64),
            allowed_scopes: self.command.allowed_scopes.clone(),
            credential_expires_at: self.command.credential_expires_at,
            trace_id: "agent-mtls-add-test".into(),
            now: self.command.now,
        }
    }
    fn revoke(&self) -> RevokeAgentCredentialCommand {
        RevokeAgentCredentialCommand {
            organization_id: self.command.organization_id,
            credential_id: CredentialId::new(),
            trace_id: "agent-key-revoke-test".into(),
            now: self.command.now,
        }
    }
    fn no_commit(&self) {
        let state = self.store.0.lock().unwrap();
        assert!(state.commits.is_empty());
        assert!(state.audits.is_empty());
    }
}

#[tokio::test]
async fn registration_is_atomic_and_never_logs_key_material() {
    let f = fixture();
    let result = RegisterAgentService::new(f.store.clone())
        .execute(f.command.clone())
        .await
        .unwrap();
    let state = f.store.0.lock().unwrap();
    assert_eq!(result.organization_id, f.command.organization_id);
    assert_eq!(
        result.owner_principal_id,
        state.owner.as_ref().unwrap().principal_id
    );
    assert_eq!(result.allowed_scopes, f.command.allowed_scopes);
    assert_eq!(
        state.commits,
        vec![vec![
            "principal",
            "profile",
            "credential",
            "assignment",
            "audit",
            "outbox"
        ]]
    );
    assert_eq!(state.audits[0].action, "identity.agent.registered");
    let audit = serde_json::to_string(&state.audits[0]).unwrap();
    assert!(!audit.contains("BEGIN PUBLIC KEY"));
    assert!(!audit.contains(&f.command.public_key_fingerprint));
}

#[tokio::test]
async fn registration_rolls_back_at_every_side_effect_boundary() {
    for failure in [
        "begin",
        "principal",
        "profile",
        "credential",
        "assignment",
        "audit",
        "outbox",
        "commit",
    ] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(
            RegisterAgentService::new(f.store.clone())
                .execute(f.command.clone())
                .await
                .is_err()
        );
        f.no_commit();
    }
}

#[tokio::test]
async fn malformed_policy_is_rejected_before_storage() {
    for invalid in 0..8 {
        let f = fixture();
        let mut command = f.command.clone();
        match invalid {
            0 => command.public_key_pem = "invalid".into(),
            1 => {
                command.public_key_pem = format!("-----BEGIN PUBLIC KEY-----{}", "a".repeat(16_384))
            }
            2 => command.allowed_scopes.clear(),
            3 => command.allowed_scopes = [Permission::BreakGlass].into_iter().collect(),
            4 => command.credential_expires_at = command.now,
            5 => command.credential_expires_at = command.now + Duration::days(366),
            6 => command.review_due_at = command.now,
            _ => command.review_due_at = command.now + Duration::days(366),
        }
        assert!(
            RegisterAgentService::new(f.store.clone())
                .execute(command)
                .await
                .is_err()
        );
        assert_eq!(f.store.0.lock().unwrap().begins, 0);
        f.no_commit();
    }
}

#[tokio::test]
async fn missing_or_mismatched_owner_never_writes_credentials() {
    for invalid in 0..3 {
        let f = fixture();
        {
            let mut state = f.store.0.lock().unwrap();
            match invalid {
                0 => state.owner = None,
                1 => state.owner.as_mut().unwrap().organization_id = OrganizationId::new(),
                _ => state.role = None,
            }
        }
        assert!(
            RegisterAgentService::new(f.store.clone())
                .execute(f.command.clone())
                .await
                .is_err()
        );
        f.no_commit();
        if invalid != 2 {
            assert!(
                AddAgentCredentialService::new(f.store.clone())
                    .execute(f.add())
                    .await
                    .is_err()
            );
            assert!(
                AddAgentMtlsCredentialService::new(f.store.clone())
                    .execute(f.mtls())
                    .await
                    .is_err()
            );
            assert!(
                RevokeAgentCredentialService::new(f.store.clone())
                    .execute(f.revoke())
                    .await
                    .is_err()
            );
            f.no_commit();
        }
    }
}

#[tokio::test]
async fn key_and_mtls_rotation_keep_exact_agent_scopes_and_atomic_evidence() {
    let f = fixture();
    let added = AddAgentCredentialService::new(f.store.clone())
        .execute(f.add())
        .await
        .unwrap();
    assert_eq!(added.principal_id, f.agent);
    assert_eq!(added.allowed_scopes, f.command.allowed_scopes);
    let mtls = AddAgentMtlsCredentialService::new(f.store.clone())
        .execute(f.mtls())
        .await
        .unwrap();
    assert_eq!(mtls.principal_id, f.agent);
    assert_ne!(mtls.credential_id, added.credential_id);
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![
            vec!["credential", "audit", "outbox"],
            vec!["mtls", "audit", "outbox"]
        ]
    );
    for failure in ["credential", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(
            AddAgentCredentialService::new(f.store.clone())
                .execute(f.add())
                .await
                .is_err()
        );
        f.no_commit();
    }
    for failure in ["mtls", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(
            AddAgentMtlsCredentialService::new(f.store.clone())
                .execute(f.mtls())
                .await
                .is_err()
        );
        f.no_commit();
    }
}

#[tokio::test]
async fn inactive_agents_and_invalid_fingerprints_cannot_receive_new_credentials() {
    let f = fixture();
    f.store.0.lock().unwrap().active = false;
    assert!(
        AddAgentCredentialService::new(f.store.clone())
            .execute(f.add())
            .await
            .is_err()
    );
    assert!(
        AddAgentMtlsCredentialService::new(f.store.clone())
            .execute(f.mtls())
            .await
            .is_err()
    );
    f.no_commit();
    let f = fixture();
    let mut command = f.add();
    command.public_key_fingerprint = "invalid".into();
    assert!(
        AddAgentCredentialService::new(f.store.clone())
            .execute(command)
            .await
            .is_err()
    );
    let mut command = f.mtls();
    command.certificate_fingerprint = "g".repeat(64);
    assert!(
        AddAgentMtlsCredentialService::new(f.store.clone())
            .execute(command)
            .await
            .is_err()
    );
    f.no_commit();
}

#[tokio::test]
async fn revoke_is_atomic_and_unavailable_credentials_never_record_success() {
    let f = fixture();
    RevokeAgentCredentialService::new(f.store.clone())
        .execute(f.revoke())
        .await
        .unwrap();
    assert_eq!(
        f.store.0.lock().unwrap().commits,
        vec![vec!["revoke", "audit", "outbox"]]
    );
    let f = fixture();
    f.store.0.lock().unwrap().revoked = false;
    assert!(
        RevokeAgentCredentialService::new(f.store.clone())
            .execute(f.revoke())
            .await
            .is_err()
    );
    f.no_commit();
    for failure in ["revoke", "audit", "outbox", "commit"] {
        let f = fixture();
        f.store.0.lock().unwrap().fail = Some(failure);
        assert!(
            RevokeAgentCredentialService::new(f.store.clone())
                .execute(f.revoke())
                .await
                .is_err()
        );
        f.no_commit();
    }
}
