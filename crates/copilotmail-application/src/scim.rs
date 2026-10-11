use crate::{
    ControlPlaneStore, ExistingScimGroup, ExistingScimUser, PendingEvent, ScimCredential,
    ScimDirectoryQueryStore, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthenticationBinding,
    AuthorizationContext, DomainError, HumanProfile, OrganizationId, Principal, PrincipalId,
    PrincipalKind, ScimGroupId, ScimTokenId,
};
use serde_json::json;
use std::collections::BTreeSet;

/// Request-proof context bound to the presenting SCIM credential so the unit of
/// work is fenced against token revocation until commit.
fn scim_request_context(credential: &ScimCredential) -> AuthorizationContext {
    AuthorizationContext {
        principal_id: credential.actor_principal_id,
        organization_id: credential.organization_id,
        permissions: BTreeSet::new(),
        authentication: Some(AuthenticationBinding::ScimToken {
            token_id: credential.token_id,
        }),
    }
}

/// Immutable input used to issue a tenant-scoped SCIM bearer token.
#[derive(Debug, Clone)]
pub struct IssueScimTokenCommand {
    pub organization_id: OrganizationId,
    pub token_id: ScimTokenId,
    pub token_hash: Vec<u8>,
    pub label: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RevokeScimTokenCommand {
    pub organization_id: OrganizationId,
    pub token_id: ScimTokenId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Provisions an enterprise directory user.
#[derive(Debug, Clone)]
pub struct ProvisionScimUserCommand {
    pub credential: ScimCredential,
    pub identity_provider: String,
    pub external_id: String,
    pub user_name: String,
    pub display_name: String,
    pub active: bool,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Fully replaces the mutable attributes of a SCIM user.
#[derive(Debug, Clone)]
pub struct ReplaceScimUserCommand {
    pub credential: ScimCredential,
    pub principal_id: PrincipalId,
    pub external_id: String,
    pub user_name: String,
    pub display_name: String,
    pub active: bool,
    pub expected_version: Option<i64>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Application result for an idempotent SCIM create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionScimUserResult {
    pub user: ExistingScimUser,
    pub created: bool,
}

#[derive(Debug, Clone)]
pub struct ProvisionScimGroupCommand {
    pub credential: ScimCredential,
    pub external_id: String,
    pub display_name: String,
    pub members: Vec<PrincipalId>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ReplaceScimGroupCommand {
    pub credential: ScimCredential,
    pub group_id: ScimGroupId,
    pub external_id: String,
    pub display_name: String,
    pub members: Vec<PrincipalId>,
    pub expected_version: Option<i64>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DeleteScimGroupCommand {
    pub credential: ScimCredential,
    pub group_id: ScimGroupId,
    pub expected_version: Option<i64>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionScimGroupResult {
    pub group: ExistingScimGroup,
    pub created: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ScimProvisioningError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("SCIM attribute is invalid: {0}")]
    InvalidAttribute(&'static str),
    #[error("SCIM resource version does not match")]
    VersionMismatch,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Issues and revokes directory credentials without persisting plaintext secrets.
pub struct IssueScimTokenService<S> {
    store: S,
}

impl<S: ControlPlaneStore> IssueScimTokenService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        command: IssueScimTokenCommand,
    ) -> Result<ScimCredential, ScimProvisioningError> {
        validate_token(&command)?;
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        let credential = ScimCredential {
            token_id: command.token_id,
            organization_id: command.organization_id,
            actor_principal_id: owner.principal_id,
            label: command.label,
            expires_at: command.expires_at,
        };
        let audit = AuditEvent::new(
            command.organization_id,
            audit_actor(&credential),
            "identity.scim_token.issued",
            AuditResource {
                kind: "identity.scim_token".into(),
                id: command.token_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({"label": credential.label, "expires_at": credential.expires_at}),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.scim_token.issued.v1",
            "identity.scim_token",
            command.token_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "token_id": command.token_id,
            }),
            command.now,
        );
        transaction
            .insert_scim_token(&credential, &command.token_hash, command.now)
            .await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(credential)
    }
}

/// Immediately revokes a SCIM provisioning credential.
pub struct RevokeScimTokenService<S> {
    store: S,
}

impl<S: ControlPlaneStore> RevokeScimTokenService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        command: RevokeScimTokenCommand,
    ) -> Result<(), ScimProvisioningError> {
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if !transaction
            .revoke_scim_token(command.organization_id, command.token_id, command.now)
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: owner.principal_id,
                on_behalf_of: None,
                credential_id: Some(command.token_id.to_string()),
            },
            "identity.scim_token.revoked",
            AuditResource {
                kind: "identity.scim_token".into(),
                id: command.token_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({"token_id": command.token_id}),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.scim_token.revoked.v1",
            "identity.scim_token",
            command.token_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "token_id": command.token_id,
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(())
    }
}

/// Applies SCIM user lifecycle changes through the shared Principal model.
pub struct ScimUserService<S> {
    store: S,
}

impl<S> ScimUserService<S>
where
    S: ControlPlaneStore + ScimDirectoryQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        command: ProvisionScimUserCommand,
    ) -> Result<ProvisionScimUserResult, ScimProvisioningError> {
        validate_directory_strings(
            &command.external_id,
            &command.user_name,
            &command.display_name,
        )?;
        let mut transaction = self
            .store
            .begin_authorized(&scim_request_context(&command.credential))
            .await?;
        if let Some(existing) = transaction
            .find_scim_user_by_external_id(command.credential.organization_id, &command.external_id)
            .await?
        {
            if existing.user_name != command.user_name
                || existing.display_name != normalized_name(&command.display_name)
                || existing.active != command.active
            {
                return Err(StoreError::Conflict.into());
            }
            // audit-coverage: no-op-idempotent-replay
            transaction.commit().await?;
            return Ok(ProvisionScimUserResult {
                user: existing,
                created: false,
            });
        }

        if let Some(human) = transaction
            .find_human_identity(
                command.credential.organization_id,
                &command.identity_provider,
                &command.external_id,
            )
            .await?
        {
            let user = ExistingScimUser {
                organization_id: human.organization_id,
                principal_id: human.principal_id,
                external_id: command.external_id,
                user_name: command.user_name,
                display_name: normalized_name(&command.display_name),
                active: command.active,
                version: 1,
                created_at: human.created_at,
                updated_at: command.now,
            };
            let (audit, event) = lifecycle_records(
                &command.credential,
                &user,
                "identity.scim_user.adopted",
                "identity.scim_user.adopted.v1",
                command.trace_id,
                command.now,
            )?;
            transaction.adopt_scim_user(&user).await?;
            transaction.append_audit_event(&audit).await?;
            transaction.enqueue_event(&event).await?;
            transaction.commit().await?;
            return Ok(ProvisionScimUserResult {
                user,
                created: true,
            });
        }

        let mut principal = Principal::new(
            command.credential.organization_id,
            PrincipalKind::Human,
            &command.display_name,
            command.now,
        )?;
        principal.activate(command.now)?;
        if !command.active {
            principal.suspend(command.now)?;
        }
        let profile = HumanProfile::new(
            &principal,
            command.identity_provider,
            command.external_id.clone(),
        )?;
        let user = ExistingScimUser {
            organization_id: command.credential.organization_id,
            principal_id: principal.id(),
            external_id: command.external_id,
            user_name: command.user_name,
            display_name: principal.display_name().into(),
            active: command.active,
            version: 1,
            created_at: command.now,
            updated_at: command.now,
        };
        let (audit, event) = lifecycle_records(
            &command.credential,
            &user,
            "identity.scim_user.provisioned",
            "identity.scim_user.provisioned.v1",
            command.trace_id,
            command.now,
        )?;
        transaction.insert_principal(&principal).await?;
        transaction.insert_human_profile(&profile).await?;
        transaction.insert_scim_user(&user).await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(ProvisionScimUserResult {
            user,
            created: true,
        })
    }

    pub async fn replace(
        &self,
        command: ReplaceScimUserCommand,
    ) -> Result<ExistingScimUser, ScimProvisioningError> {
        validate_directory_strings(
            &command.external_id,
            &command.user_name,
            &command.display_name,
        )?;
        let current = self
            .store
            .get_scim_user(command.credential.organization_id, command.principal_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if current.external_id != command.external_id {
            return Err(ScimProvisioningError::InvalidAttribute("externalId"));
        }
        if command
            .expected_version
            .is_some_and(|version| version != current.version)
        {
            return Err(ScimProvisioningError::VersionMismatch);
        }
        let mut next = current;
        next.user_name = command.user_name;
        next.display_name = normalized_name(&command.display_name);
        next.active = command.active;
        next.version += 1;
        next.updated_at = command.now;

        let mut transaction = self
            .store
            .begin_authorized(&scim_request_context(&command.credential))
            .await?;
        let updated = transaction
            .replace_scim_user(&next, next.version - 1)
            .await?;
        if !updated {
            return Err(ScimProvisioningError::VersionMismatch);
        }
        let (audit, event) = lifecycle_records(
            &command.credential,
            &next,
            "identity.scim_user.updated",
            "identity.scim_user.updated.v1",
            command.trace_id,
            command.now,
        )?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(next)
    }
}

/// Applies authoritative SCIM group and direct membership changes.
pub struct ScimGroupService<S> {
    store: S,
}

impl<S> ScimGroupService<S>
where
    S: ControlPlaneStore + ScimDirectoryQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        command: ProvisionScimGroupCommand,
    ) -> Result<ProvisionScimGroupResult, ScimProvisioningError> {
        validate_group(
            &command.external_id,
            &command.display_name,
            &command.members,
        )?;
        let members = normalized_members(command.members);
        let mut transaction = self
            .store
            .begin_authorized(&scim_request_context(&command.credential))
            .await?;
        if let Some(existing) = transaction
            .find_scim_group_by_external_id(
                command.credential.organization_id,
                &command.external_id,
            )
            .await?
        {
            if existing.display_name != command.display_name || existing.members != members {
                return Err(StoreError::Conflict.into());
            }
            // audit-coverage: no-op-idempotent-replay
            transaction.commit().await?;
            return Ok(ProvisionScimGroupResult {
                group: existing,
                created: false,
            });
        }
        let group = ExistingScimGroup {
            organization_id: command.credential.organization_id,
            group_id: ScimGroupId::new(),
            external_id: command.external_id,
            display_name: command.display_name,
            members,
            version: 1,
            created_at: command.now,
            updated_at: command.now,
        };
        let (audit, event) = group_lifecycle_records(
            &command.credential,
            &group,
            "identity.scim_group.provisioned",
            "identity.scim_group.provisioned.v1",
            command.trace_id,
            command.now,
        )?;
        transaction.insert_scim_group(&group).await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(ProvisionScimGroupResult {
            group,
            created: true,
        })
    }

    pub async fn replace(
        &self,
        command: ReplaceScimGroupCommand,
    ) -> Result<ExistingScimGroup, ScimProvisioningError> {
        validate_group(
            &command.external_id,
            &command.display_name,
            &command.members,
        )?;
        let current = self
            .store
            .get_scim_group(command.credential.organization_id, command.group_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if current.external_id != command.external_id {
            return Err(ScimProvisioningError::InvalidAttribute("externalId"));
        }
        if command
            .expected_version
            .is_some_and(|version| version != current.version)
        {
            return Err(ScimProvisioningError::VersionMismatch);
        }
        let mut next = current;
        next.display_name = command.display_name;
        next.members = normalized_members(command.members);
        next.version += 1;
        next.updated_at = command.now;
        let mut transaction = self
            .store
            .begin_authorized(&scim_request_context(&command.credential))
            .await?;
        if !transaction
            .replace_scim_group(&next, next.version - 1)
            .await?
        {
            return Err(ScimProvisioningError::VersionMismatch);
        }
        let (audit, event) = group_lifecycle_records(
            &command.credential,
            &next,
            "identity.scim_group.updated",
            "identity.scim_group.updated.v1",
            command.trace_id,
            command.now,
        )?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(next)
    }

    pub async fn delete(
        &self,
        command: DeleteScimGroupCommand,
    ) -> Result<(), ScimProvisioningError> {
        let current = self
            .store
            .get_scim_group(command.credential.organization_id, command.group_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if command
            .expected_version
            .is_some_and(|version| version != current.version)
        {
            return Err(ScimProvisioningError::VersionMismatch);
        }
        let mut transaction = self
            .store
            .begin_authorized(&scim_request_context(&command.credential))
            .await?;
        if !transaction
            .delete_scim_group(
                command.credential.organization_id,
                command.group_id,
                command.expected_version,
            )
            .await?
        {
            return Err(ScimProvisioningError::VersionMismatch);
        }
        let (audit, event) = group_lifecycle_records(
            &command.credential,
            &current,
            "identity.scim_group.deleted",
            "identity.scim_group.deleted.v1",
            command.trace_id,
            command.now,
        )?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(())
    }
}

fn validate_token(command: &IssueScimTokenCommand) -> Result<(), ScimProvisioningError> {
    if command.token_hash.len() != 32
        || command.label.trim() != command.label
        || command.label.is_empty()
        || command.label.chars().count() > 200
        || command.label.chars().any(char::is_control)
        || command
            .expires_at
            .is_some_and(|expiry| expiry <= command.now)
    {
        return Err(ScimProvisioningError::InvalidAttribute("token"));
    }
    Ok(())
}

fn validate_directory_strings(
    external_id: &str,
    user_name: &str,
    display_name: &str,
) -> Result<(), ScimProvisioningError> {
    for (name, value, maximum) in [
        ("externalId", external_id, 512),
        ("userName", user_name, 320),
        ("displayName", display_name, 200),
    ] {
        if value.is_empty()
            || value.trim() != value
            || value.chars().count() > maximum
            || value.chars().any(char::is_control)
        {
            return Err(ScimProvisioningError::InvalidAttribute(name));
        }
    }
    Ok(())
}

fn validate_group(
    external_id: &str,
    display_name: &str,
    members: &[PrincipalId],
) -> Result<(), ScimProvisioningError> {
    for (name, value, maximum) in [
        ("externalId", external_id, 512),
        ("displayName", display_name, 200),
    ] {
        if value.is_empty()
            || value.trim() != value
            || value.chars().count() > maximum
            || value.chars().any(char::is_control)
        {
            return Err(ScimProvisioningError::InvalidAttribute(name));
        }
    }
    if members.len() > 10_000 {
        return Err(ScimProvisioningError::InvalidAttribute("members"));
    }
    Ok(())
}

fn normalized_members(members: Vec<PrincipalId>) -> Vec<PrincipalId> {
    members
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalized_name(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn audit_actor(credential: &ScimCredential) -> AuditActor {
    AuditActor {
        principal_id: credential.actor_principal_id,
        on_behalf_of: None,
        credential_id: Some(credential.token_id.to_string()),
    }
}

fn lifecycle_records(
    credential: &ScimCredential,
    user: &ExistingScimUser,
    action: &str,
    event_type: &str,
    trace_id: String,
    now: DateTime<Utc>,
) -> Result<(AuditEvent, PendingEvent), DomainError> {
    let audit = AuditEvent::new(
        credential.organization_id,
        audit_actor(credential),
        action,
        AuditResource {
            kind: "identity.scim_user".into(),
            id: user.principal_id.to_string(),
        },
        AuditOutcome::Success,
        trace_id,
        json!({
            "active": user.active,
            "version": user.version,
            "credential_label": credential.label,
        }),
        now,
    )?;
    let event = PendingEvent::new(
        event_type,
        "identity.scim_user",
        user.principal_id.to_string(),
        json!({
            "organization_id": credential.organization_id,
            "principal_id": user.principal_id,
            "active": user.active,
            "version": user.version,
        }),
        now,
    );
    Ok((audit, event))
}

fn group_lifecycle_records(
    credential: &ScimCredential,
    group: &ExistingScimGroup,
    action: &str,
    event_type: &str,
    trace_id: String,
    now: DateTime<Utc>,
) -> Result<(AuditEvent, PendingEvent), DomainError> {
    let audit = AuditEvent::new(
        credential.organization_id,
        audit_actor(credential),
        action,
        AuditResource {
            kind: "identity.scim_group".into(),
            id: group.group_id.to_string(),
        },
        AuditOutcome::Success,
        trace_id,
        json!({
            "member_count": group.members.len(),
            "version": group.version,
            "credential_label": credential.label,
        }),
        now,
    )?;
    let event = PendingEvent::new(
        event_type,
        "identity.scim_group",
        group.group_id.to_string(),
        json!({
            "organization_id": credential.organization_id,
            "group_id": group.group_id,
            "member_count": group.members.len(),
            "version": group.version,
        }),
        now,
    );
    Ok((audit, event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlPlaneTransaction, ExistingOrganizationBootstrap, ScimDirectoryQueryStore};
    use async_trait::async_trait;
    use copilotmail_domain::{
        AuditEvent, AuthorizationContext, Organization, OrganizationSlug, Role, RoleAssignment,
        ScimTokenId,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemoryStore {
        writes: Arc<Mutex<Vec<&'static str>>>,
        existing: Option<ExistingScimUser>,
    }

    struct MemoryTransaction {
        writes: Arc<Mutex<Vec<&'static str>>>,
        existing: Option<ExistingScimUser>,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(MemoryTransaction {
                writes: self.writes.clone(),
                existing: self.existing.clone(),
            }))
        }
    }

    #[async_trait]
    impl ScimDirectoryQueryStore for MemoryStore {
        async fn authenticate_scim_token(
            &self,
            _: &[u8],
            _: DateTime<Utc>,
        ) -> Result<Option<ScimCredential>, StoreError> {
            Ok(None)
        }

        async fn get_scim_user(
            &self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<Option<ExistingScimUser>, StoreError> {
            Ok(self.existing.clone())
        }

        async fn list_scim_users(
            &self,
            _: OrganizationId,
            _: Option<&str>,
            _: Option<&str>,
            _: i64,
            _: i64,
        ) -> Result<(Vec<ExistingScimUser>, i64), StoreError> {
            Ok((Vec::new(), 0))
        }

        async fn get_scim_group(
            &self,
            _: OrganizationId,
            _: ScimGroupId,
        ) -> Result<Option<ExistingScimGroup>, StoreError> {
            Ok(None)
        }

        async fn list_scim_groups(
            &self,
            _: OrganizationId,
            _: Option<&str>,
            _: Option<&str>,
            _: i64,
            _: i64,
        ) -> Result<(Vec<ExistingScimGroup>, i64), StoreError> {
            Ok((Vec::new(), 0))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for MemoryTransaction {
        async fn bind_request_authentication(
            &mut self,
            _: &AuthorizationContext,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
            Ok(None)
        }
        async fn insert_organization(&mut self, _: &Organization) -> Result<(), StoreError> {
            Ok(())
        }
        async fn insert_principal(&mut self, _: &Principal) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("principal");
            Ok(())
        }
        async fn insert_role(&mut self, _: &Role) -> Result<(), StoreError> {
            Ok(())
        }
        async fn insert_role_assignment(&mut self, _: &RoleAssignment) -> Result<(), StoreError> {
            Ok(())
        }
        async fn record_organization_bootstrap(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Ok(())
        }
        async fn find_scim_user_by_external_id(
            &mut self,
            _: OrganizationId,
            _: &str,
        ) -> Result<Option<ExistingScimUser>, StoreError> {
            self.writes.lock().unwrap().push("external_lookup");
            Ok(self.existing.clone())
        }
        async fn find_human_identity(
            &mut self,
            _: OrganizationId,
            _: &str,
            _: &str,
        ) -> Result<Option<crate::ExistingHumanIdentity>, StoreError> {
            self.writes.lock().unwrap().push("human_lookup");
            Ok(None)
        }
        async fn insert_human_profile(&mut self, _: &HumanProfile) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("profile");
            Ok(())
        }
        async fn insert_scim_user(&mut self, _: &ExistingScimUser) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("scim_user");
            Ok(())
        }
        async fn append_audit_event(&mut self, _: &AuditEvent) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("audit");
            Ok(())
        }
        async fn enqueue_event(&mut self, _: &PendingEvent) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("outbox");
            Ok(())
        }
        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("commit");
            Ok(())
        }
    }

    fn credential(organization_id: OrganizationId) -> ScimCredential {
        ScimCredential {
            token_id: ScimTokenId::new(),
            organization_id,
            actor_principal_id: PrincipalId::new(),
            label: "Entra provisioning".into(),
            expires_at: None,
        }
    }

    fn create_command(organization_id: OrganizationId) -> ProvisionScimUserCommand {
        ProvisionScimUserCommand {
            credential: credential(organization_id),
            identity_provider: "https://login.example.test/tenant".into(),
            external_id: "employee-42".into(),
            user_name: "ada@example.test".into(),
            display_name: "Ada Lovelace".into(),
            active: true,
            trace_id: "scim-test".into(),
            now: Utc::now(),
        }
    }

    #[tokio::test]
    async fn create_writes_principal_profile_projection_audit_and_event_atomically() {
        let store = MemoryStore::default();
        let organization_id = OrganizationId::new();
        let result = ScimUserService::new(store.clone())
            .create(create_command(organization_id))
            .await
            .unwrap();
        assert!(result.created);
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            [
                "external_lookup",
                "human_lookup",
                "principal",
                "profile",
                "scim_user",
                "audit",
                "outbox",
                "commit"
            ]
        );
    }

    #[tokio::test]
    async fn exact_create_replay_is_idempotent() {
        let organization_id = OrganizationId::new();
        let command = create_command(organization_id);
        let existing = ExistingScimUser {
            organization_id,
            principal_id: PrincipalId::new(),
            external_id: command.external_id.clone(),
            user_name: command.user_name.clone(),
            display_name: command.display_name.clone(),
            active: true,
            version: 1,
            created_at: command.now,
            updated_at: command.now,
        };
        let store = MemoryStore {
            existing: Some(existing.clone()),
            ..MemoryStore::default()
        };
        let result = ScimUserService::new(store.clone())
            .create(command)
            .await
            .unwrap();
        assert!(!result.created);
        assert_eq!(result.user, existing);
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            ["external_lookup", "commit"]
        );
    }

    #[test]
    fn invalid_control_characters_never_reach_persistence() {
        assert!(matches!(
            validate_directory_strings("employee-42", "ada\n@example.test", "Ada"),
            Err(ScimProvisioningError::InvalidAttribute("userName"))
        ));
    }
}
