use crate::mail_ports::{
    MailAddress, MailAttachmentSummary, MailContentReader, MailEngineError, MailMessageDetail,
    MailMessagePage, MailMessageState, MailMessageSummary,
};
use crate::{
    ControlPlaneStore, HumanMailContentQueryStore, MailEngineMailboxBinding,
    MailboxAuthorizationStore, MailboxQueryStore, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, MailboxCapability,
    MailboxId, Permission,
};
use serde_json::json;
use std::{collections::HashSet, sync::Arc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ListInboxCommand {
    pub mailbox_id: MailboxId,
    pub position: u32,
    pub page_size: u32,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SearchInboxCommand {
    pub mailbox_id: MailboxId,
    pub text: String,
    pub position: u32,
    pub page_size: u32,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct GetMessageCommand {
    pub mailbox_id: MailboxId,
    pub message_id: Uuid,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpdateMessageStateCommand {
    pub mailbox_id: MailboxId,
    pub message_id: Uuid,
    pub is_read: Option<bool>,
    pub is_flagged: Option<bool>,
    pub now: DateTime<Utc>,
}

/// Human-safe inbox summary. Provider document and thread identifiers never
/// leave the application boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanInboxMessageSummary {
    pub message_id: Uuid,
    pub subject: String,
    pub from: Vec<MailAddress>,
    pub received_at: DateTime<Utc>,
    pub preview: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub has_attachment: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanInboxPage {
    pub items: Vec<HumanInboxMessageSummary>,
    pub next_position: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMessageDetail {
    pub summary: HumanInboxMessageSummary,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub text_body: String,
    pub text_body_truncated: bool,
    pub attachments: Vec<MailAttachmentSummary>,
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum InboxError {
    #[error("mailbox is unavailable or inaccessible")]
    NotFound,
    #[error("inbox request is invalid")]
    InvalidRequest,
    #[error("mail content data plane is unavailable")]
    Unavailable,
}

/// Human-facing inbox query orchestration. Authorization and the tenant-bound
/// engine account are resolved before the content adapter receives any input.
pub struct InboxService<S> {
    store: S,
    reader: Arc<dyn MailContentReader>,
}

impl<S> InboxService<S>
where
    S: ControlPlaneStore
        + HumanMailContentQueryStore
        + MailboxAuthorizationStore
        + MailboxQueryStore,
{
    #[must_use]
    pub const fn new(store: S, reader: Arc<dyn MailContentReader>) -> Self {
        Self { store, reader }
    }

    pub async fn list(
        &self,
        context: &AuthorizationContext,
        command: ListInboxCommand,
    ) -> Result<HumanInboxPage, InboxError> {
        let binding = self
            .authorize_read(context, command.mailbox_id, command.now)
            .await?;
        let page = self
            .reader
            .list_messages(&binding.account_ref, command.position, command.page_size)
            .await
            .map_err(map_mail_engine_error)?;
        let page = self
            .project_page(context, command.mailbox_id, &binding, page)
            .await?;
        self.record_query_evidence(
            context,
            command.mailbox_id,
            command.position,
            command.page_size,
            "list",
            &command.trace_id,
            command.now,
            &page,
        )
        .await?;
        Ok(page)
    }

    pub async fn search(
        &self,
        context: &AuthorizationContext,
        command: SearchInboxCommand,
    ) -> Result<HumanInboxPage, InboxError> {
        if !valid_search_text(&command.text) {
            return Err(InboxError::InvalidRequest);
        }
        let binding = self
            .authorize_read(context, command.mailbox_id, command.now)
            .await?;
        let page = self
            .reader
            .search_messages(
                &binding.account_ref,
                &command.text,
                command.position,
                command.page_size,
            )
            .await
            .map_err(map_mail_engine_error)?;
        let page = self
            .project_page(context, command.mailbox_id, &binding, page)
            .await?;
        self.record_query_evidence(
            context,
            command.mailbox_id,
            command.position,
            command.page_size,
            "search",
            &command.trace_id,
            command.now,
            &page,
        )
        .await?;
        Ok(page)
    }

    pub async fn get(
        &self,
        context: &AuthorizationContext,
        command: GetMessageCommand,
    ) -> Result<HumanMessageDetail, InboxError> {
        let binding = self
            .authorize_read(context, command.mailbox_id, command.now)
            .await?;
        let target = self
            .store
            .resolve_human_mail_content_target(
                context.organization_id,
                command.mailbox_id,
                command.message_id,
                binding.data_plane_id,
            )
            .await
            .map_err(map_store_error)?
            .filter(|target| {
                target.organization_id == context.organization_id
                    && target.mailbox_id == command.mailbox_id
                    && target.message_id == command.message_id
                    && target.data_plane_id == binding.data_plane_id
                    && target.engine_account_ref == binding.account_ref
            })
            .ok_or(InboxError::NotFound)?;
        let detail = self
            .reader
            .get_message(&target.engine_account_ref, &target.engine_document_ref)
            .await
            .map_err(map_mail_engine_error)?
            .filter(|detail| detail.summary.id == target.engine_document_ref)
            .ok_or(InboxError::NotFound)?;
        self.record_content_read_evidence(context, &command, &detail)
            .await?;

        Ok(HumanMessageDetail {
            summary: safe_summary(command.message_id, detail.summary),
            to: detail.to,
            cc: detail.cc,
            text_body: detail.text_body,
            text_body_truncated: detail.text_body_truncated,
            attachments: detail.attachments,
        })
    }

    pub async fn update_state(
        &self,
        context: &AuthorizationContext,
        command: UpdateMessageStateCommand,
    ) -> Result<MailMessageState, InboxError> {
        if command.is_read.is_none() && command.is_flagged.is_none() {
            return Err(InboxError::InvalidRequest);
        }
        let binding = self
            .authorize_read(context, command.mailbox_id, command.now)
            .await?;
        let target = self
            .store
            .resolve_human_mail_content_target(
                context.organization_id,
                command.mailbox_id,
                command.message_id,
                binding.data_plane_id,
            )
            .await
            .map_err(map_store_error)?
            .filter(|target| {
                target.organization_id == context.organization_id
                    && target.mailbox_id == command.mailbox_id
                    && target.message_id == command.message_id
                    && target.data_plane_id == binding.data_plane_id
                    && target.engine_account_ref == binding.account_ref
            })
            .ok_or(InboxError::NotFound)?;
        self.reader
            .set_message_state(
                &target.engine_account_ref,
                &target.engine_document_ref,
                command.is_read,
                command.is_flagged,
            )
            .await
            .map_err(map_mail_engine_error)
    }

    async fn authorize_read(
        &self,
        context: &AuthorizationContext,
        mailbox_id: MailboxId,
        now: DateTime<Utc>,
    ) -> Result<MailEngineMailboxBinding, InboxError> {
        if !context.permits(context.organization_id, Permission::MailboxUse) {
            return Err(InboxError::NotFound);
        }
        let snapshot = self
            .store
            .resolve_mailbox_access(context.principal_id, mailbox_id, now)
            .await
            .map_err(map_store_error)?
            .ok_or(InboxError::NotFound)?;
        if snapshot
            .authorize(
                context.organization_id,
                mailbox_id,
                MailboxCapability::Read,
                None,
            )
            .is_none()
        {
            return Err(InboxError::NotFound);
        }
        let data_plane_id = self.reader.data_plane_id();
        self.store
            .get_mail_engine_mailbox_binding(context.organization_id, mailbox_id, data_plane_id)
            .await
            .map_err(map_store_error)?
            .filter(|binding| {
                binding.organization_id == context.organization_id
                    && binding.mailbox_id == mailbox_id
                    && binding.data_plane_id == data_plane_id
            })
            .ok_or(InboxError::NotFound)
    }

    async fn project_page(
        &self,
        context: &AuthorizationContext,
        mailbox_id: MailboxId,
        binding: &MailEngineMailboxBinding,
        page: MailMessagePage,
    ) -> Result<HumanInboxPage, InboxError> {
        if page.items.is_empty() {
            return Ok(HumanInboxPage {
                items: Vec::new(),
                next_position: page.next_position,
            });
        }

        let document_refs = page
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        let mut unique_documents = HashSet::with_capacity(document_refs.len());
        if document_refs
            .iter()
            .any(|document_ref| !unique_documents.insert(document_ref.as_str()))
        {
            return Err(InboxError::Unavailable);
        }
        let projections = self
            .store
            .resolve_human_mail_message_projections(
                context.organization_id,
                mailbox_id,
                binding.data_plane_id,
                &binding.account_ref,
                &document_refs,
            )
            .await
            .map_err(map_store_error)?;
        if projections.len() != page.items.len() {
            return Err(InboxError::Unavailable);
        }

        let mut unique_message_ids = HashSet::with_capacity(projections.len());
        let mut items = Vec::with_capacity(page.items.len());
        for (item, projection) in page.items.into_iter().zip(projections) {
            if projection.engine_document_ref != item.id
                || !unique_message_ids.insert(projection.message_id)
            {
                return Err(InboxError::Unavailable);
            }
            items.push(safe_summary(projection.message_id, item));
        }
        Ok(HumanInboxPage {
            items,
            next_position: page.next_position,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_query_evidence(
        &self,
        context: &AuthorizationContext,
        mailbox_id: MailboxId,
        position: u32,
        page_size: u32,
        query_kind: &'static str,
        trace_id: &str,
        now: DateTime<Utc>,
        page: &HumanInboxPage,
    ) -> Result<(), InboxError> {
        // Search text, message envelopes, provider identifiers, and cursor
        // state are deliberately excluded from durable evidence.
        let mut transaction = self
            .store
            .begin_authorized(context)
            .await
            .map_err(map_store_error)?;
        let audit = AuditEvent::new(
            context.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail.messages.queried",
            AuditResource {
                kind: "mailbox".into(),
                id: mailbox_id.to_string(),
            },
            AuditOutcome::Success,
            trace_id.to_owned(),
            json!({
                "query_kind": query_kind,
                "position": position,
                "page_size": page_size,
                "result_count": page.items.len(),
                "has_more": page.next_position.is_some(),
            }),
            now,
        )
        .map_err(|_| InboxError::Unavailable)?;
        let event = PendingEvent::new(
            "mail.messages.queried.v1",
            "mailbox",
            mailbox_id.to_string(),
            json!({
                "organization_id": context.organization_id,
                "mailbox_id": mailbox_id,
                "query_kind": query_kind,
                "result_count": page.items.len(),
                "has_more": page.next_position.is_some(),
            }),
            now,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(map_store_error)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)
    }

    async fn record_content_read_evidence(
        &self,
        context: &AuthorizationContext,
        command: &GetMessageCommand,
        detail: &MailMessageDetail,
    ) -> Result<(), InboxError> {
        let mut transaction = self
            .store
            .begin_authorized(context)
            .await
            .map_err(map_store_error)?;
        let audit = AuditEvent::new(
            context.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail.message.read",
            AuditResource {
                kind: "mail.message".into(),
                id: command.message_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "mailbox_id": command.mailbox_id,
                "message_id": command.message_id,
                "text_body_bytes": detail.text_body.len(),
                "text_body_truncated": detail.text_body_truncated,
                "attachment_count": detail.attachments.len(),
                "has_attachment": !detail.attachments.is_empty(),
                "untrusted_external_content": true,
            }),
            command.now,
        )
        .map_err(|_| InboxError::Unavailable)?;
        let event = PendingEvent::new(
            "mail.message.read.v1",
            "mail.message",
            command.message_id.to_string(),
            json!({
                "organization_id": context.organization_id,
                "mailbox_id": command.mailbox_id,
                "message_id": command.message_id,
                "text_body_truncated": detail.text_body_truncated,
                "attachment_count": detail.attachments.len(),
                "has_attachment": !detail.attachments.is_empty(),
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
        transaction.commit().await.map_err(map_store_error)
    }
}

fn safe_summary(message_id: Uuid, item: MailMessageSummary) -> HumanInboxMessageSummary {
    HumanInboxMessageSummary {
        message_id,
        subject: item.subject,
        from: item.from,
        received_at: item.received_at,
        preview: item.preview,
        is_read: item.is_read,
        is_flagged: item.is_flagged,
        has_attachment: item.has_attachment,
    }
}

const fn map_store_error(_: StoreError) -> InboxError {
    InboxError::Unavailable
}

fn valid_search_text(value: &str) -> bool {
    value == value.trim()
        && (2..=128).contains(&value.chars().count())
        && value.len() <= 512
        && !value.chars().any(char::is_control)
}

const fn map_mail_engine_error(error: MailEngineError) -> InboxError {
    match error {
        MailEngineError::InvalidConfiguration => InboxError::InvalidRequest,
        MailEngineError::CredentialUnavailable
        | MailEngineError::Unavailable
        | MailEngineError::InvalidSession
        | MailEngineError::InvalidResponse
        | MailEngineError::Rejected
        | MailEngineError::ProjectionConflict => InboxError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ControlPlaneTransaction, ExistingOrganizationBootstrap, HumanMailContentTarget,
        HumanMailMessageProjection, MailEngineMailboxBinding, MailboxSummary,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{
        DelegationId, MailboxAccessSnapshot, Organization, OrganizationId, OrganizationSlug,
        Principal, PrincipalId, ResolvedMailboxDelegation, Role, RoleAssignment,
    };
    use std::{collections::BTreeSet, sync::Mutex};

    #[derive(Default)]
    struct Evidence {
        audits: Vec<String>,
        events: Vec<String>,
        serialized: Vec<String>,
        commits: usize,
    }

    struct Store {
        snapshot: Option<MailboxAccessSnapshot>,
        binding: Option<MailEngineMailboxBinding>,
        evidence: Arc<Mutex<Evidence>>,
        commit_fails: bool,
    }

    struct Transaction {
        evidence: Arc<Mutex<Evidence>>,
        commit_fails: bool,
    }

    #[async_trait]
    impl ControlPlaneStore for Store {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                evidence: self.evidence.clone(),
                commit_fails: self.commit_fails,
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

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            let mut evidence = self.evidence.lock().unwrap();
            evidence.audits.push(event.action.clone());
            evidence.serialized.push(event.details.to_string());
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            let mut evidence = self.evidence.lock().unwrap();
            evidence.events.push(event.event_type.clone());
            evidence.serialized.push(event.payload.to_string());
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            if self.commit_fails {
                return Err(StoreError::Unavailable);
            }
            self.evidence.lock().unwrap().commits += 1;
            Ok(())
        }
    }

    #[async_trait]
    impl MailboxAuthorizationStore for Store {
        async fn resolve_mailbox_access(
            &self,
            _principal_id: PrincipalId,
            _mailbox_id: MailboxId,
            _now: DateTime<Utc>,
        ) -> Result<Option<MailboxAccessSnapshot>, StoreError> {
            Ok(self.snapshot.clone())
        }
    }

    #[async_trait]
    impl MailboxQueryStore for Store {
        async fn get_mailbox(
            &self,
            _mailbox_id: MailboxId,
        ) -> Result<Option<MailboxSummary>, StoreError> {
            Ok(None)
        }

        async fn get_mail_engine_mailbox_binding(
            &self,
            _organization_id: OrganizationId,
            _mailbox_id: MailboxId,
            _expected_data_plane_id: copilotmail_domain::MailDataPlaneId,
        ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
            Ok(self.binding.clone())
        }
    }

    #[async_trait]
    impl HumanMailContentQueryStore for Store {
        async fn resolve_human_mail_message_projections(
            &self,
            _organization_id: OrganizationId,
            _mailbox_id: MailboxId,
            _expected_data_plane_id: copilotmail_domain::MailDataPlaneId,
            _engine_account_ref: &str,
            document_refs: &[String],
        ) -> Result<Vec<HumanMailMessageProjection>, StoreError> {
            Ok(document_refs
                .iter()
                .enumerate()
                .filter(|(_, engine_document_ref)| engine_document_ref.as_str() != "unprojected")
                .map(|(index, engine_document_ref)| HumanMailMessageProjection {
                    message_id: Uuid::from_u128(index as u128 + 1),
                    engine_document_ref: engine_document_ref.clone(),
                })
                .collect())
        }

        async fn resolve_human_mail_content_target(
            &self,
            organization_id: OrganizationId,
            mailbox_id: MailboxId,
            message_id: Uuid,
            _expected_data_plane_id: copilotmail_domain::MailDataPlaneId,
        ) -> Result<Option<HumanMailContentTarget>, StoreError> {
            Ok(self.binding.as_ref().map(|binding| HumanMailContentTarget {
                organization_id,
                mailbox_id,
                message_id,
                data_plane_id: binding.data_plane_id,
                engine_account_ref: binding.account_ref.clone(),
                engine_document_ref: "message-1".into(),
            }))
        }
    }

    fn authorized_store(
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        account_ref: &str,
        evidence: Arc<Mutex<Evidence>>,
        commit_fails: bool,
        now: DateTime<Utc>,
    ) -> Store {
        Store {
            snapshot: Some(MailboxAccessSnapshot {
                organization_id,
                mailbox_id,
                membership: None,
                delegations: vec![ResolvedMailboxDelegation {
                    delegation_id: DelegationId::new(),
                    grantor_principal_id: PrincipalId::new(),
                    capabilities: [MailboxCapability::Read].into_iter().collect(),
                }],
            }),
            binding: Some(MailEngineMailboxBinding {
                organization_id,
                mailbox_id,
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                account_ref: account_ref.into(),
                version: 1,
                updated_at: now,
            }),
            evidence,
            commit_fails,
        }
    }

    fn message_detail(now: DateTime<Utc>) -> MailMessageDetail {
        MailMessageDetail {
            summary: MailMessageSummary {
                id: "message-1".into(),
                subject: "Read me".into(),
                from: vec![MailAddress {
                    name: None,
                    email: "sender@example.test".into(),
                }],
                received_at: now,
                preview: "untrusted preview".into(),
                is_read: false,
                is_flagged: false,
                has_attachment: true,
            },
            to: vec![MailAddress {
                name: None,
                email: "reader@example.test".into(),
            }],
            cc: Vec::new(),
            text_body: "untrusted body".into(),
            text_body_truncated: false,
            attachments: vec![MailAttachmentSummary {
                name: Some("report.txt".into()),
                media_type: "text/plain".into(),
                size_bytes: 12,
            }],
        }
    }

    struct Reader {
        data_plane_id: copilotmail_domain::MailDataPlaneId,
        account_refs: Mutex<Vec<String>>,
        searches: Mutex<Vec<(String, String, u32, u32)>>,
        list_result: Mutex<Option<MailMessagePage>>,
        search_result: Mutex<Option<MailMessagePage>>,
        detail: Mutex<Option<MailMessageDetail>>,
        get_calls: Mutex<Vec<(String, String)>>,
        state_calls: Mutex<Vec<(String, String)>>,
    }

    impl Default for Reader {
        fn default() -> Self {
            Self {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                account_refs: Mutex::new(Vec::new()),
                searches: Mutex::new(Vec::new()),
                list_result: Mutex::new(None),
                search_result: Mutex::new(None),
                detail: Mutex::new(None),
                get_calls: Mutex::new(Vec::new()),
                state_calls: Mutex::new(Vec::new()),
            }
        }
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
            account_ref: &str,
            _position: u32,
            _page_size: u32,
        ) -> Result<MailMessagePage, MailEngineError> {
            self.account_refs.lock().unwrap().push(account_ref.into());
            Ok(self
                .list_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(MailMessagePage {
                    query_state: "query-1".into(),
                    items: Vec::new(),
                    next_position: None,
                }))
        }

        async fn search_messages(
            &self,
            account_ref: &str,
            text: &str,
            position: u32,
            page_size: u32,
        ) -> Result<MailMessagePage, MailEngineError> {
            self.searches.lock().unwrap().push((
                account_ref.into(),
                text.into(),
                position,
                page_size,
            ));
            Ok(self
                .search_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or(MailMessagePage {
                    query_state: "search-1".into(),
                    items: Vec::new(),
                    next_position: None,
                }))
        }

        async fn get_message(
            &self,
            account_ref: &str,
            message_id: &str,
        ) -> Result<Option<crate::mail_ports::MailMessageDetail>, MailEngineError> {
            self.get_calls
                .lock()
                .unwrap()
                .push((account_ref.into(), message_id.into()));
            Ok(self.detail.lock().unwrap().clone())
        }

        async fn set_message_state(
            &self,
            account_ref: &str,
            message_id: &str,
            _is_read: Option<bool>,
            _is_flagged: Option<bool>,
        ) -> Result<crate::mail_ports::MailMessageState, MailEngineError> {
            self.state_calls
                .lock()
                .unwrap()
                .push((account_ref.into(), message_id.into()));
            Ok(crate::mail_ports::MailMessageState {
                is_read: false,
                is_flagged: false,
            })
        }
    }

    #[tokio::test]
    async fn content_reader_only_receives_the_authorized_exact_binding() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let now = Utc::now();
        let reader = Arc::new(Reader::default());
        *reader.list_result.lock().unwrap() = Some(MailMessagePage {
            query_state: "query-safe".into(),
            items: vec![MailMessageSummary {
                id: "provider-email-42".into(),
                subject: "Quarterly report".into(),
                from: vec![MailAddress {
                    name: Some("Finance".into()),
                    email: "finance@example.test".into(),
                }],
                received_at: now,
                preview: "Summary".into(),
                is_read: false,
                is_flagged: true,
                has_attachment: false,
            }],
            next_position: None,
        });
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let service = InboxService::new(
            Store {
                snapshot: Some(MailboxAccessSnapshot {
                    organization_id,
                    mailbox_id,
                    membership: None,
                    delegations: vec![ResolvedMailboxDelegation {
                        delegation_id: DelegationId::new(),
                        grantor_principal_id: PrincipalId::new(),
                        capabilities: [MailboxCapability::Read].into_iter().collect(),
                    }],
                }),
                binding: Some(MailEngineMailboxBinding {
                    organization_id,
                    mailbox_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    account_ref: "account-42".into(),
                    version: 1,
                    updated_at: now,
                }),
                evidence: evidence.clone(),
                commit_fails: false,
            },
            reader.clone(),
        );
        let context = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
            authentication: None,
        };
        let page = service
            .list(
                &context,
                ListInboxCommand {
                    mailbox_id,
                    position: 0,
                    page_size: 30,
                    trace_id: "trace-list".into(),
                    now,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].message_id, Uuid::from_u128(1));
        assert_eq!(page.items[0].subject, "Quarterly report");
        assert_eq!(
            *reader.account_refs.lock().unwrap(),
            vec!["account-42".to_owned()]
        );
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.audits, ["mail.messages.queried"]);
        assert_eq!(evidence.events, ["mail.messages.queried.v1"]);
        assert!(evidence.serialized.iter().all(|serialized| {
            !serialized.contains("provider-email-42")
                && !serialized.contains("provider-thread-secret")
                && !serialized.contains("Quarterly report")
        }));
    }

    #[tokio::test]
    async fn an_unprojected_provider_message_fails_the_whole_page_closed() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let now = Utc::now();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let reader = Arc::new(Reader::default());
        *reader.list_result.lock().unwrap() = Some(MailMessagePage {
            query_state: "provider-state".into(),
            items: vec![MailMessageSummary {
                id: "unprojected".into(),
                subject: "Must not escape".into(),
                from: Vec::new(),
                received_at: now,
                preview: "Must not escape".into(),
                is_read: false,
                is_flagged: false,
                has_attachment: false,
            }],
            next_position: None,
        });
        let result = InboxService::new(
            authorized_store(
                organization_id,
                mailbox_id,
                "account-42",
                evidence.clone(),
                false,
                now,
            ),
            reader,
        )
        .list(
            &AuthorizationContext {
                principal_id,
                organization_id,
                permissions: [Permission::MailboxUse].into_iter().collect(),
                authentication: None,
            },
            ListInboxCommand {
                mailbox_id,
                position: 0,
                page_size: 30,
                trace_id: "trace-unprojected".into(),
                now,
            },
        )
        .await;
        assert_eq!(result, Err(InboxError::Unavailable));
        let evidence = evidence.lock().unwrap();
        assert!(evidence.audits.is_empty());
        assert!(evidence.events.is_empty());
        assert_eq!(evidence.commits, 0);
    }

    #[tokio::test]
    async fn missing_resource_permission_never_calls_content_reader() {
        let reader = Arc::new(Reader::default());
        let organization_id = OrganizationId::new();
        let service = InboxService::new(
            Store {
                snapshot: None,
                binding: None,
                evidence: Arc::new(Mutex::new(Evidence::default())),
                commit_fails: false,
            },
            reader.clone(),
        );
        let result = service
            .list(
                &AuthorizationContext {
                    principal_id: PrincipalId::new(),
                    organization_id,
                    permissions: BTreeSet::new(),
                    authentication: None,
                },
                ListInboxCommand {
                    mailbox_id: MailboxId::new(),
                    position: 0,
                    page_size: 30,
                    trace_id: "trace-list-denied".into(),
                    now: Utc::now(),
                },
            )
            .await;
        assert_eq!(result, Err(InboxError::NotFound));
        assert!(reader.account_refs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_is_bounded_and_uses_only_the_authorized_account() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let now = Utc::now();
        let reader = Arc::new(Reader::default());
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let service = InboxService::new(
            Store {
                snapshot: Some(MailboxAccessSnapshot {
                    organization_id,
                    mailbox_id,
                    membership: None,
                    delegations: vec![ResolvedMailboxDelegation {
                        delegation_id: DelegationId::new(),
                        grantor_principal_id: PrincipalId::new(),
                        capabilities: [MailboxCapability::Read].into_iter().collect(),
                    }],
                }),
                binding: Some(MailEngineMailboxBinding {
                    organization_id,
                    mailbox_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    account_ref: "account-search".into(),
                    version: 1,
                    updated_at: now,
                }),
                evidence: evidence.clone(),
                commit_fails: false,
            },
            reader.clone(),
        );
        let context = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
            authentication: None,
        };
        service
            .search(
                &context,
                SearchInboxCommand {
                    mailbox_id,
                    text: "quarterly report".into(),
                    position: 30,
                    page_size: 30,
                    trace_id: "trace-search".into(),
                    now,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            *reader.searches.lock().unwrap(),
            vec![(
                "account-search".to_owned(),
                "quarterly report".to_owned(),
                30,
                30
            )]
        );
        {
            let evidence_guard = evidence.lock().unwrap();
            assert_eq!(evidence_guard.audits, ["mail.messages.queried"]);
            assert_eq!(evidence_guard.events, ["mail.messages.queried.v1"]);
            assert_eq!(evidence_guard.commits, 1);
            assert!(
                evidence_guard
                    .serialized
                    .iter()
                    .all(|value| !value.contains("quarterly report"))
            );
        }

        for text in ["x", " padded", "line\nbreak"] {
            let result = service
                .search(
                    &context,
                    SearchInboxCommand {
                        mailbox_id,
                        text: text.into(),
                        position: 0,
                        page_size: 30,
                        trace_id: "trace-invalid".into(),
                        now,
                    },
                )
                .await;
            assert_eq!(result, Err(InboxError::InvalidRequest));
        }
        assert_eq!(reader.searches.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn search_result_is_not_returned_when_evidence_commit_fails() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let now = Utc::now();
        let reader = Arc::new(Reader::default());
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let service = InboxService::new(
            Store {
                snapshot: Some(MailboxAccessSnapshot {
                    organization_id,
                    mailbox_id,
                    membership: None,
                    delegations: vec![ResolvedMailboxDelegation {
                        delegation_id: DelegationId::new(),
                        grantor_principal_id: PrincipalId::new(),
                        capabilities: [MailboxCapability::Read].into_iter().collect(),
                    }],
                }),
                binding: Some(MailEngineMailboxBinding {
                    organization_id,
                    mailbox_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    account_ref: "account-search".into(),
                    version: 1,
                    updated_at: now,
                }),
                evidence: evidence.clone(),
                commit_fails: true,
            },
            reader,
        );
        let result = service
            .search(
                &AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: [Permission::MailboxUse].into_iter().collect(),
                    authentication: None,
                },
                SearchInboxCommand {
                    mailbox_id,
                    text: "quarterly report".into(),
                    position: 0,
                    page_size: 30,
                    trace_id: "trace-failed-commit".into(),
                    now,
                },
            )
            .await;

        assert_eq!(result, Err(InboxError::Unavailable));
        assert_eq!(evidence.lock().unwrap().commits, 0);
    }

    #[tokio::test]
    async fn content_read_uses_safe_uuid_and_is_not_returned_without_committed_evidence() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let message_id = Uuid::from_u128(9);
        let now = Utc::now();
        let context = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
            authentication: None,
        };

        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let reader = Arc::new(Reader::default());
        *reader.detail.lock().unwrap() = Some(message_detail(now));
        let detail = InboxService::new(
            authorized_store(
                organization_id,
                mailbox_id,
                "account-read",
                evidence.clone(),
                false,
                now,
            ),
            reader.clone(),
        )
        .get(
            &context,
            GetMessageCommand {
                mailbox_id,
                message_id,
                trace_id: "trace-content-read".into(),
                now,
            },
        )
        .await
        .unwrap();
        assert_eq!(detail.summary.message_id, message_id);
        assert_eq!(detail.text_body, "untrusted body");
        assert_eq!(
            *reader.get_calls.lock().unwrap(),
            [("account-read".to_owned(), "message-1".to_owned())]
        );
        {
            let evidence = evidence.lock().unwrap();
            assert_eq!(evidence.audits, ["mail.message.read"]);
            assert_eq!(evidence.events, ["mail.message.read.v1"]);
            assert_eq!(evidence.commits, 1);
            assert!(evidence.serialized.iter().all(|serialized| {
                !serialized.contains("untrusted body")
                    && !serialized.contains("provider-thread-secret")
                    && !serialized.contains("sender@example.test")
            }));
        }

        let failed_evidence = Arc::new(Mutex::new(Evidence::default()));
        let failed_reader = Arc::new(Reader::default());
        *failed_reader.detail.lock().unwrap() = Some(message_detail(now));
        let result = InboxService::new(
            authorized_store(
                organization_id,
                mailbox_id,
                "account-read",
                failed_evidence.clone(),
                true,
                now,
            ),
            failed_reader,
        )
        .get(
            &context,
            GetMessageCommand {
                mailbox_id,
                message_id,
                trace_id: "trace-content-read-failed".into(),
                now,
            },
        )
        .await;
        assert_eq!(result, Err(InboxError::Unavailable));
        assert_eq!(failed_evidence.lock().unwrap().commits, 0);
    }

    #[tokio::test]
    async fn empty_state_patch_is_rejected_before_storage_or_jmap() {
        let reader = Arc::new(Reader::default());
        let organization_id = OrganizationId::new();
        let service = InboxService::new(
            Store {
                snapshot: None,
                binding: None,
                evidence: Arc::new(Mutex::new(Evidence::default())),
                commit_fails: false,
            },
            reader.clone(),
        );
        let result = service
            .update_state(
                &AuthorizationContext {
                    principal_id: PrincipalId::new(),
                    organization_id,
                    permissions: [Permission::MailboxUse].into_iter().collect(),
                    authentication: None,
                },
                UpdateMessageStateCommand {
                    mailbox_id: MailboxId::new(),
                    message_id: Uuid::from_u128(1),
                    is_read: None,
                    is_flagged: None,
                    now: Utc::now(),
                },
            )
            .await;
        assert_eq!(result, Err(InboxError::InvalidRequest));
        assert!(reader.account_refs.lock().unwrap().is_empty());
    }
}
