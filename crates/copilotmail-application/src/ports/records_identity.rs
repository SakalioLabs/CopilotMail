use super::*;

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

/// Revokes every still-valid browser session for one principal.
///
/// Expired sessions are already unusable and are left untouched. `excluded_session_id`
/// keeps one session, used when an operator must not revoke the request that is
/// still committing this change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevokeActiveHumanSessions {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub cause: HumanSessionRevocationCause,
    pub revoked_at: chrono::DateTime<chrono::Utc>,
    pub excluded_session_id: Option<HumanSessionId>,
}

/// Consumes one exact approval so it cannot revoke sessions created later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanSessionGovernanceExecutionWrite {
    pub approval_request_id: ApprovalRequestId,
    pub organization_id: OrganizationId,
    pub target_principal_id: PrincipalId,
    pub requested_by: PrincipalId,
    pub excluded_session_id: Option<HumanSessionId>,
    pub executed_at: chrono::DateTime<chrono::Utc>,
}

/// Result of the single-use governance execution insert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanSessionGovernanceExecution {
    Inserted,
    AlreadyExecuted {
        target_principal_id: PrincipalId,
        requested_by: PrincipalId,
    },
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
/// the approval's remaining lifetime. The query store returns it only after
/// checking current validity against its persistence-authoritative clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedRequestEvidence {
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub required_approvals: u8,
    pub approval_count: u8,
}
