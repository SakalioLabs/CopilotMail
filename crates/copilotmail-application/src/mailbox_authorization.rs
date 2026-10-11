use crate::{
    ControlPlaneStore, ControlPlaneTransaction, MailboxAuthorizationStore, PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthenticationBinding,
    AuthorizationContext, DelegationId, DomainError, Mailbox, MailboxAddress,
    MailboxAuthorizationDecision, MailboxCapability, MailboxDelegation, MailboxId, MailboxKind,
    MailboxMembership, MailboxMembershipRole, OrganizationId, Permission, PrincipalId,
};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct CreateMailboxCommand {
    pub organization_id: OrganizationId,
    pub kind: MailboxKind,
    pub primary_address: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateMailboxResult {
    pub mailbox_id: MailboxId,
    pub organization_id: OrganizationId,
    pub owner_principal_id: PrincipalId,
    pub primary_address: String,
}

#[derive(Debug, Clone)]
pub struct GrantMailboxDelegationCommand {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub grantee_principal_id: PrincipalId,
    pub capabilities: BTreeSet<MailboxCapability>,
    pub reason: String,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantMailboxDelegationResult {
    pub delegation_id: DelegationId,
    pub grantor_principal_id: PrincipalId,
    pub grantee_principal_id: PrincipalId,
    pub mailbox_id: MailboxId,
}

#[derive(Debug, Clone)]
pub struct RevokeMailboxDelegationCommand {
    pub organization_id: OrganizationId,
    pub delegation_id: DelegationId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum MailboxAuthorizationError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("mailbox authorization context does not permit this operation")]
    Forbidden,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct MailboxManagementService<S> {
    store: S,
}

impl<S: ControlPlaneStore> MailboxManagementService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        command: CreateMailboxCommand,
    ) -> Result<CreateMailboxResult, MailboxAuthorizationError> {
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        persist_mailbox(transaction, command, owner.principal_id, None).await
    }

    pub async fn create_authorized(
        &self,
        context: &AuthorizationContext,
        command: CreateMailboxCommand,
    ) -> Result<CreateMailboxResult, MailboxAuthorizationError> {
        if !context.permits(command.organization_id, Permission::MailAdmin) {
            return Err(MailboxAuthorizationError::Forbidden);
        }
        let mut transaction = self.store.begin_authorized(context).await?;
        if !transaction
            .active_principal_exists(command.organization_id, context.principal_id)
            .await?
        {
            return Err(MailboxAuthorizationError::Forbidden);
        }
        persist_mailbox(
            transaction,
            command,
            context.principal_id,
            context.authentication.as_ref(),
        )
        .await
    }
}

async fn persist_mailbox(
    mut transaction: Box<dyn ControlPlaneTransaction>,
    command: CreateMailboxCommand,
    actor: PrincipalId,
    authentication: Option<&AuthenticationBinding>,
) -> Result<CreateMailboxResult, MailboxAuthorizationError> {
    let address = MailboxAddress::parse(&command.primary_address)?;
    let mailbox = Mailbox::new(command.organization_id, command.kind, address, command.now);
    let membership = MailboxMembership::new(
        command.organization_id,
        mailbox.id,
        actor,
        MailboxMembershipRole::Owner,
        command.now,
        None,
        actor,
    )?;
    let audit = AuditEvent::new(
        command.organization_id,
        AuditActor {
            principal_id: actor,
            on_behalf_of: None,
            credential_id: None,
        },
        "mailbox.created",
        AuditResource {
            kind: "mailbox".into(),
            id: mailbox.id.to_string(),
        },
        AuditOutcome::Success,
        command.trace_id,
        json!({"kind": mailbox.kind, "primary_address": mailbox.primary_address}),
        command.now,
    )?
    .correlate_human_session(authentication)?;
    let event = PendingEvent::new(
        "mailbox.created.v1",
        "mailbox",
        mailbox.id.to_string(),
        json!({
            "organization_id": command.organization_id,
            "mailbox_id": mailbox.id,
            "owner_principal_id": actor,
        }),
        command.now,
    );
    transaction.insert_mailbox(&mailbox).await?;
    transaction.insert_mailbox_membership(&membership).await?;
    transaction.append_audit_event(&audit).await?;
    transaction.enqueue_event(&event).await?;
    transaction.commit().await?;
    Ok(CreateMailboxResult {
        mailbox_id: mailbox.id,
        organization_id: command.organization_id,
        owner_principal_id: actor,
        primary_address: mailbox.primary_address.to_string(),
    })
}

pub struct MailboxDelegationService<S> {
    store: S,
}

impl<S: ControlPlaneStore> MailboxDelegationService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn grant(
        &self,
        command: GrantMailboxDelegationCommand,
    ) -> Result<GrantMailboxDelegationResult, MailboxAuthorizationError> {
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        drop(transaction);
        self.grant_as(command, owner.principal_id, None).await
    }

    pub async fn grant_authorized(
        &self,
        context: &AuthorizationContext,
        command: GrantMailboxDelegationCommand,
    ) -> Result<GrantMailboxDelegationResult, MailboxAuthorizationError> {
        if !context.permits(command.organization_id, Permission::AuthorizationManage) {
            return Err(MailboxAuthorizationError::Forbidden);
        }
        self.grant_as(command, context.principal_id, Some(context))
            .await
    }

    async fn grant_as(
        &self,
        command: GrantMailboxDelegationCommand,
        actor: PrincipalId,
        context: Option<&AuthorizationContext>,
    ) -> Result<GrantMailboxDelegationResult, MailboxAuthorizationError> {
        let mut transaction = match context {
            Some(context) => self.store.begin_authorized(context).await?,
            None => self.store.begin().await?,
        };
        if !transaction
            .mailbox_owned_by(
                command.organization_id,
                command.mailbox_id,
                actor,
                command.now,
            )
            .await?
            || !transaction
                .active_principal_exists(command.organization_id, command.grantee_principal_id)
                .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        let delegation = MailboxDelegation::new(
            command.organization_id,
            actor,
            command.grantee_principal_id,
            command.mailbox_id,
            command.capabilities,
            command.reason,
            command.valid_from,
            command.valid_until,
            command.now,
        )?;
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: actor,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.mailbox_delegation.granted",
            AuditResource {
                kind: "authorization.mailbox_delegation".into(),
                id: delegation.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "mailbox_id": command.mailbox_id,
                "grantee_principal_id": command.grantee_principal_id,
                "capabilities": delegation.capabilities().collect::<Vec<_>>(),
                "valid_from": delegation.valid_from(),
                "valid_until": delegation.valid_until(),
                "reason": delegation.reason(),
            }),
            command.now,
        )?
        .correlate_human_session(context.and_then(|item| item.authentication.as_ref()))?;
        let event = PendingEvent::new(
            "authorization.mailbox_delegation.granted.v1",
            "authorization.mailbox_delegation",
            delegation.id().to_string(),
            json!({
                "organization_id": command.organization_id,
                "mailbox_id": command.mailbox_id,
                "grantor_principal_id": actor,
                "grantee_principal_id": command.grantee_principal_id,
            }),
            command.now,
        );
        transaction.insert_mailbox_delegation(&delegation).await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(GrantMailboxDelegationResult {
            delegation_id: delegation.id(),
            grantor_principal_id: actor,
            grantee_principal_id: command.grantee_principal_id,
            mailbox_id: command.mailbox_id,
        })
    }

    pub async fn revoke(
        &self,
        command: RevokeMailboxDelegationCommand,
    ) -> Result<(), MailboxAuthorizationError> {
        let mut transaction = self.store.begin().await?;
        let owner = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        drop(transaction);
        self.revoke_as(command, owner.principal_id, None).await
    }

    pub async fn revoke_authorized(
        &self,
        context: &AuthorizationContext,
        command: RevokeMailboxDelegationCommand,
    ) -> Result<(), MailboxAuthorizationError> {
        if !context.permits(command.organization_id, Permission::AuthorizationManage) {
            return Err(MailboxAuthorizationError::Forbidden);
        }
        self.revoke_as(command, context.principal_id, Some(context))
            .await
    }

    async fn revoke_as(
        &self,
        command: RevokeMailboxDelegationCommand,
        actor: PrincipalId,
        context: Option<&AuthorizationContext>,
    ) -> Result<(), MailboxAuthorizationError> {
        let mut transaction = match context {
            Some(context) => self.store.begin_authorized(context).await?,
            None => self.store.begin().await?,
        };
        if !transaction
            .mailbox_delegation_managed_by(
                command.organization_id,
                command.delegation_id,
                actor,
                command.now,
            )
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        if !transaction
            .revoke_mailbox_delegation(command.organization_id, command.delegation_id, command.now)
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: actor,
                on_behalf_of: None,
                credential_id: None,
            },
            "authorization.mailbox_delegation.revoked",
            AuditResource {
                kind: "authorization.mailbox_delegation".into(),
                id: command.delegation_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({}),
            command.now,
        )?
        .correlate_human_session(context.and_then(|item| item.authentication.as_ref()))?;
        let event = PendingEvent::new(
            "authorization.mailbox_delegation.revoked.v1",
            "authorization.mailbox_delegation",
            command.delegation_id.to_string(),
            json!({"organization_id": command.organization_id}),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(())
    }
}

pub struct AuthorizeMailboxActionService<S> {
    store: S,
}

impl<S: MailboxAuthorizationStore> AuthorizeMailboxActionService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Returns `None` for missing, cross-tenant, cross-mailbox, expired, revoked,
    /// or insufficiently scoped access so callers cannot use it as an oracle.
    pub async fn authorize(
        &self,
        context: &AuthorizationContext,
        mailbox_id: MailboxId,
        capability: MailboxCapability,
        on_behalf_of: Option<PrincipalId>,
        now: DateTime<Utc>,
    ) -> Result<Option<MailboxAuthorizationDecision>, StoreError> {
        if !context.permits(context.organization_id, Permission::MailboxUse) {
            return Ok(None);
        }
        let Some(snapshot) = self
            .store
            .resolve_mailbox_access(context.principal_id, mailbox_id, now)
            .await?
        else {
            return Ok(None);
        };
        Ok(snapshot.authorize(
            context.organization_id,
            mailbox_id,
            capability,
            on_behalf_of,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use copilotmail_domain::{MailboxAccessSnapshot, ResolvedMailboxDelegation};

    struct StaticMailboxStore {
        snapshot: MailboxAccessSnapshot,
    }

    #[async_trait]
    impl MailboxAuthorizationStore for StaticMailboxStore {
        async fn resolve_mailbox_access(
            &self,
            _principal_id: PrincipalId,
            _mailbox_id: MailboxId,
            _now: DateTime<Utc>,
        ) -> Result<Option<MailboxAccessSnapshot>, StoreError> {
            Ok(Some(self.snapshot.clone()))
        }
    }

    #[tokio::test]
    async fn rbac_is_only_a_ceiling_and_cross_mailbox_is_hidden() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mailbox_id = MailboxId::new();
        let service = AuthorizeMailboxActionService::new(StaticMailboxStore {
            snapshot: MailboxAccessSnapshot {
                organization_id,
                mailbox_id,
                membership: None,
                delegations: vec![ResolvedMailboxDelegation {
                    delegation_id: DelegationId::new(),
                    grantor_principal_id: PrincipalId::new(),
                    capabilities: [MailboxCapability::Read].into_iter().collect(),
                }],
            },
        });
        let now = Utc::now();
        let no_rbac = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: BTreeSet::new(),
            authentication: None,
        };
        assert!(
            service
                .authorize(&no_rbac, mailbox_id, MailboxCapability::Read, None, now,)
                .await
                .unwrap()
                .is_none()
        );
        let permitted_rbac = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
            authentication: None,
        };
        assert!(
            service
                .authorize(
                    &permitted_rbac,
                    MailboxId::new(),
                    MailboxCapability::Read,
                    None,
                    now,
                )
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .authorize(
                    &permitted_rbac,
                    mailbox_id,
                    MailboxCapability::Read,
                    None,
                    now,
                )
                .await
                .unwrap()
                .is_some()
        );
    }
}
