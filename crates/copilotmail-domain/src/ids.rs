use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates a time-ordered UUIDv7 identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the wrapped UUID.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

id_type!(
    /// Stable identifier for an organization.
    OrganizationId
);
id_type!(
    /// Stable identifier for a human, Agent, or service principal.
    PrincipalId
);
id_type!(
    /// Stable identifier for one short-lived Human OIDC login transaction.
    HumanLoginTransactionId
);
id_type!(
    /// Stable identifier for one opaque Human browser session.
    HumanSessionId
);
id_type!(
    /// Stable identifier for one append-only Human session revocation.
    HumanSessionRevocationId
);
id_type!(
    /// Stable identifier for a mailbox.
    MailboxId
);
id_type!(
    /// Stable identifier for one independently operated mail data plane.
    ///
    /// Opaque remote account, message, thread, submission, and queue
    /// references are meaningful only inside this identifier's namespace.
    MailDataPlaneId
);
id_type!(
    /// Stable identifier for an organization-managed email domain.
    MailDomainId
);
id_type!(
    /// Stable identifier for one immutable DNS verification observation.
    DnsVerificationId
);
id_type!(
    /// Stable control-plane reference to a message.
    MessageId
);
id_type!(
    /// Stable control-plane identifier for a Human or Agent mail draft.
    DraftId
);
id_type!(
    /// Stable identifier for one recoverable mail-submission intent.
    MailSubmissionId
);
id_type!(
    /// Stable identifier for one approved production-Agent mail-policy activation.
    AgentMailPolicyActivationId
);
id_type!(
    /// Stable identifier for an Agent event subscription.
    SubscriptionId
);
id_type!(
    /// Stable identifier for one outbound Agent webhook subscription.
    WebhookSubscriptionId
);
id_type!(
    /// Stable identifier for an Agent processing lease.
    LeaseId
);
id_type!(
    /// Stable identifier for an audit event.
    AuditEventId
);
id_type!(
    /// Stable identifier for a signed audit-chain checkpoint.
    AuditCheckpointId
);
id_type!(
    /// Stable identifier for one verified immutable audit archive.
    AuditArchiveId
);
id_type!(
    /// Stable identifier for one short-lived governed audit export grant.
    AuditExportId
);
id_type!(
    /// Stable identifier for one immutable audit export download record.
    AuditExportDownloadId
);
id_type!(
    /// Stable identifier for one governed compliance case.
    ComplianceCaseId
);
id_type!(
    /// Stable identifier for one tenant retention policy.
    RetentionPolicyId
);
id_type!(
    /// Stable identifier for one immutable retention-policy version.
    RetentionPolicyVersionId
);
id_type!(
    /// Stable identifier for one approved retention-policy activation.
    RetentionPolicyActivationId
);
id_type!(
    /// Stable identifier for one legal-hold lifecycle.
    LegalHoldId
);
id_type!(
    /// Stable identifier for an authorization role.
    RoleId
);
id_type!(
    /// Stable identifier for a role assignment.
    RoleAssignmentId
);
id_type!(
    /// Stable identifier for a SCIM provisioning credential.
    ScimTokenId
);
id_type!(
    /// Stable identifier for a SCIM directory group.
    ScimGroupId
);
id_type!(
    /// Stable identifier for an Agent authentication credential.
    CredentialId
);
id_type!(
    /// Stable identifier for a mailbox delegation grant.
    DelegationId
);
id_type!(
    /// Stable identifier for an authorization policy.
    PolicyId
);
id_type!(
    /// Stable identifier for one immutable policy version.
    PolicyVersionId
);
id_type!(
    /// Stable identifier for a high-risk action approval request.
    ApprovalRequestId
);
id_type!(
    /// Stable identifier for an approval decision step.
    ApprovalStepId
);
