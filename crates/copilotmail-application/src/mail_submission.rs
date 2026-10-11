use crate::mail_ports::{MailEngineError, MailSubmissionAdapter, MailSubmissionReconciliation};
use crate::{
    AgentMailAdmissionFailure, ControlPlaneStore, MailSubmissionIntentRecord,
    MailSubmissionIntentWrite, MailSubmissionLease, MailSubmissionStatus,
    MailSubmissionTerminalTransition, MailSubmissionTerminalWrite, MailboxAuthorizationStore,
    PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AgentEnvironment, AuditActor, AuditContext, AuditEvent, AuditOutcome, AuditResource,
    AuthorizationContext, DomainError, DraftId, MailSubmissionId, MailboxCapability, MailboxId,
    Permission,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RequestMailSubmissionCommand {
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub expected_draft_version: i64,
    pub idempotency_key: String,
    pub first_contact_approval_request_id: Option<copilotmail_domain::ApprovalRequestId>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum MailSubmissionError {
    #[error("mailbox or draft is unavailable or inaccessible")]
    NotFound,
    #[error("mail submission request is invalid")]
    InvalidRequest,
    #[error("mail submission conflicts with existing state")]
    Conflict,
    #[error("production Agent mail admission was denied")]
    AgentAdmission(AgentMailAdmissionFailure),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("mail submission persistence is unavailable")]
    Unavailable,
}

pub struct MailSubmissionService<S> {
    store: S,
}

impl<S> MailSubmissionService<S>
where
    S: ControlPlaneStore + MailboxAuthorizationStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn request(
        &self,
        context: &AuthorizationContext,
        command: RequestMailSubmissionCommand,
    ) -> Result<MailSubmissionIntentRecord, MailSubmissionError> {
        if command.expected_draft_version <= 0 || !valid_idempotency_key(&command.idempotency_key) {
            return Err(MailSubmissionError::InvalidRequest);
        }
        if !context.permits(context.organization_id, Permission::MailboxUse) {
            return Err(MailSubmissionError::NotFound);
        }
        if !self
            .store
            .principal_can_submit_mail(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
        {
            return Err(MailSubmissionError::NotFound);
        }
        if self
            .store
            .active_agent_environment(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
            == Some(AgentEnvironment::Testing)
        {
            return Err(MailSubmissionError::AgentAdmission(
                AgentMailAdmissionFailure::SandboxRequired,
            ));
        }
        let snapshot = self
            .store
            .resolve_mailbox_access(context.principal_id, command.mailbox_id, command.now)
            .await
            .map_err(map_store_error)?
            .ok_or(MailSubmissionError::NotFound)?;
        if snapshot
            .authorize(
                context.organization_id,
                command.mailbox_id,
                MailboxCapability::Send,
                None,
            )
            .is_none()
        {
            return Err(MailSubmissionError::NotFound);
        }

        let idempotency_key_sha256: [u8; 32] =
            Sha256::digest(command.idempotency_key.as_bytes()).into();
        let request_sha256 = request_digest(
            context,
            command.mailbox_id,
            command.draft_id,
            command.expected_draft_version,
            command.first_contact_approval_request_id,
        );
        let mut transaction = self
            .store
            .begin_authorized(context)
            .await
            .map_err(map_store_error)?;
        if !transaction
            .active_principal_exists(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
        {
            return Err(MailSubmissionError::NotFound);
        }
        let creation = transaction
            .create_mail_submission_intent(&MailSubmissionIntentWrite {
                submission_id: MailSubmissionId::new(),
                organization_id: context.organization_id,
                requested_by: context.principal_id,
                mailbox_id: command.mailbox_id,
                draft_id: command.draft_id,
                expected_draft_version: command.expected_draft_version,
                idempotency_key_sha256,
                request_sha256,
                first_contact_approval_request_id: command.first_contact_approval_request_id,
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;
        if !creation.created {
            return Ok(creation.intent);
        }

        let audit = AuditEvent::new(
            context.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail.submission.requested",
            AuditResource {
                kind: "mail.submission".into(),
                id: creation.intent.submission_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "mailbox_id": command.mailbox_id,
                "draft_id": command.draft_id,
                "draft_version": creation.intent.draft_version,
            }),
            // The intent timestamp is supplied by the persistence clock,
            // which is also the source used by immutable production
            // admission evidence. Keep this request fact on that timeline.
            creation.intent.created_at,
        )?;
        let event = PendingEvent::for_mail_data_plane(
            creation.intent.data_plane_id,
            "mail.submission.requested.v1",
            "mail.submission",
            creation.intent.submission_id.to_string(),
            json!({
                "organization_id": context.organization_id,
                "mailbox_id": command.mailbox_id,
                "draft_id": command.draft_id,
                "submission_id": creation.intent.submission_id,
                "status": "pending",
            }),
            creation.intent.created_at,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(map_store_error)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(map_store_error)?;
        if let Some(admission) = &creation.production_admission {
            let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
            audit_context.subject_principal_id = Some(context.principal_id);
            if admission.first_contact_count > 0 {
                audit_context.approval_request_id = command.first_contact_approval_request_id;
            }
            let admission_audit = AuditEvent::new(
                context.organization_id,
                AuditActor {
                    principal_id: context.principal_id,
                    on_behalf_of: None,
                    credential_id: None,
                },
                "agent.mail.admitted",
                AuditResource {
                    kind: "agent.mail_admission".into(),
                    id: admission.id.to_string(),
                },
                AuditOutcome::Success,
                command.trace_id.clone(),
                json!({
                    "submission_id": admission.submission_id,
                    "policy_version_id": admission.policy_version_id,
                    "policy_version": admission.policy_version,
                    "recipient_count": admission.recipient_count,
                    "first_contact_count": admission.first_contact_count,
                }),
                // Production admission is materialized by PostgreSQL, whose
                // immutable trigger supplies the authoritative timestamp.
                // Bind the audit fact to that returned time rather than an
                // application clock so evidence reads can prove causality.
                admission.admitted_at,
            )?
            .with_context(audit_context)?;
            let admission_event = PendingEvent::for_mail_data_plane(
                creation.intent.data_plane_id,
                "agent.mail.admitted.v1",
                "agent.mail_admission",
                admission.id.to_string(),
                json!({
                    "organization_id": context.organization_id,
                    "agent_id": context.principal_id,
                    "submission_id": admission.submission_id,
                    "policy_version_id": admission.policy_version_id,
                    "policy_version": admission.policy_version,
                    "recipient_count": admission.recipient_count,
                    "first_contact_count": admission.first_contact_count,
                }),
                admission.admitted_at,
            );
            transaction
                .append_audit_event(&admission_audit)
                .await
                .map_err(map_store_error)?;
            transaction
                .enqueue_event(&admission_event)
                .await
                .map_err(map_store_error)?;
        }
        transaction.commit().await.map_err(map_store_error)?;
        Ok(creation.intent)
    }
}

#[derive(Debug, Clone)]
pub struct ExecuteMailSubmissionCommand {
    pub submission_id: MailSubmissionId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailSubmissionExecutionResult {
    NotAcquired,
    Deferred,
    Accepted,
    Rejected,
    ManualReview,
    LostLease,
}

#[derive(Debug, thiserror::Error)]
pub enum MailSubmissionExecutionError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("mail submission persistence is unavailable")]
    Unavailable,
}

/// Executes one durable Intent. The lease is committed before JMAP access, and
/// an ambiguous remote result deliberately remains `reconciling` for a later
/// query-before-create attempt.
pub struct MailSubmissionExecutor<S> {
    store: S,
    adapter: Arc<dyn MailSubmissionAdapter>,
}

impl<S> MailSubmissionExecutor<S>
where
    S: ControlPlaneStore,
{
    const LEASE_LIFETIME: chrono::Duration = chrono::Duration::seconds(60);

    #[must_use]
    pub const fn new(store: S, adapter: Arc<dyn MailSubmissionAdapter>) -> Self {
        Self { store, adapter }
    }

    pub async fn execute(
        &self,
        command: ExecuteMailSubmissionCommand,
    ) -> Result<MailSubmissionExecutionResult, MailSubmissionExecutionError> {
        let lease_id = Uuid::now_v7();
        let mut transaction = self
            .store
            .begin()
            .await
            .map_err(map_execution_store_error)?;
        let data_plane_id = self.adapter.data_plane_id();
        let lease = transaction
            .acquire_mail_submission_lease(
                command.submission_id,
                data_plane_id,
                lease_id,
                command.now,
                command.now + Self::LEASE_LIFETIME,
            )
            .await
            .map_err(map_execution_store_error)?;
        // audit-coverage: operational-lease
        // This commit only leases an already audited durable intent. It makes
        // no delivery or user-policy claim; terminal Audit/Outbox follows in
        // finalize. The governance gate restricts this exception to this call.
        transaction
            .commit()
            .await
            .map_err(map_execution_store_error)?;
        let Some(lease) = lease else {
            return Ok(MailSubmissionExecutionResult::NotAcquired);
        };
        if lease.data_plane_id != data_plane_id {
            return Err(MailSubmissionExecutionError::Unavailable);
        }

        match self
            .adapter
            .reconcile_submission(
                &lease.engine_account_ref,
                &lease.engine_email_ref,
                lease.authorized_from.as_str(),
            )
            .await
        {
            Ok(MailSubmissionReconciliation::Accepted {
                identity_ref,
                submission_ref,
            }) => {
                self.finalize(
                    &lease,
                    MailSubmissionStatus::Accepted,
                    Some(identity_ref),
                    Some(submission_ref),
                    None,
                    &command,
                )
                .await
            }
            Ok(MailSubmissionReconciliation::NotFound { identity_ref }) => {
                match self
                    .adapter
                    .create_submission(
                        &lease.engine_account_ref,
                        &lease.engine_email_ref,
                        &identity_ref,
                    )
                    .await
                {
                    Ok(created) if created.identity_ref == identity_ref => {
                        self.finalize(
                            &lease,
                            MailSubmissionStatus::Accepted,
                            Some(created.identity_ref),
                            Some(created.submission_ref),
                            None,
                            &command,
                        )
                        .await
                    }
                    Ok(_) => {
                        self.finalize(
                            &lease,
                            MailSubmissionStatus::ManualReview,
                            None,
                            None,
                            Some("submission_binding_mismatch"),
                            &command,
                        )
                        .await
                    }
                    Err(MailEngineError::Rejected) => {
                        self.finalize(
                            &lease,
                            MailSubmissionStatus::Rejected,
                            Some(identity_ref),
                            None,
                            Some("engine_rejected"),
                            &command,
                        )
                        .await
                    }
                    Err(_) => Ok(MailSubmissionExecutionResult::Deferred),
                }
            }
            Err(MailEngineError::ProjectionConflict) => {
                self.finalize(
                    &lease,
                    MailSubmissionStatus::ManualReview,
                    None,
                    None,
                    Some("remote_state_ambiguous"),
                    &command,
                )
                .await
            }
            Err(_) => Ok(MailSubmissionExecutionResult::Deferred),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn finalize(
        &self,
        lease: &MailSubmissionLease,
        status: MailSubmissionStatus,
        engine_identity_ref: Option<String>,
        engine_submission_ref: Option<String>,
        error_code: Option<&str>,
        command: &ExecuteMailSubmissionCommand,
    ) -> Result<MailSubmissionExecutionResult, MailSubmissionExecutionError> {
        let (action, outcome, event_type, result) = match status {
            MailSubmissionStatus::Accepted => (
                "mail.submission.accepted",
                AuditOutcome::Success,
                "mail.submission.accepted.v1",
                MailSubmissionExecutionResult::Accepted,
            ),
            MailSubmissionStatus::Rejected => (
                "mail.submission.rejected",
                AuditOutcome::Failure,
                "mail.submission.rejected.v1",
                MailSubmissionExecutionResult::Rejected,
            ),
            MailSubmissionStatus::ManualReview => (
                "mail.submission.review_required",
                AuditOutcome::Failure,
                "mail.submission.review_required.v1",
                MailSubmissionExecutionResult::ManualReview,
            ),
            MailSubmissionStatus::Pending | MailSubmissionStatus::Reconciling => {
                return Err(MailSubmissionExecutionError::Unavailable);
            }
        };
        let error_code = error_code.map(str::to_owned);
        let mut transaction = self
            .store
            .begin()
            .await
            .map_err(map_execution_store_error)?;
        let finalized = transaction
            .finalize_mail_submission_intent(&MailSubmissionTerminalWrite {
                submission_id: lease.submission_id,
                organization_id: lease.organization_id,
                lease_id: lease.lease_id,
                status,
                engine_identity_ref,
                engine_submission_ref,
                error_code: error_code.clone(),
            })
            .await
            .map_err(map_execution_store_error)?;
        let Some(finalized) = finalized else {
            return Ok(MailSubmissionExecutionResult::LostLease);
        };
        let terminal_at = terminal_evidence_timestamp(status, &finalized)?;
        let audit = AuditEvent::new(
            lease.organization_id,
            AuditActor {
                principal_id: lease.requested_by,
                on_behalf_of: None,
                credential_id: None,
            },
            action,
            AuditResource {
                kind: "mail.submission".into(),
                id: lease.submission_id.to_string(),
            },
            outcome,
            command.trace_id.clone(),
            json!({
                "mailbox_id": lease.mailbox_id,
                "draft_id": lease.draft_id,
                "draft_version": lease.draft_version,
                "attempt": lease.attempt_count,
                "error_code": error_code,
                "execution_mode": "system_worker",
            }),
            terminal_at,
        )?;
        let event = PendingEvent::for_mail_data_plane(
            lease.data_plane_id,
            event_type,
            "mail.submission",
            lease.submission_id.to_string(),
            json!({
                "organization_id": lease.organization_id,
                "mailbox_id": lease.mailbox_id,
                "draft_id": lease.draft_id,
                "submission_id": lease.submission_id,
                "status": mail_submission_status_name(status),
                "error_code": error_code,
            }),
            terminal_at,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(map_execution_store_error)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(map_execution_store_error)?;
        if status == MailSubmissionStatus::Accepted {
            transaction
                .enqueue_event(&PendingEvent::for_mail_data_plane(
                    lease.data_plane_id,
                    "mail.delivery.reconciliation.requested.v1",
                    "mail.submission",
                    lease.submission_id.to_string(),
                    json!({
                        "organization_id": lease.organization_id,
                        "mailbox_id": lease.mailbox_id,
                        "submission_id": lease.submission_id,
                    }),
                    terminal_at,
                ))
                .await
                .map_err(map_execution_store_error)?;
        }
        transaction
            .commit()
            .await
            .map_err(map_execution_store_error)?;
        Ok(result)
    }
}

/// Ensures adapters cannot make a terminal audit claim that is detached from
/// the durable state transition it is meant to prove. PostgreSQL returns this
/// record from its single lease-CAS `UPDATE`; other implementations must meet
/// the same invariant or fail closed before Audit/Outbox persistence.
fn terminal_evidence_timestamp(
    status: MailSubmissionStatus,
    transition: &MailSubmissionTerminalTransition,
) -> Result<DateTime<Utc>, MailSubmissionExecutionError> {
    match (status, transition.accepted_at) {
        (MailSubmissionStatus::Accepted, Some(accepted_at))
            if accepted_at == transition.terminal_at =>
        {
            Ok(transition.terminal_at)
        }
        (MailSubmissionStatus::Accepted, _)
        | (MailSubmissionStatus::Rejected | MailSubmissionStatus::ManualReview, Some(_)) => {
            Err(MailSubmissionExecutionError::Unavailable)
        }
        (MailSubmissionStatus::Rejected | MailSubmissionStatus::ManualReview, None) => {
            Ok(transition.terminal_at)
        }
        (MailSubmissionStatus::Pending | MailSubmissionStatus::Reconciling, _) => {
            Err(MailSubmissionExecutionError::Unavailable)
        }
    }
}

const fn mail_submission_status_name(status: MailSubmissionStatus) -> &'static str {
    match status {
        MailSubmissionStatus::Pending => "pending",
        MailSubmissionStatus::Reconciling => "reconciling",
        MailSubmissionStatus::Accepted => "accepted",
        MailSubmissionStatus::Rejected => "rejected",
        MailSubmissionStatus::ManualReview => "manual_review",
    }
}

fn map_execution_store_error(_error: StoreError) -> MailSubmissionExecutionError {
    MailSubmissionExecutionError::Unavailable
}

fn valid_idempotency_key(value: &str) -> bool {
    (22..=200).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn request_digest(
    context: &AuthorizationContext,
    mailbox_id: MailboxId,
    draft_id: DraftId,
    expected_draft_version: i64,
    first_contact_approval_request_id: Option<copilotmail_domain::ApprovalRequestId>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.mail.submit.v1");
    for value in [
        context.organization_id.as_uuid(),
        context.principal_id.as_uuid(),
        mailbox_id.as_uuid(),
        draft_id.as_uuid(),
    ] {
        digest.update(value.as_bytes());
    }
    digest.update(expected_draft_version.to_be_bytes());
    if let Some(request_id) = first_contact_approval_request_id {
        digest.update([1]);
        digest.update(request_id.as_uuid().as_bytes());
    } else {
        digest.update([0]);
    }
    digest.finalize().into()
}

fn map_store_error(error: StoreError) -> MailSubmissionError {
    match error {
        StoreError::Conflict => MailSubmissionError::Conflict,
        StoreError::MissingReference => MailSubmissionError::NotFound,
        StoreError::AgentMailAdmission(reason) => MailSubmissionError::AgentAdmission(reason),
        StoreError::AuthenticationInvalid | StoreError::Unavailable => {
            MailSubmissionError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use copilotmail_domain::{
        MailboxAddress, Organization, OrganizationId, OrganizationSlug, Principal, PrincipalId,
        Role, RoleAssignment,
    };
    use std::sync::Mutex;

    #[test]
    fn idempotency_keys_are_bounded_and_header_safe() {
        assert!(valid_idempotency_key("PXfM8c6jEwM6e1V0hJf3mQv_4N2s8a9L"));
        for invalid in [
            "short",
            "contains spaces but long enough",
            "contains/slash/and/is/long/enough",
            "line-break-is-forbidden----------------\n",
        ] {
            assert!(!valid_idempotency_key(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn request_digest_binds_actor_tenant_mailbox_draft_and_version() {
        let organization_id = copilotmail_domain::OrganizationId::new();
        let context = AuthorizationContext {
            principal_id: copilotmail_domain::PrincipalId::new(),
            organization_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
            authentication: None,
        };
        let mailbox_id = MailboxId::new();
        let draft_id = DraftId::new();
        let digest = request_digest(&context, mailbox_id, draft_id, 3, None);
        assert_ne!(
            digest,
            request_digest(&context, mailbox_id, draft_id, 4, None)
        );
        assert_ne!(
            digest,
            request_digest(&context, MailboxId::new(), draft_id, 3, None)
        );
        assert_ne!(
            digest,
            request_digest(
                &context,
                mailbox_id,
                draft_id,
                3,
                Some(copilotmail_domain::ApprovalRequestId::new())
            )
        );
    }

    #[derive(Default)]
    struct ExecutionState {
        lease: Option<MailSubmissionLease>,
        terminal_transition: Option<MailSubmissionTerminalTransition>,
        finalized: Vec<MailSubmissionTerminalWrite>,
        audits: Vec<String>,
        audit_occurred_at: Vec<DateTime<Utc>>,
        events: Vec<String>,
        event_occurred_at: Vec<DateTime<Utc>>,
        event_routes: Vec<Option<copilotmail_domain::MailDataPlaneId>>,
        commits: usize,
    }

    #[derive(Clone)]
    struct ExecutionStore {
        state: Arc<Mutex<ExecutionState>>,
    }

    struct ExecutionTransaction {
        state: Arc<Mutex<ExecutionState>>,
    }

    #[async_trait]
    impl ControlPlaneStore for ExecutionStore {
        async fn begin(&self) -> Result<Box<dyn crate::ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(ExecutionTransaction {
                state: Arc::clone(&self.state),
            }))
        }
    }

    #[async_trait]
    impl crate::ControlPlaneTransaction for ExecutionTransaction {
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

        async fn acquire_mail_submission_lease(
            &mut self,
            _: MailSubmissionId,
            _: copilotmail_domain::MailDataPlaneId,
            _: Uuid,
            _: DateTime<Utc>,
            _: DateTime<Utc>,
        ) -> Result<Option<MailSubmissionLease>, StoreError> {
            Ok(self.state.lock().unwrap().lease.take())
        }

        async fn finalize_mail_submission_intent(
            &mut self,
            write: &MailSubmissionTerminalWrite,
        ) -> Result<Option<MailSubmissionTerminalTransition>, StoreError> {
            let mut state = self.state.lock().unwrap();
            state.finalized.push(write.clone());
            Ok(state.terminal_transition.clone())
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            let mut state = self.state.lock().unwrap();
            state.audits.push(event.action.clone());
            state.audit_occurred_at.push(event.occurred_at);
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            let mut state = self.state.lock().unwrap();
            state.events.push(event.event_type.clone());
            state.event_occurred_at.push(event.occurred_at);
            state.event_routes.push(event.data_plane_id);
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            self.state.lock().unwrap().commits += 1;
            Ok(())
        }
    }

    #[derive(Clone, Copy)]
    enum AdapterMode {
        Existing,
        AmbiguousCreate,
    }

    struct ExecutionAdapter {
        data_plane_id: copilotmail_domain::MailDataPlaneId,
        mode: AdapterMode,
        creates: Arc<Mutex<usize>>,
        remote_calls: Arc<Mutex<usize>>,
    }

    impl crate::mail_ports::MailDataPlanePort for ExecutionAdapter {
        fn data_plane_id(&self) -> copilotmail_domain::MailDataPlaneId {
            self.data_plane_id
        }
    }

    #[async_trait]
    impl MailSubmissionAdapter for ExecutionAdapter {
        async fn probe_submissions(&self) -> Result<(), MailEngineError> {
            Ok(())
        }

        async fn reconcile_submission(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<MailSubmissionReconciliation, MailEngineError> {
            *self.remote_calls.lock().unwrap() += 1;
            match self.mode {
                AdapterMode::Existing => Ok(MailSubmissionReconciliation::Accepted {
                    identity_ref: "identity-1".into(),
                    submission_ref: "submission-1".into(),
                }),
                AdapterMode::AmbiguousCreate => Ok(MailSubmissionReconciliation::NotFound {
                    identity_ref: "identity-1".into(),
                }),
            }
        }

        async fn create_submission(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<crate::mail_ports::CreatedMailSubmission, MailEngineError> {
            *self.remote_calls.lock().unwrap() += 1;
            *self.creates.lock().unwrap() += 1;
            Err(MailEngineError::Unavailable)
        }
    }

    fn execution_fixture() -> (ExecutionStore, MailSubmissionId, DateTime<Utc>) {
        let now = Utc::now();
        let submission_id = MailSubmissionId::new();
        let state = ExecutionState {
            lease: Some(MailSubmissionLease {
                submission_id,
                organization_id: OrganizationId::new(),
                requested_by: PrincipalId::new(),
                mailbox_id: MailboxId::new(),
                draft_id: DraftId::new(),
                draft_version: 2,
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                engine_account_ref: "account-1".into(),
                engine_email_ref: "email-1".into(),
                authorized_from: MailboxAddress::parse("alice@example.test").unwrap(),
                lease_id: Uuid::now_v7(),
                lease_expires_at: now + chrono::Duration::seconds(60),
                attempt_count: 1,
            }),
            terminal_transition: Some(MailSubmissionTerminalTransition {
                // Simulate a control-plane database clock that differs from
                // the worker's request clock. Audit/Outbox evidence must use
                // this returned immutable transition fact.
                terminal_at: now + chrono::Duration::seconds(5),
                accepted_at: Some(now + chrono::Duration::seconds(5)),
            }),
            ..ExecutionState::default()
        };
        (
            ExecutionStore {
                state: Arc::new(Mutex::new(state)),
            },
            submission_id,
            now,
        )
    }

    #[tokio::test]
    async fn existing_remote_submission_is_terminal_with_atomic_evidence() {
        let (store, submission_id, now) = execution_fixture();
        let creates = Arc::new(Mutex::new(0));
        let result = MailSubmissionExecutor::new(
            store.clone(),
            Arc::new(ExecutionAdapter {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                mode: AdapterMode::Existing,
                creates: Arc::clone(&creates),
                remote_calls: Arc::new(Mutex::new(0)),
            }),
        )
        .execute(ExecuteMailSubmissionCommand {
            submission_id,
            trace_id: "submission-existing-test".into(),
            now,
        })
        .await
        .unwrap();
        assert_eq!(result, MailSubmissionExecutionResult::Accepted);
        assert_eq!(*creates.lock().unwrap(), 0);
        let state = store.state.lock().unwrap();
        assert_eq!(state.commits, 2);
        assert_eq!(state.finalized[0].status, MailSubmissionStatus::Accepted);
        assert_eq!(state.audits, ["mail.submission.accepted"]);
        assert_eq!(
            state.audit_occurred_at,
            [now + chrono::Duration::seconds(5)]
        );
        assert_eq!(
            state.events,
            [
                "mail.submission.accepted.v1",
                "mail.delivery.reconciliation.requested.v1"
            ]
        );
        assert_eq!(
            state.event_occurred_at,
            [
                now + chrono::Duration::seconds(5),
                now + chrono::Duration::seconds(5),
            ]
        );
        assert_eq!(
            state.event_routes,
            [
                Some(crate::mail_ports::test_mail_data_plane_id()),
                Some(crate::mail_ports::test_mail_data_plane_id()),
            ]
        );
    }

    #[tokio::test]
    async fn incomplete_accepted_transition_never_emits_terminal_evidence() {
        let (store, submission_id, now) = execution_fixture();
        store
            .state
            .lock()
            .unwrap()
            .terminal_transition
            .as_mut()
            .unwrap()
            .accepted_at = None;
        let result = MailSubmissionExecutor::new(
            store.clone(),
            Arc::new(ExecutionAdapter {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                mode: AdapterMode::Existing,
                creates: Arc::new(Mutex::new(0)),
                remote_calls: Arc::new(Mutex::new(0)),
            }),
        )
        .execute(ExecuteMailSubmissionCommand {
            submission_id,
            trace_id: "submission-incomplete-terminal-test".into(),
            now,
        })
        .await;

        assert!(matches!(
            result,
            Err(MailSubmissionExecutionError::Unavailable)
        ));
        let state = store.state.lock().unwrap();
        // The lease acquisition committed, but malformed persistence output
        // cannot be followed by a terminal Audit or Outbox claim.
        assert_eq!(state.commits, 1);
        assert_eq!(state.finalized.len(), 1);
        assert!(state.audits.is_empty());
        assert!(state.events.is_empty());
    }

    #[tokio::test]
    async fn mismatched_lease_data_plane_never_reaches_the_adapter() {
        let (store, submission_id, now) = execution_fixture();
        store
            .state
            .lock()
            .unwrap()
            .lease
            .as_mut()
            .unwrap()
            .data_plane_id = crate::mail_ports::other_test_mail_data_plane_id();
        let remote_calls = Arc::new(Mutex::new(0));
        let result = MailSubmissionExecutor::new(
            store,
            Arc::new(ExecutionAdapter {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                mode: AdapterMode::Existing,
                creates: Arc::new(Mutex::new(0)),
                remote_calls: Arc::clone(&remote_calls),
            }),
        )
        .execute(ExecuteMailSubmissionCommand {
            submission_id,
            trace_id: "submission-plane-mismatch-test".into(),
            now,
        })
        .await;

        assert!(matches!(
            result,
            Err(MailSubmissionExecutionError::Unavailable)
        ));
        assert_eq!(*remote_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn ambiguous_create_result_stays_reconciling_without_terminal_evidence() {
        let (store, submission_id, now) = execution_fixture();
        let creates = Arc::new(Mutex::new(0));
        let result = MailSubmissionExecutor::new(
            store.clone(),
            Arc::new(ExecutionAdapter {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                mode: AdapterMode::AmbiguousCreate,
                creates: Arc::clone(&creates),
                remote_calls: Arc::new(Mutex::new(0)),
            }),
        )
        .execute(ExecuteMailSubmissionCommand {
            submission_id,
            trace_id: "submission-ambiguous-test".into(),
            now,
        })
        .await
        .unwrap();
        assert_eq!(result, MailSubmissionExecutionResult::Deferred);
        assert_eq!(*creates.lock().unwrap(), 1);
        let state = store.state.lock().unwrap();
        assert_eq!(state.commits, 1);
        assert!(state.finalized.is_empty());
        assert!(state.audits.is_empty());
        assert!(state.events.is_empty());
    }
}
