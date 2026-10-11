use crate::{
    AgentInspectorPrincipalCandidate, AgentInspectorProfileCandidate,
    AgentInspectorSnapshotCandidate, AgentInspectorTokenAccessCandidate, ControlPlaneStore,
    PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AgentEnvironment, AgentRiskLevel, AuditActor, AuditContext, AuditEvent, AuditOutcome,
    AuditResource, AuthorizationContext, Permission, PrincipalId, PrincipalStatus,
};
use serde_json::json;
use std::collections::BTreeSet;

/// Stable version recorded with every successful Inspector read.
pub const AGENT_INSPECTOR_SNAPSHOT_VERSION: u8 = 1;

/// Input for one tenant-bound, privileged Human Agent Inspector read.
#[derive(Debug, Clone)]
pub struct GetAgentInspectorCommand {
    pub context: AuthorizationContext,
    /// Must equal the tenant established by `context`, never a caller-selected
    /// cross-tenant value.
    pub organization_id: copilotmail_domain::OrganizationId,
    pub agent_id: PrincipalId,
    pub trace_id: String,
}

/// Public, bounded Agent identity projection. Its lifecycle status is not a
/// runtime-health signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorAgent {
    pub principal_id: PrincipalId,
    pub display_name: String,
    pub principal_status: PrincipalStatus,
}

/// Verified Human owner projection. The service refuses a candidate whose
/// owner is not a same-tenant Human instead of returning a partial result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorOwner {
    pub principal_id: PrincipalId,
    pub display_name: String,
    pub principal_status: PrincipalStatus,
}

/// Whether the configured periodic review deadline has passed at the
/// persistence-observed timestamp. It never asserts that a Human has approved
/// a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInspectorReviewState {
    Scheduled,
    Due,
}

impl AgentInspectorReviewState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Due => "due",
        }
    }
}

/// Bounded Agent profile data that is safe for a tenant identity manager to
/// inspect. Mailbox ownership, delegation, and credential material are absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorProfile {
    pub environment: AgentEnvironment,
    pub risk_level: AgentRiskLevel,
    pub purpose: String,
    pub review_due_at: DateTime<Utc>,
    pub review_state: AgentInspectorReviewState,
}

/// Current token-issuance outcome, not an assertion about a running Agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInspectorTokenAccessState {
    IdentityInactive,
    NoActiveCredential,
    NoEffectiveScope,
    Effective,
}

impl AgentInspectorTokenAccessState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdentityInactive => "identity_inactive",
            Self::NoActiveCredential => "no_active_credential",
            Self::NoEffectiveScope => "no_effective_scope",
            Self::Effective => "effective",
        }
    }
}

/// Aggregate current token access. Scopes are the intersection of current RBAC
/// and at least one valid credential; no per-credential data is exposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorTokenAccess {
    pub state: AgentInspectorTokenAccessState,
    pub effective_scopes: BTreeSet<Permission>,
    pub active_credential_count: u32,
    pub next_credential_expiry_at: Option<DateTime<Utc>>,
    pub last_authenticated_at: Option<DateTime<Utc>>,
}

/// Control-plane mail admission state. It does not represent Agent liveness,
/// runtime health, queue progress, or a delivery guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentInspectorMailAdmissionState {
    /// Development and testing Agents do not require a production policy. This
    /// is not a statement that an Agent is deliverable or runtime-ready.
    NotRequired,
    /// A production Agent has a current active production mail-policy
    /// activation. It does not override principal lifecycle and is not an
    /// online, reviewed, or delivery-success signal.
    Active,
    /// A production Agent has no active production mail policy and is blocked
    /// from production mail admission.
    BlockedNoActivePolicy,
}

impl AgentInspectorMailAdmissionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotRequired => "not_required",
            Self::Active => "active",
            Self::BlockedNoActivePolicy => "blocked_no_active_policy",
        }
    }
}

/// Mail control-plane aggregates only. `last_processing_activity_at` is the
/// latest recorded control-plane operation and is explicitly not a heartbeat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorControlPlane {
    pub mail_admission_state: AgentInspectorMailAdmissionState,
    pub active_processing_subscription_count: u32,
    pub paused_processing_subscription_count: u32,
    pub retired_processing_subscription_count: u32,
    pub last_processing_activity_at: Option<DateTime<Utc>>,
}

/// One safe, tenant-bound Agent Inspector response. This type intentionally
/// excludes raw credentials, mailbox identifiers, delegations, mail content,
/// and all runtime health claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorSnapshot {
    pub organization_id: copilotmail_domain::OrganizationId,
    pub agent: AgentInspectorAgent,
    pub owner: AgentInspectorOwner,
    pub profile: AgentInspectorProfile,
    pub token_access: AgentInspectorTokenAccess,
    pub control_plane: AgentInspectorControlPlane,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentInspectorError {
    #[error("Agent Inspector request is invalid")]
    InvalidRequest,
    #[error("Agent Inspector access is forbidden")]
    Forbidden,
    #[error("Agent Inspector target is unavailable")]
    NotFound,
    #[error("Agent Inspector snapshot is unavailable")]
    Unavailable,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Privileged read service for one same-tenant Agent snapshot.
///
/// The read, final active-Human proof, Audit evidence, and Outbox event all
/// occur in one control-plane transaction. An adapter without the exact
/// aggregate implementation is default-deny through the transaction port.
pub struct AgentInspectorService<S> {
    store: S,
}

impl<S> AgentInspectorService<S>
where
    S: ControlPlaneStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn get(
        &self,
        command: GetAgentInspectorCommand,
    ) -> Result<AgentInspectorSnapshot, AgentInspectorError> {
        validate_command(&command)?;
        if command.context.organization_id != command.organization_id
            || !command
                .context
                .permits(command.organization_id, Permission::IdentityManage)
        {
            return Err(AgentInspectorError::Forbidden);
        }

        let mut transaction = self.store.begin_authorized(&command.context).await?;
        // This is intentionally the first transaction operation. It proves the
        // authentication context is still an active Human with current
        // `identity.manage` before the Inspector aggregate is read, then the
        // result is held until its evidence commits.
        if !transaction
            .authorize_agent_inspector_reader(command.organization_id, command.context.principal_id)
            .await?
        {
            return Err(AgentInspectorError::Forbidden);
        }
        let candidate = transaction
            .get_agent_inspector_snapshot(command.organization_id, command.agent_id)
            .await?
            .ok_or(AgentInspectorError::NotFound)?;
        let snapshot =
            snapshot_from_candidate(candidate, command.organization_id, command.agent_id)?;

        // Repeat the proof immediately before durable read evidence. The first
        // check prevents an unauthorized snapshot query; this one closes a
        // revocation race between the aggregate read and Audit/Outbox writes.
        if !transaction
            .authorize_agent_inspector_reader(command.organization_id, command.context.principal_id)
            .await?
        {
            return Err(AgentInspectorError::Forbidden);
        }

        let mut audit_context = AuditContext::for_request(command.trace_id.clone())
            .map_err(|_| AgentInspectorError::InvalidRequest)?;
        audit_context.subject_principal_id = Some(command.agent_id);
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.agent.queried",
            AuditResource {
                kind: "identity.agent".into(),
                id: command.agent_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "snapshot_version": AGENT_INSPECTOR_SNAPSHOT_VERSION,
                "token_access_state": snapshot.token_access.state.as_str(),
                "mail_admission_state": snapshot.control_plane.mail_admission_state.as_str(),
                "active_processing_subscription_count": snapshot
                    .control_plane
                    .active_processing_subscription_count,
                "paused_processing_subscription_count": snapshot
                    .control_plane
                    .paused_processing_subscription_count,
                "retired_processing_subscription_count": snapshot
                    .control_plane
                    .retired_processing_subscription_count,
            }),
            snapshot.observed_at,
        )
        .map_err(|_| AgentInspectorError::Unavailable)?
        .with_context(audit_context)
        .and_then(|event| event.correlate_human_session(command.context.authentication.as_ref()))
        .map_err(|_| AgentInspectorError::Unavailable)?;
        let event = PendingEvent::new(
            "identity.agent.queried.v1",
            "identity.agent",
            command.agent_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "agent_id": command.agent_id,
                "snapshot_version": AGENT_INSPECTOR_SNAPSHOT_VERSION,
                "token_access_state": snapshot.token_access.state.as_str(),
                "mail_admission_state": snapshot.control_plane.mail_admission_state.as_str(),
            }),
            snapshot.observed_at,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(snapshot)
    }
}

fn validate_command(command: &GetAgentInspectorCommand) -> Result<(), AgentInspectorError> {
    if command.organization_id.as_uuid().is_nil()
        || command.context.organization_id.as_uuid().is_nil()
        || command.context.principal_id.as_uuid().is_nil()
        || command.agent_id.as_uuid().is_nil()
        || !valid_trace_id(&command.trace_id)
    {
        return Err(AgentInspectorError::InvalidRequest);
    }
    Ok(())
}

fn snapshot_from_candidate(
    candidate: AgentInspectorSnapshotCandidate,
    organization_id: copilotmail_domain::OrganizationId,
    agent_id: PrincipalId,
) -> Result<AgentInspectorSnapshot, AgentInspectorError> {
    let AgentInspectorSnapshotCandidate {
        organization_id: candidate_organization_id,
        agent,
        owner,
        profile,
        token_access,
        control_plane,
        observed_at,
    } = candidate;
    if candidate_organization_id != organization_id
        || !valid_agent_candidate(&agent, organization_id, agent_id)
        || !valid_owner_candidate(&owner, organization_id, agent_id)
        || !valid_profile_candidate(&profile, agent_id, owner.principal_id)
        || !valid_token_access_candidate(&token_access, agent.status, observed_at)
        || control_plane
            .last_processing_activity_at
            .is_some_and(|activity_at| activity_at > observed_at + chrono::Duration::seconds(30))
    {
        return Err(AgentInspectorError::Unavailable);
    }

    let review_state = if profile.review_due_at <= observed_at {
        AgentInspectorReviewState::Due
    } else {
        AgentInspectorReviewState::Scheduled
    };
    let token_access_state = token_access_state(&token_access, agent.status);
    let mail_admission_state = match profile.environment {
        AgentEnvironment::Production if control_plane.has_active_production_mail_policy => {
            AgentInspectorMailAdmissionState::Active
        }
        AgentEnvironment::Production => AgentInspectorMailAdmissionState::BlockedNoActivePolicy,
        AgentEnvironment::Development | AgentEnvironment::Testing => {
            AgentInspectorMailAdmissionState::NotRequired
        }
    };

    Ok(AgentInspectorSnapshot {
        organization_id,
        agent: AgentInspectorAgent {
            principal_id: agent.principal_id,
            display_name: agent.display_name,
            principal_status: agent.status,
        },
        owner: AgentInspectorOwner {
            principal_id: owner.principal_id,
            display_name: owner.display_name,
            principal_status: owner.status,
        },
        profile: AgentInspectorProfile {
            environment: profile.environment,
            risk_level: profile.risk_level,
            purpose: profile.purpose,
            review_due_at: profile.review_due_at,
            review_state,
        },
        token_access: AgentInspectorTokenAccess {
            state: token_access_state,
            effective_scopes: token_access.effective_scopes,
            active_credential_count: token_access.active_credential_count,
            next_credential_expiry_at: token_access.next_credential_expiry_at,
            last_authenticated_at: token_access.last_authenticated_at,
        },
        control_plane: AgentInspectorControlPlane {
            mail_admission_state,
            active_processing_subscription_count: control_plane
                .active_processing_subscription_count,
            paused_processing_subscription_count: control_plane
                .paused_processing_subscription_count,
            retired_processing_subscription_count: control_plane
                .retired_processing_subscription_count,
            last_processing_activity_at: control_plane.last_processing_activity_at,
        },
        observed_at,
    })
}

fn valid_agent_candidate(
    candidate: &AgentInspectorPrincipalCandidate,
    organization_id: copilotmail_domain::OrganizationId,
    agent_id: PrincipalId,
) -> bool {
    candidate.organization_id == organization_id
        && candidate.principal_id == agent_id
        && candidate.kind == copilotmail_domain::PrincipalKind::Agent
        && !candidate.principal_id.as_uuid().is_nil()
        && valid_display_name(&candidate.display_name)
}

fn valid_owner_candidate(
    candidate: &AgentInspectorPrincipalCandidate,
    organization_id: copilotmail_domain::OrganizationId,
    agent_id: PrincipalId,
) -> bool {
    candidate.organization_id == organization_id
        && candidate.kind == copilotmail_domain::PrincipalKind::Human
        && candidate.principal_id != agent_id
        && !candidate.principal_id.as_uuid().is_nil()
        && valid_display_name(&candidate.display_name)
}

fn valid_profile_candidate(
    profile: &AgentInspectorProfileCandidate,
    agent_id: PrincipalId,
    owner_id: PrincipalId,
) -> bool {
    profile.principal_id == agent_id
        && profile.owner_principal_id == owner_id
        && profile.owner_principal_id != agent_id
        && valid_purpose(&profile.purpose)
}

fn valid_token_access_candidate(
    candidate: &AgentInspectorTokenAccessCandidate,
    agent_status: PrincipalStatus,
    observed_at: DateTime<Utc>,
) -> bool {
    let has_active_credential = candidate.active_credential_count > 0;
    candidate
        .effective_scopes
        .iter()
        .all(|scope| Permission::AGENT_TOKEN.contains(scope))
        && candidate.next_credential_expiry_at.is_some() == has_active_credential
        && candidate
            .next_credential_expiry_at
            .is_none_or(|expires_at| expires_at > observed_at)
        && candidate
            .last_authenticated_at
            .is_none_or(|authenticated_at| {
                authenticated_at <= observed_at + chrono::Duration::seconds(30)
            })
        && (has_active_credential || candidate.effective_scopes.is_empty())
        && (agent_status == PrincipalStatus::Active || candidate.effective_scopes.is_empty())
}

fn token_access_state(
    candidate: &AgentInspectorTokenAccessCandidate,
    agent_status: PrincipalStatus,
) -> AgentInspectorTokenAccessState {
    if agent_status != PrincipalStatus::Active {
        AgentInspectorTokenAccessState::IdentityInactive
    } else if candidate.active_credential_count == 0 {
        AgentInspectorTokenAccessState::NoActiveCredential
    } else if candidate.effective_scopes.is_empty() {
        AgentInspectorTokenAccessState::NoEffectiveScope
    } else {
        AgentInspectorTokenAccessState::Effective
    }
}

fn valid_display_name(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 200
        && !value.chars().any(char::is_control)
        && value.split_whitespace().collect::<Vec<_>>().join(" ") == value
}

fn valid_purpose(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 1_000
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn valid_trace_id(value: &str) -> bool {
    !value.is_empty() && value.chars().count() <= 200 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentInspectorControlPlaneCandidate, ControlPlaneTransaction, ExistingOrganizationBootstrap,
    };
    use async_trait::async_trait;
    use chrono::{Duration, TimeZone};
    use copilotmail_domain::{
        Organization, OrganizationId, OrganizationSlug, Principal, PrincipalKind, Role,
        RoleAssignment,
    };
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct Evidence {
        actions: Vec<String>,
        serialized: Vec<String>,
        occurred_at: Vec<DateTime<Utc>>,
    }

    #[derive(Clone)]
    struct Store {
        reader_authorized: bool,
        final_authorization_fails: bool,
        candidate: Option<AgentInspectorSnapshotCandidate>,
        evidence: Arc<Mutex<Evidence>>,
        snapshot_calls: Arc<AtomicUsize>,
        authorization_calls: Arc<AtomicUsize>,
    }

    struct Transaction {
        store: Store,
    }

    #[async_trait]
    impl ControlPlaneStore for Store {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                store: self.clone(),
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for Transaction {
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

        async fn authorize_agent_inspector_reader(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            let call = self
                .store
                .authorization_calls
                .fetch_add(1, Ordering::SeqCst);
            self.store
                .evidence
                .lock()
                .unwrap()
                .actions
                .push("reader_authorized".into());
            Ok(self.store.reader_authorized && !(self.store.final_authorization_fails && call > 0))
        }

        async fn get_agent_inspector_snapshot(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<Option<AgentInspectorSnapshotCandidate>, StoreError> {
            self.store.snapshot_calls.fetch_add(1, Ordering::SeqCst);
            self.store
                .evidence
                .lock()
                .unwrap()
                .actions
                .push("snapshot".into());
            Ok(self.store.candidate.clone())
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            let mut evidence = self.store.evidence.lock().unwrap();
            evidence.actions.push(format!("audit:{}", event.action));
            evidence.serialized.push(event.details.to_string());
            evidence.occurred_at.push(event.occurred_at);
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            let mut evidence = self.store.evidence.lock().unwrap();
            evidence
                .actions
                .push(format!("outbox:{}", event.event_type));
            evidence.serialized.push(event.payload.to_string());
            evidence.occurred_at.push(event.occurred_at);
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            self.store
                .evidence
                .lock()
                .unwrap()
                .actions
                .push("commit".into());
            Ok(())
        }
    }

    struct PanicStore;

    #[async_trait]
    impl ControlPlaneStore for PanicStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            panic!("store must not be opened before local authorization checks")
        }
    }

    struct Fixture {
        organization_id: OrganizationId,
        actor_id: PrincipalId,
        agent_id: PrincipalId,
        owner_id: PrincipalId,
        now: DateTime<Utc>,
    }

    fn fixture() -> Fixture {
        Fixture {
            organization_id: OrganizationId::new(),
            actor_id: PrincipalId::new(),
            agent_id: PrincipalId::new(),
            owner_id: PrincipalId::new(),
            now: Utc.with_ymd_and_hms(2026, 8, 23, 10, 30, 0).unwrap(),
        }
    }

    fn command(fixture: &Fixture) -> GetAgentInspectorCommand {
        GetAgentInspectorCommand {
            context: AuthorizationContext {
                organization_id: fixture.organization_id,
                principal_id: fixture.actor_id,
                permissions: BTreeSet::from([Permission::IdentityManage]),
                authentication: None,
            },
            organization_id: fixture.organization_id,
            agent_id: fixture.agent_id,
            trace_id: "agent-inspector-test".into(),
        }
    }

    fn candidate(fixture: &Fixture) -> AgentInspectorSnapshotCandidate {
        AgentInspectorSnapshotCandidate {
            organization_id: fixture.organization_id,
            agent: AgentInspectorPrincipalCandidate {
                organization_id: fixture.organization_id,
                principal_id: fixture.agent_id,
                kind: PrincipalKind::Agent,
                status: PrincipalStatus::Active,
                display_name: "Support Triage Agent".into(),
            },
            owner: AgentInspectorPrincipalCandidate {
                organization_id: fixture.organization_id,
                principal_id: fixture.owner_id,
                kind: PrincipalKind::Human,
                status: PrincipalStatus::Active,
                display_name: "Operations Owner".into(),
            },
            profile: AgentInspectorProfileCandidate {
                principal_id: fixture.agent_id,
                owner_principal_id: fixture.owner_id,
                environment: AgentEnvironment::Production,
                risk_level: AgentRiskLevel::High,
                purpose: "Triage inbound support mail".into(),
                review_due_at: fixture.now + Duration::days(30),
            },
            token_access: AgentInspectorTokenAccessCandidate {
                active_credential_count: 2,
                next_credential_expiry_at: Some(fixture.now + Duration::days(2)),
                last_authenticated_at: Some(fixture.now - Duration::minutes(1)),
                effective_scopes: BTreeSet::from([
                    Permission::AgentOperate,
                    Permission::AgentMailRead,
                ]),
            },
            control_plane: AgentInspectorControlPlaneCandidate {
                has_active_production_mail_policy: true,
                active_processing_subscription_count: 2,
                paused_processing_subscription_count: 1,
                retired_processing_subscription_count: 3,
                last_processing_activity_at: Some(fixture.now - Duration::minutes(2)),
            },
            observed_at: fixture.now,
        }
    }

    fn store(candidate: Option<AgentInspectorSnapshotCandidate>, reader_authorized: bool) -> Store {
        Store {
            reader_authorized,
            final_authorization_fails: false,
            candidate,
            evidence: Arc::new(Mutex::new(Evidence::default())),
            snapshot_calls: Arc::new(AtomicUsize::new(0)),
            authorization_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[tokio::test]
    async fn authorization_and_tenant_mismatch_fail_before_opening_storage() {
        let first_fixture = fixture();
        let mut missing_permission = command(&first_fixture);
        missing_permission.context.permissions.clear();
        assert!(matches!(
            AgentInspectorService::new(PanicStore)
                .get(missing_permission)
                .await,
            Err(AgentInspectorError::Forbidden)
        ));

        let second_fixture = fixture();
        let mut cross_tenant = command(&second_fixture);
        cross_tenant.organization_id = OrganizationId::new();
        assert!(matches!(
            AgentInspectorService::new(PanicStore)
                .get(cross_tenant)
                .await,
            Err(AgentInspectorError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn unauthorized_or_non_human_actor_never_reads_a_snapshot() {
        let fixture = fixture();
        let store = store(Some(candidate(&fixture)), false);
        let evidence = store.evidence.clone();
        let snapshot_calls = store.snapshot_calls.clone();
        assert!(matches!(
            AgentInspectorService::new(store)
                .get(command(&fixture))
                .await,
            Err(AgentInspectorError::Forbidden)
        ));
        assert_eq!(snapshot_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            evidence.lock().unwrap().actions.as_slice(),
            ["reader_authorized"]
        );
    }

    #[tokio::test]
    async fn verified_snapshot_is_audited_atomically_without_sensitive_fields() {
        let fixture = fixture();
        let store = store(Some(candidate(&fixture)), true);
        let evidence = store.evidence.clone();
        let snapshot = AgentInspectorService::new(store)
            .get(command(&fixture))
            .await
            .unwrap();

        assert_eq!(snapshot.agent.principal_id, fixture.agent_id);
        assert_eq!(snapshot.owner.principal_id, fixture.owner_id);
        assert_eq!(
            snapshot.profile.review_state,
            AgentInspectorReviewState::Scheduled
        );
        assert_eq!(
            snapshot.token_access.state,
            AgentInspectorTokenAccessState::Effective
        );
        assert_eq!(
            snapshot.control_plane.mail_admission_state,
            AgentInspectorMailAdmissionState::Active
        );
        let evidence = evidence.lock().unwrap();
        assert_eq!(
            evidence.actions.as_slice(),
            [
                "reader_authorized",
                "snapshot",
                "reader_authorized",
                "audit:identity.agent.queried",
                "outbox:identity.agent.queried.v1",
                "commit",
            ]
        );
        let serialized = evidence.serialized.join(" ");
        assert_eq!(evidence.occurred_at.as_slice(), [fixture.now, fixture.now]);
        for excluded in [
            "Triage inbound support mail",
            "agent_mail_read",
            "credential",
            "mailbox",
        ] {
            assert!(!serialized.contains(excluded));
        }
    }

    #[tokio::test]
    async fn revoked_reader_after_snapshot_never_receives_or_audits_it() {
        let fixture = fixture();
        let mut store = store(Some(candidate(&fixture)), true);
        store.final_authorization_fails = true;
        let evidence = store.evidence.clone();
        assert!(matches!(
            AgentInspectorService::new(store)
                .get(command(&fixture))
                .await,
            Err(AgentInspectorError::Forbidden)
        ));
        assert_eq!(
            evidence.lock().unwrap().actions.as_slice(),
            ["reader_authorized", "snapshot", "reader_authorized"]
        );
    }

    #[tokio::test]
    async fn missing_target_never_records_a_successful_read() {
        let fixture = fixture();
        let store = store(None, true);
        let evidence = store.evidence.clone();
        assert!(matches!(
            AgentInspectorService::new(store)
                .get(command(&fixture))
                .await,
            Err(AgentInspectorError::NotFound)
        ));
        assert_eq!(
            evidence.lock().unwrap().actions.as_slice(),
            ["reader_authorized", "snapshot"]
        );
    }

    #[tokio::test]
    async fn malformed_owner_or_token_aggregate_fails_closed_before_evidence() {
        let fixture = fixture();
        let mut bad_owner = candidate(&fixture);
        bad_owner.owner.kind = PrincipalKind::Agent;
        let bad_owner_store = store(Some(bad_owner), true);
        let bad_owner_evidence = bad_owner_store.evidence.clone();
        assert!(matches!(
            AgentInspectorService::new(bad_owner_store)
                .get(command(&fixture))
                .await,
            Err(AgentInspectorError::Unavailable)
        ));
        assert_eq!(
            bad_owner_evidence.lock().unwrap().actions.as_slice(),
            ["reader_authorized", "snapshot"]
        );

        let mut invalid_scope = candidate(&fixture);
        invalid_scope.token_access.effective_scopes = BTreeSet::from([Permission::IdentityManage]);
        let invalid_scope_store = store(Some(invalid_scope), true);
        let invalid_scope_evidence = invalid_scope_store.evidence.clone();
        assert!(matches!(
            AgentInspectorService::new(invalid_scope_store)
                .get(command(&fixture))
                .await,
            Err(AgentInspectorError::Unavailable)
        ));
        assert_eq!(
            invalid_scope_evidence.lock().unwrap().actions.as_slice(),
            ["reader_authorized", "snapshot"]
        );
    }
}
