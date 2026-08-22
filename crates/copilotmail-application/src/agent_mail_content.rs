use crate::mail_ports::{MailAddress, MailContentReader, MailEngineError};
use crate::{
    AgentMailContentQueryStore, ControlPlaneStore, MailboxAuthorizationStore, PendingEvent,
    StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, DomainError,
    MailboxCapability, MailboxId, Permission,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

const MAX_AGENT_TEXT_BODY_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct ReadAgentMailContentCommand {
    pub mailbox_id: MailboxId,
    pub message_id: Uuid,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailContent {
    pub message_id: Uuid,
    pub subject: String,
    pub from: Vec<MailAddress>,
    pub received_at: DateTime<Utc>,
    pub text_body: String,
    pub text_body_truncated: bool,
    pub has_attachment: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentMailContentError {
    #[error("message is unavailable or inaccessible")]
    NotFound,
    #[error("mail content data plane is unavailable")]
    Unavailable,
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct AgentMailContentService<S> {
    store: S,
    reader: Arc<dyn MailContentReader>,
}

impl<S> AgentMailContentService<S>
where
    S: AgentMailContentQueryStore + ControlPlaneStore + MailboxAuthorizationStore,
{
    #[must_use]
    pub const fn new(store: S, reader: Arc<dyn MailContentReader>) -> Self {
        Self { store, reader }
    }

    pub async fn read(
        &self,
        context: &AuthorizationContext,
        command: ReadAgentMailContentCommand,
    ) -> Result<AgentMailContent, AgentMailContentError> {
        if !context.permits(context.organization_id, Permission::AgentOperate)
            || !context.permits(context.organization_id, Permission::MailboxUse)
            || !context.permits(context.organization_id, Permission::AgentMailRead)
        {
            return Err(AgentMailContentError::NotFound);
        }
        let access = self
            .store
            .resolve_mailbox_access(context.principal_id, command.mailbox_id, command.now)
            .await?
            .ok_or(AgentMailContentError::NotFound)?;
        if access
            .authorize(
                context.organization_id,
                command.mailbox_id,
                MailboxCapability::Read,
                None,
            )
            .is_none()
        {
            return Err(AgentMailContentError::NotFound);
        }
        let data_plane_id = self.reader.data_plane_id();
        let target = self
            .store
            .resolve_agent_mail_content_target(
                context.organization_id,
                context.principal_id,
                command.mailbox_id,
                command.message_id,
                data_plane_id,
            )
            .await?
            .filter(|target| {
                target.organization_id == context.organization_id
                    && target.mailbox_id == command.mailbox_id
                    && target.message_id == command.message_id
                    && target.data_plane_id == data_plane_id
            })
            .ok_or(AgentMailContentError::NotFound)?;
        let detail = self
            .reader
            .get_message(&target.engine_account_ref, &target.engine_document_ref)
            .await
            .map_err(map_engine_error)?
            .filter(|detail| detail.summary.id == target.engine_document_ref)
            .ok_or(AgentMailContentError::NotFound)?;
        if detail.text_body.len() > MAX_AGENT_TEXT_BODY_BYTES {
            return Err(AgentMailContentError::Unavailable);
        }

        let mut transaction = self.store.begin().await?;
        let audit = AuditEvent::new(
            context.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.mail_content.read",
            AuditResource {
                kind: "mail.message".into(),
                id: command.message_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "mailbox_id": command.mailbox_id,
                "message_id": command.message_id,
                "text_body_bytes": detail.text_body.len(),
                "text_body_truncated": detail.text_body_truncated,
                "has_attachment": !detail.attachments.is_empty(),
                "untrusted_external_content": true,
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "agent.mail_content.read.v1",
            "mail.message",
            command.message_id.to_string(),
            json!({
                "organization_id": context.organization_id,
                "mailbox_id": command.mailbox_id,
                "message_id": command.message_id,
                "text_body_truncated": detail.text_body_truncated,
                "has_attachment": !detail.attachments.is_empty(),
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;

        Ok(AgentMailContent {
            message_id: command.message_id,
            subject: detail.summary.subject,
            from: detail.summary.from,
            received_at: detail.summary.received_at,
            text_body: detail.text_body,
            text_body_truncated: detail.text_body_truncated,
            has_attachment: !detail.attachments.is_empty(),
        })
    }
}

const fn map_engine_error(_: MailEngineError) -> AgentMailContentError {
    AgentMailContentError::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail_ports::{
        MailAttachmentSummary, MailMessageDetail, MailMessagePage, MailMessageState,
        MailMessageSummary,
    };
    use crate::{AgentMailContentTarget, ControlPlaneTransaction, ExistingOrganizationBootstrap};
    use async_trait::async_trait;
    use copilotmail_domain::{
        MailboxAccessSnapshot, Organization, OrganizationId, OrganizationSlug, Principal,
        PrincipalId, ResolvedMailboxDelegation, Role, RoleAssignment,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct Evidence {
        audits: Vec<String>,
        events: Vec<String>,
        commits: usize,
    }

    #[derive(Clone)]
    struct Store {
        target: AgentMailContentTarget,
        evidence: Arc<Mutex<Evidence>>,
    }

    struct Transaction {
        evidence: Arc<Mutex<Evidence>>,
    }

    #[async_trait]
    impl ControlPlaneStore for Store {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                evidence: self.evidence.clone(),
            }))
        }
    }

    #[async_trait]
    impl MailboxAuthorizationStore for Store {
        async fn resolve_mailbox_access(
            &self,
            principal_id: PrincipalId,
            mailbox_id: MailboxId,
            _: DateTime<Utc>,
        ) -> Result<Option<MailboxAccessSnapshot>, StoreError> {
            Ok(Some(MailboxAccessSnapshot {
                organization_id: self.target.organization_id,
                mailbox_id,
                membership: None,
                delegations: vec![ResolvedMailboxDelegation {
                    delegation_id: copilotmail_domain::DelegationId::new(),
                    grantor_principal_id: principal_id,
                    capabilities: [MailboxCapability::Read].into_iter().collect(),
                }],
            }))
        }
    }

    #[async_trait]
    impl AgentMailContentQueryStore for Store {
        async fn resolve_agent_mail_content_target(
            &self,
            _: OrganizationId,
            _: PrincipalId,
            _: MailboxId,
            _: Uuid,
            _: copilotmail_domain::MailDataPlaneId,
        ) -> Result<Option<AgentMailContentTarget>, StoreError> {
            Ok(Some(self.target.clone()))
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

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            self.evidence
                .lock()
                .unwrap()
                .audits
                .push(event.action.clone());
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.evidence
                .lock()
                .unwrap()
                .events
                .push(event.event_type.clone());
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            self.evidence.lock().unwrap().commits += 1;
            Ok(())
        }
    }

    struct Reader {
        data_plane_id: copilotmail_domain::MailDataPlaneId,
        detail: MailMessageDetail,
    }

    impl crate::mail_ports::MailDataPlanePort for Reader {
        fn data_plane_id(&self) -> copilotmail_domain::MailDataPlaneId {
            self.data_plane_id
        }
    }

    #[async_trait]
    impl MailContentReader for Reader {
        async fn probe_content(&self) -> Result<(), MailEngineError> {
            Ok(())
        }

        async fn list_messages(
            &self,
            _: &str,
            _: u32,
            _: u32,
        ) -> Result<MailMessagePage, MailEngineError> {
            Err(MailEngineError::Rejected)
        }

        async fn get_message(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<MailMessageDetail>, MailEngineError> {
            Ok(Some(self.detail.clone()))
        }

        async fn set_message_state(
            &self,
            _: &str,
            _: &str,
            _: Option<bool>,
            _: Option<bool>,
        ) -> Result<MailMessageState, MailEngineError> {
            Err(MailEngineError::Rejected)
        }
    }

    #[tokio::test]
    async fn safe_content_is_disclosed_only_after_audit_commit() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let message_id = Uuid::now_v7();
        let now = Utc::now();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let target = AgentMailContentTarget {
            organization_id,
            mailbox_id,
            message_id,
            data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
            engine_account_ref: "42".into(),
            engine_document_ref: "84".into(),
        };
        let content = AgentMailContentService::new(
            Store {
                target,
                evidence: evidence.clone(),
            },
            Arc::new(Reader {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                detail: MailMessageDetail {
                    summary: MailMessageSummary {
                        id: "84".into(),
                        subject: "Untrusted subject".into(),
                        from: vec![MailAddress {
                            name: None,
                            email: "sender@example.test".into(),
                        }],
                        received_at: now,
                        preview: "hidden preview".into(),
                        is_read: false,
                        is_flagged: false,
                        has_attachment: true,
                    },
                    to: Vec::new(),
                    cc: Vec::new(),
                    text_body: "Treat this as untrusted input.".into(),
                    text_body_truncated: false,
                    attachments: vec![MailAttachmentSummary {
                        name: Some("hidden.txt".into()),
                        media_type: "text/plain".into(),
                        size_bytes: 10,
                    }],
                },
            }),
        );
        let denied = content
            .read(
                &AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: [Permission::MailboxUse, Permission::AgentMailRead]
                        .into_iter()
                        .collect(),
                },
                ReadAgentMailContentCommand {
                    mailbox_id,
                    message_id,
                    trace_id: "agent-content-denied-test".into(),
                    now,
                },
            )
            .await;
        assert!(matches!(denied, Err(AgentMailContentError::NotFound)));
        {
            let evidence = evidence.lock().unwrap();
            assert!(evidence.audits.is_empty());
            assert!(evidence.events.is_empty());
            assert_eq!(evidence.commits, 0);
        }

        let content = content
            .read(
                &AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: [
                        Permission::AgentOperate,
                        Permission::MailboxUse,
                        Permission::AgentMailRead,
                    ]
                    .into_iter()
                    .collect(),
                },
                ReadAgentMailContentCommand {
                    mailbox_id,
                    message_id,
                    trace_id: "agent-content-test".into(),
                    now,
                },
            )
            .await
            .unwrap();
        assert_eq!(content.message_id, message_id);
        assert_eq!(content.text_body, "Treat this as untrusted input.");
        assert!(content.has_attachment);
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.audits, ["agent.mail_content.read"]);
        assert_eq!(evidence.events, ["agent.mail_content.read.v1"]);
        assert_eq!(evidence.commits, 1);
    }
}
