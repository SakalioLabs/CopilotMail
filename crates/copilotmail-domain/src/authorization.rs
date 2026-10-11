use crate::{
    CredentialId, DomainError, HumanSessionId, OrganizationId, PrincipalId, RoleAssignmentId,
    RoleId, ScimTokenId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Stable capabilities used by both human and Agent authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read tenant metadata and health.
    OrganizationRead,
    /// Change tenant metadata and lifecycle.
    OrganizationManage,
    /// Manage human, Agent, and service principals.
    IdentityManage,
    /// Manage roles, assignments, policy, and delegation.
    AuthorizationManage,
    /// Manage domains, routing, mailboxes, and delivery settings.
    MailAdmin,
    /// Manage security controls and incident response.
    SecurityAdmin,
    /// Revoke another principal's browser sessions. This is not implied by
    /// `identity.manage` or `security.admin`.
    SessionRevokeOthers,
    /// Search append-only audit metadata.
    AuditRead,
    /// Export audit evidence.
    AuditExport,
    /// Manage case-bound, approval-gated legal holds.
    LegalHoldManage,
    /// Create, version, and activate tenant retention policy.
    RetentionManage,
    /// Review message content only through the compliance case workflow.
    ContentReview,
    /// Operate Agent credentials, subscriptions, and delivery state.
    AgentOperate,
    /// Read bounded plain-text content for an exact mailbox-assigned Agent.
    AgentMailRead,
    /// Use mailboxes assigned to the principal.
    MailboxUse,
    /// Invoke the separately controlled break-glass workflow.
    BreakGlass,
}

impl Permission {
    /// Returns the stable database and policy identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrganizationRead => "organization.read",
            Self::OrganizationManage => "organization.manage",
            Self::IdentityManage => "identity.manage",
            Self::AuthorizationManage => "authorization.manage",
            Self::MailAdmin => "mail.admin",
            Self::SecurityAdmin => "security.admin",
            Self::SessionRevokeOthers => "session.revoke_others",
            Self::AuditRead => "audit.read",
            Self::AuditExport => "audit.export",
            Self::LegalHoldManage => "legal_hold.manage",
            Self::RetentionManage => "retention.manage",
            Self::ContentReview => "content.review",
            Self::AgentOperate => "agent.operate",
            Self::AgentMailRead => "agent.mail.read",
            Self::MailboxUse => "mailbox.use",
            Self::BreakGlass => "break_glass.invoke",
        }
    }

    /// Complete stable permission catalog.
    pub const CATALOG: [Self; 16] = [
        Self::OrganizationRead,
        Self::OrganizationManage,
        Self::IdentityManage,
        Self::AuthorizationManage,
        Self::MailAdmin,
        Self::SecurityAdmin,
        Self::SessionRevokeOthers,
        Self::AuditRead,
        Self::AuditExport,
        Self::LegalHoldManage,
        Self::RetentionManage,
        Self::ContentReview,
        Self::AgentOperate,
        Self::AgentMailRead,
        Self::MailboxUse,
        Self::BreakGlass,
    ];

    /// Initial organization-owner capabilities.
    ///
    /// Content review and break-glass are intentionally excluded because they
    /// require separate roles, case context, and additional approval.
    pub const ORG_OWNER: [Self; 13] = [
        Self::OrganizationRead,
        Self::OrganizationManage,
        Self::IdentityManage,
        Self::AuthorizationManage,
        Self::MailAdmin,
        Self::SecurityAdmin,
        Self::SessionRevokeOthers,
        Self::AuditRead,
        Self::AuditExport,
        Self::LegalHoldManage,
        Self::RetentionManage,
        Self::AgentOperate,
        Self::MailboxUse,
    ];

    /// Capabilities that may appear in an Agent access token.
    pub const AGENT_TOKEN: [Self; 4] = [
        Self::OrganizationRead,
        Self::AgentOperate,
        Self::AgentMailRead,
        Self::MailboxUse,
    ];
}

/// Built-in roles from the enterprise permission matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemRole {
    /// Full tenant governance; high-risk actions still require policy approval.
    OrgOwner,
    /// Domain, routing, mailbox, and delivery administration without content access.
    MailAdmin,
    /// Security configuration and incident response.
    SecurityAdmin,
    /// Read-only audit and compliance evidence access.
    ComplianceAuditor,
    /// Case-bound content review.
    ContentReviewer,
    /// Limited identity and mailbox support.
    Helpdesk,
    /// Agent lifecycle and delivery operations.
    AgentOperator,
    /// Normal mailbox use.
    Employee,
    /// Emergency access through a separately audited workflow.
    BreakGlass,
}

impl SystemRole {
    /// Stable built-in role catalog created for every tenant.
    pub const CATALOG: [Self; 9] = [
        Self::OrgOwner,
        Self::MailAdmin,
        Self::SecurityAdmin,
        Self::ComplianceAuditor,
        Self::ContentReviewer,
        Self::Helpdesk,
        Self::AgentOperator,
        Self::Employee,
        Self::BreakGlass,
    ];

    /// Returns the stable role key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrgOwner => "org_owner",
            Self::MailAdmin => "mail_admin",
            Self::SecurityAdmin => "security_admin",
            Self::ComplianceAuditor => "compliance_auditor",
            Self::ContentReviewer => "content_reviewer",
            Self::Helpdesk => "helpdesk",
            Self::AgentOperator => "agent_operator",
            Self::Employee => "employee",
            Self::BreakGlass => "break_glass",
        }
    }

    /// Human-readable default name used when seeding a tenant.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::OrgOwner => "Organization Owner",
            Self::MailAdmin => "Mail Administrator",
            Self::SecurityAdmin => "Security Administrator",
            Self::ComplianceAuditor => "Compliance Auditor",
            Self::ContentReviewer => "Content Reviewer",
            Self::Helpdesk => "Helpdesk",
            Self::AgentOperator => "Agent Operator",
            Self::Employee => "Employee",
            Self::BreakGlass => "Break Glass Operator",
        }
    }

    /// Least-privilege baseline for each built-in role.
    ///
    /// Resource-level authorization, case context, and approval remain
    /// mandatory narrowing layers; these permissions never grant blanket
    /// mailbox or content access.
    #[must_use]
    pub const fn permissions(self) -> &'static [Permission] {
        match self {
            Self::OrgOwner => &Permission::ORG_OWNER,
            Self::MailAdmin => &[
                Permission::OrganizationRead,
                Permission::MailAdmin,
                Permission::MailboxUse,
            ],
            Self::SecurityAdmin => &[
                Permission::OrganizationRead,
                Permission::SecurityAdmin,
                Permission::AuditRead,
            ],
            Self::ComplianceAuditor => &[
                Permission::OrganizationRead,
                Permission::AuditRead,
                Permission::AuditExport,
            ],
            Self::ContentReviewer => &[Permission::OrganizationRead, Permission::ContentReview],
            Self::Helpdesk => &[Permission::OrganizationRead, Permission::IdentityManage],
            Self::AgentOperator => &Permission::AGENT_TOKEN,
            Self::Employee => &[Permission::OrganizationRead, Permission::MailboxUse],
            Self::BreakGlass => &[Permission::OrganizationRead, Permission::BreakGlass],
        }
    }
}

/// A tenant-scoped role and its immutable initial permission set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    id: RoleId,
    organization_id: OrganizationId,
    system_role: SystemRole,
    display_name: String,
    permissions: BTreeSet<Permission>,
    created_at: DateTime<Utc>,
}

impl Role {
    /// Creates one built-in role from the authoritative system catalog.
    #[must_use]
    pub fn built_in(
        organization_id: OrganizationId,
        system_role: SystemRole,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: RoleId::new(),
            organization_id,
            system_role,
            display_name: system_role.display_name().into(),
            permissions: system_role.permissions().iter().copied().collect(),
            created_at: now,
        }
    }

    /// Creates the built-in organization owner role.
    #[must_use]
    pub fn organization_owner(organization_id: OrganizationId, now: DateTime<Utc>) -> Self {
        Self::built_in(organization_id, SystemRole::OrgOwner, now)
    }

    /// Creates the built-in least-privilege Agent runtime role.
    #[must_use]
    pub fn agent_operator(organization_id: OrganizationId, now: DateTime<Utc>) -> Self {
        Self::built_in(organization_id, SystemRole::AgentOperator, now)
    }

    /// Returns the role identifier.
    #[must_use]
    pub const fn id(&self) -> RoleId {
        self.id
    }

    /// Returns the tenant boundary.
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    /// Returns the built-in role key.
    #[must_use]
    pub const fn system_role(&self) -> SystemRole {
        self.system_role
    }

    /// Returns the display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the permission set.
    pub fn permissions(&self) -> impl Iterator<Item = Permission> + '_ {
        self.permissions.iter().copied()
    }

    /// Returns the creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

/// Time-bounded assignment of a tenant role to a principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAssignment {
    id: RoleAssignmentId,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    role_id: RoleId,
    created_by: PrincipalId,
    valid_from: DateTime<Utc>,
    valid_until: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl RoleAssignment {
    /// Creates a role assignment after validating its lifetime.
    pub fn new(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        role_id: RoleId,
        created_by: PrincipalId,
        valid_from: DateTime<Utc>,
        valid_until: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if valid_until.is_some_and(|until| until <= valid_from) {
            return Err(DomainError::InvalidRoleAssignmentWindow);
        }
        Ok(Self {
            id: RoleAssignmentId::new(),
            organization_id,
            principal_id,
            role_id,
            created_by,
            valid_from,
            valid_until,
            created_at,
        })
    }

    /// Returns the assignment identifier.
    #[must_use]
    pub const fn id(&self) -> RoleAssignmentId {
        self.id
    }

    /// Returns the tenant boundary.
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    /// Returns the assigned principal.
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }

    /// Returns the assigned role.
    #[must_use]
    pub const fn role_id(&self) -> RoleId {
        self.role_id
    }

    /// Returns the principal that granted the role.
    #[must_use]
    pub const fn created_by(&self) -> PrincipalId {
        self.created_by
    }

    /// Returns the start of the validity window.
    #[must_use]
    pub const fn valid_from(&self) -> DateTime<Utc> {
        self.valid_from
    }

    /// Returns the optional expiry.
    #[must_use]
    pub const fn valid_until(&self) -> Option<DateTime<Utc>> {
        self.valid_until
    }

    /// Returns the creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

/// Effective identity and permissions resolved by an authentication adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationContext {
    /// Authenticated principal.
    pub principal_id: PrincipalId,
    /// Tenant established by the authenticated identity, never by request input.
    pub organization_id: OrganizationId,
    /// Effective permissions from active assignments and policy narrowing.
    pub permissions: BTreeSet<Permission>,
    /// Server-derived, non-secret authentication generation. None is reserved
    /// for trusted local CLI/background work, never a browser/Agent request.
    pub authentication: Option<AuthenticationBinding>,
}

/// Immutable credential identity carried from verification into each unit of
/// work. It is intentionally not deserializable from an HTTP request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticationBinding {
    /// The immutable session ID is the generation: session rows cannot rotate
    /// in place, and a fresh login always gets a different ID.
    HumanSession {
        session_id: HumanSessionId,
    },
    /// Production Human endpoints require sessions. Legacy/dev OIDC bearer
    /// access retains its verified lifetime, never an invented local session.
    HumanBearer {
        expires_at: DateTime<Utc>,
    },
    AgentCredential {
        credential_id: CredentialId,
        expires_at: DateTime<Utc>,
        scopes: BTreeSet<Permission>,
    },
    ScimToken {
        token_id: ScimTokenId,
    },
}

impl AuthorizationContext {
    /// Applies tenant isolation and default-deny permission evaluation.
    #[must_use]
    pub fn permits(&self, target: OrganizationId, required: Permission) -> bool {
        self.organization_id == target
            && self.permissions.contains(&required)
            && match &self.authentication {
                Some(AuthenticationBinding::AgentCredential { scopes, .. }) => {
                    scopes.contains(&required)
                }
                _ => true,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_is_default_deny_and_tenant_scoped() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let context = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::OrganizationRead].into_iter().collect(),
            authentication: None,
        };
        assert!(context.permits(organization_id, Permission::OrganizationRead));
        assert!(!context.permits(organization_id, Permission::OrganizationManage));
        assert!(!context.permits(OrganizationId::new(), Permission::OrganizationRead));
    }

    #[test]
    fn role_assignment_rejects_empty_window() {
        let now = Utc::now();
        assert_eq!(
            RoleAssignment::new(
                OrganizationId::new(),
                PrincipalId::new(),
                RoleId::new(),
                PrincipalId::new(),
                now,
                Some(now),
                now,
            )
            .unwrap_err(),
            DomainError::InvalidRoleAssignmentWindow
        );
    }

    #[test]
    fn organization_owner_excludes_case_and_emergency_access() {
        let role = Role::organization_owner(OrganizationId::new(), Utc::now());
        let permissions = role.permissions().collect::<BTreeSet<_>>();
        assert!(permissions.contains(&Permission::AuthorizationManage));
        assert!(permissions.contains(&Permission::SessionRevokeOthers));
        assert!(!permissions.contains(&Permission::ContentReview));
        assert!(!permissions.contains(&Permission::BreakGlass));
        let helpdesk = Role::built_in(OrganizationId::new(), SystemRole::Helpdesk, Utc::now());
        assert!(
            !helpdesk
                .permissions()
                .any(|permission| permission == Permission::SessionRevokeOthers)
        );
        let security = Role::built_in(OrganizationId::new(), SystemRole::SecurityAdmin, Utc::now());
        assert!(
            security
                .permissions()
                .any(|permission| permission == Permission::SecurityAdmin)
        );
        assert!(
            !security
                .permissions()
                .any(|permission| permission == Permission::SessionRevokeOthers)
        );
    }

    #[test]
    fn built_in_role_catalog_is_complete_unique_and_least_privilege() {
        let organization_id = OrganizationId::new();
        let roles = SystemRole::CATALOG
            .into_iter()
            .map(|system_role| Role::built_in(organization_id, system_role, Utc::now()))
            .collect::<Vec<_>>();
        assert_eq!(roles.len(), 9);
        assert_eq!(
            roles
                .iter()
                .map(Role::system_role)
                .collect::<BTreeSet<_>>()
                .len(),
            9
        );
        let content_reviewer = roles
            .iter()
            .find(|role| role.system_role() == SystemRole::ContentReviewer)
            .unwrap();
        assert_eq!(
            content_reviewer.permissions().collect::<BTreeSet<_>>(),
            [Permission::OrganizationRead, Permission::ContentReview]
                .into_iter()
                .collect()
        );
        let break_glass = roles
            .iter()
            .find(|role| role.system_role() == SystemRole::BreakGlass)
            .unwrap();
        assert!(
            !break_glass
                .permissions()
                .any(|permission| permission == Permission::ContentReview)
        );
    }
}
