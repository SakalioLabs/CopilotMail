use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxSummary {
    pub mailbox_id: MailboxId,
    pub organization_id: OrganizationId,
    pub kind: MailboxKind,
    pub primary_address: MailboxAddress,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Bounded, tenant-scoped query used to build the Human mailbox switcher.
///
/// `after_mailbox_id` is an exclusive cursor anchor. Persistence adapters must
/// prove that an anchor is still visible under the same authorization snapshot
/// used for the returned page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanMailboxDirectoryQuery {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub after_mailbox_id: Option<MailboxId>,
    pub limit: u16,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// One directory candidate with the exact resource-authorization snapshot
/// observed by persistence. The application layer evaluates this snapshot a
/// second time before exposing the mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailboxDirectoryCandidate {
    pub mailbox: MailboxSummary,
    pub data_plane_id: MailDataPlaneId,
    pub authorization: MailboxAccessSnapshot,
}

/// One consistent read result for the Human mailbox directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailboxDirectoryBatch {
    pub active_human: bool,
    pub cursor_valid: bool,
    pub candidates: Vec<HumanMailboxDirectoryCandidate>,
    pub has_more: bool,
}

/// Immutable DNS observation persistence payload.
#[derive(Debug, Clone, PartialEq)]
pub struct MailDomainDnsObservation {
    pub organization_id: OrganizationId,
    pub mail_domain_id: MailDomainId,
    pub evidence: DnsVerificationEvidence,
    pub raw_answers: serde_json::Value,
    pub failure_codes: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Tenant-safe domain administration projection shared by Human and Agent APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDomainSummary {
    pub mail_domain_id: MailDomainId,
    pub organization_id: OrganizationId,
    pub name: MailDomainName,
    pub status: copilotmail_domain::MailDomainStatus,
    pub route_mode: copilotmail_domain::MailDomainRouteMode,
    pub ownership_challenge_version: i64,
    pub route_version: i64,
    pub inbound_enabled: bool,
    pub outbound_enabled: bool,
    pub smart_host_ref: Option<String>,
    pub dkim_key_ref: Option<String>,
    pub mail_engine_domain_ref: Option<String>,
    pub mail_engine_projection_status: MailEngineProjectionStatus,
    pub mail_engine_projected_enabled: Option<bool>,
    pub mail_engine_projection_last_attempt_at: Option<chrono::DateTime<chrono::Utc>>,
    pub mail_engine_projection_succeeded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub mail_engine_projection_error_code: Option<String>,
    pub latest_dns_observed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub latest_dns_valid_until: Option<chrono::DateTime<chrono::Utc>>,
    pub latest_dns_activation_ready: bool,
    pub latest_dns_failure_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookSubscriptionSummary {
    pub subscription_id: WebhookSubscriptionId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub endpoint_url: String,
    pub event_types: Vec<WebhookEventType>,
    pub signing_key_reference: String,
    pub enabled: bool,
    pub version: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Latest terminal delivery evidence exposed to privileged operators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookDeadLetterSummary {
    pub delivery_id: uuid::Uuid,
    pub subscription_id: WebhookSubscriptionId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub event_id: uuid::Uuid,
    pub event_type: String,
    pub attempt: i32,
    pub response_status: Option<i32>,
    pub error_code: String,
    pub failed_at: chrono::DateTime<chrono::Utc>,
}

/// One externally safe event available to an Agent subscription cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentCursorEvent {
    pub event_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub event_type: WebhookEventType,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub schema_version: i32,
    pub payload: serde_json::Value,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEventStreamLease {
    pub lease_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub subscription_id: WebhookSubscriptionId,
    pub opened_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEventStreamLeaseAcquisition {
    Acquired(AgentEventStreamLease),
    TenantCapacityReached,
    PrincipalCapacityReached,
}

/// Versioned mapping from a vendor-neutral mail-engine account reference to a
/// tenant mailbox. Source references are never exposed to Agent consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailEngineMailboxBinding {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub data_plane_id: MailDataPlaneId,
    pub account_ref: String,
    pub version: i64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailDraftStatus {
    Active,
    Frozen,
    Sent,
}

/// Control-plane provenance for a remote immutable JMAP draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDraftRecord {
    pub draft_id: DraftId,
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub created_by: PrincipalId,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_email_ref: String,
    pub engine_thread_ref: String,
    /// Externally safe inbound message identity for an explicit reply. The
    /// provider document reference is deliberately not persisted here.
    pub reply_to_message_id: Option<uuid::Uuid>,
    pub reply_to_automatic: bool,
    /// Tenant-and-principal-scoped digest of the normalized plain-text body.
    /// It supports duplicate-content loop detection without storing content.
    pub body_ref: [u8; 32],
    pub content_sha256: [u8; 32],
    /// Pseudonymous, tenant-and-Agent-scoped evidence used for production
    /// Agent admission. Raw recipient addresses never enter the control plane.
    pub recipients: Vec<MailDraftRecipientEvidence>,
    pub version: i64,
    pub status: MailDraftStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDraftWrite {
    pub draft_id: DraftId,
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub created_by: PrincipalId,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_email_ref: String,
    pub engine_thread_ref: String,
    pub reply_to_message_id: Option<uuid::Uuid>,
    pub reply_to_automatic: bool,
    pub body_ref: [u8; 32],
    pub content_sha256: [u8; 32],
    pub recipients: Vec<MailDraftRecipientEvidence>,
    pub expected_version: Option<i64>,
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDraftRecipientEvidence {
    pub recipient_ref: [u8; 32],
    pub domain: String,
}

/// Immutable-key pagination request for an active Human's own drafts.
///
/// Persistence must use `after_draft_id` as an exclusive descending cursor,
/// prove the cursor is still eligible, and return only drafts matching the
/// exact current data-plane mailbox binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanDraftListQuery {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub expected_data_plane_id: MailDataPlaneId,
    pub after_draft_id: Option<DraftId>,
    pub limit: u16,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Internal list candidate. It carries the fields needed to repeat tenant,
/// creator, lifecycle, data-plane, and ordering checks without loading draft
/// content or provider object references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftListCandidate {
    pub draft_id: DraftId,
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub created_by: PrincipalId,
    pub data_plane_id: MailDataPlaneId,
    pub version: i64,
    pub status: MailDraftStatus,
    pub reply_to_message_id: Option<uuid::Uuid>,
    pub recipient_count: u16,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// One database-authoritative Human draft page and its authorization proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftListBatch {
    pub active_human: bool,
    pub cursor_valid: bool,
    pub authorization: Option<MailboxAccessSnapshot>,
    pub binding: Option<MailEngineMailboxBinding>,
    pub candidates: Vec<HumanDraftListCandidate>,
    pub has_more: bool,
}

/// Exact identity and version of one validated Human draft-list item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanDraftListItemRecheck {
    pub draft_id: DraftId,
    pub expected_version: i64,
}

/// Exact authorization, binding, and result-set proof repeated in the
/// transaction that records a successful Human draft-list disclosure.
/// `expected_drafts` is ordered exactly as the disclosed page, may be empty,
/// and is bounded by the application page-size limit of 50 items. The
/// persistence adapter must evaluate expiring grants with its transaction-time
/// database clock; callers deliberately cannot supply a reusable authorization
/// timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftListRecheck {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub expected_data_plane_id: MailDataPlaneId,
    pub expected_engine_account_ref: String,
    pub expected_drafts: Vec<HumanDraftListItemRecheck>,
}

/// Immutable-key pagination request for an active Human's own accepted
/// submissions. This is a content-free delivery-status directory: persistence
/// must not load recipient evidence, SMTP replies, or provider references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanSentListQuery {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub after_submission_id: Option<MailSubmissionId>,
    pub limit: u16,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Latest immutable delivery snapshot reduced to browser-safe counters. The
/// snapshot identifier is retained only for the final database recheck and
/// must never cross the Human HTTP contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSentDeliveryEvidence {
    pub snapshot_id: uuid::Uuid,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub terminal: bool,
    pub recipient_count: u16,
    pub delivered_count: u16,
    pub failed_count: u16,
    pub queued_count: u16,
    pub unknown_count: u16,
}

/// Internal, content-free accepted-submission candidate. It deliberately
/// omits Draft, recipient, provider, and SMTP details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSentListCandidate {
    pub submission_id: MailSubmissionId,
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub requested_by: PrincipalId,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
    pub recipient_count: u16,
    pub delivery: Option<HumanSentDeliveryEvidence>,
}

/// One database-authoritative Human accepted-submission page and its current
/// authorization proof. Historical summary visibility intentionally does not
/// depend on the mailbox's current data-plane binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSentListBatch {
    pub active_human: bool,
    pub cursor_valid: bool,
    pub authorization: Option<MailboxAccessSnapshot>,
    pub candidates: Vec<HumanSentListCandidate>,
    pub has_more: bool,
}

/// Exact non-content evidence repeated in the transaction that records a
/// successful Human accepted-submission disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanSentListItemRecheck {
    pub submission_id: MailSubmissionId,
    pub expected_accepted_at: chrono::DateTime<chrono::Utc>,
    pub expected_recipient_count: u16,
    pub expected_delivery_snapshot_id: Option<uuid::Uuid>,
}

/// Exact authorization and result-set proof for the Human sent directory.
/// Entries are ordered exactly as disclosed and bounded by the page-size
/// limit. The persistence adapter must use its transaction-time clock when it
/// validates expiring grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSentListRecheck {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub expected_submissions: Vec<HumanSentListItemRecheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanDraftRestoreQuery {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub expected_data_plane_id: MailDataPlaneId,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Exact internal target for a Human draft body read. Provider references are
/// confined to this infrastructure-facing value and must never be serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftRestoreTarget {
    pub authorization: MailboxAccessSnapshot,
    pub mailbox: MailboxSummary,
    pub binding: MailEngineMailboxBinding,
    pub draft: MailDraftRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftRestoreResolution {
    pub active_human: bool,
    pub target: Option<HumanDraftRestoreTarget>,
}

/// Exact proof re-evaluated in the same transaction that records successful
/// body disclosure. Implementations must re-check Active Human, live
/// `mailbox.use`, `Read(None)`, `Send(None)`, current binding, ownership,
/// lifecycle, version, and the immutable content evidence. Expiring grants
/// must be evaluated with the persistence transaction's database clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDraftReadRecheck {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub expected_version: i64,
    pub expected_data_plane_id: MailDataPlaneId,
    pub expected_engine_account_ref: String,
    pub expected_engine_email_ref: String,
    pub expected_engine_thread_ref: String,
    pub expected_primary_address: MailboxAddress,
    pub expected_body_ref: [u8; 32],
    pub expected_content_sha256: [u8; 32],
    pub expected_recipients: Vec<MailDraftRecipientEvidence>,
    pub expected_reply_to_message_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailSubmissionStatus {
    Pending,
    Reconciling,
    Accepted,
    Rejected,
    ManualReview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSubmissionIntentRecord {
    pub submission_id: MailSubmissionId,
    pub organization_id: OrganizationId,
    pub requested_by: PrincipalId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub draft_version: i64,
    /// Data plane inherited from the frozen draft. It is the durable routing
    /// key for submission execution and every terminal delivery event.
    pub data_plane_id: MailDataPlaneId,
    pub request_sha256: [u8; 32],
    pub status: MailSubmissionStatus,
    pub state_version: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSubmissionIntentWrite {
    pub submission_id: MailSubmissionId,
    pub organization_id: OrganizationId,
    pub requested_by: PrincipalId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub expected_draft_version: i64,
    pub idempotency_key_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub first_contact_approval_request_id: Option<ApprovalRequestId>,
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailAdmissionRecord {
    pub id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub agent_id: PrincipalId,
    pub submission_id: MailSubmissionId,
    pub policy_version_id: uuid::Uuid,
    pub policy_version: u32,
    pub recipient_count: u16,
    pub first_contact_count: u16,
    pub admitted_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSubmissionIntentCreation {
    pub intent: MailSubmissionIntentRecord,
    pub created: bool,
    pub production_admission: Option<AgentMailAdmissionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSandboxSubmissionRecord {
    pub submission_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub agent_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub draft_version: i64,
    pub recipient_count: u16,
    pub captured_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSandboxSubmissionWrite {
    pub submission_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub agent_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub expected_draft_version: i64,
    pub idempotency_key_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSandboxSubmissionCreation {
    pub record: AgentMailSandboxSubmissionRecord,
    pub created: bool,
}

/// A committed, bounded lease for one outbound reconciliation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSubmissionLease {
    pub submission_id: MailSubmissionId,
    pub organization_id: OrganizationId,
    pub requested_by: PrincipalId,
    pub mailbox_id: MailboxId,
    pub draft_id: DraftId,
    pub draft_version: i64,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_email_ref: String,
    pub authorized_from: MailboxAddress,
    pub lease_id: uuid::Uuid,
    pub lease_expires_at: chrono::DateTime<chrono::Utc>,
    pub attempt_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSubmissionTerminalWrite {
    pub submission_id: MailSubmissionId,
    pub organization_id: OrganizationId,
    pub lease_id: uuid::Uuid,
    pub status: MailSubmissionStatus,
    pub engine_identity_ref: Option<String>,
    pub engine_submission_ref: Option<String>,
    pub error_code: Option<String>,
}

/// Authoritative terminal timestamp returned by the same PostgreSQL statement
/// that applies a lease-CAS submission transition.
///
/// `terminal_at` is the persisted Intent `updated_at`. For an accepted
/// submission, `accepted_at` must be present and equal to `terminal_at`; every
/// other terminal state must return `None`. Callers use this fact, rather than
/// an application-host clock, for the transaction's Audit and Outbox evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailSubmissionTerminalTransition {
    pub terminal_at: chrono::DateTime<chrono::Utc>,
    pub accepted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailDeliveryRecipientState {
    Queued,
    Delivered,
    PermanentFailure,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDeliveryReconciliationTarget {
    pub submission_id: MailSubmissionId,
    pub organization_id: OrganizationId,
    pub requested_by: PrincipalId,
    pub mailbox_id: MailboxId,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_email_ref: String,
    pub engine_identity_ref: String,
    pub engine_submission_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDeliveryRecipientWrite {
    /// Submission-scoped digest; it cannot correlate one address across mail.
    pub recipient_ref: [u8; 32],
    pub state: MailDeliveryRecipientState,
    pub smtp_reply: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDeliverySnapshotWrite {
    pub snapshot_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub submission_id: MailSubmissionId,
    pub data_plane_id: MailDataPlaneId,
    pub source_state: String,
    pub source_sha256: [u8; 32],
    pub recipients: Vec<MailDeliveryRecipientWrite>,
}

/// Result of appending immutable delivery evidence. The inserted timestamp is
/// selected by the persistence adapter's authoritative clock after binding the
/// exact accepted submission; source/worker clocks never become P3 causal
/// facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailDeliverySnapshotInsert {
    Inserted {
        observed_at: chrono::DateTime<chrono::Utc>,
    },
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDeliverySnapshotRecord {
    pub snapshot_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub submission_id: MailSubmissionId,
    pub source_state: String,
    pub source_sha256: [u8; 32],
    pub recipients: Vec<MailDeliveryRecipientWrite>,
    pub terminal: bool,
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailEngineMailboxBindingWrite {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub data_plane_id: MailDataPlaneId,
    pub account_ref: String,
    pub expected_version: i64,
    pub updated_by: PrincipalId,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Immutable minimal record proposed for authenticated inbound ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMailEventRecord {
    pub event_id: uuid::Uuid,
    pub message_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub data_plane_id: MailDataPlaneId,
    pub source_event_id: String,
    pub source_event_sha256: [u8; 32],
    pub account_ref: String,
    pub document_ref: String,
    pub classification: String,
    pub source_occurred_at: chrono::DateTime<chrono::Utc>,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

/// Result of the two-key replay guard. A source event ID reused with different
/// normalized content is returned as `StoreError::Conflict`, never as replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundMailEventInsert {
    Inserted,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundMailEventKind {
    Delivered,
    PermanentFailure,
    Rescheduled,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMailEventRecord {
    pub event_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub data_plane_id: MailDataPlaneId,
    pub source_event_id: String,
    pub source_event_sha256: [u8; 32],
    pub queue_ref: String,
    pub kind: OutboundMailEventKind,
    pub recipient_ref: Option<[u8; 32]>,
    pub smtp_code: Option<u16>,
    pub next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    pub source_occurred_at: chrono::DateTime<chrono::Utc>,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundMailEventInsert {
    Inserted,
    Duplicate,
}
