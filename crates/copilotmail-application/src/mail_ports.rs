//! Vendor-neutral mail data-plane ports owned by the application layer.
//!
//! Infrastructure adapters implement these traits, but application use cases
//! depend only on this module. Provider object identifiers are opaque strings;
//! provider payloads, credentials, and persistence details do not cross the
//! boundary.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use copilotmail_domain::{MailDataPlaneId, MailDomainName, MailboxAddress};
use url::Url;

/// Stable identity exposed by every mail data-plane adapter.
///
/// Remote references are opaque and may be compared or forwarded only when
/// they belong to the same data-plane identifier.
pub trait MailDataPlanePort: Send + Sync {
    fn data_plane_id(&self) -> MailDataPlaneId;
}

#[cfg(test)]
pub(crate) fn test_mail_data_plane_id() -> MailDataPlaneId {
    MailDataPlaneId::from_uuid(uuid::Uuid::from_u128(
        0x018f_0000_0000_7000_8000_0000_0000_00d1,
    ))
}

#[cfg(test)]
pub(crate) fn other_test_mail_data_plane_id() -> MailDataPlaneId {
    MailDataPlaneId::from_uuid(uuid::Uuid::from_u128(
        0x018f_0000_0000_7000_8000_0000_0000_00d2,
    ))
}

/// A bounded, presentation-neutral address returned by the mail data plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailAddress {
    pub name: Option<String>,
    pub email: String,
}

/// Envelope-only message data used by inbox lists. Message bodies, raw
/// headers, and blob identifiers are intentionally excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessageSummary {
    pub id: String,
    pub subject: String,
    pub from: Vec<MailAddress>,
    pub received_at: DateTime<Utc>,
    pub preview: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub has_attachment: bool,
}

/// One bounded provider query page. `next_position` is an adapter cursor
/// component and must not be accepted without the application re-binding it
/// to a mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessagePage {
    pub query_state: String,
    pub items: Vec<MailMessageSummary>,
    pub next_position: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailAttachmentSummary {
    pub name: Option<String>,
    pub media_type: String,
    pub size_bytes: u64,
}

/// Safe message detail. Only bounded `text/plain` body values are returned;
/// HTML, raw MIME, headers, and blob identifiers never cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailMessageDetail {
    pub summary: MailMessageSummary,
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub text_body: String,
    pub text_body_truncated: bool,
    pub attachments: Vec<MailAttachmentSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailMessageState {
    pub is_read: bool,
    pub is_flagged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDraftInput {
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: String,
    /// Opaque provider Email identity resolved server-side from a safe
    /// CopilotMail message UUID. Callers cannot supply raw reply headers.
    pub reply_to_email_id: Option<String>,
    /// Server-derived marker for mail authored by an Agent principal.
    /// The adapter emits RFC 3834 Auto-Submitted metadata; callers cannot
    /// choose its value.
    pub automatic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDraft {
    pub id: String,
    pub thread_id: String,
    pub reply_source_automatic: bool,
    pub previous_draft_destroyed: bool,
}

/// A bounded plain-text draft read from one exact provider object.
///
/// Provider account, Email, Thread, Blob, and collection-state identifiers
/// deliberately remain inputs to the adapter and never appear in this value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDraftContent {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: String,
}

/// Least-privilege boundary for restoring a control-plane-owned draft.
///
/// Implementations must issue an exact object read, never enumerate the
/// provider Drafts mailbox. They must also verify that the returned object is
/// the requested Email, belongs only to the unique readable Drafts mailbox,
/// carries the `$draft` keyword, and has the exact server-derived From address.
#[async_trait]
pub trait MailDraftContentReader: MailDataPlanePort {
    async fn read_exact_text_draft(
        &self,
        account_ref: &str,
        email_ref: &str,
        expected_thread_ref: &str,
        expected_from: &MailboxAddress,
    ) -> Result<Option<MailDraftContent>, MailEngineError>;
}

#[async_trait]
pub trait MailDraftStore: MailDataPlanePort {
    async fn probe_drafts(&self) -> Result<(), MailEngineError>;

    /// Creates a draft, or atomically creates a replacement and destroys the
    /// previous draft because provider message bodies are immutable.
    async fn save_text_draft(
        &self,
        account_ref: &str,
        previous_draft_id: Option<&str>,
        input: &TextDraftInput,
    ) -> Result<MailDraft, MailEngineError>;
}

/// A submission discovered while reconciling a frozen provider Email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailSubmissionReconciliation {
    /// No submission exists. The returned identity is the only identity whose
    /// address exactly matches the control-plane-authorized From address.
    NotFound { identity_ref: String },
    /// Exactly one submission exists and is bound to the expected Email and
    /// uniquely resolved identity.
    Accepted {
        identity_ref: String,
        submission_ref: String,
    },
}

/// Result of creating one provider EmailSubmission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedMailSubmission {
    pub identity_ref: String,
    pub submission_ref: String,
}

/// Vendor-neutral recipient state reported by the exact remote submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipientDeliveryState {
    Queued,
    Delivered,
    PermanentFailure,
    Unknown,
}

/// One recipient outcome in an authenticated delivery snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipientDeliveryEvidence {
    pub recipient: MailboxAddress,
    pub state: RecipientDeliveryState,
    pub smtp_reply: String,
}

/// Complete server-side delivery projection for one exact submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDeliverySnapshot {
    pub submission_ref: String,
    /// Opaque provider collection state observed with the result.
    pub source_state: String,
    pub recipients: Vec<RecipientDeliveryEvidence>,
}

/// Replaceable least-privilege outbound-submission boundary.
///
/// Callers must reconcile before every create attempt. In particular, a
/// transport error from `create_submission` is ambiguous and must never be
/// interpreted as proof that the server did not accept the request.
#[async_trait]
pub trait MailSubmissionAdapter: MailDataPlanePort {
    async fn probe_submissions(&self) -> Result<(), MailEngineError>;

    async fn reconcile_submission(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        expected_from: &str,
    ) -> Result<MailSubmissionReconciliation, MailEngineError>;

    async fn create_submission(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        identity_ref: &str,
    ) -> Result<CreatedMailSubmission, MailEngineError>;

    /// Reads recipient-level delivery state from the exact remote submission.
    /// The default keeps alternate adapters fail-closed until they implement
    /// an equally strong submission-to-delivery binding.
    async fn get_delivery_snapshot(
        &self,
        _account_ref: &str,
        _engine_email_ref: &str,
        _identity_ref: &str,
        _submission_ref: &str,
    ) -> Result<MailDeliverySnapshot, MailEngineError> {
        Err(MailEngineError::Unavailable)
    }
}

/// Replaceable least-privilege mail-content boundary. It is deliberately
/// separate from management projection because management credentials cannot
/// and must not access mailbox content.
#[async_trait]
pub trait MailContentReader: MailDataPlanePort {
    async fn probe_content(&self) -> Result<(), MailEngineError>;

    async fn list_messages(
        &self,
        account_ref: &str,
        position: u32,
        page_size: u32,
    ) -> Result<MailMessagePage, MailEngineError>;

    /// Searches the exact authorized account without exposing the search text
    /// through a URL. Implementations must validate and bound the text before
    /// sending it to the mail data plane.
    async fn search_messages(
        &self,
        account_ref: &str,
        text: &str,
        position: u32,
        page_size: u32,
    ) -> Result<MailMessagePage, MailEngineError> {
        let _ = (account_ref, text, position, page_size);
        Err(MailEngineError::InvalidConfiguration)
    }

    async fn get_message(
        &self,
        account_ref: &str,
        message_id: &str,
    ) -> Result<Option<MailMessageDetail>, MailEngineError>;

    async fn set_message_state(
        &self,
        account_ref: &str,
        message_id: &str,
        is_read: Option<bool>,
        is_flagged: Option<bool>,
    ) -> Result<MailMessageState, MailEngineError>;
}

/// Classification emitted by a replaceable mail engine for an inbound
/// message. It deliberately excludes vendor-specific event names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundMessageClassification {
    Ham,
    Spam,
}

/// Minimal vendor-neutral inbound message evidence. Raw message content,
/// headers, addresses, blob identifiers, and message sizes never cross this
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMailEngineEvent {
    pub source_event_id: String,
    pub source_occurred_at: DateTime<Utc>,
    pub account_ref: String,
    pub document_ref: String,
    pub classification: InboundMessageClassification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundDeliveryEventKind {
    Delivered,
    PermanentFailure,
    Rescheduled,
    Completed,
}

/// Minimal queue-level evidence from one authenticated delivery trace. It is
/// intentionally not presented as a remote submission binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMailEngineEvent {
    pub source_event_id: String,
    pub source_occurred_at: DateTime<Utc>,
    pub queue_ref: String,
    pub envelope_from: MailboxAddress,
    pub recipient: Option<MailboxAddress>,
    pub kind: OutboundDeliveryEventKind,
    pub smtp_code: Option<u16>,
    pub next_retry_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailEngineWebhookEvent {
    Inbound(InboundMailEngineEvent),
    Outbound(OutboundMailEngineEvent),
}

/// Vendor-neutral readiness evidence exposed to the control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailEngineReadiness {
    /// Authenticated provider username reported by the engine.
    pub username: String,
    /// Opaque session state, useful for detecting configuration changes.
    pub session_state: String,
    /// Validated same-origin API URL.
    pub api_url: Url,
    /// Whether this credential's session also advertises end-user mail access.
    ///
    /// Management credentials intentionally need not grant mail access, so
    /// this is evidence only and is not a readiness requirement.
    pub mail_capability_advertised: bool,
}

/// Minimal replaceable boundary required by control-plane readiness.
#[async_trait]
pub trait MailEngine: MailDataPlanePort {
    /// Performs an authenticated live probe of the documented protocol surface.
    async fn probe(&self) -> Result<MailEngineReadiness, MailEngineError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailEngineDomainProjection {
    pub engine_ref: String,
    pub name: String,
    pub enabled: bool,
    pub created: bool,
}

/// Vendor-neutral domain projection boundary. Suspension disables rather than
/// destroys the engine object so mailbox data cannot be orphaned accidentally.
#[async_trait]
pub trait MailDomainProjector: MailDataPlanePort {
    async fn reconcile_domain(
        &self,
        name: &MailDomainName,
        current_engine_ref: Option<&str>,
        enabled: bool,
    ) -> Result<MailEngineDomainProjection, MailEngineError>;
}

/// Stable failures suitable for sanitised operational status.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum MailEngineError {
    /// The configured URL, path, or timeout violates the security baseline.
    #[error("mail engine configuration is invalid")]
    InvalidConfiguration,
    /// The API credential file is absent, unsafe, or unreadable.
    #[error("mail engine credential is unavailable")]
    CredentialUnavailable,
    /// The engine could not be reached or returned a non-success response.
    #[error("mail engine is unavailable")]
    Unavailable,
    /// The protocol session omitted required capabilities or advertised an unsafe URL.
    #[error("mail engine JMAP session is invalid")]
    InvalidSession,
    /// A protocol response violated the bounded adapter contract.
    #[error("mail engine returned an invalid JMAP response")]
    InvalidResponse,
    /// The engine rejected a syntactically valid management operation.
    #[error("mail engine rejected the management operation")]
    Rejected,
    /// Existing engine state conflicts with the control-plane binding.
    #[error("mail engine domain projection conflicts with control-plane state")]
    ProjectionConflict,
}
