use crate::mail_ports::{MailDraftStore, MailEngineError, TextDraftInput};
use crate::{
    MailDraftMetadataStore, MailDraftRecipientEvidence, MailDraftStatus, MailDraftWrite,
    MailReplyTargetQueryStore, MailboxAuthorizationStore, MailboxQueryStore, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuthorizationContext, DraftId, MailboxAddress, MailboxCapability, MailboxId, Permission,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SaveTextDraftCommand {
    pub mailbox_id: MailboxId,
    pub draft_id: Option<DraftId>,
    pub expected_version: Option<i64>,
    pub reply_to_message_id: Option<uuid::Uuid>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedTextDraft {
    pub draft_id: DraftId,
    pub version: i64,
    pub reply_to_message_id: Option<uuid::Uuid>,
    pub previous_draft_destroyed: bool,
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum DraftError {
    #[error("mailbox is unavailable or inaccessible")]
    NotFound,
    #[error("draft input is invalid")]
    InvalidRequest,
    #[error("draft changed or is no longer editable")]
    Conflict,
    #[error("mail draft data plane is unavailable")]
    Unavailable,
}

pub struct DraftService<S> {
    store: S,
    drafts: Arc<dyn MailDraftStore>,
}

impl<S> DraftService<S>
where
    S: MailboxAuthorizationStore
        + MailboxQueryStore
        + MailDraftMetadataStore
        + MailReplyTargetQueryStore,
{
    #[must_use]
    pub const fn new(store: S, drafts: Arc<dyn MailDraftStore>) -> Self {
        Self { store, drafts }
    }

    pub async fn save(
        &self,
        context: &AuthorizationContext,
        command: SaveTextDraftCommand,
    ) -> Result<SavedTextDraft, DraftError> {
        if command.draft_id.is_some() != command.expected_version.is_some()
            || command.expected_version.is_some_and(|version| version <= 0)
        {
            return Err(DraftError::InvalidRequest);
        }
        let input = normalized_input(
            command.to,
            command.cc,
            command.bcc,
            command.subject,
            command.text_body,
        )?;
        let body_ref = body_reference(context, &input.text_body);
        let content_sha256 = draft_digest(&input, command.reply_to_message_id);
        let recipients = recipient_evidence(context, &input);
        if !context.permits(context.organization_id, Permission::MailboxUse) {
            return Err(DraftError::NotFound);
        }
        let automatic = self
            .store
            .active_agent_environment(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
            .is_some();
        if !self
            .store
            .principal_can_submit_mail(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
        {
            return Err(DraftError::NotFound);
        }
        let production_policy_rejected = self
            .store
            .active_production_agent_mail_policy(context.organization_id, context.principal_id)
            .await
            .map_err(map_store_error)?
            .is_some_and(|policy| {
                recipients.len() > usize::from(policy.max_recipients_per_message())
                    || recipients.iter().any(|recipient| {
                        !policy
                            .allowed_recipient_domains()
                            .contains(&recipient.domain)
                    })
            });
        if production_policy_rejected {
            return Err(DraftError::InvalidRequest);
        }
        let snapshot = self
            .store
            .resolve_mailbox_access(context.principal_id, command.mailbox_id, command.now)
            .await
            .map_err(map_store_error)?
            .ok_or(DraftError::NotFound)?;
        if snapshot
            .authorize(
                context.organization_id,
                command.mailbox_id,
                MailboxCapability::Send,
                None,
            )
            .is_none()
        {
            return Err(DraftError::NotFound);
        }
        let data_plane_id = self.drafts.data_plane_id();
        let reply_target = if let Some(message_id) = command.reply_to_message_id {
            if snapshot
                .authorize(
                    context.organization_id,
                    command.mailbox_id,
                    MailboxCapability::Read,
                    None,
                )
                .is_none()
            {
                return Err(DraftError::NotFound);
            }
            Some(
                self.store
                    .resolve_mail_reply_target(
                        context.organization_id,
                        command.mailbox_id,
                        message_id,
                        data_plane_id,
                    )
                    .await
                    .map_err(map_store_error)?
                    .filter(|target| {
                        target.organization_id == context.organization_id
                            && target.mailbox_id == command.mailbox_id
                            && target.message_id == message_id
                            && target.data_plane_id == data_plane_id
                    })
                    .ok_or(DraftError::NotFound)?,
            )
        } else {
            None
        };
        let mailbox = self
            .store
            .get_mailbox(command.mailbox_id)
            .await
            .map_err(map_store_error)?
            .filter(|mailbox| mailbox.organization_id == context.organization_id)
            .ok_or(DraftError::NotFound)?;
        let binding = self
            .store
            .get_mail_engine_mailbox_binding(
                context.organization_id,
                command.mailbox_id,
                data_plane_id,
            )
            .await
            .map_err(map_store_error)?
            .filter(|binding| {
                binding.organization_id == context.organization_id
                    && binding.mailbox_id == command.mailbox_id
                    && binding.data_plane_id == data_plane_id
            })
            .ok_or(DraftError::NotFound)?;
        if reply_target.as_ref().is_some_and(|target| {
            target.data_plane_id != data_plane_id
                || target.engine_account_ref != binding.account_ref
        }) {
            return Err(DraftError::NotFound);
        }
        let previous = if let (Some(draft_id), Some(expected_version)) =
            (command.draft_id, command.expected_version)
        {
            let previous = self
                .store
                .get_mail_draft(
                    context.organization_id,
                    command.mailbox_id,
                    context.principal_id,
                    draft_id,
                    data_plane_id,
                )
                .await
                .map_err(map_store_error)?
                .ok_or(DraftError::NotFound)?;
            if previous.status != MailDraftStatus::Active
                || previous.version != expected_version
                || previous.data_plane_id != data_plane_id
                || previous.engine_account_ref != binding.account_ref
                || previous.reply_to_message_id != command.reply_to_message_id
            {
                return Err(DraftError::Conflict);
            }
            if previous.content_sha256 == content_sha256 {
                return Ok(SavedTextDraft {
                    draft_id,
                    version: previous.version,
                    reply_to_message_id: previous.reply_to_message_id,
                    previous_draft_destroyed: true,
                });
            }
            Some(previous)
        } else {
            None
        };
        let draft_id = previous
            .as_ref()
            .map_or_else(DraftId::new, |previous| previous.draft_id);
        let engine_draft = self
            .drafts
            .save_text_draft(
                &binding.account_ref,
                previous
                    .as_ref()
                    .map(|previous| previous.engine_email_ref.as_str()),
                &TextDraftInput {
                    from: mailbox.primary_address.to_string(),
                    to: input.to,
                    cc: input.cc,
                    bcc: input.bcc,
                    subject: input.subject,
                    text_body: input.text_body,
                    reply_to_email_id: reply_target
                        .as_ref()
                        .map(|target| target.engine_document_ref.clone()),
                    automatic,
                },
            )
            .await
            .map_err(map_engine_error)?;
        let record = self
            .store
            .record_mail_draft(MailDraftWrite {
                draft_id,
                organization_id: context.organization_id,
                mailbox_id: command.mailbox_id,
                created_by: context.principal_id,
                data_plane_id,
                engine_account_ref: binding.account_ref,
                engine_email_ref: engine_draft.id,
                engine_thread_ref: engine_draft.thread_id,
                reply_to_message_id: command.reply_to_message_id,
                reply_to_automatic: engine_draft.reply_source_automatic,
                body_ref,
                content_sha256,
                recipients,
                expected_version: previous.map(|previous| previous.version),
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;
        Ok(SavedTextDraft {
            draft_id: record.draft_id,
            version: record.version,
            reply_to_message_id: record.reply_to_message_id,
            previous_draft_destroyed: engine_draft.previous_draft_destroyed,
        })
    }
}

#[derive(Debug)]
pub(crate) struct NormalizedDraftInput {
    pub(crate) to: Vec<String>,
    pub(crate) cc: Vec<String>,
    pub(crate) bcc: Vec<String>,
    pub(crate) subject: String,
    pub(crate) text_body: String,
}

pub(crate) fn normalized_input(
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    subject: String,
    text_body: String,
) -> Result<NormalizedDraftInput, DraftError> {
    if subject.len() > 998
        || subject.chars().any(char::is_control)
        || text_body.len() > 256 * 1024
        || text_body.contains('\0')
        || to.len() + cc.len() + bcc.len() == 0
        || to.len() + cc.len() + bcc.len() > 100
    {
        return Err(DraftError::InvalidRequest);
    }
    let normalize = |values: Vec<String>| {
        values
            .into_iter()
            .map(|value| {
                MailboxAddress::parse(value)
                    .map(|address| address.to_string())
                    .map_err(|_| DraftError::InvalidRequest)
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let normalized = NormalizedDraftInput {
        to: normalize(to)?,
        cc: normalize(cc)?,
        bcc: normalize(bcc)?,
        subject,
        text_body,
    };
    let mut unique = std::collections::BTreeSet::new();
    if normalized
        .to
        .iter()
        .chain(&normalized.cc)
        .chain(&normalized.bcc)
        .any(|address| !unique.insert(address))
    {
        return Err(DraftError::InvalidRequest);
    }
    Ok(normalized)
}

pub(crate) fn recipient_evidence(
    context: &AuthorizationContext,
    input: &NormalizedDraftInput,
) -> Vec<MailDraftRecipientEvidence> {
    let mut recipients = input
        .to
        .iter()
        .chain(&input.cc)
        .chain(&input.bcc)
        .map(|address| {
            let mut digest = Sha256::new();
            digest.update(b"copilotmail.agent.recipient.v1");
            digest.update(context.organization_id.as_uuid().as_bytes());
            digest.update(context.principal_id.as_uuid().as_bytes());
            digest.update(address.as_bytes());
            MailDraftRecipientEvidence {
                recipient_ref: digest.finalize().into(),
                domain: address
                    .rsplit_once('@')
                    .expect("normalized mailbox address has a domain")
                    .1
                    .to_owned(),
            }
        })
        .collect::<Vec<_>>();
    recipients.sort_by_key(|recipient| recipient.recipient_ref);
    recipients
}

pub(crate) fn draft_digest(
    input: &NormalizedDraftInput,
    reply_to_message_id: Option<uuid::Uuid>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    match reply_to_message_id {
        Some(message_id) => {
            digest.update([1]);
            digest.update(message_id.as_bytes());
        }
        None => digest.update([0]),
    }
    for (label, values) in [
        (b"to".as_slice(), &input.to),
        (b"cc".as_slice(), &input.cc),
        (b"bcc".as_slice(), &input.bcc),
    ] {
        digest.update((label.len() as u64).to_be_bytes());
        digest.update(label);
        digest.update((values.len() as u64).to_be_bytes());
        for value in values {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
    }
    for value in [&input.subject, &input.text_body] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.finalize().into()
}

pub(crate) fn body_reference(context: &AuthorizationContext, text_body: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.agent.body-reference.v1");
    digest.update(context.organization_id.as_uuid().as_bytes());
    digest.update(context.principal_id.as_uuid().as_bytes());
    digest.update((text_body.len() as u64).to_be_bytes());
    digest.update(text_body.as_bytes());
    digest.finalize().into()
}

const fn map_store_error(error: StoreError) -> DraftError {
    match error {
        StoreError::Conflict => DraftError::Conflict,
        StoreError::MissingReference => DraftError::NotFound,
        StoreError::AgentMailAdmission(_) => DraftError::InvalidRequest,
        StoreError::AuthenticationInvalid | StoreError::Unavailable => DraftError::Unavailable,
    }
}

const fn map_engine_error(error: MailEngineError) -> DraftError {
    match error {
        MailEngineError::InvalidConfiguration => DraftError::InvalidRequest,
        MailEngineError::CredentialUnavailable
        | MailEngineError::Unavailable
        | MailEngineError::InvalidSession
        | MailEngineError::InvalidResponse
        | MailEngineError::Rejected
        | MailEngineError::ProjectionConflict => DraftError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail_ports::MailDraft;
    use crate::{MailDraftRecord, MailEngineMailboxBinding, MailReplyTarget, MailboxSummary};
    use async_trait::async_trait;
    use copilotmail_domain::{
        AgentEnvironment, DelegationId, MailboxAccessSnapshot, MailboxAddress, MailboxKind,
        OrganizationId, PrincipalId, ResolvedMailboxDelegation,
    };
    use std::{collections::BTreeSet, sync::Mutex};

    struct Store {
        mail_submission_allowed: bool,
        agent_environment: Option<AgentEnvironment>,
        snapshot: Option<MailboxAccessSnapshot>,
        mailbox: Option<MailboxSummary>,
        binding: Option<MailEngineMailboxBinding>,
        reply_target: Option<MailReplyTarget>,
        draft: Option<MailDraftRecord>,
        writes: Arc<Mutex<Vec<MailDraftWrite>>>,
    }

    #[async_trait]
    impl MailboxAuthorizationStore for Store {
        async fn principal_can_submit_mail(
            &self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(self.mail_submission_allowed)
        }

        async fn active_agent_environment(
            &self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<Option<AgentEnvironment>, StoreError> {
            Ok(self.agent_environment)
        }

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
            Ok(self.mailbox.clone())
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
    impl MailDraftMetadataStore for Store {
        async fn get_mail_draft(
            &self,
            _organization_id: OrganizationId,
            _mailbox_id: MailboxId,
            _created_by: PrincipalId,
            _draft_id: DraftId,
            _expected_data_plane_id: copilotmail_domain::MailDataPlaneId,
        ) -> Result<Option<MailDraftRecord>, StoreError> {
            Ok(self.draft.clone())
        }

        async fn record_mail_draft(
            &self,
            write: MailDraftWrite,
        ) -> Result<MailDraftRecord, StoreError> {
            let version = write.expected_version.map_or(1, |value| value + 1);
            self.writes.lock().unwrap().push(write.clone());
            Ok(MailDraftRecord {
                draft_id: write.draft_id,
                organization_id: write.organization_id,
                mailbox_id: write.mailbox_id,
                created_by: write.created_by,
                data_plane_id: write.data_plane_id,
                engine_account_ref: write.engine_account_ref,
                engine_email_ref: write.engine_email_ref,
                engine_thread_ref: write.engine_thread_ref,
                reply_to_message_id: write.reply_to_message_id,
                reply_to_automatic: write.reply_to_automatic,
                body_ref: write.body_ref,
                content_sha256: write.content_sha256,
                recipients: write.recipients,
                version,
                status: MailDraftStatus::Active,
                created_at: write.now,
                updated_at: write.now,
            })
        }
    }

    #[async_trait]
    impl MailReplyTargetQueryStore for Store {
        async fn resolve_mail_reply_target(
            &self,
            _organization_id: OrganizationId,
            _mailbox_id: MailboxId,
            _message_id: uuid::Uuid,
            _expected_data_plane_id: copilotmail_domain::MailDataPlaneId,
        ) -> Result<Option<MailReplyTarget>, StoreError> {
            Ok(self.reply_target.clone())
        }
    }

    struct Drafts {
        data_plane_id: copilotmail_domain::MailDataPlaneId,
        calls: Mutex<Vec<(String, Option<String>, TextDraftInput)>>,
    }

    impl Default for Drafts {
        fn default() -> Self {
            Self {
                data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl crate::mail_ports::MailDataPlanePort for Drafts {
        fn data_plane_id(&self) -> copilotmail_domain::MailDataPlaneId {
            self.data_plane_id
        }
    }

    #[async_trait]
    impl MailDraftStore for Drafts {
        async fn probe_drafts(&self) -> Result<(), MailEngineError> {
            Ok(())
        }

        async fn save_text_draft(
            &self,
            account_ref: &str,
            previous_draft_id: Option<&str>,
            input: &TextDraftInput,
        ) -> Result<MailDraft, MailEngineError> {
            self.calls.lock().unwrap().push((
                account_ref.into(),
                previous_draft_id.map(str::to_owned),
                input.clone(),
            ));
            Ok(MailDraft {
                id: "draft-2".into(),
                thread_id: "thread-1".into(),
                reply_source_automatic: input.reply_to_email_id.is_some(),
                previous_draft_destroyed: true,
            })
        }
    }

    fn command(mailbox_id: MailboxId, now: DateTime<Utc>) -> SaveTextDraftCommand {
        SaveTextDraftCommand {
            mailbox_id,
            draft_id: None,
            expected_version: None,
            reply_to_message_id: None,
            to: vec!["recipient@example.test".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Status".into(),
            text_body: "Ready.".into(),
            now,
        }
    }

    #[tokio::test]
    async fn sender_and_agent_auto_submitted_marker_are_server_derived() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let now = Utc::now();
        let drafts = Arc::new(Drafts::default());
        let writes = Arc::new(Mutex::new(Vec::new()));
        let service = DraftService::new(
            Store {
                mail_submission_allowed: true,
                agent_environment: Some(AgentEnvironment::Testing),
                snapshot: Some(MailboxAccessSnapshot {
                    organization_id,
                    mailbox_id,
                    membership: None,
                    delegations: vec![ResolvedMailboxDelegation {
                        delegation_id: DelegationId::new(),
                        grantor_principal_id: PrincipalId::new(),
                        capabilities: [MailboxCapability::Send].into_iter().collect(),
                    }],
                }),
                mailbox: Some(MailboxSummary {
                    mailbox_id,
                    organization_id,
                    kind: MailboxKind::Human,
                    primary_address: MailboxAddress::parse("owner@example.test").unwrap(),
                    created_at: now,
                }),
                binding: Some(MailEngineMailboxBinding {
                    organization_id,
                    mailbox_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    account_ref: "account-42".into(),
                    version: 1,
                    updated_at: now,
                }),
                reply_target: None,
                draft: None,
                writes: writes.clone(),
            },
            drafts.clone(),
        );
        service
            .save(
                &AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: [Permission::MailboxUse].into_iter().collect(),
                    authentication: None,
                },
                command(mailbox_id, now),
            )
            .await
            .unwrap();

        let calls = drafts.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "account-42");
        assert_eq!(calls[0].1, None);
        assert_eq!(calls[0].2.from, "owner@example.test");
        assert!(calls[0].2.automatic);
        let writes = writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].recipients.len(), 1);
        assert_eq!(writes[0].recipients[0].domain, "example.test");
    }

    #[test]
    fn duplicate_recipients_across_headers_are_rejected() {
        assert!(matches!(
            normalized_input(
                vec!["same@example.test".into()],
                vec!["SAME@example.test".into()],
                Vec::new(),
                "subject".into(),
                "body".into(),
            ),
            Err(DraftError::InvalidRequest)
        ));
    }

    #[test]
    fn duplicate_body_reference_is_scoped_to_the_exact_tenant_and_principal() {
        let organization_id = OrganizationId::new();
        let context = AuthorizationContext {
            principal_id: PrincipalId::new(),
            organization_id,
            permissions: BTreeSet::new(),
            authentication: None,
        };
        let same = context.clone();
        let other_principal = AuthorizationContext {
            principal_id: PrincipalId::new(),
            organization_id,
            permissions: BTreeSet::new(),
            authentication: None,
        };
        assert_eq!(
            body_reference(&context, "same body"),
            body_reference(&same, "same body")
        );
        assert_ne!(
            body_reference(&context, "same body"),
            body_reference(&other_principal, "same body")
        );
        assert_ne!(
            body_reference(&context, "same body"),
            body_reference(&context, "different body")
        );
    }

    #[tokio::test]
    async fn missing_send_capability_never_calls_the_draft_adapter() {
        let organization_id = OrganizationId::new();
        let mailbox_id = MailboxId::new();
        let now = Utc::now();
        let drafts = Arc::new(Drafts::default());
        let service = DraftService::new(
            Store {
                mail_submission_allowed: true,
                agent_environment: None,
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
                mailbox: None,
                binding: None,
                reply_target: None,
                draft: None,
                writes: Arc::new(Mutex::new(Vec::new())),
            },
            drafts.clone(),
        );
        let result = service
            .save(
                &AuthorizationContext {
                    principal_id: PrincipalId::new(),
                    organization_id,
                    permissions: [Permission::MailboxUse]
                        .into_iter()
                        .collect::<BTreeSet<_>>(),
                    authentication: None,
                },
                command(mailbox_id, now),
            )
            .await;

        assert_eq!(result, Err(DraftError::NotFound));
        assert!(drafts.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn replacement_uses_only_the_persisted_remote_reference() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let draft_id = DraftId::new();
        let now = Utc::now();
        let drafts = Arc::new(Drafts::default());
        let writes = Arc::new(Mutex::new(Vec::new()));
        let service = DraftService::new(
            Store {
                mail_submission_allowed: true,
                agent_environment: None,
                snapshot: Some(MailboxAccessSnapshot {
                    organization_id,
                    mailbox_id,
                    membership: None,
                    delegations: vec![ResolvedMailboxDelegation {
                        delegation_id: DelegationId::new(),
                        grantor_principal_id: PrincipalId::new(),
                        capabilities: [MailboxCapability::Send].into_iter().collect(),
                    }],
                }),
                mailbox: Some(MailboxSummary {
                    mailbox_id,
                    organization_id,
                    kind: MailboxKind::Human,
                    primary_address: MailboxAddress::parse("owner@example.test").unwrap(),
                    created_at: now,
                }),
                binding: Some(MailEngineMailboxBinding {
                    organization_id,
                    mailbox_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    account_ref: "account-42".into(),
                    version: 1,
                    updated_at: now,
                }),
                reply_target: None,
                draft: Some(MailDraftRecord {
                    draft_id,
                    organization_id,
                    mailbox_id,
                    created_by: principal_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    engine_account_ref: "account-42".into(),
                    engine_email_ref: "engine-draft-secret".into(),
                    engine_thread_ref: "engine-thread-secret".into(),
                    reply_to_message_id: None,
                    reply_to_automatic: false,
                    body_ref: [6; 32],
                    content_sha256: [7; 32],
                    recipients: vec![MailDraftRecipientEvidence {
                        recipient_ref: [8; 32],
                        domain: "example.test".into(),
                    }],
                    version: 4,
                    status: MailDraftStatus::Active,
                    created_at: now,
                    updated_at: now,
                }),
                writes: writes.clone(),
            },
            drafts.clone(),
        );
        let mut update = command(mailbox_id, now);
        update.draft_id = Some(draft_id);
        update.expected_version = Some(4);
        let saved = service
            .save(
                &AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: [Permission::MailboxUse].into_iter().collect(),
                    authentication: None,
                },
                update,
            )
            .await
            .unwrap();

        assert_eq!(saved.draft_id, draft_id);
        assert_eq!(saved.version, 5);
        assert_eq!(
            drafts.calls.lock().unwrap()[0].1.as_deref(),
            Some("engine-draft-secret")
        );
        assert_eq!(writes.lock().unwrap()[0].expected_version, Some(4));
    }

    #[tokio::test]
    async fn explicit_reply_forwards_only_the_server_resolved_engine_target() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let message_id = uuid::Uuid::now_v7();
        let now = Utc::now();
        let drafts = Arc::new(Drafts::default());
        let writes = Arc::new(Mutex::new(Vec::new()));
        let service = DraftService::new(
            Store {
                mail_submission_allowed: true,
                agent_environment: None,
                snapshot: Some(MailboxAccessSnapshot {
                    organization_id,
                    mailbox_id,
                    membership: None,
                    delegations: vec![ResolvedMailboxDelegation {
                        delegation_id: DelegationId::new(),
                        grantor_principal_id: PrincipalId::new(),
                        capabilities: [MailboxCapability::Read, MailboxCapability::Send]
                            .into_iter()
                            .collect(),
                    }],
                }),
                mailbox: Some(MailboxSummary {
                    mailbox_id,
                    organization_id,
                    kind: MailboxKind::Human,
                    primary_address: MailboxAddress::parse("owner@example.test").unwrap(),
                    created_at: now,
                }),
                binding: Some(MailEngineMailboxBinding {
                    organization_id,
                    mailbox_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    account_ref: "account-42".into(),
                    version: 1,
                    updated_at: now,
                }),
                reply_target: Some(MailReplyTarget {
                    organization_id,
                    mailbox_id,
                    message_id,
                    data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
                    engine_account_ref: "account-42".into(),
                    engine_document_ref: "opaque-source-email".into(),
                }),
                draft: None,
                writes: writes.clone(),
            },
            drafts.clone(),
        );
        let mut reply = command(mailbox_id, now);
        reply.reply_to_message_id = Some(message_id);
        let saved = service
            .save(
                &AuthorizationContext {
                    principal_id,
                    organization_id,
                    permissions: [Permission::MailboxUse].into_iter().collect(),
                    authentication: None,
                },
                reply,
            )
            .await
            .unwrap();

        assert_eq!(saved.reply_to_message_id, Some(message_id));
        assert_eq!(
            drafts.calls.lock().unwrap()[0]
                .2
                .reply_to_email_id
                .as_deref(),
            Some("opaque-source-email")
        );
        assert_eq!(
            writes.lock().unwrap()[0].reply_to_message_id,
            Some(message_id)
        );
        assert!(writes.lock().unwrap()[0].reply_to_automatic);
    }

    #[tokio::test]
    async fn ineligible_principal_fails_before_mail_data_plane_access() {
        let organization_id = OrganizationId::new();
        let mailbox_id = MailboxId::new();
        let now = Utc::now();
        let drafts = Arc::new(Drafts::default());
        let result = DraftService::new(
            Store {
                mail_submission_allowed: false,
                agent_environment: None,
                snapshot: None,
                mailbox: None,
                binding: None,
                reply_target: None,
                draft: None,
                writes: Arc::new(Mutex::new(Vec::new())),
            },
            drafts.clone(),
        )
        .save(
            &AuthorizationContext {
                principal_id: PrincipalId::new(),
                organization_id,
                permissions: [Permission::MailboxUse].into_iter().collect(),
                authentication: None,
            },
            command(mailbox_id, now),
        )
        .await;
        assert_eq!(result, Err(DraftError::NotFound));
        assert!(drafts.calls.lock().unwrap().is_empty());
    }
}
