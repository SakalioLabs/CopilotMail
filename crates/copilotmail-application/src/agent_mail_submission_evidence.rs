use crate::{
    AgentMailSubmissionAdmissionEvidenceCandidate, AgentMailSubmissionAuditReferenceCandidate,
    AgentMailSubmissionDeliveryEvidenceCandidate, AgentMailSubmissionEvidenceCandidate,
    ControlPlaneStore, MailSubmissionStatus, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    ApprovalRequestId, AuditActor, AuditContext, AuditEvent, AuditOutcome, AuditResource,
    AuthorizationContext, MailSubmissionId, OrganizationId, Permission, PrincipalId,
};
use serde_json::json;

/// Stable version recorded with every successful exact Agent mail-evidence
/// read. It does not describe an Agent runtime or delivery protocol.
pub const AGENT_MAIL_SUBMISSION_EVIDENCE_VERSION: u8 = 1;

/// Input for one Human-only, exact production-Agent mail evidence read.
#[derive(Debug, Clone)]
pub struct GetAgentMailSubmissionEvidenceCommand {
    pub context: AuthorizationContext,
    /// Must equal the tenant established by `context`, never a caller-selected
    /// cross-tenant value.
    pub organization_id: OrganizationId,
    pub agent_id: PrincipalId,
    pub submission_id: MailSubmissionId,
    pub trace_id: String,
}

/// Stable submission lifecycle vocabulary. This is intentionally not reduced
/// to a delivery outcome: an accepted submission may still have no immutable
/// delivery snapshot, while pending and review states are not delivery claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMailSubmissionStatusEvidence {
    Pending,
    Reconciling,
    Accepted,
    Rejected,
    ManualReview,
}

impl AgentMailSubmissionStatusEvidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Reconciling => "reconciling",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::ManualReview => "manual_review",
        }
    }
}

/// Immutable admission facts for the exact production-Agent submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionAdmissionEvidence {
    pub admission_id: uuid::Uuid,
    pub admitted_at: DateTime<Utc>,
    pub policy_activation_id: uuid::Uuid,
    pub policy_activated_at: DateTime<Utc>,
    pub policy_version_id: uuid::Uuid,
    pub policy_version: u32,
    pub policy_activation_approval_request_id: ApprovalRequestId,
    pub policy_activation_approval_verified: bool,
    pub recipient_count: u16,
    pub first_contact_count: u16,
    pub first_contact_approval_request_id: Option<ApprovalRequestId>,
    pub first_contact_approval_verified: bool,
}

/// Latest immutable aggregate delivery observation, when one exists. This is
/// deliberately a counter projection only: no recipient, provider, SMTP, or
/// message-content evidence is disclosed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionDeliveryEvidence {
    pub observed_at: DateTime<Utc>,
    pub terminal: bool,
    pub recipient_count: u16,
    pub delivered_count: u16,
    pub failed_count: u16,
    pub queued_count: u16,
    pub unknown_count: u16,
}

/// Minimal reference to one immutable audit event in the evidence chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionAuditReference {
    pub event_id: uuid::Uuid,
    pub action: String,
    pub occurred_at: DateTime<Utc>,
}

/// One bounded, tenant-bound evidence chain for a production Agent's exact
/// submission. It makes no statement about Agent liveness, execution,
/// delegation, mailbox access, recipient identity, or delivery success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionEvidence {
    pub agent_id: PrincipalId,
    pub submission_id: MailSubmissionId,
    pub submission_status: AgentMailSubmissionStatusEvidence,
    pub submission_created_at: DateTime<Utc>,
    pub submission_updated_at: DateTime<Utc>,
    pub admission: AgentMailSubmissionAdmissionEvidence,
    pub delivery: Option<AgentMailSubmissionDeliveryEvidence>,
    pub audit_references: Vec<AgentMailSubmissionAuditReference>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentMailSubmissionEvidenceError {
    #[error("Agent mail submission evidence request is invalid")]
    InvalidRequest,
    #[error("Agent mail submission evidence access is forbidden")]
    Forbidden,
    #[error("Agent mail submission evidence target is unavailable")]
    NotFound,
    #[error("Agent mail submission evidence is unavailable")]
    Unavailable,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Human-only exact evidence reader for immutable Agent production-mail
/// admission, approval, and delivery facts.
///
/// The target read, final active-Human authorization proof, Audit evidence,
/// and Outbox event are committed in one control-plane transaction. It is
/// intentionally not a list, a poller, or a runtime/processing endpoint.
pub struct AgentMailSubmissionEvidenceService<S> {
    store: S,
}

impl<S> AgentMailSubmissionEvidenceService<S>
where
    S: ControlPlaneStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn get(
        &self,
        command: GetAgentMailSubmissionEvidenceCommand,
    ) -> Result<AgentMailSubmissionEvidence, AgentMailSubmissionEvidenceError> {
        validate_command(&command)?;
        if command.context.organization_id != command.organization_id
            || !command
                .context
                .permits(command.organization_id, Permission::IdentityManage)
            || !command
                .context
                .permits(command.organization_id, Permission::AuditRead)
        {
            return Err(AgentMailSubmissionEvidenceError::Forbidden);
        }

        let mut transaction = self.store.begin_authorized(&command.context).await?;
        // This must be the first transaction operation: it proves the caller
        // is still an active Human with both required permissions before the
        // exact target pair is read.
        if !transaction
            .authorize_agent_mail_submission_evidence_reader(
                command.organization_id,
                command.context.principal_id,
            )
            .await?
        {
            return Err(AgentMailSubmissionEvidenceError::Forbidden);
        }
        let candidate = transaction
            .get_agent_mail_submission_evidence(
                command.organization_id,
                command.agent_id,
                command.submission_id,
            )
            .await?
            .ok_or(AgentMailSubmissionEvidenceError::NotFound)?;
        let evidence = evidence_from_candidate(
            candidate,
            command.organization_id,
            command.agent_id,
            command.submission_id,
        )?;

        // Repeat the proof immediately before durable read evidence. This
        // closes a revocation race between the target read and the Audit/Outbox
        // writes that make the disclosure accountable.
        if !transaction
            .authorize_agent_mail_submission_evidence_reader(
                command.organization_id,
                command.context.principal_id,
            )
            .await?
        {
            return Err(AgentMailSubmissionEvidenceError::Forbidden);
        }

        let mut audit_context = AuditContext::for_request(command.trace_id.clone())
            .map_err(|_| AgentMailSubmissionEvidenceError::InvalidRequest)?;
        audit_context.subject_principal_id = Some(command.agent_id);
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.mail_submission_evidence.queried",
            AuditResource {
                kind: "agent.mail_submission_evidence".into(),
                id: command.submission_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "evidence_version": AGENT_MAIL_SUBMISSION_EVIDENCE_VERSION,
                "agent_id": evidence.agent_id,
                "submission_status": evidence.submission_status.as_str(),
                "admission_id": evidence.admission.admission_id,
                "policy_activation_id": evidence.admission.policy_activation_id,
                "has_delivery_snapshot": evidence.delivery.is_some(),
                "audit_reference_count": evidence.audit_references.len(),
            }),
            evidence.observed_at,
        )
        .map_err(|_| AgentMailSubmissionEvidenceError::Unavailable)?
        .with_context(audit_context)
        .map_err(|_| AgentMailSubmissionEvidenceError::Unavailable)?;
        let event = PendingEvent::new(
            "agent.mail_submission_evidence.queried.v1",
            "agent.mail_submission_evidence",
            command.submission_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "agent_id": command.agent_id,
                "submission_id": command.submission_id,
                "evidence_version": AGENT_MAIL_SUBMISSION_EVIDENCE_VERSION,
                "submission_status": evidence.submission_status.as_str(),
                "has_delivery_snapshot": evidence.delivery.is_some(),
            }),
            evidence.observed_at,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(evidence)
    }
}

fn validate_command(
    command: &GetAgentMailSubmissionEvidenceCommand,
) -> Result<(), AgentMailSubmissionEvidenceError> {
    if command.organization_id.as_uuid().is_nil()
        || command.context.organization_id.as_uuid().is_nil()
        || command.context.principal_id.as_uuid().is_nil()
        || command.agent_id.as_uuid().is_nil()
        || command.submission_id.as_uuid().is_nil()
        || !valid_trace_id(&command.trace_id)
    {
        return Err(AgentMailSubmissionEvidenceError::InvalidRequest);
    }
    Ok(())
}

fn evidence_from_candidate(
    candidate: AgentMailSubmissionEvidenceCandidate,
    organization_id: OrganizationId,
    agent_id: PrincipalId,
    submission_id: MailSubmissionId,
) -> Result<AgentMailSubmissionEvidence, AgentMailSubmissionEvidenceError> {
    if candidate.organization_id != organization_id
        || candidate.agent_id != agent_id
        || candidate.submission_id != submission_id
        || agent_id.as_uuid().is_nil()
        || submission_id.as_uuid().is_nil()
        || candidate.submission_created_at > candidate.submission_updated_at
        || candidate.submission_updated_at > candidate.observed_at
    {
        return Err(AgentMailSubmissionEvidenceError::Unavailable);
    }

    let admission = admission_from_candidate(
        candidate.admission,
        candidate.submission_created_at,
        candidate.observed_at,
    )?;
    let submission_status = status_from_stored(candidate.submission_status);
    let submission_accepted_at = match (submission_status, candidate.submission_accepted_at) {
        (AgentMailSubmissionStatusEvidence::Accepted, Some(accepted_at))
            if accepted_at >= admission.admitted_at && accepted_at <= candidate.observed_at =>
        {
            Some(accepted_at)
        }
        (AgentMailSubmissionStatusEvidence::Accepted, _) | (_, Some(_)) => {
            return Err(AgentMailSubmissionEvidenceError::Unavailable);
        }
        (_, None) => None,
    };
    if candidate.delivery.is_some()
        && submission_status != AgentMailSubmissionStatusEvidence::Accepted
    {
        return Err(AgentMailSubmissionEvidenceError::Unavailable);
    }
    let delivery = candidate
        .delivery
        .map(|delivery| {
            delivery_from_candidate(
                delivery,
                admission.admitted_at,
                submission_accepted_at.ok_or(AgentMailSubmissionEvidenceError::Unavailable)?,
                admission.recipient_count,
                candidate.observed_at,
            )
        })
        .transpose()?;
    let audit_references = audit_references_from_candidates(
        candidate.audit_references,
        submission_status,
        candidate.submission_created_at,
        admission.admitted_at,
        candidate.submission_updated_at,
        submission_accepted_at,
        candidate.observed_at,
    )?;

    Ok(AgentMailSubmissionEvidence {
        agent_id,
        submission_id,
        submission_status,
        submission_created_at: candidate.submission_created_at,
        submission_updated_at: candidate.submission_updated_at,
        admission,
        delivery,
        audit_references,
        observed_at: candidate.observed_at,
    })
}

fn admission_from_candidate(
    candidate: AgentMailSubmissionAdmissionEvidenceCandidate,
    submission_created_at: DateTime<Utc>,
    observed_at: DateTime<Utc>,
) -> Result<AgentMailSubmissionAdmissionEvidence, AgentMailSubmissionEvidenceError> {
    if candidate.admission_id.is_nil()
        || candidate.policy_activation_id.is_nil()
        || candidate.policy_version_id.is_nil()
        || candidate
            .policy_activation_approval_request_id
            .as_uuid()
            .is_nil()
        || !candidate.policy_activation_approval_verified
        || !candidate.first_contact_approval_verified
        || candidate.policy_version == 0
        || !(1..=100).contains(&candidate.recipient_count)
        || candidate.first_contact_count > candidate.recipient_count
        || candidate.policy_activated_at > candidate.admitted_at
        || candidate.admitted_at < submission_created_at
        || candidate.admitted_at > observed_at
        || candidate.policy_activated_at > observed_at
        || (candidate.first_contact_count == 0
            && candidate.first_contact_approval_request_id.is_some())
        || (candidate.first_contact_count > 0
            && candidate.first_contact_approval_request_id.is_none())
        || candidate
            .first_contact_approval_request_id
            .is_some_and(|request_id| request_id.as_uuid().is_nil())
    {
        return Err(AgentMailSubmissionEvidenceError::Unavailable);
    }
    Ok(AgentMailSubmissionAdmissionEvidence {
        admission_id: candidate.admission_id,
        admitted_at: candidate.admitted_at,
        policy_activation_id: candidate.policy_activation_id,
        policy_activated_at: candidate.policy_activated_at,
        policy_version_id: candidate.policy_version_id,
        policy_version: candidate.policy_version,
        policy_activation_approval_request_id: candidate.policy_activation_approval_request_id,
        policy_activation_approval_verified: candidate.policy_activation_approval_verified,
        recipient_count: candidate.recipient_count,
        first_contact_count: candidate.first_contact_count,
        first_contact_approval_request_id: candidate.first_contact_approval_request_id,
        first_contact_approval_verified: candidate.first_contact_approval_verified,
    })
}

fn delivery_from_candidate(
    candidate: AgentMailSubmissionDeliveryEvidenceCandidate,
    admitted_at: DateTime<Utc>,
    accepted_at: DateTime<Utc>,
    admission_recipient_count: u16,
    observed_at: DateTime<Utc>,
) -> Result<AgentMailSubmissionDeliveryEvidence, AgentMailSubmissionEvidenceError> {
    let count_sum = u32::from(candidate.delivered_count)
        + u32::from(candidate.failed_count)
        + u32::from(candidate.queued_count)
        + u32::from(candidate.unknown_count);
    if candidate.snapshot_id.is_nil()
        || admitted_at > accepted_at
        || candidate.observed_at < accepted_at
        || candidate.observed_at > observed_at
        || candidate.recipient_count != admission_recipient_count
        || !(1..=100).contains(&candidate.recipient_count)
        || count_sum != u32::from(candidate.recipient_count)
        || candidate.terminal != (candidate.queued_count == 0 && candidate.unknown_count == 0)
    {
        return Err(AgentMailSubmissionEvidenceError::Unavailable);
    }
    Ok(AgentMailSubmissionDeliveryEvidence {
        observed_at: candidate.observed_at,
        terminal: candidate.terminal,
        recipient_count: candidate.recipient_count,
        delivered_count: candidate.delivered_count,
        failed_count: candidate.failed_count,
        queued_count: candidate.queued_count,
        unknown_count: candidate.unknown_count,
    })
}

fn audit_references_from_candidates(
    candidates: Vec<AgentMailSubmissionAuditReferenceCandidate>,
    submission_status: AgentMailSubmissionStatusEvidence,
    submission_created_at: DateTime<Utc>,
    admitted_at: DateTime<Utc>,
    submission_updated_at: DateTime<Utc>,
    submission_accepted_at: Option<DateTime<Utc>>,
    observed_at: DateTime<Utc>,
) -> Result<Vec<AgentMailSubmissionAuditReference>, AgentMailSubmissionEvidenceError> {
    let expected_terminal_action = match submission_status {
        AgentMailSubmissionStatusEvidence::Pending
        | AgentMailSubmissionStatusEvidence::Reconciling => None,
        AgentMailSubmissionStatusEvidence::Accepted => Some("mail.submission.accepted"),
        AgentMailSubmissionStatusEvidence::Rejected => Some("mail.submission.rejected"),
        AgentMailSubmissionStatusEvidence::ManualReview => Some("mail.submission.review_required"),
    };
    let expected_len = if expected_terminal_action.is_some() {
        3
    } else {
        2
    };
    if candidates.len() != expected_len
        || candidates[0].action != "mail.submission.requested"
        || candidates[1].action != "agent.mail.admitted"
        || expected_terminal_action.is_some_and(|action| candidates[2].action != action)
        || candidates[0].occurred_at != submission_created_at
        || candidates[1].occurred_at != admitted_at
        || expected_terminal_action.is_some_and(|_| {
            let terminal_at = candidates[2].occurred_at;
            terminal_at < admitted_at
                || terminal_at > submission_updated_at
                || submission_accepted_at.is_some_and(|accepted_at| terminal_at > accepted_at)
        })
        || candidates
            .iter()
            .any(|candidate| candidate.event_id.is_nil() || candidate.occurred_at > observed_at)
        || candidates.iter().enumerate().any(|(index, candidate)| {
            candidates
                .iter()
                .skip(index + 1)
                .any(|other| other.event_id == candidate.event_id)
        })
    {
        return Err(AgentMailSubmissionEvidenceError::Unavailable);
    }
    Ok(candidates
        .into_iter()
        .map(|candidate| AgentMailSubmissionAuditReference {
            event_id: candidate.event_id,
            action: candidate.action,
            occurred_at: candidate.occurred_at,
        })
        .collect())
}

const fn status_from_stored(status: MailSubmissionStatus) -> AgentMailSubmissionStatusEvidence {
    match status {
        MailSubmissionStatus::Pending => AgentMailSubmissionStatusEvidence::Pending,
        MailSubmissionStatus::Reconciling => AgentMailSubmissionStatusEvidence::Reconciling,
        MailSubmissionStatus::Accepted => AgentMailSubmissionStatusEvidence::Accepted,
        MailSubmissionStatus::Rejected => AgentMailSubmissionStatusEvidence::Rejected,
        MailSubmissionStatus::ManualReview => AgentMailSubmissionStatusEvidence::ManualReview,
    }
}

fn valid_trace_id(value: &str) -> bool {
    !value.is_empty() && value.chars().count() <= 200 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentMailSubmissionAdmissionEvidenceCandidate, AgentMailSubmissionAuditReferenceCandidate,
        AgentMailSubmissionDeliveryEvidenceCandidate, AgentMailSubmissionEvidenceCandidate,
        ControlPlaneTransaction, ExistingOrganizationBootstrap,
    };
    use async_trait::async_trait;
    use chrono::{Duration, TimeZone};
    use copilotmail_domain::{Organization, OrganizationSlug, Principal, Role, RoleAssignment};
    use std::collections::BTreeSet;
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
        candidate: Option<AgentMailSubmissionEvidenceCandidate>,
        evidence: Arc<Mutex<Evidence>>,
        candidate_calls: Arc<AtomicUsize>,
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

        async fn authorize_agent_mail_submission_evidence_reader(
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

        async fn get_agent_mail_submission_evidence(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            _: MailSubmissionId,
        ) -> Result<Option<AgentMailSubmissionEvidenceCandidate>, StoreError> {
            self.store.candidate_calls.fetch_add(1, Ordering::SeqCst);
            self.store
                .evidence
                .lock()
                .unwrap()
                .actions
                .push("candidate".into());
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
        submission_id: MailSubmissionId,
        now: DateTime<Utc>,
    }

    fn fixture() -> Fixture {
        Fixture {
            organization_id: OrganizationId::new(),
            actor_id: PrincipalId::new(),
            agent_id: PrincipalId::new(),
            submission_id: MailSubmissionId::new(),
            now: Utc.with_ymd_and_hms(2026, 8, 24, 10, 30, 0).unwrap(),
        }
    }

    fn command(fixture: &Fixture) -> GetAgentMailSubmissionEvidenceCommand {
        GetAgentMailSubmissionEvidenceCommand {
            context: AuthorizationContext {
                organization_id: fixture.organization_id,
                principal_id: fixture.actor_id,
                permissions: BTreeSet::from([Permission::IdentityManage, Permission::AuditRead]),
                authentication: None,
            },
            organization_id: fixture.organization_id,
            agent_id: fixture.agent_id,
            submission_id: fixture.submission_id,
            trace_id: "agent-mail-evidence-test".into(),
        }
    }

    fn candidate(fixture: &Fixture) -> AgentMailSubmissionEvidenceCandidate {
        let submission_created_at = fixture.now - Duration::minutes(5);
        let admitted_at = fixture.now - Duration::minutes(4);
        AgentMailSubmissionEvidenceCandidate {
            organization_id: fixture.organization_id,
            agent_id: fixture.agent_id,
            submission_id: fixture.submission_id,
            submission_status: MailSubmissionStatus::Accepted,
            submission_created_at,
            submission_updated_at: fixture.now - Duration::minutes(3),
            submission_accepted_at: Some(fixture.now - Duration::minutes(3)),
            admission: AgentMailSubmissionAdmissionEvidenceCandidate {
                admission_id: uuid::Uuid::new_v4(),
                admitted_at,
                policy_activation_id: uuid::Uuid::new_v4(),
                policy_activated_at: fixture.now - Duration::minutes(10),
                policy_version_id: uuid::Uuid::new_v4(),
                policy_version: 3,
                policy_activation_approval_request_id: ApprovalRequestId::new(),
                policy_activation_approval_verified: true,
                recipient_count: 2,
                first_contact_count: 1,
                first_contact_approval_request_id: Some(ApprovalRequestId::new()),
                first_contact_approval_verified: true,
            },
            delivery: Some(AgentMailSubmissionDeliveryEvidenceCandidate {
                snapshot_id: uuid::Uuid::new_v4(),
                observed_at: fixture.now - Duration::minutes(2),
                terminal: false,
                recipient_count: 2,
                delivered_count: 1,
                failed_count: 0,
                queued_count: 1,
                unknown_count: 0,
            }),
            audit_references: vec![
                AgentMailSubmissionAuditReferenceCandidate {
                    event_id: uuid::Uuid::new_v4(),
                    action: "mail.submission.requested".into(),
                    occurred_at: submission_created_at,
                },
                AgentMailSubmissionAuditReferenceCandidate {
                    event_id: uuid::Uuid::new_v4(),
                    action: "agent.mail.admitted".into(),
                    occurred_at: admitted_at,
                },
                AgentMailSubmissionAuditReferenceCandidate {
                    event_id: uuid::Uuid::new_v4(),
                    action: "mail.submission.accepted".into(),
                    occurred_at: fixture.now - Duration::minutes(3),
                },
            ],
            observed_at: fixture.now,
        }
    }

    fn store(
        candidate: Option<AgentMailSubmissionEvidenceCandidate>,
        reader_authorized: bool,
    ) -> Store {
        Store {
            reader_authorized,
            final_authorization_fails: false,
            candidate,
            evidence: Arc::new(Mutex::new(Evidence::default())),
            candidate_calls: Arc::new(AtomicUsize::new(0)),
            authorization_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[tokio::test]
    async fn missing_required_permission_or_tenant_mismatch_fails_before_storage() {
        let first = fixture();
        let mut missing_audit_read = command(&first);
        missing_audit_read
            .context
            .permissions
            .remove(&Permission::AuditRead);
        assert!(matches!(
            AgentMailSubmissionEvidenceService::new(PanicStore)
                .get(missing_audit_read)
                .await,
            Err(AgentMailSubmissionEvidenceError::Forbidden)
        ));

        let second = fixture();
        let mut cross_tenant = command(&second);
        cross_tenant.organization_id = OrganizationId::new();
        assert!(matches!(
            AgentMailSubmissionEvidenceService::new(PanicStore)
                .get(cross_tenant)
                .await,
            Err(AgentMailSubmissionEvidenceError::Forbidden)
        ));
    }

    #[tokio::test]
    async fn unauthorized_reader_never_loads_evidence() {
        let fixture = fixture();
        let store = store(Some(candidate(&fixture)), false);
        let evidence = store.evidence.clone();
        let calls = store.candidate_calls.clone();
        assert!(matches!(
            AgentMailSubmissionEvidenceService::new(store)
                .get(command(&fixture))
                .await,
            Err(AgentMailSubmissionEvidenceError::Forbidden)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            evidence.lock().unwrap().actions.as_slice(),
            ["reader_authorized"]
        );
    }

    #[tokio::test]
    async fn exact_evidence_is_audited_atomically_without_sensitive_fields() {
        let fixture = fixture();
        let store = store(Some(candidate(&fixture)), true);
        let recorded = store.evidence.clone();
        let evidence = AgentMailSubmissionEvidenceService::new(store)
            .get(command(&fixture))
            .await
            .unwrap();

        assert_eq!(evidence.agent_id, fixture.agent_id);
        assert_eq!(evidence.submission_id, fixture.submission_id);
        assert_eq!(
            evidence.submission_status,
            AgentMailSubmissionStatusEvidence::Accepted
        );
        assert_eq!(evidence.audit_references.len(), 3);
        let recorded = recorded.lock().unwrap();
        assert_eq!(
            recorded.actions.as_slice(),
            [
                "reader_authorized",
                "candidate",
                "reader_authorized",
                "audit:agent.mail_submission_evidence.queried",
                "outbox:agent.mail_submission_evidence.queried.v1",
                "commit",
            ]
        );
        assert_eq!(recorded.occurred_at.as_slice(), [fixture.now, fixture.now]);
        let serialized = recorded.serialized.join(" ");
        for excluded in ["mailbox", "recipient", "smtp", "provider", "execution"] {
            assert!(!serialized.contains(excluded));
        }
    }

    #[tokio::test]
    async fn pending_admission_may_follow_the_initial_intent_update() {
        let fixture = fixture();
        let mut pending_candidate = candidate(&fixture);
        pending_candidate.submission_status = MailSubmissionStatus::Pending;
        pending_candidate.submission_updated_at = pending_candidate.submission_created_at;
        pending_candidate.submission_accepted_at = None;
        pending_candidate.delivery = None;
        pending_candidate.audit_references.truncate(2);

        let evidence =
            AgentMailSubmissionEvidenceService::new(store(Some(pending_candidate), true))
                .get(command(&fixture))
                .await
                .expect("pending production admission remains coherent");

        assert_eq!(
            evidence.submission_status,
            AgentMailSubmissionStatusEvidence::Pending
        );
        assert!(evidence.delivery.is_none());
        assert_eq!(evidence.audit_references.len(), 2);
    }

    #[tokio::test]
    async fn final_permission_revocation_prevents_disclosure_and_audit() {
        let fixture = fixture();
        let mut store = store(Some(candidate(&fixture)), true);
        store.final_authorization_fails = true;
        let recorded = store.evidence.clone();
        assert!(matches!(
            AgentMailSubmissionEvidenceService::new(store)
                .get(command(&fixture))
                .await,
            Err(AgentMailSubmissionEvidenceError::Forbidden)
        ));
        assert_eq!(
            recorded.lock().unwrap().actions.as_slice(),
            ["reader_authorized", "candidate", "reader_authorized"]
        );
    }

    #[tokio::test]
    async fn missing_or_malformed_evidence_never_records_a_successful_read() {
        let fixture = fixture();
        let missing = store(None, true);
        let missing_recorded = missing.evidence.clone();
        assert!(matches!(
            AgentMailSubmissionEvidenceService::new(missing)
                .get(command(&fixture))
                .await,
            Err(AgentMailSubmissionEvidenceError::NotFound)
        ));
        assert_eq!(
            missing_recorded.lock().unwrap().actions.as_slice(),
            ["reader_authorized", "candidate"]
        );

        let mut malformed_candidate = candidate(&fixture);
        malformed_candidate
            .delivery
            .as_mut()
            .unwrap()
            .recipient_count = 1;
        let malformed = store(Some(malformed_candidate), true);
        let malformed_recorded = malformed.evidence.clone();
        assert!(matches!(
            AgentMailSubmissionEvidenceService::new(malformed)
                .get(command(&fixture))
                .await,
            Err(AgentMailSubmissionEvidenceError::Unavailable)
        ));
        assert_eq!(
            malformed_recorded.lock().unwrap().actions.as_slice(),
            ["reader_authorized", "candidate"]
        );

        let mut missing_first_contact_proof = candidate(&fixture);
        missing_first_contact_proof
            .admission
            .first_contact_approval_request_id = None;
        let missing_first_contact_proof_store = store(Some(missing_first_contact_proof), true);
        let missing_first_contact_recorded = missing_first_contact_proof_store.evidence.clone();
        assert!(matches!(
            AgentMailSubmissionEvidenceService::new(missing_first_contact_proof_store)
                .get(command(&fixture))
                .await,
            Err(AgentMailSubmissionEvidenceError::Unavailable)
        ));
        assert_eq!(
            missing_first_contact_recorded
                .lock()
                .unwrap()
                .actions
                .as_slice(),
            ["reader_authorized", "candidate"]
        );
    }

    #[tokio::test]
    async fn causal_audit_and_delivery_timestamps_are_required_before_disclosure() {
        let fixture = fixture();

        let mut requested_after_admission = candidate(&fixture);
        requested_after_admission.audit_references[0].occurred_at =
            requested_after_admission.admission.admitted_at;

        let mut admitted_after_terminal = candidate(&fixture);
        admitted_after_terminal.audit_references[1].occurred_at =
            admitted_after_terminal.submission_updated_at;

        let mut terminal_before_admission = candidate(&fixture);
        terminal_before_admission.audit_references[2].occurred_at =
            terminal_before_admission.admission.admitted_at - Duration::nanoseconds(1);

        let mut delivery_before_acceptance = candidate(&fixture);
        let accepted_at = delivery_before_acceptance.submission_accepted_at.unwrap();
        delivery_before_acceptance
            .delivery
            .as_mut()
            .unwrap()
            .observed_at = accepted_at - Duration::nanoseconds(1);

        for malformed_candidate in [
            requested_after_admission,
            admitted_after_terminal,
            terminal_before_admission,
            delivery_before_acceptance,
        ] {
            let store = store(Some(malformed_candidate), true);
            let recorded = store.evidence.clone();
            assert!(matches!(
                AgentMailSubmissionEvidenceService::new(store)
                    .get(command(&fixture))
                    .await,
                Err(AgentMailSubmissionEvidenceError::Unavailable)
            ));
            assert_eq!(
                recorded.lock().unwrap().actions.as_slice(),
                ["reader_authorized", "candidate"]
            );
        }
    }
}
