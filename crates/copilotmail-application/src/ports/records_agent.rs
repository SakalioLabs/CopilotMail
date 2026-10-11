use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailPolicyVersionRecord {
    pub id: Uuid,
    pub policy: AgentMailPolicyVersion,
    pub created_by: PrincipalId,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Privacy-preserving runtime view exposed only to the owning active Agent.
///
/// Recipient and thread ledgers remain deliberately absent. The Agent receives
/// its configured limits and aggregate hourly usage without learning hashed
/// recipient evidence from the control plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailPolicyRuntimeSnapshot {
    pub organization_id: OrganizationId,
    pub agent_id: PrincipalId,
    pub environment: AgentEnvironment,
    pub active_policy: Option<AgentMailPolicyVersion>,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub hourly_window_started_at: chrono::DateTime<chrono::Utc>,
    pub admitted_messages_in_hour: u32,
}

/// Adapter-only identity projection used to construct one privileged Agent
/// Inspector snapshot. The application layer verifies the tenant and kind of
/// both records before exposing either one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorPrincipalCandidate {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub kind: PrincipalKind,
    pub status: PrincipalStatus,
    pub display_name: String,
}

/// Adapter-only Agent profile projection. It deliberately contains no
/// mailbox, credential fingerprint, public key, certificate, or delegation
/// data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorProfileCandidate {
    pub principal_id: PrincipalId,
    pub owner_principal_id: PrincipalId,
    pub environment: AgentEnvironment,
    pub risk_level: copilotmail_domain::AgentRiskLevel,
    pub purpose: String,
    pub review_due_at: chrono::DateTime<chrono::Utc>,
}

/// Aggregate token-issuance evidence for the Inspector. `effective_scopes`
/// is the current intersection of valid credentials and RBAC, never the
/// scope list of an individual credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorTokenAccessCandidate {
    pub active_credential_count: u32,
    pub next_credential_expiry_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_authenticated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub effective_scopes: std::collections::BTreeSet<Permission>,
}

/// Control-plane-only aggregate for the Inspector. It intentionally has no
/// Agent runtime health, deployment, task, mailbox, or message data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorControlPlaneCandidate {
    /// Whether a current production mail-policy activation exists. The
    /// application derives the externally visible admission state from this
    /// value and the Agent environment.
    pub has_active_production_mail_policy: bool,
    pub active_processing_subscription_count: u32,
    pub paused_processing_subscription_count: u32,
    pub retired_processing_subscription_count: u32,
    /// Latest append-only control-plane processing operation, not a runtime
    /// heartbeat or online indicator.
    pub last_processing_activity_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One bounded, same-transaction candidate used by the privileged Human
/// Agent Inspector. Persistence must set `observed_at` from its authoritative
/// clock in the same read boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInspectorSnapshotCandidate {
    pub organization_id: OrganizationId,
    pub agent: AgentInspectorPrincipalCandidate,
    pub owner: AgentInspectorPrincipalCandidate,
    pub profile: AgentInspectorProfileCandidate,
    pub token_access: AgentInspectorTokenAccessCandidate,
    pub control_plane: AgentInspectorControlPlaneCandidate,
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

/// One exact, content-free production-Agent submission evidence candidate.
///
/// Persistence must build this projection in the same transaction as the
/// final privileged-reader authorization proof. It intentionally excludes
/// mailbox, draft, recipient, provider, SMTP, and message-content fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionEvidenceCandidate {
    pub organization_id: OrganizationId,
    pub agent_id: PrincipalId,
    pub submission_id: MailSubmissionId,
    pub submission_status: MailSubmissionStatus,
    pub submission_created_at: chrono::DateTime<chrono::Utc>,
    pub submission_updated_at: chrono::DateTime<chrono::Utc>,
    /// Internal acceptance fact used to validate any delivery observation.
    /// It is intentionally not included in the Human-facing DTO.
    pub submission_accepted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub admission: AgentMailSubmissionAdmissionEvidenceCandidate,
    pub delivery: Option<AgentMailSubmissionDeliveryEvidenceCandidate>,
    /// Immutable request and admission Audit references plus, for terminal
    /// submissions, the exact matching terminal reference. The application
    /// validates their action, cardinality, and order before disclosure.
    pub audit_references: Vec<AgentMailSubmissionAuditReferenceCandidate>,
    /// Persistence-authoritative observation time for this bounded read.
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

/// Immutable production-mail admission facts for one Agent submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionAdmissionEvidenceCandidate {
    pub admission_id: Uuid,
    pub admitted_at: chrono::DateTime<chrono::Utc>,
    pub policy_activation_id: Uuid,
    pub policy_activated_at: chrono::DateTime<chrono::Utc>,
    pub policy_version_id: Uuid,
    pub policy_version: u32,
    /// Dual approval that authorized the exact immutable policy activation.
    pub policy_activation_approval_request_id: ApprovalRequestId,
    /// Persistence revalidated the approval semantics at the immutable
    /// activation timestamp, rather than treating a bare foreign key as proof.
    pub policy_activation_approval_verified: bool,
    pub recipient_count: u16,
    pub first_contact_count: u16,
    /// Optional approval for first-contact recipients. It is absent when the
    /// active policy did not require one for this admission.
    pub first_contact_approval_request_id: Option<ApprovalRequestId>,
    /// `true` when no first-contact approval is present, or when persistence
    /// revalidated its exact draft/version semantics at admission time.
    pub first_contact_approval_verified: bool,
}

/// Latest immutable delivery counters for the exact admitted submission.
/// It is not a delivery guarantee, provider trace, or recipient directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionDeliveryEvidenceCandidate {
    pub snapshot_id: Uuid,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub terminal: bool,
    pub recipient_count: u16,
    pub delivered_count: u16,
    pub failed_count: u16,
    pub queued_count: u16,
    pub unknown_count: u16,
}

/// Minimal immutable Audit reference exposed with one evidence read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailSubmissionAuditReferenceCandidate {
    pub event_id: Uuid,
    pub action: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessingSubscriptionRecord {
    pub subscription_id: SubscriptionId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
    pub status: AgentProcessingSubscriptionStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentProcessingSubscriptionStatus {
    Active,
    Paused,
    Retired,
}

impl AgentProcessingSubscriptionStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Retired => "retired",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessingSubscriptionChangeWrite {
    pub operation_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub subscription_id: SubscriptionId,
    pub expected_status: AgentProcessingSubscriptionStatus,
    pub target_status: AgentProcessingSubscriptionStatus,
    pub idempotency_key_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessingSubscriptionChangeResult {
    pub operation_id: uuid::Uuid,
    pub subscription: AgentProcessingSubscriptionRecord,
    pub previous_status: AgentProcessingSubscriptionStatus,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub replayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageProcessingOperation {
    Claimed,
    LeaseExtended,
    Acknowledged,
    Nacked,
    DeadLettered,
}

impl AgentMessageProcessingOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "claimed",
            Self::LeaseExtended => "lease_extended",
            Self::Acknowledged => "acknowledged",
            Self::Nacked => "nacked",
            Self::DeadLettered => "dead_lettered",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMessageProcessingState {
    Pending,
    Leased,
    Acknowledged,
    DeadLettered,
}

impl AgentMessageProcessingState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Leased => "leased",
            Self::Acknowledged => "acknowledged",
            Self::DeadLettered => "dead_lettered",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessageProcessingResult {
    pub operation_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub subscription_id: SubscriptionId,
    pub message_id: uuid::Uuid,
    pub principal_id: PrincipalId,
    pub lease_id: LeaseId,
    pub operation: AgentMessageProcessingOperation,
    pub state: AgentMessageProcessingState,
    pub attempt_count: u32,
    pub lease_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reason_code: Option<String>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessageClaimWrite {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub subscription_id: SubscriptionId,
    pub lease_id: LeaseId,
    pub idempotency_key_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub lease_expires_at: chrono::DateTime<chrono::Utc>,
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessageTransitionWrite {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub subscription_id: SubscriptionId,
    pub message_id: uuid::Uuid,
    pub lease_id: LeaseId,
    pub operation: AgentMessageProcessingOperation,
    pub idempotency_key_sha256: [u8; 32],
    pub request_sha256: [u8; 32],
    pub lease_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reason_code: Option<String>,
    pub max_attempts: u32,
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessingDeadLetterRecord {
    pub organization_id: OrganizationId,
    pub subscription_id: SubscriptionId,
    pub message_id: uuid::Uuid,
    pub principal_id: PrincipalId,
    pub attempt_count: u32,
    pub reason_code: String,
    pub failed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessingDeadLetterQuery {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub subscription_id: SubscriptionId,
    pub before_message_id: Option<uuid::Uuid>,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessingDeadLetterBatch {
    pub items: Vec<AgentProcessingDeadLetterRecord>,
    pub cursor_valid: bool,
}
