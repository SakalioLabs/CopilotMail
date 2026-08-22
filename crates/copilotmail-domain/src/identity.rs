use crate::{CredentialId, DomainError, OrganizationId, Permission, PrincipalId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The type of actor represented by a principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// A company employee or other interactive user.
    Human,
    /// An autonomous or supervised software Agent.
    Agent,
    /// A non-Agent infrastructure service.
    Service,
}

/// Lifecycle state shared by every principal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalStatus {
    /// Created but not yet permitted to operate.
    Draft,
    /// Permitted to authenticate and operate within assigned policy.
    Active,
    /// Temporarily unable to authenticate or perform new actions.
    Suspended,
    /// Permanently retired while retained for referential and audit integrity.
    Retired,
}

/// Deployment environment assigned to an Agent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEnvironment {
    /// Local or shared development environment.
    Development,
    /// Non-production validation environment.
    Testing,
    /// Production environment.
    Production,
}

/// Risk classification used to derive Agent policy defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRiskLevel {
    /// Read-only or tightly constrained low-impact automation.
    Low,
    /// Automation with normal mailbox read or send privileges.
    Medium,
    /// Broad, sensitive, externally facing, or delegated automation.
    High,
}

/// A unified identity for a human, Agent, or service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    id: PrincipalId,
    organization_id: OrganizationId,
    kind: PrincipalKind,
    status: PrincipalStatus,
    display_name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Principal {
    /// Creates a draft principal after enforcing shared identity invariants.
    pub fn new(
        organization_id: OrganizationId,
        kind: PrincipalKind,
        display_name: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let display_name = normalize_display_name(display_name.into())?;
        Ok(Self {
            id: PrincipalId::new(),
            organization_id,
            kind,
            status: PrincipalStatus::Draft,
            display_name,
            created_at: now,
            updated_at: now,
        })
    }

    /// Returns the principal identifier.
    #[must_use]
    pub const fn id(&self) -> PrincipalId {
        self.id
    }

    /// Returns the owning organization.
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    /// Returns the principal type.
    #[must_use]
    pub const fn kind(&self) -> PrincipalKind {
        self.kind
    }

    /// Returns the lifecycle status.
    #[must_use]
    pub const fn status(&self) -> PrincipalStatus {
        self.status
    }

    /// Returns the normalized display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Returns the most recent lifecycle update timestamp.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Activates a draft or suspended principal.
    pub fn activate(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        match self.status {
            PrincipalStatus::Draft | PrincipalStatus::Suspended => {
                self.status = PrincipalStatus::Active;
                self.updated_at = now;
                Ok(())
            }
            PrincipalStatus::Active => Ok(()),
            PrincipalStatus::Retired => Err(DomainError::InvalidProcessingTransition),
        }
    }

    /// Suspends an active principal without deleting its identity history.
    pub fn suspend(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        match self.status {
            PrincipalStatus::Active => {
                self.status = PrincipalStatus::Suspended;
                self.updated_at = now;
                Ok(())
            }
            PrincipalStatus::Suspended => Ok(()),
            PrincipalStatus::Draft | PrincipalStatus::Retired => {
                Err(DomainError::InvalidProcessingTransition)
            }
        }
    }

    /// Permanently retires the principal while preserving audit references.
    pub fn retire(&mut self, now: DateTime<Utc>) {
        self.status = PrincipalStatus::Retired;
        self.updated_at = now;
    }
}

/// Agent-specific ownership and policy classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
    principal_id: PrincipalId,
    owner_principal_id: PrincipalId,
    environment: AgentEnvironment,
    risk_level: AgentRiskLevel,
    purpose: String,
    review_due_at: DateTime<Utc>,
}

impl AgentProfile {
    /// Creates ownership and periodic-review metadata for an Agent Principal.
    pub fn new(
        principal: &Principal,
        owner_principal_id: PrincipalId,
        environment: AgentEnvironment,
        risk_level: AgentRiskLevel,
        purpose: impl Into<String>,
        review_due_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let purpose = purpose.into();
        if principal.kind() != PrincipalKind::Agent
            || principal.id() == owner_principal_id
            || purpose.trim() != purpose
            || purpose.is_empty()
            || purpose.chars().count() > 1_000
            || purpose.chars().any(char::is_control)
            || review_due_at <= principal.created_at()
        {
            return Err(DomainError::InvalidAgentIdentity);
        }
        Ok(Self {
            principal_id: principal.id(),
            owner_principal_id,
            environment,
            risk_level,
            purpose,
            review_due_at,
        })
    }

    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }
    #[must_use]
    pub const fn owner_principal_id(&self) -> PrincipalId {
        self.owner_principal_id
    }
    #[must_use]
    pub const fn environment(&self) -> AgentEnvironment {
        self.environment
    }
    #[must_use]
    pub const fn risk_level(&self) -> AgentRiskLevel {
        self.risk_level
    }
    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
    #[must_use]
    pub const fn review_due_at(&self) -> DateTime<Utc> {
        self.review_due_at
    }
}

/// Public-key credential metadata for an Agent OAuth client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCredential {
    id: CredentialId,
    principal_id: PrincipalId,
    fingerprint: String,
    allowed_scopes: BTreeSet<Permission>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

/// X.509 certificate credential used for native Agent mTLS authentication.
///
/// The fingerprint is the lowercase SHA-256 digest of the leaf certificate's
/// DER encoding. The certificate itself remains in the workload and PKI
/// boundaries; the control plane only needs the verified transport binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMtlsCredential {
    id: CredentialId,
    principal_id: PrincipalId,
    certificate_fingerprint: String,
    allowed_scopes: BTreeSet<Permission>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl AgentMtlsCredential {
    /// Creates an mTLS credential after the application layer has verified
    /// that the referenced Principal is an active Agent in the target tenant.
    pub fn for_agent_principal(
        principal_id: PrincipalId,
        certificate_fingerprint: impl Into<String>,
        allowed_scopes: impl IntoIterator<Item = Permission>,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let certificate_fingerprint = certificate_fingerprint.into();
        let allowed_scopes = allowed_scopes.into_iter().collect::<BTreeSet<_>>();
        if certificate_fingerprint.len() != 64
            || !certificate_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || allowed_scopes.is_empty()
            || !allowed_scopes
                .iter()
                .all(|scope| Permission::AGENT_TOKEN.contains(scope))
            || expires_at <= created_at
        {
            return Err(DomainError::InvalidAgentIdentity);
        }
        Ok(Self {
            id: CredentialId::new(),
            principal_id,
            certificate_fingerprint: certificate_fingerprint.to_ascii_lowercase(),
            allowed_scopes,
            created_at,
            expires_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> CredentialId {
        self.id
    }
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }
    #[must_use]
    pub fn certificate_fingerprint(&self) -> &str {
        &self.certificate_fingerprint
    }
    pub fn allowed_scopes(&self) -> impl Iterator<Item = Permission> + '_ {
        self.allowed_scopes.iter().copied()
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

impl AgentCredential {
    pub fn new(
        principal: &Principal,
        fingerprint: impl Into<String>,
        allowed_scopes: impl IntoIterator<Item = Permission>,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if principal.kind() != PrincipalKind::Agent {
            return Err(DomainError::InvalidAgentIdentity);
        }
        Self::for_agent_principal(
            principal.id(),
            fingerprint,
            allowed_scopes,
            created_at,
            expires_at,
        )
    }

    /// Creates a credential after the application layer has verified the
    /// referenced Principal is an active Agent in the target tenant.
    pub fn for_agent_principal(
        principal_id: PrincipalId,
        fingerprint: impl Into<String>,
        allowed_scopes: impl IntoIterator<Item = Permission>,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let fingerprint = fingerprint.into();
        let allowed_scopes = allowed_scopes.into_iter().collect::<BTreeSet<_>>();
        if fingerprint.len() != 64
            || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
            || allowed_scopes.is_empty()
            || !allowed_scopes
                .iter()
                .all(|scope| Permission::AGENT_TOKEN.contains(scope))
            || expires_at <= created_at
        {
            return Err(DomainError::InvalidAgentIdentity);
        }
        Ok(Self {
            id: CredentialId::new(),
            principal_id,
            fingerprint: fingerprint.to_ascii_lowercase(),
            allowed_scopes,
            created_at,
            expires_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> CredentialId {
        self.id
    }
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn allowed_scopes(&self) -> impl Iterator<Item = Permission> + '_ {
        self.allowed_scopes.iter().copied()
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

/// Immutable enterprise identity mapping for a human principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanProfile {
    principal_id: PrincipalId,
    identity_provider: String,
    identity_subject: String,
}

impl HumanProfile {
    /// Creates an exact issuer/subject mapping for a human principal.
    pub fn new(
        principal: &Principal,
        identity_provider: impl Into<String>,
        identity_subject: impl Into<String>,
    ) -> Result<Self, DomainError> {
        if principal.kind() != PrincipalKind::Human {
            return Err(DomainError::InvalidHumanProfile);
        }
        let identity_provider = identity_provider.into();
        let identity_subject = identity_subject.into();
        if !valid_external_identifier(&identity_provider, 2048)
            || !valid_external_identifier(&identity_subject, 512)
        {
            return Err(DomainError::InvalidExternalIdentity);
        }
        Ok(Self {
            principal_id: principal.id(),
            identity_provider,
            identity_subject,
        })
    }

    /// Returns the mapped human principal.
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }

    /// Returns the exact OIDC issuer identifier.
    #[must_use]
    pub fn identity_provider(&self) -> &str {
        &self.identity_provider
    }

    /// Returns the exact stable OIDC subject.
    #[must_use]
    pub fn identity_subject(&self) -> &str {
        &self.identity_subject
    }
}

fn normalize_display_name(value: String) -> Result<String, DomainError> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() || value.chars().count() > 200 {
        Err(DomainError::InvalidDisplayName)
    } else {
        Ok(value)
    }
}

fn valid_external_identifier(value: &str, maximum_length: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_length
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_lifecycle_preserves_retired_identity() {
        let now = Utc::now();
        let mut principal = Principal::new(
            OrganizationId::new(),
            PrincipalKind::Agent,
            " Mail Agent ",
            now,
        )
        .unwrap();
        assert_eq!(principal.display_name(), "Mail Agent");
        principal.activate(now).unwrap();
        principal.suspend(now).unwrap();
        principal.retire(now);
        assert_eq!(principal.status(), PrincipalStatus::Retired);
        assert!(principal.activate(now).is_err());
    }

    #[test]
    fn human_profile_rejects_service_identity_and_control_characters() {
        let now = Utc::now();
        let service = Principal::new(
            OrganizationId::new(),
            PrincipalKind::Service,
            "Service",
            now,
        )
        .unwrap();
        assert_eq!(
            HumanProfile::new(&service, "https://login.example", "subject").unwrap_err(),
            DomainError::InvalidHumanProfile
        );
        let human =
            Principal::new(OrganizationId::new(), PrincipalKind::Human, "Human", now).unwrap();
        assert_eq!(
            HumanProfile::new(&human, "https://login.example", "bad\nsubject").unwrap_err(),
            DomainError::InvalidExternalIdentity
        );
    }

    #[test]
    fn agent_profile_and_credential_enforce_machine_identity_policy() {
        let now = Utc::now();
        let mut agent = Principal::new(
            OrganizationId::new(),
            PrincipalKind::Agent,
            "Triage Agent",
            now,
        )
        .unwrap();
        agent.activate(now).unwrap();
        let owner = PrincipalId::new();
        let profile = AgentProfile::new(
            &agent,
            owner,
            AgentEnvironment::Production,
            AgentRiskLevel::Medium,
            "Triage inbound support mail",
            now + chrono::Duration::days(90),
        )
        .unwrap();
        assert_eq!(profile.owner_principal_id(), owner);
        let credential = AgentCredential::new(
            &agent,
            "a".repeat(64),
            [Permission::MailboxUse, Permission::AgentOperate],
            now,
            now + chrono::Duration::days(30),
        )
        .unwrap();
        assert_eq!(credential.principal_id(), agent.id());
        assert!(
            AgentCredential::new(
                &agent,
                "b".repeat(64),
                [Permission::SecurityAdmin],
                now,
                now + chrono::Duration::days(30)
            )
            .is_err()
        );
        let mtls = AgentMtlsCredential::for_agent_principal(
            agent.id(),
            "c".repeat(64),
            [Permission::MailboxUse],
            now,
            now + chrono::Duration::days(30),
        )
        .unwrap();
        assert_eq!(mtls.principal_id(), agent.id());
    }
}
