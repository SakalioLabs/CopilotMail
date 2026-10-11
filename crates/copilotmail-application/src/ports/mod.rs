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

mod records_identity;
pub use records_identity::*;
mod records_audit;
pub use records_audit::*;
mod records_agent;
pub use records_agent::*;
mod records_mail;
pub use records_mail::*;
mod records_org;
pub use records_org::*;
mod error;
pub use error::*;
mod transaction;
pub use transaction::*;
mod stores;
pub use stores::*;
