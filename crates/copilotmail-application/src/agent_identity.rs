use crate::{ControlPlaneStore, PendingEvent, StoreError};
use chrono::{DateTime, Duration, Utc};
use copilotmail_domain::{
    AgentCredential, AgentEnvironment, AgentMtlsCredential, AgentProfile, AgentRiskLevel,
    AuditActor, AuditEvent, AuditOutcome, AuditResource, CredentialId, DomainError, OrganizationId,
    Permission, Principal, PrincipalId, PrincipalKind, RoleAssignment, SystemRole,
};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct RegisterAgentCommand {
    pub organization_id: OrganizationId,
    pub display_name: String,
    pub environment: AgentEnvironment,
    pub risk_level: AgentRiskLevel,
    pub purpose: String,
    pub review_due_at: DateTime<Utc>,
    pub public_key_pem: String,
    pub public_key_fingerprint: String,
    pub allowed_scopes: BTreeSet<Permission>,
    pub credential_expires_at: DateTime<Utc>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterAgentResult {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub credential_id: CredentialId,
    pub owner_principal_id: PrincipalId,
    pub allowed_scopes: BTreeSet<Permission>,
    pub credential_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AddAgentCredentialCommand {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub public_key_pem: String,
    pub public_key_fingerprint: String,
    pub allowed_scopes: BTreeSet<Permission>,
    pub credential_expires_at: DateTime<Utc>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AddAgentMtlsCredentialCommand {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub certificate_fingerprint: String,
    pub allowed_scopes: BTreeSet<Permission>,
    pub credential_expires_at: DateTime<Utc>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddAgentCredentialResult {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub credential_id: CredentialId,
    pub allowed_scopes: BTreeSet<Permission>,
    pub credential_expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RevokeAgentCredentialCommand {
    pub organization_id: OrganizationId,
    pub credential_id: CredentialId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentIdentityError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("agent credential policy is invalid")]
    InvalidCredentialPolicy,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct RegisterAgentService<S> {
    store: S,
}

impl<S: ControlPlaneStore> RegisterAgentService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        command: RegisterAgentCommand,
    ) -> Result<RegisterAgentResult, AgentIdentityError> {
        validate_credential_policy(&command)?;
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        let role_id = transaction
            .find_system_role(command.organization_id, SystemRole::AgentOperator)
            .await?
            .ok_or(StoreError::MissingReference)?;

        let mut agent = Principal::new(
            command.organization_id,
            PrincipalKind::Agent,
            command.display_name,
            command.now,
        )?;
        agent.activate(command.now)?;
        let profile = AgentProfile::new(
            &agent,
            owner.principal_id,
            command.environment,
            command.risk_level,
            command.purpose,
            command.review_due_at,
        )?;
        let credential = AgentCredential::new(
            &agent,
            command.public_key_fingerprint,
            command.allowed_scopes.iter().copied(),
            command.now,
            command.credential_expires_at,
        )?;
        let assignment = RoleAssignment::new(
            command.organization_id,
            agent.id(),
            role_id,
            owner.principal_id,
            command.now,
            Some(command.review_due_at),
            command.now,
        )?;
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: owner.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.agent.registered",
            AuditResource {
                kind: "identity.agent".into(),
                id: agent.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "credential_id": credential.id(),
                "environment": command.environment,
                "risk_level": command.risk_level,
                "allowed_scopes": credential.allowed_scopes().collect::<Vec<_>>(),
                "review_due_at": command.review_due_at,
                "credential_expires_at": credential.expires_at(),
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.agent.registered.v1",
            "identity.agent",
            agent.id().to_string(),
            json!({
                "organization_id": command.organization_id,
                "principal_id": agent.id(),
                "credential_id": credential.id(),
                "owner_principal_id": owner.principal_id,
            }),
            command.now,
        );

        transaction.insert_principal(&agent).await?;
        transaction.insert_agent_profile(&profile).await?;
        transaction
            .insert_agent_credential(&credential, &command.public_key_pem)
            .await?;
        transaction.insert_role_assignment(&assignment).await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(RegisterAgentResult {
            organization_id: command.organization_id,
            principal_id: agent.id(),
            credential_id: credential.id(),
            owner_principal_id: owner.principal_id,
            allowed_scopes: credential.allowed_scopes().collect(),
            credential_expires_at: credential.expires_at(),
        })
    }
}

pub struct RevokeAgentCredentialService<S> {
    store: S,
}

pub struct AddAgentCredentialService<S> {
    store: S,
}

pub struct AddAgentMtlsCredentialService<S> {
    store: S,
}

impl<S: ControlPlaneStore> AddAgentCredentialService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        command: AddAgentCredentialCommand,
    ) -> Result<AddAgentCredentialResult, AgentIdentityError> {
        validate_public_credential(
            &command.public_key_pem,
            &command.allowed_scopes,
            command.credential_expires_at,
            command.now,
        )?;
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if !transaction
            .active_agent_exists(command.organization_id, command.principal_id)
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        let credential = AgentCredential::for_agent_principal(
            command.principal_id,
            command.public_key_fingerprint,
            command.allowed_scopes,
            command.now,
            command.credential_expires_at,
        )?;
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: owner.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.agent_credential.added",
            AuditResource {
                kind: "identity.agent_credential".into(),
                id: credential.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "agent_principal_id": command.principal_id,
                "allowed_scopes": credential.allowed_scopes().collect::<Vec<_>>(),
                "credential_expires_at": credential.expires_at(),
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.agent_credential.added.v1",
            "identity.agent_credential",
            credential.id().to_string(),
            json!({
                "organization_id": command.organization_id,
                "principal_id": command.principal_id,
                "credential_id": credential.id(),
            }),
            command.now,
        );
        transaction
            .insert_agent_credential(&credential, &command.public_key_pem)
            .await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(AddAgentCredentialResult {
            organization_id: command.organization_id,
            principal_id: command.principal_id,
            credential_id: credential.id(),
            allowed_scopes: credential.allowed_scopes().collect(),
            credential_expires_at: credential.expires_at(),
        })
    }
}

impl<S: ControlPlaneStore> AddAgentMtlsCredentialService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        command: AddAgentMtlsCredentialCommand,
    ) -> Result<AddAgentCredentialResult, AgentIdentityError> {
        validate_machine_scope_policy(
            &command.allowed_scopes,
            command.credential_expires_at,
            command.now,
        )?;
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if !transaction
            .active_agent_exists(command.organization_id, command.principal_id)
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        let credential = AgentMtlsCredential::for_agent_principal(
            command.principal_id,
            command.certificate_fingerprint,
            command.allowed_scopes,
            command.now,
            command.credential_expires_at,
        )?;
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: owner.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.agent_mtls_credential.added",
            AuditResource {
                kind: "identity.agent_credential".into(),
                id: credential.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "agent_principal_id": command.principal_id,
                "credential_kind": "mtls",
                "certificate_fingerprint": credential.certificate_fingerprint(),
                "allowed_scopes": credential.allowed_scopes().collect::<Vec<_>>(),
                "credential_expires_at": credential.expires_at(),
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.agent_mtls_credential.added.v1",
            "identity.agent_credential",
            credential.id().to_string(),
            json!({
                "organization_id": command.organization_id,
                "principal_id": command.principal_id,
                "credential_id": credential.id(),
                "credential_kind": "mtls",
            }),
            command.now,
        );
        transaction
            .insert_agent_mtls_credential(&credential)
            .await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(AddAgentCredentialResult {
            organization_id: command.organization_id,
            principal_id: command.principal_id,
            credential_id: credential.id(),
            allowed_scopes: credential.allowed_scopes().collect(),
            credential_expires_at: credential.expires_at(),
        })
    }
}

impl<S: ControlPlaneStore> RevokeAgentCredentialService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        command: RevokeAgentCredentialCommand,
    ) -> Result<(), AgentIdentityError> {
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if !transaction
            .revoke_agent_credential(command.organization_id, command.credential_id, command.now)
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: owner.principal_id,
                on_behalf_of: None,
                credential_id: Some(command.credential_id.to_string()),
            },
            "identity.agent_credential.revoked",
            AuditResource {
                kind: "identity.agent_credential".into(),
                id: command.credential_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({}),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.agent_credential.revoked.v1",
            "identity.agent_credential",
            command.credential_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "credential_id": command.credential_id,
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn validate_credential_policy(command: &RegisterAgentCommand) -> Result<(), AgentIdentityError> {
    validate_public_credential(
        &command.public_key_pem,
        &command.allowed_scopes,
        command.credential_expires_at,
        command.now,
    )?;
    if command.review_due_at > command.now + Duration::days(365) {
        return Err(AgentIdentityError::InvalidCredentialPolicy);
    }
    Ok(())
}

fn validate_public_credential(
    public_key_pem: &str,
    allowed_scopes: &BTreeSet<Permission>,
    expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(), AgentIdentityError> {
    if public_key_pem.len() < 256
        || public_key_pem.len() > 16_384
        || !public_key_pem.starts_with("-----BEGIN PUBLIC KEY-----")
    {
        return Err(AgentIdentityError::InvalidCredentialPolicy);
    }
    validate_machine_scope_policy(allowed_scopes, expires_at, now)
}

fn validate_machine_scope_policy(
    allowed_scopes: &BTreeSet<Permission>,
    expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(), AgentIdentityError> {
    if allowed_scopes.is_empty() || expires_at <= now || expires_at > now + Duration::days(365) {
        return Err(AgentIdentityError::InvalidCredentialPolicy);
    }
    Ok(())
}
