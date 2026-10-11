//! Bulk Human browser-session revocation.
//!
//! One transaction appends every revocation, one audit and Outbox event per
//! session, and one principal-level summary. A later write failure drops the
//! transaction, so a partial batch never becomes visible.

use crate::{
    ApprovalProof, ApprovalQueryStore, ApprovedRequestEvidence, ControlPlaneStore,
    ControlPlaneTransaction, HumanSessionGovernanceExecution, HumanSessionGovernanceExecutionWrite,
    HumanSessionRevocationCause, HumanSessionRevocationResult, PendingEvent,
    RevokeActiveHumanSessions, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    ApprovalAction, AuditActor, AuditContext, AuditEvent, AuditOutcome, AuditResource,
    AuthenticationBinding, AuthorizationContext, HumanSessionId, OrganizationId, Permission,
    PrincipalId,
};
use serde_json::json;

const RESOURCE_KIND: &str = "identity.principal";

/// Revokes every current browser session of one principal after dual approval.
pub struct RevokePrincipalBrowserSessionsCommand {
    pub context: AuthorizationContext,
    pub path_organization_id: OrganizationId,
    pub target_principal_id: PrincipalId,
    pub approval: ApprovalProof,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Signs the caller out of every current browser session.
///
/// `retain_current_session` defaults to false at the HTTP boundary. Keeping the
/// current session is explicit because an omitted field must also sign out a
/// stolen session.
pub struct RevokeOwnBrowserSessionsCommand {
    pub context: AuthorizationContext,
    pub retain_current_session: bool,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Authorized revocation outcome. It intentionally carries no session count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptedBrowserSessionRevocation {
    pub clear_current_cookie: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HumanSessionGovernanceError {
    #[error("Human session governance request is invalid")]
    InvalidRequest,
    #[error("Human session governance is not permitted")]
    Forbidden,
    #[error("Human session governance permission is missing")]
    PermissionDenied,
    #[error("Human session governance conflicted and was rolled back")]
    Conflict,
    #[error("Human session governance authentication is no longer valid")]
    AuthenticationInvalid,
    #[error("Human session governance persistence is unavailable")]
    Unavailable,
}

pub struct HumanSessionGovernanceService<S> {
    store: S,
}

impl<S> HumanSessionGovernanceService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S> HumanSessionGovernanceService<S>
where
    S: ControlPlaneStore,
{
    pub async fn revoke_own_browser_sessions(
        &self,
        command: RevokeOwnBrowserSessionsCommand,
    ) -> Result<AcceptedBrowserSessionRevocation, HumanSessionGovernanceError> {
        let session_id = match command.context.authentication.clone() {
            Some(AuthenticationBinding::HumanSession { session_id }) => session_id,
            _ => return Err(HumanSessionGovernanceError::InvalidRequest),
        };
        let excluded = command.retain_current_session.then_some(session_id);
        let mut transaction = if command.retain_current_session {
            self.store.begin_authorized(&command.context).await?
        } else {
            self.store.begin().await?
        };
        let revoked = revoke_active_human_sessions(
            &mut *transaction,
            &ActiveHumanSessionRevocation {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                cause: HumanSessionRevocationCause::Logout,
                actor: command.context.principal_id,
                trace_id: command.trace_id.clone(),
                now: command.now,
                excluded_session_id: excluded,
                authentication: command.context.authentication.clone(),
            },
        )
        .await?;
        append_human_sessions_revoked_audit(
            &mut *transaction,
            &HumanSessionsRevokedAudit {
                organization_id: command.context.organization_id,
                actor: command.context.principal_id,
                target_principal_id: command.context.principal_id,
                cause: HumanSessionRevocationCause::Logout,
                trace_id: command.trace_id,
                now: command.now,
                eligible_target: true,
                revoked_session_count: revoked.len(),
                retained_current_session: command.retain_current_session,
                approval_request_id: None,
                authentication: command.context.authentication,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(AcceptedBrowserSessionRevocation {
            clear_current_cookie: !command.retain_current_session,
        })
    }
}

impl<S> HumanSessionGovernanceService<S>
where
    S: ControlPlaneStore + ApprovalQueryStore,
{
    pub async fn revoke_principal_browser_sessions(
        &self,
        command: RevokePrincipalBrowserSessionsCommand,
    ) -> Result<AcceptedBrowserSessionRevocation, HumanSessionGovernanceError> {
        if !command.context.permits(
            command.context.organization_id,
            Permission::SessionRevokeOthers,
        ) {
            return Err(HumanSessionGovernanceError::PermissionDenied);
        }
        if !approval_matches(&command) {
            return Err(HumanSessionGovernanceError::Forbidden);
        }
        let evidence = self
            .store
            .approved_request_evidence(&command.approval)
            .await?
            .ok_or(HumanSessionGovernanceError::Forbidden)?;
        if !evidence_is_dual(&evidence, command.now) {
            return Err(HumanSessionGovernanceError::Forbidden);
        }

        let excluded = match command.context.authentication.clone() {
            Some(AuthenticationBinding::HumanSession { session_id }) => Some(session_id),
            _ => None,
        };
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        if !transaction
            .authorize_human_permission(
                command.context.organization_id,
                command.context.principal_id,
                Permission::SessionRevokeOthers,
            )
            .await?
        {
            return Err(HumanSessionGovernanceError::PermissionDenied);
        }
        if !transaction
            .lock_exact_dual_approval(&command.approval)
            .await?
        {
            return Err(HumanSessionGovernanceError::Forbidden);
        }

        if command.path_organization_id != command.context.organization_id {
            append_human_sessions_revoked_audit(
                &mut *transaction,
                &HumanSessionsRevokedAudit {
                    organization_id: command.context.organization_id,
                    actor: command.context.principal_id,
                    target_principal_id: command.target_principal_id,
                    cause: HumanSessionRevocationCause::SecurityResponse,
                    trace_id: command.trace_id,
                    now: command.now,
                    eligible_target: false,
                    revoked_session_count: 0,
                    retained_current_session: excluded.is_some(),
                    approval_request_id: Some(command.approval.request_id),
                    authentication: command.context.authentication,
                },
            )
            .await?;
            transaction.commit().await?;
            return Ok(AcceptedBrowserSessionRevocation {
                clear_current_cookie: false,
            });
        }

        match transaction
            .insert_human_session_governance_execution(&HumanSessionGovernanceExecutionWrite {
                approval_request_id: command.approval.request_id,
                organization_id: command.context.organization_id,
                target_principal_id: command.target_principal_id,
                requested_by: command.context.principal_id,
                excluded_session_id: excluded,
                executed_at: command.now,
            })
            .await?
        {
            HumanSessionGovernanceExecution::AlreadyExecuted {
                target_principal_id,
                requested_by,
            } if target_principal_id == command.target_principal_id
                && requested_by == command.context.principal_id =>
            {
                // audit-coverage: no-op-idempotent-replay
                transaction.commit().await?;
                return Ok(AcceptedBrowserSessionRevocation {
                    clear_current_cookie: false,
                });
            }
            HumanSessionGovernanceExecution::AlreadyExecuted { .. } => {
                return Err(HumanSessionGovernanceError::Forbidden);
            }
            HumanSessionGovernanceExecution::Inserted => {}
        }

        let eligible = transaction
            .human_principal_exists(command.context.organization_id, command.target_principal_id)
            .await?;
        let revoked = if eligible {
            revoke_active_human_sessions(
                &mut *transaction,
                &ActiveHumanSessionRevocation {
                    organization_id: command.context.organization_id,
                    principal_id: command.target_principal_id,
                    cause: HumanSessionRevocationCause::SecurityResponse,
                    actor: command.context.principal_id,
                    trace_id: command.trace_id.clone(),
                    now: command.now,
                    excluded_session_id: excluded,
                    authentication: command.context.authentication.clone(),
                },
            )
            .await?
        } else {
            Vec::new()
        };
        append_human_sessions_revoked_audit(
            &mut *transaction,
            &HumanSessionsRevokedAudit {
                organization_id: command.context.organization_id,
                actor: command.context.principal_id,
                target_principal_id: command.target_principal_id,
                cause: HumanSessionRevocationCause::SecurityResponse,
                trace_id: command.trace_id,
                now: command.now,
                eligible_target: eligible,
                revoked_session_count: revoked.len(),
                retained_current_session: excluded.is_some(),
                approval_request_id: Some(command.approval.request_id),
                authentication: command.context.authentication,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(AcceptedBrowserSessionRevocation {
            clear_current_cookie: false,
        })
    }
}

pub(crate) struct ActiveHumanSessionRevocation {
    pub(crate) organization_id: OrganizationId,
    pub(crate) principal_id: PrincipalId,
    pub(crate) cause: HumanSessionRevocationCause,
    pub(crate) actor: PrincipalId,
    pub(crate) trace_id: String,
    pub(crate) now: DateTime<Utc>,
    pub(crate) excluded_session_id: Option<HumanSessionId>,
    pub(crate) authentication: Option<AuthenticationBinding>,
}

pub(crate) async fn revoke_active_human_sessions(
    transaction: &mut dyn ControlPlaneTransaction,
    command: &ActiveHumanSessionRevocation,
) -> Result<Vec<HumanSessionRevocationResult>, StoreError> {
    let revoked = transaction
        .revoke_active_human_browser_sessions(&RevokeActiveHumanSessions {
            organization_id: command.organization_id,
            principal_id: command.principal_id,
            cause: command.cause,
            revoked_at: command.now,
            excluded_session_id: command.excluded_session_id,
        })
        .await?;
    for result in &revoked {
        let mut context = AuditContext::for_request(command.trace_id.clone())
            .map_err(|_| StoreError::Unavailable)?;
        context.subject_principal_id = Some(result.session.principal_id);
        context.token_jti = result.session.oidc_jti.clone();
        let audit = AuditEvent::new(
            result.session.organization_id,
            AuditActor {
                principal_id: command.actor,
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.human_session.revoked",
            AuditResource {
                kind: "identity.human_session".into(),
                id: result.session.session_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "cause": result.cause.as_str(),
                "client_key": result.session.client_key.as_str(),
                "revoked_at": result.revoked_at,
            }),
            command.now,
        )
        .and_then(|event| event.with_context(context))
        .map_err(|_| StoreError::Unavailable)?
        .correlate_human_session(command.authentication.as_ref())
        .map_err(|_| StoreError::Unavailable)?;
        let event = PendingEvent::new(
            "identity.human_session.revoked.v1",
            "identity.human_session",
            result.session.session_id.to_string(),
            json!({
                "organization_id": result.session.organization_id,
                "principal_id": result.session.principal_id,
                "session_id": result.session.session_id,
                "client_key": result.session.client_key.as_str(),
                "cause": result.cause.as_str(),
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
    }
    Ok(revoked)
}

struct HumanSessionsRevokedAudit {
    organization_id: OrganizationId,
    actor: PrincipalId,
    target_principal_id: PrincipalId,
    cause: HumanSessionRevocationCause,
    trace_id: String,
    now: DateTime<Utc>,
    eligible_target: bool,
    revoked_session_count: usize,
    retained_current_session: bool,
    approval_request_id: Option<copilotmail_domain::ApprovalRequestId>,
    authentication: Option<AuthenticationBinding>,
}

async fn append_human_sessions_revoked_audit(
    transaction: &mut dyn ControlPlaneTransaction,
    command: &HumanSessionsRevokedAudit,
) -> Result<(), StoreError> {
    let mut context =
        AuditContext::for_request(command.trace_id.clone()).map_err(|_| StoreError::Unavailable)?;
    context.subject_principal_id = Some(command.target_principal_id);
    context.approval_request_id = command.approval_request_id;
    let details = json!({
        "cause": command.cause.as_str(),
        "eligible_target": command.eligible_target,
        "revoked_session_count": command.revoked_session_count,
        "retained_current_session": command.retained_current_session,
    });
    let audit = AuditEvent::new(
        command.organization_id,
        AuditActor {
            principal_id: command.actor,
            on_behalf_of: None,
            credential_id: None,
        },
        "identity.human_sessions.revoked",
        AuditResource {
            kind: RESOURCE_KIND.into(),
            id: command.target_principal_id.to_string(),
        },
        AuditOutcome::Success,
        command.trace_id.clone(),
        details,
        command.now,
    )
    .and_then(|event| event.with_context(context))
    .map_err(|_| StoreError::Unavailable)?
    .correlate_human_session(command.authentication.as_ref())
    .map_err(|_| StoreError::Unavailable)?;
    let event = PendingEvent::new(
        "identity.human_sessions.revoked.v1",
        RESOURCE_KIND,
        command.target_principal_id.to_string(),
        json!({
            "organization_id": command.organization_id,
            "principal_id": command.target_principal_id,
            "cause": command.cause.as_str(),
            "eligible_target": command.eligible_target,
            "retained_current_session": command.retained_current_session,
        }),
        command.now,
    );
    transaction.append_audit_event(&audit).await?;
    transaction.enqueue_event(&event).await?;
    Ok(())
}

fn approval_matches(command: &RevokePrincipalBrowserSessionsCommand) -> bool {
    command.approval.organization_id == command.context.organization_id
        && command.approval.requested_by == command.context.principal_id
        && command.approval.action == ApprovalAction::SessionRevokeOthers
        && command.approval.resource_kind == RESOURCE_KIND
        && command.approval.resource_id == command.target_principal_id.to_string()
        && command.approval.now == command.now
}

fn evidence_is_dual(evidence: &ApprovedRequestEvidence, now: DateTime<Utc>) -> bool {
    evidence.required_approvals >= 2
        && evidence.approval_count >= evidence.required_approvals
        && evidence.expires_at > now
}

const fn map_store_error(error: StoreError) -> HumanSessionGovernanceError {
    match error {
        StoreError::Conflict => HumanSessionGovernanceError::Conflict,
        StoreError::AuthenticationInvalid => HumanSessionGovernanceError::AuthenticationInvalid,
        StoreError::MissingReference => HumanSessionGovernanceError::Forbidden,
        StoreError::Unavailable | StoreError::AgentMailAdmission(_) => {
            HumanSessionGovernanceError::Unavailable
        }
    }
}

impl From<StoreError> for HumanSessionGovernanceError {
    fn from(error: StoreError) -> Self {
        map_store_error(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ApprovalQueryStore, ApprovalRequestDetail, ApprovalRequestView, ControlPlaneTransaction,
        HumanSessionClient, HumanSessionRecord, HumanSessionRevocationResult,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{ApprovalRequestId, ApprovalStatus, HumanLoginTransactionId};
    use serde_json::Value;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct Shared {
        inner: Arc<Mutex<Memory>>,
    }

    struct Memory {
        authorized: bool,
        evidence: Option<ApprovedRequestEvidence>,
        lock_ok: bool,
        principal_exists: bool,
        sessions: Vec<HumanSessionRecord>,
        revoked: Vec<HumanSessionId>,
        execution: Option<(PrincipalId, PrincipalId)>,
        audits: Vec<AuditEvent>,
        writes: Vec<&'static str>,
        commits: u32,
        begin_calls: u32,
        bind_calls: u32,
        fail_audit_at: Option<usize>,
    }

    struct MemoryTransaction {
        shared: Shared,
        pending_execution: Option<(PrincipalId, PrincipalId)>,
        pending_revoked: Vec<HumanSessionId>,
        pending_audits: Vec<AuditEvent>,
        audit_attempts: usize,
    }

    impl Shared {
        fn lock(&self) -> std::sync::MutexGuard<'_, Memory> {
            self.inner.lock().unwrap()
        }
    }

    #[async_trait]
    impl ControlPlaneStore for Shared {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            self.lock().begin_calls += 1;
            Ok(Box::new(MemoryTransaction {
                shared: self.clone(),
                pending_execution: None,
                pending_revoked: Vec::new(),
                pending_audits: Vec::new(),
                audit_attempts: 0,
            }))
        }
    }

    #[async_trait]
    impl ApprovalQueryStore for Shared {
        async fn approval_permits(&self, _: &ApprovalProof) -> Result<bool, StoreError> {
            Ok(false)
        }

        async fn approved_request_evidence(
            &self,
            _: &ApprovalProof,
        ) -> Result<Option<ApprovedRequestEvidence>, StoreError> {
            Ok(self.lock().evidence.clone())
        }

        async fn get_approval_request(
            &self,
            _: OrganizationId,
            _: ApprovalRequestId,
            _: DateTime<Utc>,
        ) -> Result<Option<ApprovalRequestDetail>, StoreError> {
            Ok(None)
        }

        async fn approval_request_cursor_exists(
            &self,
            _: OrganizationId,
            _: ApprovalRequestView,
            _: PrincipalId,
            _: Option<ApprovalStatus>,
            _: ApprovalRequestId,
            _: DateTime<Utc>,
        ) -> Result<bool, StoreError> {
            Ok(false)
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for MemoryTransaction {
        async fn bind_request_authentication(
            &mut self,
            _: &AuthorizationContext,
        ) -> Result<(), StoreError> {
            self.shared.lock().bind_calls += 1;
            Ok(())
        }

        async fn find_organization_bootstrap(
            &mut self,
            _: &copilotmail_domain::OrganizationSlug,
        ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
            Ok(None)
        }

        async fn insert_organization(
            &mut self,
            _: &copilotmail_domain::Organization,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn insert_principal(
            &mut self,
            _: &copilotmail_domain::Principal,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn insert_role(&mut self, _: &copilotmail_domain::Role) -> Result<(), StoreError> {
            Ok(())
        }

        async fn insert_role_assignment(
            &mut self,
            _: &copilotmail_domain::RoleAssignment,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn record_organization_bootstrap(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            let fail_at = self.shared.lock().fail_audit_at;
            if Some(self.audit_attempts) == fail_at {
                self.shared.lock().writes.push("audit_failed");
                return Err(StoreError::Conflict);
            }
            self.audit_attempts += 1;
            self.pending_audits.push(event.clone());
            self.shared.lock().writes.push("audit");
            Ok(())
        }

        async fn enqueue_event(&mut self, _: &PendingEvent) -> Result<(), StoreError> {
            self.shared.lock().writes.push("outbox");
            Ok(())
        }

        async fn authorize_human_permission(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            permission: Permission,
        ) -> Result<bool, StoreError> {
            Ok(self.shared.lock().authorized && permission == Permission::SessionRevokeOthers)
        }

        async fn lock_exact_dual_approval(
            &mut self,
            _: &ApprovalProof,
        ) -> Result<bool, StoreError> {
            Ok(self.shared.lock().lock_ok)
        }

        async fn human_principal_exists(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(self.shared.lock().principal_exists)
        }

        async fn insert_human_session_governance_execution(
            &mut self,
            write: &HumanSessionGovernanceExecutionWrite,
        ) -> Result<HumanSessionGovernanceExecution, StoreError> {
            if let Some((target, requester)) = self.shared.lock().execution {
                return Ok(HumanSessionGovernanceExecution::AlreadyExecuted {
                    target_principal_id: target,
                    requested_by: requester,
                });
            }
            self.pending_execution = Some((write.target_principal_id, write.requested_by));
            self.shared.lock().writes.push("execution");
            Ok(HumanSessionGovernanceExecution::Inserted)
        }

        async fn revoke_active_human_browser_sessions(
            &mut self,
            write: &RevokeActiveHumanSessions,
        ) -> Result<Vec<HumanSessionRevocationResult>, StoreError> {
            let selected = {
                let memory = self.shared.lock();
                memory
                    .sessions
                    .iter()
                    .filter(|session| {
                        session.organization_id == write.organization_id
                            && session.principal_id == write.principal_id
                            && write.excluded_session_id != Some(session.session_id)
                            && !memory.revoked.contains(&session.session_id)
                            && !self.pending_revoked.contains(&session.session_id)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            };
            let mut results = Vec::new();
            for session in selected {
                self.pending_revoked.push(session.session_id);
                results.push(HumanSessionRevocationResult {
                    revocation_id: copilotmail_domain::HumanSessionRevocationId::new(),
                    cause: write.cause,
                    revoked_at: write.revoked_at,
                    newly_revoked: true,
                    session,
                });
            }
            self.shared.lock().writes.push("revocation");
            Ok(results)
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            let mut memory = self.shared.lock();
            if let Some(execution) = self.pending_execution {
                memory.execution = Some(execution);
            }
            memory.revoked.extend(self.pending_revoked);
            memory.audits.extend(self.pending_audits);
            memory.commits += 1;
            memory.writes.push("commit");
            Ok(())
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-12T02:30:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn session(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        client: HumanSessionClient,
    ) -> HumanSessionRecord {
        let created_at = now() - chrono::Duration::minutes(5);
        HumanSessionRecord {
            session_id: HumanSessionId::new(),
            organization_id,
            principal_id,
            login_transaction_id: HumanLoginTransactionId::new(),
            client_key: client,
            oidc_jti: Some("jti-visible".into()),
            upstream_expires_at: now() + chrono::Duration::hours(2),
            created_at,
            expires_at: now() + chrono::Duration::hours(1),
        }
    }

    fn owner_context(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        session_id: HumanSessionId,
    ) -> AuthorizationContext {
        AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::SessionRevokeOthers].into(),
            authentication: Some(AuthenticationBinding::HumanSession { session_id }),
        }
    }

    fn proof(
        context: &AuthorizationContext,
        target: PrincipalId,
        at: DateTime<Utc>,
    ) -> ApprovalProof {
        ApprovalProof {
            organization_id: context.organization_id,
            request_id: ApprovalRequestId::new(),
            requested_by: context.principal_id,
            action: ApprovalAction::SessionRevokeOthers,
            resource_kind: RESOURCE_KIND.into(),
            resource_id: target.to_string(),
            now: at,
        }
    }

    fn ready_store(sessions: Vec<HumanSessionRecord>) -> Shared {
        Shared {
            inner: Arc::new(Mutex::new(Memory {
                authorized: true,
                evidence: Some(ApprovedRequestEvidence {
                    expires_at: now() + chrono::Duration::hours(1),
                    required_approvals: 2,
                    approval_count: 2,
                }),
                lock_ok: true,
                principal_exists: true,
                sessions,
                revoked: Vec::new(),
                execution: None,
                audits: Vec::new(),
                writes: Vec::new(),
                commits: 0,
                begin_calls: 0,
                bind_calls: 0,
                fail_audit_at: None,
            })),
        }
    }

    fn forbidden_keys(value: &Value) -> bool {
        const FORBIDDEN: [&str; 13] = [
            "password",
            "access_token",
            "refresh_token",
            "client_assertion",
            "authorization",
            "cookie",
            "secret",
            "private_key",
            "plaintext",
            "body",
            "raw_mime",
            "attachment_content",
            "message_body",
        ];
        match value {
            Value::Object(map) => map
                .iter()
                .any(|(key, child)| FORBIDDEN.contains(&key.as_str()) || forbidden_keys(child)),
            Value::Array(items) => items.iter().any(forbidden_keys),
            _ => false,
        }
    }

    #[tokio::test]
    async fn bulk_revocation_rolls_back_when_a_later_audit_write_fails() {
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let target = PrincipalId::new();
        let current = HumanSessionId::new();
        let store = ready_store(vec![
            session(organization_id, target, HumanSessionClient::Admin),
            session(organization_id, target, HumanSessionClient::Mail),
        ]);
        store.lock().fail_audit_at = Some(1);
        let at = now();
        let context = owner_context(organization_id, actor, current);
        let error = HumanSessionGovernanceService::new(store.clone())
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                approval: proof(&context, target, at),
                context,
                path_organization_id: organization_id,
                target_principal_id: target,
                trace_id: "trace-rollback".into(),
                now: at,
            })
            .await
            .unwrap_err();
        assert_eq!(error, HumanSessionGovernanceError::Conflict);
        let memory = store.lock();
        assert_eq!(memory.commits, 0);
        assert!(memory.revoked.is_empty());
        assert!(memory.execution.is_none());
        assert!(memory.audits.is_empty());
        assert!(!memory.writes.contains(&"commit"));
        assert!(memory.writes.contains(&"audit_failed"));
    }

    #[tokio::test]
    async fn governance_outcomes_share_one_result_and_consume_approval_once() {
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let current = HumanSessionId::new();
        let target = PrincipalId::new();
        let store = ready_store(vec![
            session(organization_id, target, HumanSessionClient::Admin),
            session(organization_id, actor, HumanSessionClient::Mail),
        ]);
        let at = now();
        let context = owner_context(organization_id, actor, current);
        let approval = proof(&context, target, at);
        let service = HumanSessionGovernanceService::new(store.clone());
        let revoked = service
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                context: context.clone(),
                path_organization_id: organization_id,
                target_principal_id: target,
                approval: approval.clone(),
                trace_id: "trace-revoke".into(),
                now: at,
            })
            .await
            .unwrap();
        assert_eq!(
            revoked,
            AcceptedBrowserSessionRevocation {
                clear_current_cookie: false
            }
        );
        {
            let memory = store.lock();
            assert_eq!(memory.revoked.len(), 1);
            assert_eq!(memory.execution, Some((target, actor)));
            assert!(memory.audits.iter().any(|event| {
                event.action == "identity.human_session.revoked" && !forbidden_keys(&event.details)
            }));
            let summary = memory
                .audits
                .iter()
                .find(|event| event.action == "identity.human_sessions.revoked")
                .unwrap();
            assert_eq!(summary.details["revoked_session_count"], 1);
            assert_eq!(summary.details["eligible_target"], true);
            assert_eq!(summary.details["cause"], "security_response");
            assert!(!forbidden_keys(&summary.details));
            assert!(summary.context.token_jti.is_none());
        }

        let replay = service
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                context: context.clone(),
                path_organization_id: organization_id,
                target_principal_id: target,
                approval,
                trace_id: "trace-replay".into(),
                now: at,
            })
            .await
            .unwrap();
        assert_eq!(replay, revoked);
        assert_eq!(
            store
                .lock()
                .audits
                .iter()
                .filter(|event| event.action == "identity.human_sessions.revoked")
                .count(),
            1
        );

        {
            let mut memory = store.lock();
            memory.sessions.clear();
            memory.execution = None;
        }
        let quiet = service
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                context: context.clone(),
                path_organization_id: organization_id,
                target_principal_id: target,
                approval: proof(&context, target, at),
                trace_id: "trace-quiet".into(),
                now: at,
            })
            .await
            .unwrap();
        assert_eq!(quiet, revoked);
        {
            let memory = store.lock();
            assert_eq!(memory.revoked.len(), 1);
            let summary = memory
                .audits
                .iter()
                .rev()
                .find(|event| event.action == "identity.human_sessions.revoked")
                .unwrap();
            assert_eq!(summary.details["eligible_target"], true);
            assert_eq!(summary.details["revoked_session_count"], 0);
        }

        let missing = PrincipalId::new();
        let mut missing_context = context.clone();
        missing_context.permissions = [Permission::SessionRevokeOthers].into();
        store.lock().principal_exists = false;
        store.lock().execution = None;
        let missing_result = service
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                approval: proof(&missing_context, missing, at),
                context: missing_context,
                path_organization_id: organization_id,
                target_principal_id: missing,
                trace_id: "trace-missing".into(),
                now: at,
            })
            .await
            .unwrap();
        assert_eq!(missing_result, revoked);

        let foreign = service
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                approval: proof(&context, target, at),
                context: context.clone(),
                path_organization_id: OrganizationId::new(),
                target_principal_id: target,
                trace_id: "trace-foreign".into(),
                now: at,
            })
            .await
            .unwrap();
        assert_eq!(foreign, revoked);
        let memory = store.lock();
        let foreign_audit = memory.audits.last().unwrap();
        assert_eq!(foreign_audit.details["eligible_target"], false);
        assert_eq!(foreign_audit.details["revoked_session_count"], 0);
        assert!(!foreign_audit.details.to_string().contains("cross"));
    }

    #[tokio::test]
    async fn own_logout_all_defaults_to_revoking_the_current_session() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let current = session(organization_id, principal_id, HumanSessionClient::Admin);
        let other = session(organization_id, principal_id, HumanSessionClient::Mail);
        let current_id = current.session_id;
        let store = ready_store(vec![current, other]);
        let context = owner_context(organization_id, principal_id, current_id);
        let result = HumanSessionGovernanceService::new(store.clone())
            .revoke_own_browser_sessions(RevokeOwnBrowserSessionsCommand {
                context,
                retain_current_session: false,
                trace_id: "trace-own".into(),
                now: now(),
            })
            .await
            .unwrap();
        assert!(result.clear_current_cookie);
        let memory = store.lock();
        assert_eq!(memory.revoked.len(), 2);
        assert!(memory.revoked.contains(&current_id));
        assert_eq!(memory.bind_calls, 0);
        assert_eq!(memory.begin_calls, 1);
        let summary = memory
            .audits
            .iter()
            .find(|event| event.action == "identity.human_sessions.revoked")
            .unwrap();
        assert_eq!(summary.details["cause"], "logout");
        assert_eq!(summary.details["retained_current_session"], false);
        assert!(!forbidden_keys(&summary.details));
    }

    #[tokio::test]
    async fn own_logout_all_can_retain_the_current_session() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let current = session(organization_id, principal_id, HumanSessionClient::Mail);
        let other = session(organization_id, principal_id, HumanSessionClient::Admin);
        let current_id = current.session_id;
        let store = ready_store(vec![current, other]);
        let result = HumanSessionGovernanceService::new(store.clone())
            .revoke_own_browser_sessions(RevokeOwnBrowserSessionsCommand {
                context: owner_context(organization_id, principal_id, current_id),
                retain_current_session: true,
                trace_id: "trace-retain".into(),
                now: now(),
            })
            .await
            .unwrap();
        assert!(!result.clear_current_cookie);
        let memory = store.lock();
        assert_eq!(memory.revoked.len(), 1);
        assert!(!memory.revoked.contains(&current_id));
        assert_eq!(memory.bind_calls, 1);
        assert_eq!(
            memory
                .audits
                .iter()
                .find(|event| event.action == "identity.human_sessions.revoked")
                .unwrap()
                .details["retained_current_session"],
            true
        );
    }

    #[tokio::test]
    async fn governance_revoke_keeps_the_callers_current_session() {
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let current = session(organization_id, actor, HumanSessionClient::Admin);
        let other = session(organization_id, actor, HumanSessionClient::Mail);
        let current_id = current.session_id;
        let other_id = other.session_id;
        let store = ready_store(vec![current, other]);
        let at = now();
        let context = owner_context(organization_id, actor, current_id);
        let result = HumanSessionGovernanceService::new(store.clone())
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                approval: proof(&context, actor, at),
                context,
                path_organization_id: organization_id,
                target_principal_id: actor,
                trace_id: "trace-self".into(),
                now: at,
            })
            .await
            .unwrap();
        assert!(!result.clear_current_cookie);
        let memory = store.lock();
        assert_eq!(memory.revoked.as_slice(), &[other_id]);
        assert!(!memory.revoked.contains(&current_id));
        assert_eq!(
            memory
                .audits
                .iter()
                .find(|event| event.action == "identity.human_sessions.revoked")
                .unwrap()
                .details["retained_current_session"],
            true
        );
    }

    #[tokio::test]
    async fn governance_rejects_callers_without_the_dedicated_permission() {
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let target = PrincipalId::new();
        let store = ready_store(vec![]);
        let at = now();
        let mut context = owner_context(organization_id, actor, HumanSessionId::new());
        context.permissions = [Permission::IdentityManage, Permission::SecurityAdmin].into();
        let error = HumanSessionGovernanceService::new(store.clone())
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                approval: proof(&context, target, at),
                context,
                path_organization_id: organization_id,
                target_principal_id: target,
                trace_id: "trace-denied".into(),
                now: at,
            })
            .await
            .unwrap_err();
        assert_eq!(error, HumanSessionGovernanceError::PermissionDenied);
        assert_eq!(store.lock().commits, 0);
        store.lock().authorized = false;
        let context = owner_context(organization_id, actor, HumanSessionId::new());
        let error = HumanSessionGovernanceService::new(store.clone())
            .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
                approval: proof(&context, target, at),
                context,
                path_organization_id: organization_id,
                target_principal_id: target,
                trace_id: "trace-recheck".into(),
                now: at,
            })
            .await
            .unwrap_err();
        assert_eq!(error, HumanSessionGovernanceError::PermissionDenied);
        assert_eq!(store.lock().commits, 0);
    }
}
