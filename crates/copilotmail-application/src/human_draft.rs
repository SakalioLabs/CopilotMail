use crate::draft::{body_reference, draft_digest, normalized_input, recipient_evidence};
use crate::mail_ports::{MailDraftContentReader, MailEngineError};
use crate::{
    ControlPlaneStore, HumanDraftAttemptListQuery, HumanDraftAttemptListRecheck,
    HumanDraftListItemRecheck, HumanDraftListQuery, HumanDraftListRecheck, HumanDraftQueryStore,
    HumanDraftReadRecheck, HumanDraftRestoreQuery, MailDraftStatus, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext,
    DraftCreationAttemptId, DraftId, MailboxCapability, MailboxId, Permission,
};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

const MIN_LIMIT: u16 = 1;
const MAX_LIMIT: u16 = 50;
const CURSOR_PREFIX: &str = "v1.";

#[derive(Debug, Clone)]
pub struct ListHumanDraftsCommand {
    pub context: AuthorizationContext,
    pub mailbox_id: MailboxId,
    pub limit: u16,
    pub cursor: Option<String>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RestoreHumanDraftCommand {
    pub context: AuthorizationContext,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub expected_version: i64,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Content-free list projection safe for the Human browser contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftListItem {
    pub draft_id: DraftId,
    pub version: i64,
    pub reply_to_message_id: Option<Uuid>,
    pub recipient_count: u16,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftPage {
    pub items: Vec<HumanDraftListItem>,
    pub next_cursor: Option<String>,
}

/// Exact active draft content returned only after the final authorization,
/// Audit, and Outbox transaction commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredHumanDraft {
    pub draft_id: DraftId,
    pub version: i64,
    pub reply_to_message_id: Option<Uuid>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: String,
}

#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum HumanDraftError {
    #[error("Human draft access is forbidden")]
    Forbidden,
    #[error("the mailbox or draft is unavailable or inaccessible")]
    NotFound,
    #[error("the Human draft request is invalid")]
    InvalidRequest,
    #[error("Human draft page size must be between 1 and 50")]
    InvalidPageSize,
    #[error("the Human draft cursor is invalid or no longer visible")]
    InvalidCursor,
    #[error("the draft changed or is no longer editable")]
    Conflict,
    #[error("Human draft state or content is temporarily unavailable")]
    Unavailable,
}

pub struct HumanDraftService<S> {
    store: S,
    reader: Arc<dyn MailDraftContentReader>,
}

impl<S> HumanDraftService<S>
where
    S: HumanDraftQueryStore + ControlPlaneStore,
{
    #[must_use]
    pub const fn new(store: S, reader: Arc<dyn MailDraftContentReader>) -> Self {
        Self { store, reader }
    }

    pub async fn list(
        &self,
        command: ListHumanDraftsCommand,
    ) -> Result<HumanDraftPage, HumanDraftError> {
        require_mailbox_use(&command.context)?;
        if !(MIN_LIMIT..=MAX_LIMIT).contains(&command.limit) {
            return Err(HumanDraftError::InvalidPageSize);
        }
        let after_draft_id = command.cursor.as_deref().map(parse_cursor).transpose()?;
        let data_plane_id = self.reader.data_plane_id();
        let batch = self
            .store
            .list_human_drafts(&HumanDraftListQuery {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                mailbox_id: command.mailbox_id,
                expected_data_plane_id: data_plane_id,
                after_draft_id,
                limit: command.limit,
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;

        if !batch.active_human {
            return Err(HumanDraftError::Forbidden);
        }
        if after_draft_id.is_some() && !batch.cursor_valid {
            return Err(HumanDraftError::InvalidCursor);
        }
        if batch.candidates.len() > usize::from(command.limit)
            || (batch.has_more && batch.candidates.is_empty())
        {
            return Err(HumanDraftError::Unavailable);
        }
        let authorization = batch.authorization.ok_or(HumanDraftError::NotFound)?;
        if !can_read_and_send(
            &authorization,
            command.context.organization_id,
            command.mailbox_id,
        ) {
            return Err(HumanDraftError::NotFound);
        }
        let binding = batch.binding.ok_or(HumanDraftError::NotFound)?;
        if binding.organization_id != command.context.organization_id
            || binding.mailbox_id != command.mailbox_id
            || binding.data_plane_id != data_plane_id
            || !valid_provider_ref(&binding.account_ref)
        {
            return Err(HumanDraftError::Unavailable);
        }

        let mut previous = after_draft_id;
        let mut items = Vec::with_capacity(batch.candidates.len());
        for candidate in batch.candidates {
            if candidate.organization_id != command.context.organization_id
                || candidate.mailbox_id != command.mailbox_id
                || candidate.created_by != command.context.principal_id
                || candidate.data_plane_id != data_plane_id
                || candidate.status != MailDraftStatus::Active
                || candidate.version <= 0
                || candidate.draft_id.as_uuid().is_nil()
                || candidate.updated_at < candidate.created_at
                || candidate
                    .reply_to_message_id
                    .is_some_and(|message_id| message_id.is_nil())
                || !(1..=100).contains(&candidate.recipient_count)
                || previous.is_some_and(|cursor| candidate.draft_id >= cursor)
            {
                return Err(HumanDraftError::Unavailable);
            }
            previous = Some(candidate.draft_id);
            items.push(HumanDraftListItem {
                draft_id: candidate.draft_id,
                version: candidate.version,
                reply_to_message_id: candidate.reply_to_message_id,
                recipient_count: candidate.recipient_count,
                created_at: candidate.created_at,
                updated_at: candidate.updated_at,
            });
        }
        let next_cursor = if batch.has_more {
            items.last().map(|item| format_cursor(item.draft_id))
        } else {
            None
        };
        let page = HumanDraftPage { items, next_cursor };
        let expected_drafts = page
            .items
            .iter()
            .map(|item| HumanDraftListItemRecheck {
                draft_id: item.draft_id,
                expected_version: item.version,
            })
            .collect();
        self.record_list_evidence(
            &command,
            &HumanDraftListRecheck {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                mailbox_id: command.mailbox_id,
                expected_data_plane_id: data_plane_id,
                expected_engine_account_ref: binding.account_ref,
                expected_drafts,
            },
            &page,
        )
        .await?;
        Ok(page)
    }

    pub async fn restore(
        &self,
        command: RestoreHumanDraftCommand,
    ) -> Result<RestoredHumanDraft, HumanDraftError> {
        require_mailbox_use(&command.context)?;
        if command.expected_version <= 0 || command.draft_id.as_uuid().is_nil() {
            return Err(HumanDraftError::InvalidRequest);
        }
        let data_plane_id = self.reader.data_plane_id();
        let resolution = self
            .store
            .resolve_human_draft_restore_target(&HumanDraftRestoreQuery {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                mailbox_id: command.mailbox_id,
                draft_id: command.draft_id,
                expected_data_plane_id: data_plane_id,
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;
        if !resolution.active_human {
            return Err(HumanDraftError::Forbidden);
        }
        let target = resolution.target.ok_or(HumanDraftError::NotFound)?;
        if !can_read_and_send(
            &target.authorization,
            command.context.organization_id,
            command.mailbox_id,
        ) {
            return Err(HumanDraftError::NotFound);
        }
        if target.mailbox.organization_id != command.context.organization_id
            || target.mailbox.mailbox_id != command.mailbox_id
            || target.binding.organization_id != command.context.organization_id
            || target.binding.mailbox_id != command.mailbox_id
            || target.binding.data_plane_id != data_plane_id
            || target.draft.draft_id != command.draft_id
            || target.draft.organization_id != command.context.organization_id
            || target.draft.mailbox_id != command.mailbox_id
            || target.draft.created_by != command.context.principal_id
            || target.draft.data_plane_id != data_plane_id
            || target.draft.engine_account_ref != target.binding.account_ref
            || !valid_provider_ref(&target.binding.account_ref)
            || !valid_provider_ref(&target.draft.engine_email_ref)
            || target.draft.updated_at < target.draft.created_at
        {
            return Err(HumanDraftError::Unavailable);
        }
        if target.draft.status != MailDraftStatus::Active
            || target.draft.version != command.expected_version
        {
            return Err(HumanDraftError::Conflict);
        }

        // External I/O deliberately happens without a PostgreSQL transaction
        // or lock. Authorization and the exact target are re-proved below.
        let remote = self
            .reader
            .read_exact_text_draft(
                &target.binding.account_ref,
                &target.draft.engine_email_ref,
                &target.draft.engine_thread_ref,
                &target.mailbox.primary_address,
            )
            .await
            .map_err(map_reader_error)?
            .ok_or(HumanDraftError::Conflict)?;
        let normalized = normalized_input(
            remote.to,
            remote.cc,
            remote.bcc,
            remote.subject,
            remote.text_body,
        )
        .map_err(|_| HumanDraftError::Conflict)?;
        let content_sha256 = draft_digest(&normalized, target.draft.reply_to_message_id);
        let body_ref = body_reference(&command.context, &normalized.text_body);
        let recipients = recipient_evidence(&command.context, &normalized);
        if content_sha256 != target.draft.content_sha256
            || body_ref != target.draft.body_ref
            || recipients != target.draft.recipients
        {
            return Err(HumanDraftError::Conflict);
        }

        let proof = HumanDraftReadRecheck {
            organization_id: command.context.organization_id,
            principal_id: command.context.principal_id,
            mailbox_id: command.mailbox_id,
            draft_id: command.draft_id,
            expected_version: command.expected_version,
            expected_data_plane_id: data_plane_id,
            expected_engine_account_ref: target.binding.account_ref,
            expected_engine_email_ref: target.draft.engine_email_ref,
            expected_engine_thread_ref: target.draft.engine_thread_ref,
            expected_primary_address: target.mailbox.primary_address,
            expected_body_ref: body_ref,
            expected_content_sha256: content_sha256,
            expected_recipients: recipients.clone(),
            expected_reply_to_message_id: target.draft.reply_to_message_id,
        };
        self.record_read_evidence(
            &command,
            &proof,
            normalized.text_body.len(),
            recipients.len(),
        )
        .await?;

        Ok(RestoredHumanDraft {
            draft_id: command.draft_id,
            version: command.expected_version,
            reply_to_message_id: proof.expected_reply_to_message_id,
            to: normalized.to,
            cc: normalized.cc,
            bcc: normalized.bcc,
            subject: normalized.subject,
            text_body: normalized.text_body,
        })
    }

    async fn record_list_evidence(
        &self,
        command: &ListHumanDraftsCommand,
        proof: &HumanDraftListRecheck,
        page: &HumanDraftPage,
    ) -> Result<(), HumanDraftError> {
        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        if !transaction
            .human_draft_list_still_authorized(proof)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?
        {
            return Err(HumanDraftError::Conflict);
        }
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail.drafts.queried",
            AuditResource {
                kind: "mailbox".into(),
                id: command.mailbox_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "page_size": command.limit,
                "result_count": page.items.len(),
                "has_more": page.next_cursor.is_some(),
            }),
            command.now,
        )
        .and_then(|event| event.correlate_human_session(command.context.authentication.as_ref()))
        .map_err(|_| HumanDraftError::Unavailable)?;
        let event = PendingEvent::new(
            "mail.drafts.queried.v1",
            "mailbox",
            command.mailbox_id.to_string(),
            json!({
                "organization_id": command.context.organization_id,
                "mailbox_id": command.mailbox_id,
                "result_count": page.items.len(),
                "has_more": page.next_cursor.is_some(),
            }),
            command.now,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| HumanDraftError::Unavailable)
    }

    async fn record_read_evidence(
        &self,
        command: &RestoreHumanDraftCommand,
        proof: &HumanDraftReadRecheck,
        text_body_bytes: usize,
        recipient_count: usize,
    ) -> Result<(), HumanDraftError> {
        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        if !transaction
            .human_draft_read_still_authorized(proof)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?
        {
            return Err(HumanDraftError::Conflict);
        }
        let is_reply = proof.expected_reply_to_message_id.is_some();
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail.draft.read",
            AuditResource {
                kind: "mail.draft".into(),
                id: command.draft_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "mailbox_id": command.mailbox_id,
                "draft_id": command.draft_id,
                "draft_version": command.expected_version,
                "text_body_bytes": text_body_bytes,
                "recipient_count": recipient_count,
                "is_reply": is_reply,
            }),
            command.now,
        )
        .and_then(|event| event.correlate_human_session(command.context.authentication.as_ref()))
        .map_err(|_| HumanDraftError::Unavailable)?;
        let event = PendingEvent::new(
            "mail.draft.read.v1",
            "mail.draft",
            command.draft_id.to_string(),
            json!({
                "organization_id": command.context.organization_id,
                "mailbox_id": command.mailbox_id,
                "draft_id": command.draft_id,
                "draft_version": command.expected_version,
                "text_body_bytes": text_body_bytes,
                "recipient_count": recipient_count,
                "is_reply": is_reply,
            }),
            command.now,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| HumanDraftError::Unavailable)
    }
}

/// Command for the owner-private unreconciled draft-attempt list.
///
/// The list is scoped to the presenting principal only. It is the local
/// orphan-detection surface for B-10: every row proves a remote draft write
/// was attempted while the matching local record never committed.
#[derive(Debug, Clone)]
pub struct ListHumanDraftAttemptsCommand {
    pub context: AuthorizationContext,
    pub limit: u16,
    pub cursor: Option<String>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Content-free attempt-list projection safe for the Human browser contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftAttemptListItem {
    pub attempt_id: DraftCreationAttemptId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub attempted_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftAttemptPage {
    pub items: Vec<HumanDraftAttemptListItem>,
    pub next_cursor: Option<String>,
}

/// Owner-private draft-attempt queries. This surface deliberately keeps no
/// remote content reader: detection must stay available while the mail
/// engine is unreachable, which is exactly when unreconciled attempts occur.
pub struct HumanDraftAttemptService<S> {
    store: S,
}

impl<S> HumanDraftAttemptService<S>
where
    S: HumanDraftQueryStore + ControlPlaneStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Lists the caller's own unreconciled draft-creation attempts, newest
    /// first. Rows are disclosed only after the exact authorization/result
    /// recheck, the dedicated audit event, and the Outbox record commit in
    /// one transaction.
    pub async fn list(
        &self,
        command: ListHumanDraftAttemptsCommand,
    ) -> Result<HumanDraftAttemptPage, HumanDraftError> {
        require_mailbox_use(&command.context)?;
        if !(MIN_LIMIT..=MAX_LIMIT).contains(&command.limit) {
            return Err(HumanDraftError::InvalidPageSize);
        }
        let after_attempt_id = command
            .cursor
            .as_deref()
            .map(parse_attempt_cursor)
            .transpose()?;
        let batch = self
            .store
            .list_human_draft_attempts(&HumanDraftAttemptListQuery {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                after_attempt_id,
                limit: command.limit,
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;

        if !batch.active_human {
            return Err(HumanDraftError::Forbidden);
        }
        if after_attempt_id.is_some() && !batch.cursor_valid {
            return Err(HumanDraftError::InvalidCursor);
        }
        if batch.candidates.len() > usize::from(command.limit)
            || (batch.has_more && batch.candidates.is_empty())
        {
            return Err(HumanDraftError::Unavailable);
        }

        let mut previous = after_attempt_id;
        let mut items = Vec::with_capacity(batch.candidates.len());
        for candidate in batch.candidates {
            if candidate.organization_id != command.context.organization_id
                || candidate.created_by != command.context.principal_id
                || candidate.attempt_id.as_uuid().is_nil()
                || candidate.mailbox_id.as_uuid().is_nil()
                || candidate.draft_id.as_uuid().is_nil()
                || previous.is_some_and(|cursor| candidate.attempt_id >= cursor)
            {
                return Err(HumanDraftError::Unavailable);
            }
            previous = Some(candidate.attempt_id);
            items.push(HumanDraftAttemptListItem {
                attempt_id: candidate.attempt_id,
                mailbox_id: candidate.mailbox_id,
                draft_id: candidate.draft_id,
                attempted_at: candidate.attempted_at,
            });
        }
        let next_cursor = if batch.has_more {
            items
                .last()
                .map(|item| format_attempt_cursor(item.attempt_id))
        } else {
            None
        };
        let page = HumanDraftAttemptPage { items, next_cursor };
        let proof = HumanDraftAttemptListRecheck {
            organization_id: command.context.organization_id,
            principal_id: command.context.principal_id,
            expected_attempt_ids: page.items.iter().map(|item| item.attempt_id).collect(),
        };
        self.record_attempt_list_evidence(&command, &proof, &page)
            .await?;
        Ok(page)
    }

    async fn record_attempt_list_evidence(
        &self,
        command: &ListHumanDraftAttemptsCommand,
        proof: &HumanDraftAttemptListRecheck,
        page: &HumanDraftAttemptPage,
    ) -> Result<(), HumanDraftError> {
        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        if !transaction
            .human_draft_attempt_list_still_authorized(proof)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?
        {
            return Err(HumanDraftError::Conflict);
        }
        let audit = AuditEvent::new(
            command.context.organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail.draft_attempts.queried",
            AuditResource {
                kind: "mail.draft_attempt_query".into(),
                id: command.context.principal_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "page_size": command.limit,
                "result_count": page.items.len(),
                "has_more": page.next_cursor.is_some(),
            }),
            command.now,
        )
        .and_then(|event| event.correlate_human_session(command.context.authentication.as_ref()))
        .map_err(|_| HumanDraftError::Unavailable)?;
        let event = PendingEvent::new(
            "mail.draft_attempts.queried.v1",
            "mail.draft_attempt_query",
            command.context.principal_id.to_string(),
            json!({
                "organization_id": command.context.organization_id,
                "principal_id": command.context.principal_id,
                "result_count": page.items.len(),
                "has_more": page.next_cursor.is_some(),
            }),
            command.now,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(|_| HumanDraftError::Unavailable)?;
        transaction
            .commit()
            .await
            .map_err(|_| HumanDraftError::Unavailable)
    }
}

fn parse_attempt_cursor(value: &str) -> Result<DraftCreationAttemptId, HumanDraftError> {
    let raw = value
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(HumanDraftError::InvalidCursor)?;
    let parsed = Uuid::parse_str(raw).map_err(|_| HumanDraftError::InvalidCursor)?;
    if parsed.is_nil() || raw != parsed.to_string() {
        return Err(HumanDraftError::InvalidCursor);
    }
    Ok(DraftCreationAttemptId::from_uuid(parsed))
}

fn format_attempt_cursor(attempt_id: DraftCreationAttemptId) -> String {
    format!("{CURSOR_PREFIX}{attempt_id}")
}

fn require_mailbox_use(context: &AuthorizationContext) -> Result<(), HumanDraftError> {
    if context.permits(context.organization_id, Permission::MailboxUse) {
        Ok(())
    } else {
        Err(HumanDraftError::Forbidden)
    }
}

fn can_read_and_send(
    authorization: &copilotmail_domain::MailboxAccessSnapshot,
    organization_id: copilotmail_domain::OrganizationId,
    mailbox_id: MailboxId,
) -> bool {
    authorization
        .authorize(organization_id, mailbox_id, MailboxCapability::Read, None)
        .is_some()
        && authorization
            .authorize(organization_id, mailbox_id, MailboxCapability::Send, None)
            .is_some()
}

fn parse_cursor(value: &str) -> Result<DraftId, HumanDraftError> {
    let raw = value
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(HumanDraftError::InvalidCursor)?;
    let parsed = Uuid::parse_str(raw).map_err(|_| HumanDraftError::InvalidCursor)?;
    if parsed.is_nil() || raw != parsed.to_string() {
        return Err(HumanDraftError::InvalidCursor);
    }
    Ok(DraftId::from_uuid(parsed))
}

fn format_cursor(draft_id: DraftId) -> String {
    format!("{CURSOR_PREFIX}{draft_id}")
}

fn valid_provider_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

const fn map_store_error(error: StoreError) -> HumanDraftError {
    match error {
        StoreError::Conflict => HumanDraftError::Conflict,
        StoreError::MissingReference => HumanDraftError::NotFound,
        StoreError::AuthenticationInvalid => HumanDraftError::Forbidden,
        StoreError::AgentMailAdmission(_) | StoreError::Unavailable => HumanDraftError::Unavailable,
    }
}

const fn map_reader_error(error: MailEngineError) -> HumanDraftError {
    match error {
        MailEngineError::Rejected | MailEngineError::ProjectionConflict => {
            HumanDraftError::Conflict
        }
        MailEngineError::InvalidConfiguration
        | MailEngineError::CredentialUnavailable
        | MailEngineError::Unavailable
        | MailEngineError::InvalidSession
        | MailEngineError::InvalidResponse => HumanDraftError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail_ports::{MailDataPlanePort, MailDraftContent};
    use crate::{
        ControlPlaneTransaction, ExistingOrganizationBootstrap, HumanDraftAttemptListBatch,
        HumanDraftAttemptListCandidate, HumanDraftListBatch, HumanDraftListCandidate,
        HumanDraftRestoreResolution, HumanDraftRestoreTarget, MailDraftRecord,
        MailEngineMailboxBinding, MailboxSummary,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{
        MailDataPlaneId, MailboxAccessSnapshot, MailboxAddress, MailboxKind, MailboxMembershipRole,
        Organization, OrganizationId, OrganizationSlug, Principal, PrincipalId, Role,
        RoleAssignment,
    };
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Default)]
    struct Evidence {
        audits: Vec<String>,
        events: Vec<String>,
        serialized: Vec<String>,
        commits: usize,
        rechecks: Vec<HumanDraftReadRecheck>,
        list_rechecks: Vec<HumanDraftListRecheck>,
        attempt_rechecks: Vec<HumanDraftAttemptListRecheck>,
    }

    #[derive(Clone)]
    struct Store {
        batch: HumanDraftListBatch,
        attempt_batch: HumanDraftAttemptListBatch,
        resolution: HumanDraftRestoreResolution,
        evidence: Arc<Mutex<Evidence>>,
        query_calls: Arc<AtomicUsize>,
        final_authorized: bool,
        commit_fails: bool,
    }

    struct Transaction {
        evidence: Arc<Mutex<Evidence>>,
        final_authorized: bool,
        commit_fails: bool,
    }

    #[async_trait]
    impl HumanDraftQueryStore for Store {
        async fn list_human_drafts(
            &self,
            _: &HumanDraftListQuery,
        ) -> Result<HumanDraftListBatch, StoreError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.batch.clone())
        }

        async fn list_human_draft_attempts(
            &self,
            _: &HumanDraftAttemptListQuery,
        ) -> Result<HumanDraftAttemptListBatch, StoreError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.attempt_batch.clone())
        }

        async fn resolve_human_draft_restore_target(
            &self,
            _: &HumanDraftRestoreQuery,
        ) -> Result<HumanDraftRestoreResolution, StoreError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.resolution.clone())
        }
    }

    #[async_trait]
    impl ControlPlaneStore for Store {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                evidence: self.evidence.clone(),
                final_authorized: self.final_authorized,
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

        async fn human_draft_read_still_authorized(
            &mut self,
            proof: &HumanDraftReadRecheck,
        ) -> Result<bool, StoreError> {
            self.evidence.lock().unwrap().rechecks.push(proof.clone());
            Ok(self.final_authorized)
        }

        async fn human_draft_list_still_authorized(
            &mut self,
            proof: &HumanDraftListRecheck,
        ) -> Result<bool, StoreError> {
            self.evidence
                .lock()
                .unwrap()
                .list_rechecks
                .push(proof.clone());
            Ok(self.final_authorized)
        }

        async fn human_draft_attempt_list_still_authorized(
            &mut self,
            proof: &HumanDraftAttemptListRecheck,
        ) -> Result<bool, StoreError> {
            self.evidence
                .lock()
                .unwrap()
                .attempt_rechecks
                .push(proof.clone());
            Ok(self.final_authorized)
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

    struct Reader {
        data_plane_id: MailDataPlaneId,
        content: Mutex<Option<MailDraftContent>>,
        error: Option<MailEngineError>,
        calls: Mutex<Vec<(String, String, String)>>,
    }

    impl MailDataPlanePort for Reader {
        fn data_plane_id(&self) -> MailDataPlaneId {
            self.data_plane_id
        }
    }

    #[async_trait]
    impl MailDraftContentReader for Reader {
        async fn read_exact_text_draft(
            &self,
            account_ref: &str,
            email_ref: &str,
            expected_thread_ref: &str,
            expected_from: &MailboxAddress,
        ) -> Result<Option<MailDraftContent>, MailEngineError> {
            self.calls.lock().unwrap().push((
                account_ref.into(),
                email_ref.into(),
                format!("{expected_thread_ref}:{}", expected_from),
            ));
            if let Some(error) = self.error {
                return Err(error);
            }
            Ok(self.content.lock().unwrap().clone())
        }
    }

    struct Fixture {
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        draft_id: DraftId,
        data_plane_id: MailDataPlaneId,
        now: DateTime<Utc>,
        context: AuthorizationContext,
        target: HumanDraftRestoreTarget,
        remote: MailDraftContent,
    }

    fn fixture() -> Fixture {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let draft_id = DraftId::new();
        let data_plane_id = crate::mail_ports::test_mail_data_plane_id();
        let now = DateTime::parse_from_rfc3339("2026-08-13T09:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let context = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
            authentication: None,
        };
        let authorization = MailboxAccessSnapshot {
            organization_id,
            mailbox_id,
            membership: Some(MailboxMembershipRole::Member),
            delegations: Vec::new(),
        };
        let mailbox = MailboxSummary {
            mailbox_id,
            organization_id,
            kind: MailboxKind::Shared,
            primary_address: MailboxAddress::parse("shared@example.test").unwrap(),
            created_at: now,
        };
        let binding = MailEngineMailboxBinding {
            organization_id,
            mailbox_id,
            data_plane_id,
            account_ref: "provider-account-secret".into(),
            version: 2,
            updated_at: now,
        };
        let remote = MailDraftContent {
            to: vec!["recipient@example.test".into()],
            cc: vec!["copy@example.test".into()],
            bcc: vec!["blind@example.test".into()],
            subject: "Quarterly status".into(),
            text_body: "Sensitive draft body".into(),
        };
        let normalized = normalized_input(
            remote.to.clone(),
            remote.cc.clone(),
            remote.bcc.clone(),
            remote.subject.clone(),
            remote.text_body.clone(),
        )
        .unwrap();
        let reply_to_message_id = Some(Uuid::now_v7());
        let draft = MailDraftRecord {
            draft_id,
            organization_id,
            mailbox_id,
            created_by: principal_id,
            data_plane_id,
            engine_account_ref: binding.account_ref.clone(),
            engine_email_ref: "provider-email-secret".into(),
            engine_thread_ref: "provider-thread-secret".into(),
            reply_to_message_id,
            reply_to_automatic: false,
            body_ref: body_reference(&context, &normalized.text_body),
            content_sha256: draft_digest(&normalized, reply_to_message_id),
            recipients: recipient_evidence(&context, &normalized),
            version: 4,
            status: MailDraftStatus::Active,
            created_at: now,
            updated_at: now,
        };
        Fixture {
            organization_id,
            principal_id,
            mailbox_id,
            draft_id,
            data_plane_id,
            now,
            context,
            target: HumanDraftRestoreTarget {
                authorization,
                mailbox,
                binding,
                draft,
            },
            remote,
        }
    }

    fn empty_batch(fixture: &Fixture) -> HumanDraftListBatch {
        HumanDraftListBatch {
            active_human: true,
            cursor_valid: true,
            authorization: Some(fixture.target.authorization.clone()),
            binding: Some(fixture.target.binding.clone()),
            candidates: Vec::new(),
            has_more: false,
        }
    }

    fn store(
        fixture: &Fixture,
        batch: HumanDraftListBatch,
        evidence: Arc<Mutex<Evidence>>,
    ) -> Store {
        Store {
            batch,
            attempt_batch: HumanDraftAttemptListBatch {
                active_human: true,
                cursor_valid: true,
                candidates: Vec::new(),
                has_more: false,
            },
            resolution: HumanDraftRestoreResolution {
                active_human: true,
                target: Some(fixture.target.clone()),
            },
            evidence,
            query_calls: Arc::new(AtomicUsize::new(0)),
            final_authorized: true,
            commit_fails: false,
        }
    }

    fn reader(fixture: &Fixture, content: Option<MailDraftContent>) -> Arc<Reader> {
        Arc::new(Reader {
            data_plane_id: fixture.data_plane_id,
            content: Mutex::new(content),
            error: None,
            calls: Mutex::new(Vec::new()),
        })
    }

    fn list_command(fixture: &Fixture) -> ListHumanDraftsCommand {
        ListHumanDraftsCommand {
            context: fixture.context.clone(),
            mailbox_id: fixture.mailbox_id,
            limit: 2,
            cursor: None,
            trace_id: "trace-list-drafts".into(),
            now: fixture.now,
        }
    }

    fn restore_command(fixture: &Fixture) -> RestoreHumanDraftCommand {
        RestoreHumanDraftCommand {
            context: fixture.context.clone(),
            mailbox_id: fixture.mailbox_id,
            draft_id: fixture.draft_id,
            expected_version: 4,
            trace_id: "trace-restore-draft".into(),
            now: fixture.now,
        }
    }

    #[tokio::test]
    async fn lists_only_safe_descending_candidates_and_audits_before_return() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let newest = DraftId::from_uuid(Uuid::from_u128(0x30));
        let older = DraftId::from_uuid(Uuid::from_u128(0x20));
        let candidate = |draft_id| HumanDraftListCandidate {
            draft_id,
            organization_id: fixture.organization_id,
            mailbox_id: fixture.mailbox_id,
            created_by: fixture.principal_id,
            data_plane_id: fixture.data_plane_id,
            version: 2,
            status: MailDraftStatus::Active,
            reply_to_message_id: None,
            recipient_count: 2,
            created_at: fixture.now,
            updated_at: fixture.now,
        };
        let mut batch = empty_batch(&fixture);
        batch.candidates = vec![candidate(newest), candidate(older)];
        batch.has_more = true;
        let reader = reader(&fixture, None);
        let page = HumanDraftService::new(store(&fixture, batch, evidence.clone()), reader)
            .list(list_command(&fixture))
            .await
            .unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.draft_id)
                .collect::<Vec<_>>(),
            vec![newest, older]
        );
        assert_eq!(page.next_cursor, Some(format_cursor(older)));
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.list_rechecks.len(), 1);
        assert_eq!(
            evidence.list_rechecks[0].expected_engine_account_ref,
            "provider-account-secret"
        );
        assert_eq!(
            evidence.list_rechecks[0].expected_drafts,
            [
                HumanDraftListItemRecheck {
                    draft_id: newest,
                    expected_version: 2,
                },
                HumanDraftListItemRecheck {
                    draft_id: older,
                    expected_version: 2,
                },
            ]
        );
        assert_eq!(evidence.audits, ["mail.drafts.queried"]);
        assert_eq!(evidence.events, ["mail.drafts.queried.v1"]);
        assert_eq!(evidence.commits, 1);
        let serialized = evidence.serialized.join(" ");
        for secret in [
            "provider-account-secret",
            "provider-email-secret",
            "Quarterly status",
            "recipient@example.test",
        ] {
            assert!(!serialized.contains(secret));
        }
    }

    #[tokio::test]
    async fn empty_list_page_passes_an_exact_empty_recheck_proof() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let page = HumanDraftService::new(
            store(&fixture, empty_batch(&fixture), evidence.clone()),
            reader(&fixture, None),
        )
        .list(list_command(&fixture))
        .await
        .unwrap();

        assert!(page.items.is_empty());
        assert!(page.next_cursor.is_none());
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.list_rechecks.len(), 1);
        assert!(evidence.list_rechecks[0].expected_drafts.is_empty());
        assert_eq!(evidence.audits, ["mail.drafts.queried"]);
        assert_eq!(evidence.events, ["mail.drafts.queried.v1"]);
        assert_eq!(evidence.commits, 1);
    }

    #[tokio::test]
    async fn rejects_missing_rbac_and_noncanonical_cursor_before_query() {
        let first_fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let first_store = store(&first_fixture, empty_batch(&first_fixture), evidence);
        let calls = first_store.query_calls.clone();
        let service = HumanDraftService::new(first_store, reader(&first_fixture, None));
        let mut command = list_command(&first_fixture);
        command.context.permissions.clear();
        assert_eq!(service.list(command).await, Err(HumanDraftError::Forbidden));
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let second_fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let second_store = store(&second_fixture, empty_batch(&second_fixture), evidence);
        let calls = second_store.query_calls.clone();
        let service = HumanDraftService::new(second_store, reader(&second_fixture, None));
        let mut command = list_command(&second_fixture);
        command.cursor = Some(format!(
            "v1.{}",
            Uuid::from_u128(0xabcdef).to_string().to_uppercase()
        ));
        assert_eq!(
            service.list(command).await,
            Err(HumanDraftError::InvalidCursor)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn restore_exact_content_rechecks_and_commits_minimal_evidence() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let reader = reader(&fixture, Some(fixture.remote.clone()));
        let restored = HumanDraftService::new(
            store(&fixture, empty_batch(&fixture), evidence.clone()),
            reader.clone(),
        )
        .restore(restore_command(&fixture))
        .await
        .unwrap();

        assert_eq!(restored.draft_id, fixture.draft_id);
        assert_eq!(restored.version, 4);
        assert_eq!(restored.text_body, "Sensitive draft body");
        assert_eq!(restored.bcc, ["blind@example.test"]);
        assert_eq!(
            reader.calls.lock().unwrap().as_slice(),
            [(
                "provider-account-secret".into(),
                "provider-email-secret".into(),
                "provider-thread-secret:shared@example.test".into(),
            )]
        );
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.rechecks.len(), 1);
        assert_eq!(evidence.rechecks[0].expected_version, 4);
        assert_eq!(evidence.audits, ["mail.draft.read"]);
        assert_eq!(evidence.events, ["mail.draft.read.v1"]);
        assert_eq!(evidence.commits, 1);
        let serialized = evidence.serialized.join(" ");
        for secret in [
            "provider-account-secret",
            "provider-email-secret",
            "provider-thread-secret",
            "Sensitive draft body",
            "Quarterly status",
            "recipient@example.test",
            "blind@example.test",
        ] {
            assert!(!serialized.contains(secret));
        }
    }

    #[tokio::test]
    async fn remote_tampering_never_reaches_disclosure_transaction() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let mut changed = fixture.remote.clone();
        changed.text_body.push_str(" tampered");
        let reader = reader(&fixture, Some(changed));
        let result = HumanDraftService::new(
            store(&fixture, empty_batch(&fixture), evidence.clone()),
            reader.clone(),
        )
        .restore(restore_command(&fixture))
        .await;

        assert_eq!(result, Err(HumanDraftError::Conflict));
        assert_eq!(reader.calls.lock().unwrap().len(), 1);
        let evidence = evidence.lock().unwrap();
        assert!(evidence.rechecks.is_empty());
        assert!(evidence.audits.is_empty());
        assert_eq!(evidence.commits, 0);
    }

    #[tokio::test]
    async fn final_authorization_drift_and_commit_failure_do_not_return_body() {
        let fixture = fixture();
        let candidate = HumanDraftListCandidate {
            draft_id: fixture.draft_id,
            organization_id: fixture.organization_id,
            mailbox_id: fixture.mailbox_id,
            created_by: fixture.principal_id,
            data_plane_id: fixture.data_plane_id,
            version: 4,
            status: MailDraftStatus::Active,
            reply_to_message_id: fixture.target.draft.reply_to_message_id,
            recipient_count: 3,
            created_at: fixture.now,
            updated_at: fixture.now,
        };
        let list_evidence = Arc::new(Mutex::new(Evidence::default()));
        let mut batch = empty_batch(&fixture);
        batch.candidates = vec![candidate];
        let mut denied_list = store(&fixture, batch, list_evidence.clone());
        denied_list.final_authorized = false;
        let result = HumanDraftService::new(denied_list, reader(&fixture, None))
            .list(list_command(&fixture))
            .await;
        assert_eq!(result, Err(HumanDraftError::Conflict));
        {
            let evidence = list_evidence.lock().unwrap();
            assert_eq!(evidence.list_rechecks.len(), 1);
            assert!(evidence.audits.is_empty());
            assert_eq!(evidence.commits, 0);
        }

        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let mut denied = store(&fixture, empty_batch(&fixture), evidence.clone());
        denied.final_authorized = false;
        let result = HumanDraftService::new(denied, reader(&fixture, Some(fixture.remote.clone())))
            .restore(restore_command(&fixture))
            .await;
        assert_eq!(result, Err(HumanDraftError::Conflict));
        {
            let denied_evidence = evidence.lock().unwrap();
            assert_eq!(denied_evidence.rechecks.len(), 1);
            assert!(denied_evidence.audits.is_empty());
            assert_eq!(denied_evidence.commits, 0);
        }

        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let mut failing = store(&fixture, empty_batch(&fixture), evidence.clone());
        failing.commit_fails = true;
        let result =
            HumanDraftService::new(failing, reader(&fixture, Some(fixture.remote.clone())))
                .restore(restore_command(&fixture))
                .await;
        assert_eq!(result, Err(HumanDraftError::Unavailable));
        assert_eq!(evidence.lock().unwrap().commits, 0);
    }

    #[tokio::test]
    async fn shared_mailbox_access_never_broadens_private_draft_ownership() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let mut missing = store(&fixture, empty_batch(&fixture), evidence);
        missing.resolution.target = None;
        let first_reader = reader(&fixture, Some(fixture.remote.clone()));
        let result = HumanDraftService::new(missing, first_reader.clone())
            .restore(restore_command(&fixture))
            .await;
        assert_eq!(result, Err(HumanDraftError::NotFound));
        assert!(first_reader.calls.lock().unwrap().is_empty());

        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let mut target = fixture.target.clone();
        target.authorization.membership = None;
        target.authorization.delegations = vec![copilotmail_domain::ResolvedMailboxDelegation {
            delegation_id: copilotmail_domain::DelegationId::new(),
            grantor_principal_id: PrincipalId::new(),
            capabilities: [MailboxCapability::Read, MailboxCapability::SendAs]
                .into_iter()
                .collect(),
        }];
        let mut denied = store(&fixture, empty_batch(&fixture), evidence);
        denied.resolution.target = Some(target);
        let second_reader = reader(&fixture, Some(fixture.remote.clone()));
        let result = HumanDraftService::new(denied, second_reader.clone())
            .restore(restore_command(&fixture))
            .await;
        assert_eq!(result, Err(HumanDraftError::NotFound));
        assert!(second_reader.calls.lock().unwrap().is_empty());
    }

    fn attempt_candidate(
        fixture: &Fixture,
        attempt_id: DraftCreationAttemptId,
    ) -> HumanDraftAttemptListCandidate {
        HumanDraftAttemptListCandidate {
            attempt_id,
            organization_id: fixture.organization_id,
            mailbox_id: fixture.mailbox_id,
            created_by: fixture.principal_id,
            draft_id: fixture.draft_id,
            attempted_at: fixture.now,
        }
    }

    fn attempt_list_command(fixture: &Fixture) -> ListHumanDraftAttemptsCommand {
        ListHumanDraftAttemptsCommand {
            context: fixture.context.clone(),
            limit: 2,
            cursor: None,
            trace_id: "trace-draft-attempts".into(),
            now: fixture.now,
        }
    }

    #[tokio::test]
    async fn draft_attempts_stay_owner_scoped_ordered_and_audited_before_return() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let newest = DraftCreationAttemptId::from_uuid(Uuid::from_u128(0x30));
        let older = DraftCreationAttemptId::from_uuid(Uuid::from_u128(0x20));
        let store = store(&fixture, empty_batch(&fixture), evidence.clone());
        let service = HumanDraftAttemptService::new(Store {
            attempt_batch: HumanDraftAttemptListBatch {
                active_human: true,
                cursor_valid: true,
                candidates: vec![
                    attempt_candidate(&fixture, newest),
                    attempt_candidate(&fixture, older),
                ],
                has_more: true,
            },
            ..store
        });

        let page = service.list(attempt_list_command(&fixture)).await.unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.attempt_id)
                .collect::<Vec<_>>(),
            [newest, older]
        );
        assert_eq!(page.items[0].mailbox_id, fixture.mailbox_id);
        assert_eq!(page.items[0].draft_id, fixture.draft_id);
        assert_eq!(page.next_cursor, Some(format_attempt_cursor(older)));
        let evidence = evidence.lock().unwrap();
        assert_eq!(evidence.attempt_rechecks.len(), 1);
        assert_eq!(
            evidence.attempt_rechecks[0],
            HumanDraftAttemptListRecheck {
                organization_id: fixture.organization_id,
                principal_id: fixture.principal_id,
                expected_attempt_ids: vec![newest, older],
            }
        );
        assert_eq!(evidence.audits, ["mail.draft_attempts.queried"]);
        assert_eq!(evidence.events, ["mail.draft_attempts.queried.v1"]);
        assert_eq!(evidence.commits, 1);
        let serialized = evidence.serialized.join(" ");
        for secret in [
            "provider-account-secret",
            "provider-email-secret",
            "provider-thread-secret",
            "Sensitive draft body",
            "Quarterly status",
            "recipient@example.test",
        ] {
            assert!(!serialized.contains(secret));
        }
    }

    #[tokio::test]
    async fn draft_attempt_list_rejects_rbac_cursor_and_anomalies_early() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let store = store(&fixture, empty_batch(&fixture), evidence);
        let calls = store.query_calls.clone();
        let service = HumanDraftAttemptService::new(store.clone());
        let mut command = attempt_list_command(&fixture);
        command.context.permissions.clear();
        assert_eq!(service.list(command).await, Err(HumanDraftError::Forbidden));
        let mut command = attempt_list_command(&fixture);
        command.limit = 0;
        assert_eq!(
            service.list(command).await,
            Err(HumanDraftError::InvalidPageSize)
        );
        let mut command = attempt_list_command(&fixture);
        command.cursor = Some("v1.not-a-uuid".into());
        assert_eq!(
            service.list(command).await,
            Err(HumanDraftError::InvalidCursor)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let inactive = HumanDraftAttemptService::new(Store {
            attempt_batch: HumanDraftAttemptListBatch {
                active_human: false,
                cursor_valid: true,
                candidates: Vec::new(),
                has_more: false,
            },
            ..store.clone()
        });
        assert_eq!(
            inactive.list(attempt_list_command(&fixture)).await,
            Err(HumanDraftError::Forbidden)
        );

        let stale_cursor = HumanDraftAttemptService::new(Store {
            attempt_batch: HumanDraftAttemptListBatch {
                active_human: true,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            },
            ..store.clone()
        });
        let mut command = attempt_list_command(&fixture);
        command.cursor = Some(format_attempt_cursor(DraftCreationAttemptId::new()));
        assert_eq!(
            stale_cursor.list(command).await,
            Err(HumanDraftError::InvalidCursor)
        );

        for candidate in [
            HumanDraftAttemptListCandidate {
                organization_id: OrganizationId::new(),
                ..attempt_candidate(&fixture, DraftCreationAttemptId::new())
            },
            HumanDraftAttemptListCandidate {
                created_by: PrincipalId::new(),
                ..attempt_candidate(&fixture, DraftCreationAttemptId::new())
            },
            HumanDraftAttemptListCandidate {
                draft_id: DraftId::from_uuid(Uuid::nil()),
                ..attempt_candidate(&fixture, DraftCreationAttemptId::new())
            },
        ] {
            let service = HumanDraftAttemptService::new(Store {
                attempt_batch: HumanDraftAttemptListBatch {
                    active_human: true,
                    cursor_valid: true,
                    candidates: vec![candidate],
                    has_more: false,
                },
                ..store.clone()
            });
            assert_eq!(
                service.list(attempt_list_command(&fixture)).await,
                Err(HumanDraftError::Unavailable)
            );
        }
    }

    #[tokio::test]
    async fn draft_attempt_list_discloses_nothing_when_recheck_or_commit_fails() {
        let fixture = fixture();
        let evidence = Arc::new(Mutex::new(Evidence::default()));
        let attempt_batch = HumanDraftAttemptListBatch {
            active_human: true,
            cursor_valid: true,
            candidates: vec![attempt_candidate(&fixture, DraftCreationAttemptId::new())],
            has_more: false,
        };
        let mut denied = Store {
            attempt_batch: attempt_batch.clone(),
            ..store(&fixture, empty_batch(&fixture), evidence.clone())
        };
        denied.final_authorized = false;
        assert_eq!(
            HumanDraftAttemptService::new(denied)
                .list(attempt_list_command(&fixture))
                .await,
            Err(HumanDraftError::Conflict)
        );
        {
            let evidence = evidence.lock().unwrap();
            assert_eq!(evidence.attempt_rechecks.len(), 1);
            assert!(evidence.audits.is_empty());
            assert_eq!(evidence.commits, 0);
        }

        let mut failing = Store {
            attempt_batch,
            ..store(&fixture, empty_batch(&fixture), evidence.clone())
        };
        failing.commit_fails = true;
        assert_eq!(
            HumanDraftAttemptService::new(failing)
                .list(attempt_list_command(&fixture))
                .await,
            Err(HumanDraftError::Unavailable)
        );
        assert_eq!(evidence.lock().unwrap().commits, 0);
    }
}
