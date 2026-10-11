//! Pure domain types and state machines for the CopilotMail control plane.
//!
//! This crate intentionally has no dependency on HTTP, PostgreSQL, SMTP, or
//! any concrete mail engine. Infrastructure crates adapt these types at the
//! system boundaries.

#![forbid(unsafe_code)]

mod agent_mail_policy;
mod approval;
mod audit;
mod audit_export;
mod authorization;
mod compliance;
mod identity;
mod ids;
mod mail_domain;
mod mailbox;
mod mailbox_authorization;
mod organization;
mod processing;
mod retention;
mod webhook;

pub use agent_mail_policy::{AgentMailPolicyActivation, AgentMailPolicyVersion};
pub use approval::{
    ApprovalAction, ApprovalDecision, ApprovalPolicyVersion, ApprovalRequest, ApprovalStatus,
    ApprovalStep,
};
pub use audit::{
    AUDIT_EVENT_CATALOG, AuditActor, AuditContext, AuditEvent, AuditEventDefinition, AuditOutcome,
    AuditResource, AuditRisk, audit_event_definition,
};
pub use audit_export::AuditExportGrant;
pub use authorization::{
    AuthenticationBinding, AuthorizationContext, Permission, Role, RoleAssignment, SystemRole,
};
pub use compliance::{ComplianceCase, ComplianceCaseStatus, LegalHold, LegalHoldStatus};
pub use identity::{
    AgentCredential, AgentEnvironment, AgentMtlsCredential, AgentProfile, AgentRiskLevel,
    HumanProfile, Principal, PrincipalKind, PrincipalStatus,
};
pub use ids::{
    AgentMailPolicyActivationId, ApprovalRequestId, ApprovalStepId, AuditArchiveId,
    AuditCheckpointId, AuditEventId, AuditExportDownloadId, AuditExportId, ComplianceCaseId,
    CredentialId, DelegationId, DnsVerificationId, DraftId, HumanLoginTransactionId,
    HumanSessionId, HumanSessionRevocationId, LeaseId, LegalHoldId, MailDataPlaneId, MailDomainId,
    MailSubmissionId, MailboxId, MessageId, OrganizationId, PolicyId, PolicyVersionId, PrincipalId,
    RetentionPolicyActivationId, RetentionPolicyId, RetentionPolicyVersionId, RoleAssignmentId,
    RoleId, ScimGroupId, ScimTokenId, SubscriptionId, WebhookSubscriptionId,
};
pub use mail_domain::{
    DnsVerificationChecks, DnsVerificationEvidence, MailDomain, MailDomainName, MailDomainRoute,
    MailDomainRouteMode, MailDomainStatus, MailEngineProjectionStatus,
};
pub use mailbox::{Mailbox, MailboxAddress, MailboxKind, MailboxMembershipRole};
pub use mailbox_authorization::{
    MailboxAccessSnapshot, MailboxAuthorizationDecision, MailboxCapability, MailboxDelegation,
    MailboxGrantSource, MailboxMembership, ResolvedMailboxDelegation,
};
pub use organization::{Organization, OrganizationSlug};
pub use processing::{AgentMessageState, AgentProcessingStatus};
pub use retention::{RetentionPolicy, RetentionPolicyVersion, RetentionResourceKind};
pub use webhook::{WebhookEventType, WebhookSubscription};

/// Reports ASCII controls and Unicode format or bidi marks that can hide or
/// reorder identifier text. Letters, including internationalized ones, are not
/// confusion marks.
#[must_use]
pub fn is_unicode_confusion_mark(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00ad}'
                | '\u{034f}'
                | '\u{061c}'
                | '\u{180e}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{feff}'
                | '\u{fff9}'..='\u{fffb}'
        )
}

/// Errors raised when a domain invariant would be violated.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainError {
    /// A display name was empty or exceeded the supported length.
    #[error("display name must contain 1 to 200 non-whitespace characters")]
    InvalidDisplayName,
    /// An organization slug was not a safe canonical DNS-style label.
    #[error("organization slug must be 3 to 64 lowercase letters, digits, or interior hyphens")]
    InvalidOrganizationSlug,
    /// A role assignment has an empty or invalid lifetime.
    #[error("role assignment validity window is invalid")]
    InvalidRoleAssignmentWindow,
    /// An external issuer or subject was empty, oversized, or contained controls.
    #[error("external identity issuer or subject is invalid")]
    InvalidExternalIdentity,
    /// A human profile was attached to a non-human principal.
    #[error("human profile requires a human principal")]
    InvalidHumanProfile,
    /// Agent ownership, purpose, review, or credential policy was invalid.
    #[error("agent identity or credential policy is invalid")]
    InvalidAgentIdentity,
    /// A production Agent outbound-mail policy exceeded a safety bound.
    #[error("production Agent mail policy is invalid")]
    InvalidAgentMailPolicy,
    /// A mailbox membership, delegation, or resource authorization was invalid.
    #[error("mailbox authorization policy is invalid")]
    InvalidMailboxAuthorization,
    /// An approval request or decision violated expiry, scope, or separation of duty.
    #[error("approval policy or decision is invalid")]
    InvalidApproval,
    /// A compliance case violated identity, text, or lifecycle invariants.
    #[error("invalid compliance case")]
    InvalidComplianceCase,
    /// A legal hold violated case, archive, approval, or lifecycle invariants.
    #[error("invalid legal hold")]
    InvalidLegalHold,
    /// An audit-export authorization exceeded its lifetime or evidence scope.
    #[error("invalid audit export grant")]
    InvalidAuditExport,
    /// A retention policy violated scope, duration, version, or rule bounds.
    #[error("invalid retention policy")]
    InvalidRetentionPolicy,
    /// An email address was not a supported ASCII enterprise mailbox address.
    #[error("invalid mailbox address")]
    InvalidMailboxAddress,
    /// A managed mail domain was not a canonical DNS name.
    #[error("invalid managed mail domain name")]
    InvalidMailDomainName,
    /// DNS evidence was stale, internally inconsistent, or otherwise unsafe.
    #[error("invalid mail domain DNS verification evidence")]
    InvalidDnsVerificationEvidence,
    /// The requested mail-domain lifecycle operation was not permitted.
    #[error("invalid mail domain lifecycle transition")]
    InvalidMailDomainTransition,
    /// Domain routing references, version, or enablement violated policy.
    #[error("invalid mail domain route")]
    InvalidMailDomainRoute,
    /// An Agent webhook endpoint, event selection, key reference, or version was invalid.
    #[error("invalid Agent webhook subscription")]
    InvalidWebhookSubscription,
    /// A lease duration was zero or negative.
    #[error("lease duration must be positive")]
    InvalidLeaseDuration,
    /// A processing operation did not match the active lease.
    #[error("lease does not match the active message lease")]
    LeaseMismatch,
    /// The message is already in a terminal processing state.
    #[error("message is already in a terminal processing state")]
    TerminalProcessingState,
    /// The requested operation is not valid for the current processing state.
    #[error("operation is not valid for the current processing state")]
    InvalidProcessingTransition,
    /// An audit action or resource name was empty or malformed.
    #[error("audit action and resource names must be non-empty dotted identifiers")]
    InvalidAuditIdentifier,
    /// An audit action is not present in the governed event catalog.
    #[error("audit action is not registered in the event catalog")]
    UnregisteredAuditEvent,
    /// Request or governance audit correlation contains invalid values.
    #[error("audit request or governance context is invalid")]
    InvalidAuditContext,
    /// Audit metadata contained secrets, message content, or excessive data.
    #[error("audit details contain forbidden or excessive data")]
    InvalidAuditDetails,
}

#[cfg(test)]
mod tests {
    use super::is_unicode_confusion_mark;

    #[test]
    fn confusion_marks_cover_controls_and_bidi_not_letters() {
        assert!(is_unicode_confusion_mark('\n'));
        assert!(is_unicode_confusion_mark('\r'));
        assert!(is_unicode_confusion_mark('\u{202e}'));
        assert!(is_unicode_confusion_mark('\u{200b}'));
        assert!(is_unicode_confusion_mark('\u{feff}'));
        assert!(is_unicode_confusion_mark('\u{00ad}'));
        assert!(!is_unicode_confusion_mark('a'));
        assert!(!is_unicode_confusion_mark('ü'));
        assert!(!is_unicode_confusion_mark('\u{0430}'));
    }
}
