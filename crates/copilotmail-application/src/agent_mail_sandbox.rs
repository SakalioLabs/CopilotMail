use crate::{
    AgentMailSandboxSubmissionRecord, AgentMailSandboxSubmissionWrite, ControlPlaneStore,
    MailboxAuthorizationStore, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AgentEnvironment, AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext,
    DomainError, DraftId, MailboxCapability, MailboxId, Permission,
};
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct CaptureAgentMailSandboxCommand {
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub expected_draft_version: i64,
    pub idempotency_key: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAgentMailSandboxSubmission {
    pub record: AgentMailSandboxSubmissionRecord,
    pub replayed: bool,
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum AgentMailSandboxError {
    #[error("sandbox request is invalid")]
    InvalidRequest,
    #[error("sandbox resource is unavailable or inaccessible")]
    NotFound,
    #[error("sandbox request conflicts with existing state")]
    Conflict,
    #[error("sandbox persistence is unavailable")]
    Unavailable,
}

pub struct AgentMailSandboxService<S> {
    store: S,
}

impl<S> AgentMailSandboxService<S>
where
    S: ControlPlaneStore + MailboxAuthorizationStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn capture(
        &self,
        context: &AuthorizationContext,
        command: CaptureAgentMailSandboxCommand,
    ) -> Result<CapturedAgentMailSandboxSubmission, AgentMailSandboxError> {
        if command.expected_draft_version <= 0
            || !valid_idempotency_key(&command.idempotency_key)
            || !context.permits(context.organization_id, Permission::MailboxUse)
            || !context.permits(context.organization_id, Permission::AgentOperate)
        {
            return Err(AgentMailSandboxError::InvalidRequest);
        }
        if self
            .store
            .active_agent_environment(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
            != Some(AgentEnvironment::Testing)
        {
            return Err(AgentMailSandboxError::NotFound);
        }
        let snapshot = self
            .store
            .resolve_mailbox_access(context.principal_id, command.mailbox_id, command.now)
            .await
            .map_err(map_store_error)?
            .ok_or(AgentMailSandboxError::NotFound)?;
        if snapshot
            .authorize(
                context.organization_id,
                command.mailbox_id,
                MailboxCapability::Send,
                None,
            )
            .is_none()
        {
            return Err(AgentMailSandboxError::NotFound);
        }
        let idempotency_key_sha256: [u8; 32] =
            Sha256::digest(command.idempotency_key.as_bytes()).into();
        let request_sha256 = request_digest(
            context,
            command.mailbox_id,
            command.draft_id,
            command.expected_draft_version,
        );
        let mut transaction = self.store.begin().await.map_err(map_store_error)?;
        if !transaction
            .active_principal_exists(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
        {
            return Err(AgentMailSandboxError::NotFound);
        }
        let creation = transaction
            .create_agent_mail_sandbox_submission(&AgentMailSandboxSubmissionWrite {
                submission_id: uuid::Uuid::now_v7(),
                organization_id: context.organization_id,
                agent_id: context.principal_id,
                mailbox_id: command.mailbox_id,
                draft_id: command.draft_id,
                expected_draft_version: command.expected_draft_version,
                idempotency_key_sha256,
                request_sha256,
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;
        if !creation.created {
            return Ok(CapturedAgentMailSandboxSubmission {
                record: creation.record,
                replayed: true,
            });
        }
        let audit = AuditEvent::new(
            context.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.mail_sandbox.captured",
            AuditResource {
                kind: "agent.mail_sandbox_submission".into(),
                id: creation.record.submission_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "mailbox_id": creation.record.mailbox_id,
                "draft_id": creation.record.draft_id,
                "draft_version": creation.record.draft_version,
                "recipient_count": creation.record.recipient_count,
                "delivery_suppressed": true,
            }),
            command.now,
        )
        .map_err(map_domain_error)?;
        let event = PendingEvent::new(
            "agent.mail_sandbox.captured.v1",
            "agent.mail_sandbox_submission",
            creation.record.submission_id.to_string(),
            json!({
                "organization_id": creation.record.organization_id,
                "agent_id": creation.record.agent_id,
                "mailbox_id": creation.record.mailbox_id,
                "sandbox_submission_id": creation.record.submission_id,
                "recipient_count": creation.record.recipient_count,
                "delivery_suppressed": true,
            }),
            command.now,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(map_store_error)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(CapturedAgentMailSandboxSubmission {
            record: creation.record,
            replayed: false,
        })
    }
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
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.agent.mail-sandbox.capture.v1");
    for value in [
        context.organization_id.as_uuid(),
        context.principal_id.as_uuid(),
        mailbox_id.as_uuid(),
        draft_id.as_uuid(),
    ] {
        digest.update(value.as_bytes());
    }
    digest.update(expected_draft_version.to_be_bytes());
    digest.finalize().into()
}

const fn map_store_error(error: StoreError) -> AgentMailSandboxError {
    match error {
        StoreError::Conflict | StoreError::AgentMailAdmission(_) => AgentMailSandboxError::Conflict,
        StoreError::MissingReference => AgentMailSandboxError::NotFound,
        StoreError::Unavailable => AgentMailSandboxError::Unavailable,
    }
}

const fn map_domain_error(_error: DomainError) -> AgentMailSandboxError {
    AgentMailSandboxError::InvalidRequest
}

#[cfg(test)]
mod tests {
    use super::*;
    use copilotmail_domain::{OrganizationId, PrincipalId};
    use std::collections::BTreeSet;

    #[test]
    fn sandbox_request_digest_binds_actor_tenant_mailbox_draft_and_version() {
        let context = AuthorizationContext {
            organization_id: OrganizationId::new(),
            principal_id: PrincipalId::new(),
            permissions: BTreeSet::new(),
        };
        let mailbox_id = MailboxId::new();
        let draft_id = DraftId::new();
        let digest = request_digest(&context, mailbox_id, draft_id, 3);
        assert_ne!(digest, request_digest(&context, mailbox_id, draft_id, 4));
        assert_ne!(
            digest,
            request_digest(&context, MailboxId::new(), draft_id, 3)
        );
    }

    #[test]
    fn sandbox_idempotency_key_is_bounded_and_header_safe() {
        assert!(valid_idempotency_key("sandbox-key-1234567890"));
        assert!(!valid_idempotency_key("short"));
        assert!(!valid_idempotency_key("sandbox-key-1234567890\r\n"));
    }
}
