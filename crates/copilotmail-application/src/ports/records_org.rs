use super::*;

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
