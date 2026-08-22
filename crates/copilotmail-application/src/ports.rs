use crate::PendingEvent;
use async_trait::async_trait;
use copilotmail_audit::{AuditCheckpointCandidate, SignedAuditCheckpoint};
use copilotmail_domain::{
    AgentCredential, AgentEnvironment, AgentMailPolicyActivation, AgentMailPolicyVersion,
    AgentMtlsCredential, AgentProfile, ApprovalAction, ApprovalDecision, ApprovalPolicyVersion,
    ApprovalRequest, ApprovalRequestId, ApprovalStatus, ApprovalStepId, AuditArchiveId,
    AuditCheckpointId, AuditEvent, AuditExportDownloadId, AuditExportGrant, AuditExportId,
    AuditOutcome, AuthorizationContext, ComplianceCase, ComplianceCaseId, CredentialId,
    DelegationId, DnsVerificationEvidence, DraftId, HumanLoginTransactionId, HumanProfile,
    HumanSessionId, HumanSessionRevocationId, LeaseId, LegalHold, LegalHoldId, MailDataPlaneId,
    MailDomain, MailDomainId, MailDomainName, MailDomainRoute, MailEngineProjectionStatus,
    MailSubmissionId, Mailbox, MailboxAccessSnapshot, MailboxAddress, MailboxDelegation, MailboxId,
    MailboxKind, MailboxMembership, Organization, OrganizationId, OrganizationSlug, Permission,
    PolicyId, PolicyVersionId, Principal, PrincipalId, PrincipalKind, PrincipalStatus,
    RetentionPolicy, RetentionPolicyActivationId, RetentionPolicyId, RetentionPolicyVersion,
    RetentionPolicyVersionId, Role, RoleAssignment, RoleAssignmentId, RoleId, ScimGroupId,
    ScimTokenId, SubscriptionId, SystemRole, WebhookEventType, WebhookSubscription,
    WebhookSubscriptionId,
};
use uuid::Uuid;

/// Fixed browser applications allowed to establish a Human session.
///
/// Keeping this value closed prevents a login transaction from becoming an
/// open redirect or a cross-application cookie confused deputy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanSessionClient {
    Admin,
    Mail,
}

impl HumanSessionClient {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Mail => "mail",
        }
    }
}

/// Digest-only persistence input for a short-lived OIDC login transaction.
///
/// The raw state and browser nonce never cross the application/adapter port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanLoginTransactionWrite {
    pub login_transaction_id: HumanLoginTransactionId,
    pub client_key: HumanSessionClient,
    pub return_to: String,
    pub state_sha256: [u8; 32],
    pub browser_nonce_sha256: [u8; 32],
    pub encrypted_pkce_verifier: Vec<u8>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// One successfully consumed login transaction.
///
/// It intentionally excludes both raw browser correlation values and their
/// digests. The encrypted verifier is opaque to persistence and must only be
/// decrypted by the trusted OIDC boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanLoginTransactionRecord {
    pub login_transaction_id: HumanLoginTransactionId,
    pub client_key: HumanSessionClient,
    pub return_to: String,
    pub encrypted_pkce_verifier: Vec<u8>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Result of the cluster-wide fixed-window admission performed atomically
/// with one pre-authentication login write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanLoginAdmissionResult {
    Admitted,
    RateLimited { retry_after_seconds: u32 },
}

/// Digest-only persistence input for a Human browser session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSessionWrite {
    pub session_id: HumanSessionId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub login_transaction_id: HumanLoginTransactionId,
    pub client_key: HumanSessionClient,
    pub token_sha256: [u8; 32],
    pub oidc_token_sha256: [u8; 32],
    pub oidc_jti: Option<String>,
    pub upstream_expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Non-secret session identity returned to authentication orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSessionRecord {
    pub session_id: HumanSessionId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub login_transaction_id: HumanLoginTransactionId,
    pub client_key: HumanSessionClient,
    pub oidc_jti: Option<String>,
    pub upstream_expires_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Stable, non-free-form cause recorded with a session revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanSessionRevocationCause {
    Logout,
    SecurityResponse,
    PrincipalDeactivated,
    UpstreamRevoked,
}

impl HumanSessionRevocationCause {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Logout => "logout",
            Self::SecurityResponse => "security_response",
            Self::PrincipalDeactivated => "principal_deactivated",
            Self::UpstreamRevoked => "upstream_revoked",
        }
    }
}

/// Result of the append-only revocation write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSessionRevocationResult {
    pub session: HumanSessionRecord,
    pub revocation_id: HumanSessionRevocationId,
    pub cause: HumanSessionRevocationCause,
    pub revoked_at: chrono::DateTime<chrono::Utc>,
    pub newly_revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveApprovalPolicy {
    pub policy_id: PolicyId,
    pub policy_version_id: PolicyVersionId,
    pub version: u32,
    pub action: ApprovalAction,
    pub required_approvals: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalDecisionRecord {
    pub status: ApprovalStatus,
    pub approval_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalDecisionWrite {
    pub organization_id: OrganizationId,
    pub request_id: ApprovalRequestId,
    pub step_id: ApprovalStepId,
    pub approver: PrincipalId,
    pub decision: ApprovalDecision,
    pub reason: String,
    pub decided_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalCancellationWrite {
    pub organization_id: OrganizationId,
    pub request_id: ApprovalRequestId,
    pub requested_by: PrincipalId,
    pub cancelled_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalRequestView {
    Requested,
    Actionable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalRequestListQuery {
    pub organization_id: OrganizationId,
    pub view: ApprovalRequestView,
    pub actor: PrincipalId,
    pub status: Option<ApprovalStatus>,
    pub after_request_id: Option<ApprovalRequestId>,
    pub limit: i64,
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequestSummary {
    pub request_id: ApprovalRequestId,
    pub organization_id: OrganizationId,
    pub policy_version_id: PolicyVersionId,
    pub action: ApprovalAction,
    pub resource_kind: String,
    pub resource_id: String,
    pub requested_by: PrincipalId,
    pub reason: String,
    pub ticket_reference: Option<String>,
    pub required_approvals: u8,
    pub approval_count: u8,
    pub rejection_count: u8,
    pub status: ApprovalStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub cancelled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalStepSummary {
    pub step_id: ApprovalStepId,
    pub approver_principal_id: PrincipalId,
    pub decision: ApprovalDecision,
    pub reason: String,
    pub decided_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequestDetail {
    pub request: ApprovalRequestSummary,
    pub steps: Vec<ApprovalStepSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleSummary {
    pub role_id: RoleId,
    pub organization_id: OrganizationId,
    pub system_role: SystemRole,
    pub display_name: String,
    pub permissions: Vec<Permission>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssignmentSummary {
    pub assignment_id: RoleAssignmentId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub principal_kind: PrincipalKind,
    pub principal_status: PrincipalStatus,
    pub principal_display_name: String,
    pub role_id: RoleId,
    pub system_role: SystemRole,
    pub role_display_name: String,
    pub created_by: PrincipalId,
    pub approval_request_id: Option<ApprovalRequestId>,
    pub valid_from: chrono::DateTime<chrono::Utc>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl RoleAssignmentSummary {
    #[must_use]
    pub fn is_active_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.principal_status == PrincipalStatus::Active
            && self.revoked_at.is_none()
            && self.valid_from <= now
            && self.valid_until.is_none_or(|until| until > now)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleAssignmentGrantRecord {
    pub assignment_id: RoleAssignmentId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub role_id: RoleId,
    pub approval_request_id: ApprovalRequestId,
    pub granted_by: PrincipalId,
    pub valid_from: chrono::DateTime<chrono::Utc>,
    pub valid_until: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleAssignmentRevocationResult {
    Revoked,
    AlreadyRevoked,
    LastActiveOwner,
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleAssignmentListQuery {
    pub organization_id: OrganizationId,
    pub principal_id: Option<PrincipalId>,
    pub include_inactive: bool,
    pub after_assignment_id: Option<RoleAssignmentId>,
    pub limit: i64,
    pub now: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalProof {
    pub organization_id: OrganizationId,
    pub request_id: ApprovalRequestId,
    /// The active Principal executing the approved operation. Approval cannot
    /// be transferred to a different caller.
    pub requested_by: PrincipalId,
    pub action: ApprovalAction,
    pub resource_kind: String,
    pub resource_id: String,
    pub now: chrono::DateTime<chrono::Utc>,
}

/// Exact approval evidence used when an approved action must be bounded by
/// the approval's remaining lifetime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedRequestEvidence {
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub required_approvals: u8,
    pub approval_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingAuditCheckpoint {
    pub checkpoint: SignedAuditCheckpoint,
    pub archive_object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArchivedObjectRecord {
    pub key: String,
    pub version_id: String,
    pub sha256_hex: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AuditArchiveRecord {
    pub archive_id: AuditArchiveId,
    pub organization_id: OrganizationId,
    pub checkpoint_id: AuditCheckpointId,
    pub provider: String,
    pub bucket: String,
    pub events: ArchivedObjectRecord,
    pub manifest: ArchivedObjectRecord,
    pub retention_mode: String,
    pub retain_until: chrono::DateTime<chrono::Utc>,
    pub verified_at: chrono::DateTime<chrono::Utc>,
    pub created_by: PrincipalId,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Names the exact artifact of a WORM archive that an export grant may serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditExportArtifact {
    Events,
    Manifest,
}

impl AuditExportArtifact {
    /// Returns the stable wire and storage representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Manifest => "manifest",
        }
    }
}

/// Append-only evidence that one exact archive object version was served
/// under one exact export grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditExportDownloadRecord {
    pub id: AuditExportDownloadId,
    pub organization_id: OrganizationId,
    pub grant_id: AuditExportId,
    pub archive_id: AuditArchiveId,
    pub downloaded_by: PrincipalId,
    pub artifact: AuditExportArtifact,
    pub object: ArchivedObjectRecord,
    pub downloaded_at: chrono::DateTime<chrono::Utc>,
}

/// Transactional retention-policy identity plus its activation/version
/// cursors. The latest version may remain a draft and therefore differ from
/// the active version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicyRecord {
    pub policy: RetentionPolicy,
    pub active_version_id: Option<RetentionPolicyVersionId>,
    pub latest_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicyDetail {
    pub policy: RetentionPolicy,
    pub active_version: Option<RetentionPolicyVersion>,
    pub latest_version: RetentionPolicyVersion,
}

/// Append-only proof that an exact retention-policy version was activated by
/// its requesting Human under an exact dual approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicyActivationRecord {
    pub id: RetentionPolicyActivationId,
    pub organization_id: OrganizationId,
    pub policy_id: RetentionPolicyId,
    pub policy_version_id: RetentionPolicyVersionId,
    pub approval_request_id: ApprovalRequestId,
    pub activated_by: PrincipalId,
    pub activated_at: chrono::DateTime<chrono::Utc>,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegalHoldObjectEvidence {
    pub events_version_id: String,
    pub manifest_version_id: String,
    pub enabled: bool,
    pub verified_at: chrono::DateTime<chrono::Utc>,
}

/// Bounded, tenant-scoped filters for the append-only audit event read model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEventSearch {
    pub organization_id: OrganizationId,
    pub occurred_from: chrono::DateTime<chrono::Utc>,
    pub occurred_to: chrono::DateTime<chrono::Utc>,
    pub action: Option<String>,
    pub actor_principal_id: Option<PrincipalId>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub outcome: Option<AuditOutcome>,
    pub request_id: Option<String>,
    pub before_chain_sequence: Option<i64>,
    pub limit: i64,
}

/// One immutable audit row exposed to authorized tenant auditors.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEventSummary {
    pub event_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub actor_kind: String,
    pub on_behalf_of_principal_id: Option<PrincipalId>,
    pub credential_id: Option<String>,
    pub subject_principal_id: Option<PrincipalId>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub outcome: AuditOutcome,
    pub trace_id: String,
    pub request_id: String,
    pub source_ip: Option<String>,
    pub client_id: Option<String>,
    pub session_id: Option<String>,
    pub token_jti: Option<String>,
    pub policy_id: Option<PolicyId>,
    pub policy_version_id: Option<PolicyVersionId>,
    pub approval_request_id: Option<ApprovalRequestId>,
    pub ticket_reference: Option<String>,
    pub environment: Option<String>,
    pub region: Option<String>,
    pub details: serde_json::Value,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub received_at: chrono::DateTime<chrono::Utc>,
    pub chain_sequence: i64,
    pub hash_version: i16,
    pub previous_hash_hex: Option<String>,
    pub event_hash_hex: String,
    pub signature_batch_id: Option<uuid::Uuid>,
}

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
    pub now: chrono::DateTime<chrono::Utc>,
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
    pub observed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailDeliverySnapshotInsert {
    Inserted,
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

/// Existing result of an idempotent organization bootstrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingOrganizationBootstrap {
    /// Existing organization identifier.
    pub organization_id: OrganizationId,
    /// Existing root service principal.
    pub root_principal_id: PrincipalId,
    /// Canonical organization slug.
    pub slug: String,
    /// Organization display name.
    pub display_name: String,
}

/// Safe organization projection returned to authorized administration APIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationSummary {
    /// Stable organization identifier.
    pub organization_id: OrganizationId,
    /// Canonical organization slug.
    pub slug: String,
    /// Human-readable display name.
    pub display_name: String,
}

/// Existing result of the one-time OIDC owner handover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingOidcOwnerBinding {
    /// Organization that was handed over.
    pub organization_id: OrganizationId,
    /// Human principal receiving the owner role.
    pub principal_id: PrincipalId,
    /// Exact OIDC issuer.
    pub issuer: String,
    /// Exact OIDC subject.
    pub subject: String,
    /// Human directory display name.
    pub display_name: String,
}

/// Authenticated tenant and audit context for one SCIM credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimCredential {
    pub token_id: ScimTokenId,
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub label: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Stable SCIM projection backed by a Human Principal and HumanProfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingScimUser {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub external_id: String,
    pub user_name: String,
    pub display_name: String,
    pub active: bool,
    pub version: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Existing Human Principal already mapped by OIDC before SCIM takes ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingHumanIdentity {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub display_name: String,
    pub active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Tenant-scoped SCIM group with authoritative direct user membership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingScimGroup {
    pub organization_id: OrganizationId,
    pub group_id: ScimGroupId,
    pub external_id: String,
    pub display_name: String,
    pub members: Vec<PrincipalId>,
    pub version: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Active public-key credential used for Agent client authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingAgentCredential {
    pub credential_id: CredentialId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub public_key_pem: String,
    pub allowed_scopes: std::collections::BTreeSet<Permission>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Active certificate credential resolved only after native TLS verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingAgentMtlsCredential {
    pub credential_id: CredentialId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub certificate_fingerprint: String,
    pub allowed_scopes: std::collections::BTreeSet<Permission>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Persistence failure with a safe high-level classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMailAdmissionFailure {
    RecipientPolicy,
    HourlyQuota,
    RecipientDailyQuota,
    ThreadCircuitBreaker,
    AutomaticReplyLoop,
    DuplicateContentCircuitBreaker,
    SandboxRequired,
    FirstContactApproval,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A unique or check constraint rejected the requested state.
    #[error("requested state conflicts with existing data")]
    Conflict,
    /// A referenced organization, principal, or resource does not exist.
    #[error("referenced resource does not exist")]
    MissingReference,
    /// An Agent message was rejected by one stable outbound safety rule.
    #[error("Agent mail admission was denied")]
    AgentMailAdmission(AgentMailAdmissionFailure),
    /// Infrastructure failed without exposing credentials or database details.
    #[error("control-plane persistence failed")]
    Unavailable,
}

/// Transaction boundary required by application use cases.
#[async_trait]
pub trait ControlPlaneTransaction: Send {
    /// Serializes bootstrap attempts for a slug and returns an existing result.
    async fn find_organization_bootstrap(
        &mut self,
        slug: &OrganizationSlug,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError>;

    /// Inserts an organization.
    async fn insert_organization(&mut self, organization: &Organization) -> Result<(), StoreError>;

    /// Inserts a principal.
    async fn insert_principal(&mut self, principal: &Principal) -> Result<(), StoreError>;

    /// Inserts a tenant-scoped role and its permissions.
    async fn insert_role(&mut self, role: &Role) -> Result<(), StoreError>;

    /// Grants a role to a principal.
    async fn insert_role_assignment(
        &mut self,
        assignment: &RoleAssignment,
    ) -> Result<(), StoreError>;

    /// Links a newly inserted bounded assignment to one exact dual approval.
    async fn insert_role_assignment_grant(
        &mut self,
        _assignment: &RoleAssignment,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_role_assignment_grant_by_approval(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<RoleAssignmentGrantRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes one assignment under a tenant lock and refuses to remove the
    /// last effective Organization Owner.
    async fn revoke_governed_role_assignment(
        &mut self,
        _organization_id: OrganizationId,
        _assignment_id: RoleAssignmentId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<RoleAssignmentRevocationResult, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Records the root service identity created by organization bootstrap.
    async fn record_organization_bootstrap(
        &mut self,
        organization_id: OrganizationId,
        root_principal_id: PrincipalId,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError>;

    /// Serializes owner handover and returns an existing binding for the tenant.
    async fn find_oidc_owner_binding(
        &mut self,
        _organization_id: OrganizationId,
    ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Loads the immutable bootstrap identity for an organization.
    async fn find_organization_bootstrap_by_id(
        &mut self,
        _organization_id: OrganizationId,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Loads a built-in tenant role.
    async fn find_system_role(
        &mut self,
        _organization_id: OrganizationId,
        _system_role: SystemRole,
    ) -> Result<Option<RoleId>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts the exact OIDC issuer/subject mapping for a human.
    async fn insert_human_profile(&mut self, _profile: &HumanProfile) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Records completion of the one-time owner handover.
    async fn record_oidc_owner_binding(
        &mut self,
        _binding: &ExistingOidcOwnerBinding,
        _created_by: PrincipalId,
        _created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Permanently retires a bootstrap principal.
    async fn retire_principal(
        &mut self,
        _principal_id: PrincipalId,
        _retired_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes every active role assignment held by a principal.
    async fn revoke_active_role_assignments(
        &mut self,
        _principal_id: PrincipalId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists a tenant-scoped SCIM credential digest; plaintext is never accepted.
    async fn insert_scim_token(
        &mut self,
        _credential: &ScimCredential,
        _token_hash: &[u8],
        _created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes one active SCIM token within its tenant.
    async fn revoke_scim_token(
        &mut self,
        _organization_id: OrganizationId,
        _token_id: ScimTokenId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Serializes and locates a user by the IdP's immutable external identifier.
    async fn find_scim_user_by_external_id(
        &mut self,
        _organization_id: OrganizationId,
        _external_id: &str,
    ) -> Result<Option<ExistingScimUser>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Adds the SCIM-specific projection for a newly inserted Human Principal.
    async fn insert_scim_user(&mut self, _user: &ExistingScimUser) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locates an existing tenant HumanProfile for SCIM adoption.
    async fn find_human_identity(
        &mut self,
        _organization_id: OrganizationId,
        _identity_provider: &str,
        _external_id: &str,
    ) -> Result<Option<ExistingHumanIdentity>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Updates an existing Human Principal and attaches its SCIM projection.
    async fn adopt_scim_user(&mut self, _user: &ExistingScimUser) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Replaces mutable SCIM user fields and Principal lifecycle using CAS versioning.
    async fn replace_scim_user(
        &mut self,
        _user: &ExistingScimUser,
        _expected_version: i64,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_scim_group_by_external_id(
        &mut self,
        _organization_id: OrganizationId,
        _external_id: &str,
    ) -> Result<Option<ExistingScimGroup>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_scim_group(&mut self, _group: &ExistingScimGroup) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn replace_scim_group(
        &mut self,
        _group: &ExistingScimGroup,
        _expected_version: i64,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn delete_scim_group(
        &mut self,
        _organization_id: OrganizationId,
        _group_id: ScimGroupId,
        _expected_version: Option<i64>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_profile(&mut self, _profile: &AgentProfile) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_credential(
        &mut self,
        _credential: &AgentCredential,
        _public_key_pem: &str,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists an Agent mTLS certificate fingerprint and permitted scopes.
    async fn insert_agent_mtls_credential(
        &mut self,
        _credential: &AgentMtlsCredential,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn active_agent_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn revoke_agent_credential(
        &mut self,
        _organization_id: OrganizationId,
        _credential_id: CredentialId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Returns whether an active principal exists inside the exact tenant.
    async fn active_principal_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Default-deny proof that the target is an active Agent in the tenant.
    async fn active_agent_principal_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Default-deny proof that the actor is an active Human in the tenant.
    async fn active_human_principal_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Final disclosure guard for an exact Human draft body. This method is
    /// intentionally default-deny so an adapter cannot expose content merely
    /// by compiling against a newly extended transaction trait.
    async fn human_draft_read_still_authorized(
        &mut self,
        _proof: &HumanDraftReadRecheck,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Final authorization guard for a content-free Human draft list. Like
    /// body reads, list metadata is returned only after this proof and its
    /// Audit/Outbox evidence commit atomically.
    async fn human_draft_list_still_authorized(
        &mut self,
        _proof: &HumanDraftListRecheck,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Final authorization guard for the content-free Human accepted-
    /// submission directory. Default deny prevents adapters which have not
    /// implemented the exact ownership and snapshot recheck from disclosing
    /// historical delivery metadata.
    async fn human_sent_list_still_authorized(
        &mut self,
        _proof: &HumanSentListRecheck,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Inserts one browser session using only a SHA-256 token digest.
    async fn insert_human_session(&mut self, _write: &HumanSessionWrite) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends an immutable revocation selected by an exact token digest.
    async fn revoke_human_session_by_token_sha256(
        &mut self,
        _token_sha256: &[u8; 32],
        _revocation_id: HumanSessionRevocationId,
        _cause: HumanSessionRevocationCause,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<HumanSessionRevocationResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_webhook_subscription(
        &mut self,
        _subscription: &WebhookSubscription,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_webhook_subscription_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscription>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn update_webhook_subscription(
        &mut self,
        _subscription: &WebhookSubscription,
        _expected_version: i64,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn acquire_agent_event_stream_lease(
        &mut self,
        _lease: &AgentEventStreamLease,
        _tenant_limit: u32,
        _principal_limit: u32,
    ) -> Result<AgentEventStreamLeaseAcquisition, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends a replay marker after an exact latest dead letter and makes the
    /// original Outbox event claimable again. Existing delivery evidence is
    /// never updated or deleted.
    async fn replay_dead_lettered_webhook_delivery(
        &mut self,
        _organization_id: OrganizationId,
        _subscription_id: WebhookSubscriptionId,
        _event_id: uuid::Uuid,
        _requested_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<i32>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts a mailbox owned by the tenant.
    async fn insert_mailbox(&mut self, _mailbox: &Mailbox) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// CAS-upserts the only mail-engine account binding for a tenant mailbox.
    async fn upsert_mail_engine_mailbox_binding(
        &mut self,
        _write: &MailEngineMailboxBindingWrite,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Proves a mailbox exists inside the exact tenant without exposing a
    /// cross-tenant distinction to the caller.
    async fn tenant_mailbox_exists(
        &mut self,
        _organization_id: OrganizationId,
        _mailbox_id: MailboxId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Inserts a globally unique managed domain and its challenge digest.
    async fn insert_mail_domain(
        &mut self,
        _domain: &MailDomain,
        _created_by: PrincipalId,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts the initial disabled route projection.
    async fn insert_mail_domain_route(
        &mut self,
        _route: &MailDomainRoute,
        _updated_by: PrincipalId,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks and loads one tenant-owned domain.
    async fn find_mail_domain_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomain>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists lifecycle state after the row has been locked.
    async fn update_mail_domain(
        &mut self,
        _domain: &MailDomain,
        _updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends challenge-bound DNS evidence.
    async fn insert_mail_domain_dns_observation(
        &mut self,
        _observation: &MailDomainDnsObservation,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Confirms a complete, unexpired observation exists for the current challenge.
    async fn latest_mail_domain_dns_is_activation_ready(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
        _ownership_challenge_sha256: &[u8; 32],
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Assigns a verified domain to one exact mail data plane before its
    /// activation event can become visible. Implementations must use CAS
    /// semantics: an existing different assignment is a conflict.
    async fn assign_mail_domain_data_plane(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
        _data_plane_id: MailDataPlaneId,
        _assigned_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Reads the durable assignment while the domain row is locked. A missing
    /// assignment must fail closed for operations that project remote state.
    async fn assigned_mail_domain_data_plane(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDataPlaneId>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks the versioned route for a domain.
    async fn find_mail_domain_route_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomainRoute>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// CAS-updates a route so stale administrators cannot overwrite each other.
    async fn update_mail_domain_route(
        &mut self,
        _route: &MailDomainRoute,
        _expected_version: i64,
        _updated_by: PrincipalId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts a tenant-bound mailbox membership.
    async fn insert_mailbox_membership(
        &mut self,
        _membership: &MailboxMembership,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Checks an effective, non-expired owner membership.
    async fn mailbox_owned_by(
        &mut self,
        _organization_id: OrganizationId,
        _mailbox_id: MailboxId,
        _principal_id: PrincipalId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts one explicit, time-bounded mailbox delegation.
    async fn insert_mailbox_delegation(
        &mut self,
        _delegation: &MailboxDelegation,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes an active delegation within the exact tenant.
    async fn revoke_mailbox_delegation(
        &mut self,
        _organization_id: OrganizationId,
        _delegation_id: DelegationId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Checks whether the actor currently owns the mailbox referenced by a delegation.
    async fn mailbox_delegation_managed_by(
        &mut self,
        _organization_id: OrganizationId,
        _delegation_id: DelegationId,
        _actor: PrincipalId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Creates the stable policy identity.
    async fn insert_approval_policy(
        &mut self,
        _policy_id: PolicyId,
        _organization_id: OrganizationId,
        _policy_key: &str,
        _display_name: &str,
        _created_by: PrincipalId,
        _created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends one immutable policy version.
    async fn insert_approval_policy_version(
        &mut self,
        _version: &ApprovalPolicyVersion,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Atomically switches the policy's active immutable version.
    async fn activate_approval_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: PolicyId,
        _policy_version_id: PolicyVersionId,
        _expected_active_version_id: Option<PolicyVersionId>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Resolves an active policy under transaction consistency.
    async fn find_active_approval_policy(
        &mut self,
        _organization_id: OrganizationId,
        _policy_key: &str,
    ) -> Result<Option<ActiveApprovalPolicy>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts a policy-version-bound approval request.
    async fn insert_approval_request(
        &mut self,
        _request: &ApprovalRequest,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks the request and appends one irreversible decision.
    async fn record_approval_decision(
        &mut self,
        _decision: &ApprovalDecisionWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Withdraws one live pending request only when the actor is its original
    /// requester. The guarded status transition is committed with audit/outbox.
    async fn cancel_approval_request(
        &mut self,
        _cancellation: &ApprovalCancellationWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists an immutable signed audit checkpoint.
    async fn insert_audit_checkpoint(
        &mut self,
        _checkpoint: &SignedAuditCheckpoint,
        _archive_object_key: &str,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Finds an immutable checkpoint for exact idempotent replay.
    async fn find_audit_checkpoint(
        &mut self,
        _organization_id: OrganizationId,
        _checkpoint_id: copilotmail_domain::AuditCheckpointId,
    ) -> Result<Option<ExistingAuditCheckpoint>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists evidence only after exact remote Object Lock verification.
    async fn insert_audit_archive(
        &mut self,
        _archive: &AuditArchiveRecord,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Finds immutable archive evidence for exact idempotent replay.
    async fn find_audit_archive(
        &mut self,
        _organization_id: OrganizationId,
        _archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Creates one immutable, approval-bound export grant or returns the
    /// existing grant when the exact approval is replayed.
    async fn authorize_audit_export_grant(
        &mut self,
        _grant: &AuditExportGrant,
    ) -> Result<AuditExportGrant, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends immutable evidence that one exact archive object version was
    /// served. The database independently reproves the grant, the requesting
    /// Human, the grant lifetime, and the exact object identity.
    async fn record_audit_export_download(
        &mut self,
        _download: &AuditExportDownloadRecord,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_retention_policy(
        &mut self,
        _policy: &RetentionPolicy,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_retention_policy_version(
        &mut self,
        _version: &RetentionPolicyVersion,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_retention_policy_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_retention_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: RetentionPolicyId,
        _version_id: RetentionPolicyVersionId,
    ) -> Result<Option<RetentionPolicyVersion>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_retention_policy_activation(
        &mut self,
        _activation: &RetentionPolicyActivationRecord,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_retention_policy_activation_by_approval(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<RetentionPolicyActivationRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn activate_retention_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: RetentionPolicyId,
        _version_id: RetentionPolicyVersionId,
        _expected_active_version_id: Option<RetentionPolicyVersionId>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_agent_mail_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _agent_id: PrincipalId,
        _policy_version_id: Uuid,
    ) -> Result<Option<AgentMailPolicyVersionRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn next_agent_mail_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _agent_id: PrincipalId,
    ) -> Result<u32, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_mail_policy_version(
        &mut self,
        _record: &AgentMailPolicyVersionRecord,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_agent_mail_policy_activation_by_approval(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<AgentMailPolicyActivation>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_mail_policy_activation(
        &mut self,
        _activation: &AgentMailPolicyActivation,
        _policy_version_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn ensure_agent_processing_subscription(
        &mut self,
        _record: &AgentProcessingSubscriptionRecord,
    ) -> Result<(AgentProcessingSubscriptionRecord, bool), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn change_agent_processing_subscription(
        &mut self,
        _write: &AgentProcessingSubscriptionChangeWrite,
    ) -> Result<Option<AgentProcessingSubscriptionChangeResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn claim_next_agent_message(
        &mut self,
        _write: &AgentMessageClaimWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn transition_agent_message(
        &mut self,
        _write: &AgentMessageTransitionWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_compliance_case(&mut self, _case: &ComplianceCase) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_compliance_case_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn update_compliance_case(&mut self, _case: &ComplianceCase) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn active_legal_hold_exists_for_case(
        &mut self,
        _organization_id: OrganizationId,
        _case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_legal_hold(
        &mut self,
        _hold: &LegalHold,
        _evidence: &LegalHoldObjectEvidence,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_legal_hold_by_apply_approval_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_legal_hold_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn update_legal_hold(
        &mut self,
        _hold: &LegalHold,
        _evidence: &LegalHoldObjectEvidence,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Freezes one exact owned draft and creates its idempotent submission
    /// intent. Exact replay returns the existing intent without new writes.
    async fn create_mail_submission_intent(
        &mut self,
        _write: &MailSubmissionIntentWrite,
    ) -> Result<MailSubmissionIntentCreation, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Freezes one exact testing-Agent draft and captures immutable sandbox
    /// evidence without creating a deliverable mail submission Intent.
    async fn create_agent_mail_sandbox_submission(
        &mut self,
        _write: &AgentMailSandboxSubmissionWrite,
    ) -> Result<AgentMailSandboxSubmissionCreation, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Commits ownership before any remote JMAP operation. Expired leases are
    /// reclaimable; active and terminal intents are not.
    async fn acquire_mail_submission_lease(
        &mut self,
        _submission_id: MailSubmissionId,
        _expected_data_plane_id: MailDataPlaneId,
        _lease_id: uuid::Uuid,
        _now: chrono::DateTime<chrono::Utc>,
        _lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MailSubmissionLease>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Applies one lease-CAS terminal transition. Accepted transitions also
    /// move the exact frozen draft to `sent` in this transaction.
    async fn finalize_mail_submission_intent(
        &mut self,
        _write: &MailSubmissionTerminalWrite,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends one complete recipient-level delivery snapshot. Exact replay is
    /// idempotent; changed content under an existing source identity conflicts.
    async fn append_mail_delivery_snapshot(
        &mut self,
        _write: &MailDeliverySnapshotWrite,
    ) -> Result<MailDeliverySnapshotInsert, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends an audit event in the same transaction as the state change.
    async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError>;

    /// Enqueues an integration event in the same transaction as the state change.
    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError>;

    /// Atomically commits all writes.
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

/// Factory for application transaction boundaries.
#[async_trait]
pub trait ControlPlaneStore: Send + Sync {
    /// Begins an isolated unit of work.
    async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError>;
}

/// Specialized transaction for high-volume operational mail events. It keeps
/// source receipts and Outbox publication atomic without misattributing a
/// machine-generated event to a Human audit actor.
#[async_trait]
pub trait MailEventIngestionTransaction: Send {
    async fn resolve_mail_engine_mailbox_binding(
        &mut self,
        data_plane_id: MailDataPlaneId,
        account_ref: &str,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError>;

    async fn insert_inbound_mail_event(
        &mut self,
        record: &InboundMailEventRecord,
    ) -> Result<InboundMailEventInsert, StoreError>;

    async fn fanout_agent_message_states(
        &mut self,
        _record: &InboundMailEventRecord,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Resolves an outbound Envelope From only when it maps to exactly one
    /// currently bound control-plane mailbox.
    async fn resolve_mail_engine_mailbox_by_primary_address(
        &mut self,
        data_plane_id: MailDataPlaneId,
        primary_address: &MailboxAddress,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError>;

    async fn insert_outbound_mail_event(
        &mut self,
        record: &OutboundMailEventRecord,
    ) -> Result<OutboundMailEventInsert, StoreError>;

    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError>;

    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

/// Factory for the isolated authenticated mail-event ingestion boundary.
#[async_trait]
pub trait MailEventIngestionStore: Send + Sync {
    async fn begin_mail_event_ingestion(
        &self,
    ) -> Result<Box<dyn MailEventIngestionTransaction>, StoreError>;
}

/// Resolves effective permissions only for active principals and assignments.
#[async_trait]
pub trait AuthorizationStore: Send + Sync {
    /// Resolves the authenticated principal's tenant-scoped permission snapshot.
    async fn resolve_authorization(
        &self,
        principal_id: PrincipalId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<AuthorizationContext>, StoreError>;
}

/// Short-lived, pre-authentication OIDC login transaction storage.
///
/// Both operations are single-statement atomic adapter boundaries because no
/// authenticated tenant actor exists yet. Session creation after the callback
/// uses the regular audited control-plane transaction.
#[async_trait]
pub trait HumanLoginTransactionStore: Send + Sync {
    /// Applies one database-authoritative fixed-window admission for
    /// `write.client_key`, prunes at most `cleanup_limit` eligible expired
    /// transactions, and persists the new digest-only login evidence in one
    /// transaction. A rejected attempt must not insert a login transaction.
    async fn admit_human_login_transaction(
        &self,
        write: &HumanLoginTransactionWrite,
        admission_limit: u32,
        cleanup_limit: u32,
    ) -> Result<HumanLoginAdmissionResult, StoreError>;

    /// Atomically consumes one unexpired transaction matching both browser
    /// correlation digests. A transaction can be consumed only once.
    async fn consume_human_login_transaction(
        &self,
        state_sha256: &[u8; 32],
        browser_nonce_sha256: &[u8; 32],
        consumed_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<HumanLoginTransactionRecord>, StoreError>;
}

/// Resolves an active Human browser session from a token digest.
///
/// Implementations must re-check session expiry, revocation, tenant binding,
/// and current Human Principal activity on every lookup.
#[async_trait]
pub trait HumanSessionQueryStore: Send + Sync {
    async fn resolve_human_session_by_token_sha256(
        &self,
        token_sha256: &[u8; 32],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<HumanSessionRecord>, StoreError>;
}

/// Resolves resource-level mailbox grants for an authenticated principal.
#[async_trait]
pub trait MailboxAuthorizationStore: Send + Sync {
    /// Allows active Humans, non-production Agents, and production Agents with
    /// a committed active policy candidate. Per-message production admission
    /// remains a separate transactional decision.
    async fn principal_can_submit_mail(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Returns the exact latest active policy only when the principal is an
    /// active production Agent in this tenant.
    async fn active_production_agent_mail_policy(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyVersion>, StoreError> {
        Ok(None)
    }

    /// Returns the environment only for an active Agent in the exact tenant.
    async fn active_agent_environment(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<Option<AgentEnvironment>, StoreError> {
        Ok(None)
    }

    /// Returns only grants that are active at `now`; missing and inaccessible
    /// mailboxes deliberately share the same `None` representation.
    async fn resolve_mailbox_access(
        &self,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MailboxAccessSnapshot>, StoreError>;
}

/// Reads the Human mailbox switcher directory from one consistent snapshot.
#[async_trait]
pub trait HumanMailboxDirectoryStore: Send + Sync {
    /// Returns only same-tenant mailboxes with a current data-plane binding and
    /// a SQL-level `Read(None)` grant. The application layer intentionally
    /// repeats the pure authorization decision before returning any item.
    async fn list_human_mailbox_directory(
        &self,
        query: &HumanMailboxDirectoryQuery,
    ) -> Result<HumanMailboxDirectoryBatch, StoreError>;
}

/// Reads the authenticated Agent's own outbound policy and aggregate usage.
#[async_trait]
pub trait AgentMailPolicyQueryStore: Send + Sync {
    async fn get_agent_mail_policy_runtime(
        &self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyRuntimeSnapshot>, StoreError>;
}

#[async_trait]
pub trait AgentProcessingQueryStore: Send + Sync {
    async fn get_agent_processing_subscription(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        subscription_id: SubscriptionId,
    ) -> Result<Option<AgentProcessingSubscriptionRecord>, StoreError>;

    async fn list_agent_processing_dead_letters(
        &self,
        query: &AgentProcessingDeadLetterQuery,
    ) -> Result<AgentProcessingDeadLetterBatch, StoreError>;
}

#[async_trait]
pub trait MailboxQueryStore: Send + Sync {
    async fn get_mailbox(
        &self,
        mailbox_id: MailboxId,
    ) -> Result<Option<MailboxSummary>, StoreError>;

    /// Resolves only the exact tenant mailbox binding used for outbound
    /// content access. Missing and cross-tenant rows share `None`.
    async fn get_mail_engine_mailbox_binding(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError>;
}

#[async_trait]
pub trait MailDraftMetadataStore: Send + Sync {
    async fn get_mail_draft(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        created_by: PrincipalId,
        draft_id: DraftId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDraftRecord>, StoreError>;

    /// Inserts a first remote version or replaces an active version using CAS.
    async fn record_mail_draft(&self, write: MailDraftWrite)
    -> Result<MailDraftRecord, StoreError>;
}

/// Database-authoritative queries for an active Human's private draft view.
///
/// Shared mailbox membership is not draft collaboration authority: both
/// methods must scope rows to `created_by == principal_id`. Missing, denied,
/// cross-tenant, stale-binding, and other-creator targets must not leak a row.
#[async_trait]
pub trait HumanDraftQueryStore: Send + Sync {
    async fn list_human_drafts(
        &self,
        query: &HumanDraftListQuery,
    ) -> Result<HumanDraftListBatch, StoreError>;

    async fn resolve_human_draft_restore_target(
        &self,
        query: &HumanDraftRestoreQuery,
    ) -> Result<HumanDraftRestoreResolution, StoreError>;
}

/// Database-authoritative queries for an active Human's private accepted-
/// submission directory. Shared mailbox membership is never permission to
/// browse another sender's history; rows must be scoped to
/// `requested_by == principal_id` and must not load message content or
/// recipient evidence.
#[async_trait]
pub trait HumanSentQueryStore: Send + Sync {
    async fn list_human_sent_submissions(
        &self,
        query: &HumanSentListQuery,
    ) -> Result<HumanSentListBatch, StoreError>;
}

#[async_trait]
pub trait MailSubmissionQueryStore: Send + Sync {
    /// Human-facing lookup. Missing, cross-tenant, and other-actor intents are
    /// deliberately indistinguishable.
    async fn get_mail_submission_intent(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError>;

    /// Internal worker lookup used only to distinguish an already-terminal
    /// Intent from a currently leased Intent after Outbox redelivery.
    async fn get_mail_submission_intent_for_execution(
        &self,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError>;
}

#[async_trait]
pub trait MailDeliveryQueryStore: Send + Sync {
    /// Internal worker lookup. Only accepted submissions with complete remote
    /// bindings are eligible for reconciliation.
    async fn get_mail_delivery_reconciliation_target(
        &self,
        submission_id: MailSubmissionId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDeliveryReconciliationTarget>, StoreError>;

    /// Human-facing latest snapshot lookup with tenant/requester isolation.
    async fn get_latest_mail_delivery_snapshot(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailDeliverySnapshotRecord>, StoreError>;
}

#[async_trait]
pub trait MailDomainQueryStore: Send + Sync {
    async fn get_mail_domain(
        &self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MailDomainSummary>, StoreError>;

    /// Lists domains after an exclusive canonical-name cursor.
    async fn list_mail_domains(
        &self,
        organization_id: OrganizationId,
        after_name: Option<&MailDomainName>,
        limit: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<MailDomainSummary>, StoreError>;
}

#[async_trait]
pub trait WebhookSubscriptionQueryStore: Send + Sync {
    async fn get_webhook_subscription(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscriptionSummary>, StoreError>;

    async fn list_webhook_subscriptions(
        &self,
        organization_id: OrganizationId,
        principal_id: Option<PrincipalId>,
        after_id: Option<WebhookSubscriptionId>,
        limit: i64,
    ) -> Result<Vec<WebhookSubscriptionSummary>, StoreError>;
}

#[async_trait]
pub trait WebhookDeadLetterQueryStore: Send + Sync {
    async fn list_webhook_dead_letters(
        &self,
        organization_id: OrganizationId,
        after_delivery_id: Option<uuid::Uuid>,
        limit: i64,
    ) -> Result<Vec<WebhookDeadLetterSummary>, StoreError>;
}

#[async_trait]
pub trait AgentEventCursorQueryStore: Send + Sync {
    /// Proves that a cursor anchor is an eligible event for the exact
    /// subscription. Missing anchors are rejected rather than treated as an
    /// arbitrary ordering key.
    async fn webhook_cursor_event_exists(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        event_id: uuid::Uuid,
    ) -> Result<bool, StoreError>;

    async fn list_webhook_cursor_events(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        after_event_id: Option<uuid::Uuid>,
        limit: i64,
    ) -> Result<Vec<AgentCursorEvent>, StoreError>;

    async fn release_agent_event_stream_lease(
        &self,
        _lease: &AgentEventStreamLease,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }
}

/// Cluster-wide admission result for one authenticated Agent Cursor/SSE
/// request. Retry delays are derived from the database clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentIngressRateLimitDecision {
    Allowed,
    TenantLimited { retry_after_seconds: u32 },
    PrincipalLimited { retry_after_seconds: u32 },
}

#[async_trait]
pub trait AgentIngressRateLimitStore: Send + Sync {
    /// Atomically consumes both tenant and Agent capacity. A principal
    /// rejection must not consume tenant capacity.
    async fn consume_agent_ingress_capacity(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        tenant_requests_per_minute: u32,
        principal_requests_per_minute: u32,
    ) -> Result<AgentIngressRateLimitDecision, StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailContentTarget {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub message_id: uuid::Uuid,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_document_ref: String,
}

/// One existing inbound message projected from an engine document identifier
/// to the externally safe CopilotMail UUID.
///
/// Batch callers must require exactly one unique projection for every unique
/// requested document reference. Missing or duplicate rows are deliberately
/// returned for the application layer to reject fail closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailMessageProjection {
    pub message_id: uuid::Uuid,
    pub engine_document_ref: String,
}

/// Exact current engine target resolved from an externally safe Human-mail
/// message UUID. Authorization remains an application-layer responsibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailContentTarget {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub message_id: uuid::Uuid,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_document_ref: String,
}

#[async_trait]
pub trait HumanMailContentQueryStore: Send + Sync {
    /// Resolves existing inbound engine document references in caller order.
    ///
    /// Implementations must join the current mailbox binding and must never
    /// create an identifier for an unprojected document. Missing inputs
    /// produce no row and duplicate inputs remain duplicate rows so callers
    /// can reject either condition without disclosing a partial page.
    async fn resolve_human_mail_message_projections(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
        engine_account_ref: &str,
        document_refs: &[String],
    ) -> Result<Vec<HumanMailMessageProjection>, StoreError>;

    /// Resolves one safe UUID only when its immutable inbound event still
    /// matches the exact tenant, mailbox, and current engine binding.
    async fn resolve_human_mail_content_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: uuid::Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<HumanMailContentTarget>, StoreError>;
}

/// Exact current engine target resolved from an externally safe inbound
/// message UUID. Authorization remains an application-layer responsibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailReplyTarget {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub message_id: uuid::Uuid,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_document_ref: String,
}

#[async_trait]
pub trait MailReplyTargetQueryStore: Send + Sync {
    async fn resolve_mail_reply_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: uuid::Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailReplyTarget>, StoreError>;

    /// Maps an already-authorized Human inbox engine document back to its
    /// externally safe inbound UUID. Implementations must enforce the current
    /// mailbox binding and exact tenant/mailbox tuple.
    async fn resolve_mail_reply_message_id_by_engine_document(
        &self,
        _organization_id: OrganizationId,
        _mailbox_id: MailboxId,
        _expected_data_plane_id: MailDataPlaneId,
        _engine_document_ref: &str,
    ) -> Result<Option<uuid::Uuid>, StoreError> {
        Err(StoreError::Unavailable)
    }
}

#[async_trait]
pub trait AgentMailContentQueryStore: Send + Sync {
    /// Resolves an externally safe CopilotMail message UUID to one exact
    /// current engine document only for an active Agent.
    async fn resolve_agent_mail_content_target(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        message_id: uuid::Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<AgentMailContentTarget>, StoreError>;
}

/// Read-side proof that an approval is exact, complete, and unexpired.
#[async_trait]
pub trait ApprovalQueryStore: Send + Sync {
    async fn approval_permits(&self, proof: &ApprovalProof) -> Result<bool, StoreError>;

    async fn approved_request_evidence(
        &self,
        _proof: &ApprovalProof,
    ) -> Result<Option<ApprovedRequestEvidence>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn get_approval_request(
        &self,
        _organization_id: OrganizationId,
        _request_id: ApprovalRequestId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ApprovalRequestDetail>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn approval_request_cursor_exists(
        &self,
        _organization_id: OrganizationId,
        _view: ApprovalRequestView,
        _actor: PrincipalId,
        _status: Option<ApprovalStatus>,
        _request_id: ApprovalRequestId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn list_approval_requests(
        &self,
        _query: ApprovalRequestListQuery,
    ) -> Result<Vec<ApprovalRequestSummary>, StoreError> {
        Err(StoreError::Unavailable)
    }
}

#[async_trait]
pub trait RoleGovernanceQueryStore: Send + Sync {
    async fn list_roles(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<RoleSummary>, StoreError>;

    async fn get_role_assignment(
        &self,
        organization_id: OrganizationId,
        assignment_id: RoleAssignmentId,
    ) -> Result<Option<RoleAssignmentSummary>, StoreError>;

    async fn role_assignment_cursor_exists(
        &self,
        query: RoleAssignmentListQuery,
        assignment_id: RoleAssignmentId,
    ) -> Result<bool, StoreError>;

    async fn list_role_assignments(
        &self,
        query: RoleAssignmentListQuery,
    ) -> Result<Vec<RoleAssignmentSummary>, StoreError>;
}

#[async_trait]
pub trait AuditExportQueryStore: Send + Sync {
    async fn get_audit_export_grant(
        &self,
        organization_id: OrganizationId,
        grant_id: AuditExportId,
    ) -> Result<Option<AuditExportGrant>, StoreError>;

    async fn get_audit_export_grant_by_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: ApprovalRequestId,
    ) -> Result<Option<AuditExportGrant>, StoreError>;
}

#[async_trait]
pub trait RetentionPolicyQueryStore: Send + Sync {
    async fn get_retention_policy(
        &self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError>;
}

/// Tenant-scoped read model used by Legal Hold orchestration before remote
/// object-version operations begin.
#[async_trait]
pub trait ComplianceQueryStore: Send + Sync {
    async fn active_human_compliance_actor_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError>;

    async fn get_compliance_case(
        &self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError>;

    async fn compliance_case_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::ComplianceCaseStatus>,
        case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError>;

    async fn list_compliance_cases(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::ComplianceCaseStatus>,
        after_case_id: Option<ComplianceCaseId>,
        limit: i64,
    ) -> Result<Vec<ComplianceCase>, StoreError>;

    async fn get_audit_archive(
        &self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError>;

    async fn get_legal_hold(
        &self,
        organization_id: OrganizationId,
        hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError>;

    async fn legal_hold_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        hold_id: LegalHoldId,
    ) -> Result<bool, StoreError>;

    async fn list_legal_holds(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        after_hold_id: Option<LegalHoldId>,
        limit: i64,
    ) -> Result<Vec<LegalHold>, StoreError>;

    async fn get_legal_hold_by_apply_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError>;
}

/// Produces a fully reverified chain-head candidate for checkpoint signing.
#[async_trait]
pub trait AuditCheckpointQueryStore: Send + Sync {
    async fn audit_checkpoint_candidate(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<AuditCheckpointCandidate>, StoreError>;
}

#[async_trait]
pub trait AuditEventQueryStore: Send + Sync {
    /// Cheap pre-query guard. The Human status is checked again in the audit
    /// write transaction before any result is returned.
    async fn active_human_audit_reader_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError>;

    /// Rejects fabricated ordering positions by proving the cursor anchor
    /// matches the exact tenant and active filters.
    async fn audit_event_search_anchor_exists(
        &self,
        search: &AuditEventSearch,
        chain_sequence: i64,
    ) -> Result<bool, StoreError>;

    async fn search_audit_events(
        &self,
        search: &AuditEventSearch,
    ) -> Result<Vec<AuditEventSummary>, StoreError>;
}

/// Read-side organization projection.
#[async_trait]
pub trait OrganizationQueryStore: Send + Sync {
    /// Loads an organization by stable ID.
    async fn get_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<OrganizationSummary>, StoreError>;
}

/// Maps a verified enterprise identity to an internal human principal.
#[async_trait]
pub trait ExternalIdentityResolver: Send + Sync {
    /// Resolves an exact issuer and subject pair for an active human principal.
    async fn resolve_human_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<PrincipalId>, StoreError>;
}

/// Authenticates SCIM credentials and serves tenant-isolated directory projections.
#[async_trait]
pub trait ScimDirectoryQueryStore: Send + Sync {
    /// Resolves an active, non-expired credential by a fixed-length token digest.
    async fn authenticate_scim_token(
        &self,
        token_hash: &[u8],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ScimCredential>, StoreError>;

    /// Loads one SCIM user, scoped to the credential's organization.
    async fn get_scim_user(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<ExistingScimUser>, StoreError>;

    /// Lists SCIM users with bounded offset pagination and optional exact filters.
    async fn list_scim_users(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        user_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimUser>, i64), StoreError>;

    async fn get_scim_group(
        &self,
        organization_id: OrganizationId,
        group_id: ScimGroupId,
    ) -> Result<Option<ExistingScimGroup>, StoreError>;

    async fn list_scim_groups(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        display_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimGroup>, i64), StoreError>;
}

/// Public-key Agent credential and token-revocation persistence boundary.
#[async_trait]
pub trait AgentCredentialStore: Send + Sync {
    /// Loads an active credential for cryptographic client assertion validation.
    async fn get_agent_credential(
        &self,
        client_id: PrincipalId,
        credential_id: CredentialId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ExistingAgentCredential>, StoreError>;

    /// Atomically consumes a client assertion JTI and updates usage metadata.
    async fn consume_agent_assertion(
        &self,
        credential_id: CredentialId,
        jti_hash: &[u8],
        assertion_expires_at: chrono::DateTime<chrono::Utc>,
        source: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError>;

    /// Resolves and records use of an active credential from a TLS-verified
    /// leaf certificate fingerprint.
    async fn authenticate_agent_mtls(
        &self,
        client_id: PrincipalId,
        certificate_fingerprint: &str,
        source: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ExistingAgentMtlsCredential>, StoreError>;

    /// Rechecks live Principal/credential state and token scopes on every request.
    async fn validate_agent_token_binding(
        &self,
        principal_id: PrincipalId,
        credential_id: CredentialId,
        scopes: &std::collections::BTreeSet<Permission>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError>;
}
