use crate::{ControlPlaneStore, ExistingOidcOwnerBinding, PendingEvent, StoreError};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, DomainError, HumanProfile, OrganizationId,
    Principal, PrincipalId, PrincipalKind, RoleAssignment, SystemRole,
};
use serde_json::json;

/// Input for the one-time handover from bootstrap service to a human owner.
#[derive(Debug, Clone)]
pub struct BindOidcOwnerCommand {
    /// Organization being handed over.
    pub organization_id: OrganizationId,
    /// Exact configured OIDC issuer.
    pub issuer: String,
    /// Stable OIDC subject for the human.
    pub subject: String,
    /// Human-readable directory name.
    pub display_name: String,
    /// Correlation identifier written to audit and events.
    pub trace_id: String,
    /// Authoritative server timestamp.
    pub now: DateTime<Utc>,
}

/// Stable result of the idempotent owner handover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindOidcOwnerResult {
    /// Organization receiving the human owner.
    pub organization_id: OrganizationId,
    /// Active human principal created for the OIDC subject.
    pub principal_id: PrincipalId,
    /// Bootstrap service principal permanently retired by the handover.
    pub retired_bootstrap_principal_id: PrincipalId,
    /// Exact OIDC issuer.
    pub issuer: String,
    /// Exact OIDC subject.
    pub subject: String,
    /// Whether this invocation performed the handover.
    pub created: bool,
}

/// Failure returned by the owner handover.
#[derive(Debug, thiserror::Error)]
pub enum BindOidcOwnerError {
    /// Domain validation rejected the identity.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// Transactional persistence failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Hands control from the offline bootstrap identity to an enterprise human.
pub struct BindOidcOwnerService<S> {
    store: S,
}

impl<S> BindOidcOwnerService<S>
where
    S: ControlPlaneStore,
{
    /// Creates the handover service.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Performs the full handover in one transaction.
    pub async fn execute(
        &self,
        command: BindOidcOwnerCommand,
    ) -> Result<BindOidcOwnerResult, BindOidcOwnerError> {
        let mut transaction = self.store.begin().await?;
        let bootstrap = transaction
            .find_organization_bootstrap_by_id(command.organization_id)
            .await?
            .ok_or(StoreError::MissingReference)?;
        if let Some(existing) = transaction
            .find_oidc_owner_binding(command.organization_id)
            .await?
        {
            if existing.issuer != command.issuer || existing.subject != command.subject {
                return Err(StoreError::Conflict.into());
            }
            // audit-coverage: no-op-idempotent-replay
            transaction.commit().await?;
            return Ok(BindOidcOwnerResult {
                organization_id: existing.organization_id,
                principal_id: existing.principal_id,
                retired_bootstrap_principal_id: bootstrap.root_principal_id,
                issuer: existing.issuer,
                subject: existing.subject,
                created: false,
            });
        }
        let owner_role_id = transaction
            .find_system_role(command.organization_id, SystemRole::OrgOwner)
            .await?
            .ok_or(StoreError::MissingReference)?;
        let mut human = Principal::new(
            command.organization_id,
            PrincipalKind::Human,
            command.display_name,
            command.now,
        )?;
        human.activate(command.now)?;
        let profile = HumanProfile::new(&human, command.issuer.clone(), command.subject.clone())?;
        let assignment = RoleAssignment::new(
            command.organization_id,
            human.id(),
            owner_role_id,
            bootstrap.root_principal_id,
            command.now,
            None,
            command.now,
        )?;
        let binding = ExistingOidcOwnerBinding {
            organization_id: command.organization_id,
            principal_id: human.id(),
            issuer: command.issuer,
            subject: command.subject,
            display_name: human.display_name().into(),
        };
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: bootstrap.root_principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.oidc_owner.bound",
            AuditResource {
                kind: "identity.principal".into(),
                id: human.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "identity_provider": profile.identity_provider(),
                "role": SystemRole::OrgOwner,
                "bootstrap_principal_retired": true,
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.oidc_owner.bound.v1",
            "identity.principal",
            human.id().to_string(),
            json!({
                "organization_id": command.organization_id,
                "principal_id": human.id(),
                "retired_bootstrap_principal_id": bootstrap.root_principal_id,
            }),
            command.now,
        );

        transaction.insert_principal(&human).await?;
        transaction.insert_human_profile(&profile).await?;
        transaction.insert_role_assignment(&assignment).await?;
        transaction
            .record_oidc_owner_binding(&binding, bootstrap.root_principal_id, command.now)
            .await?;
        transaction
            .revoke_active_role_assignments(bootstrap.root_principal_id, command.now)
            .await?;
        transaction
            .retire_principal(bootstrap.root_principal_id, command.now)
            .await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;

        Ok(BindOidcOwnerResult {
            organization_id: binding.organization_id,
            principal_id: binding.principal_id,
            retired_bootstrap_principal_id: bootstrap.root_principal_id,
            issuer: binding.issuer,
            subject: binding.subject,
            created: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlPlaneTransaction, ExistingOrganizationBootstrap};
    use async_trait::async_trait;
    use copilotmail_domain::{
        AuditEvent, HumanProfile, Organization, OrganizationSlug, Principal, Role, RoleId,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MemoryStore {
        writes: Arc<Mutex<Vec<&'static str>>>,
        bootstrap: ExistingOrganizationBootstrap,
        existing: Option<ExistingOidcOwnerBinding>,
        owner_role_id: RoleId,
    }

    struct MemoryTransaction {
        writes: Arc<Mutex<Vec<&'static str>>>,
        bootstrap: ExistingOrganizationBootstrap,
        existing: Option<ExistingOidcOwnerBinding>,
        owner_role_id: RoleId,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(MemoryTransaction {
                writes: self.writes.clone(),
                bootstrap: self.bootstrap.clone(),
                existing: self.existing.clone(),
                owner_role_id: self.owner_role_id,
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for MemoryTransaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
            Ok(None)
        }

        async fn find_organization_bootstrap_by_id(
            &mut self,
            _: OrganizationId,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
            self.writes.lock().unwrap().push("bootstrap_lookup");
            Ok(Some(self.bootstrap.clone()))
        }

        async fn find_oidc_owner_binding(
            &mut self,
            _: OrganizationId,
        ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
            self.writes.lock().unwrap().push("binding_lookup");
            Ok(self.existing.clone())
        }

        async fn find_system_role(
            &mut self,
            _: OrganizationId,
            _: SystemRole,
        ) -> Result<Option<RoleId>, StoreError> {
            self.writes.lock().unwrap().push("role_lookup");
            Ok(Some(self.owner_role_id))
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
            self.writes.lock().unwrap().push("assignment");
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

        async fn insert_human_profile(&mut self, _: &HumanProfile) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("profile");
            Ok(())
        }

        async fn record_oidc_owner_binding(
            &mut self,
            _: &ExistingOidcOwnerBinding,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("binding");
            Ok(())
        }

        async fn revoke_active_role_assignments(
            &mut self,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("revoke_bootstrap");
            Ok(())
        }

        async fn retire_principal(
            &mut self,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("retire_bootstrap");
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

    fn store(existing: Option<ExistingOidcOwnerBinding>) -> MemoryStore {
        let organization_id = OrganizationId::new();
        MemoryStore {
            writes: Arc::new(Mutex::new(Vec::new())),
            bootstrap: ExistingOrganizationBootstrap {
                organization_id,
                root_principal_id: PrincipalId::new(),
                slug: "example-corp".into(),
                display_name: "Example Corporation".into(),
            },
            existing,
            owner_role_id: RoleId::new(),
        }
    }

    fn command(organization_id: OrganizationId) -> BindOidcOwnerCommand {
        BindOidcOwnerCommand {
            organization_id,
            issuer: "https://login.example.test/tenant".into(),
            subject: "employee-123".into(),
            display_name: "Initial Owner".into(),
            trace_id: "handover-test".into(),
            now: Utc::now(),
        }
    }

    #[tokio::test]
    async fn handover_is_atomic_and_retires_bootstrap_access() {
        let store = store(None);
        let result = BindOidcOwnerService::new(store.clone())
            .execute(command(store.bootstrap.organization_id))
            .await
            .unwrap();
        assert!(result.created);
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            [
                "bootstrap_lookup",
                "binding_lookup",
                "role_lookup",
                "principal",
                "profile",
                "assignment",
                "binding",
                "revoke_bootstrap",
                "retire_bootstrap",
                "audit",
                "outbox",
                "commit"
            ]
        );
    }

    #[tokio::test]
    async fn exact_replay_returns_existing_human_without_writes() {
        let base = store(None);
        let existing = ExistingOidcOwnerBinding {
            organization_id: base.bootstrap.organization_id,
            principal_id: PrincipalId::new(),
            issuer: "https://login.example.test/tenant".into(),
            subject: "employee-123".into(),
            display_name: "Initial Owner".into(),
        };
        let store = MemoryStore {
            existing: Some(existing.clone()),
            ..base
        };
        let result = BindOidcOwnerService::new(store.clone())
            .execute(command(store.bootstrap.organization_id))
            .await
            .unwrap();
        assert!(!result.created);
        assert_eq!(result.principal_id, existing.principal_id);
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            ["bootstrap_lookup", "binding_lookup", "commit"]
        );
    }

    #[tokio::test]
    async fn replay_with_different_subject_is_rejected() {
        let base = store(None);
        let organization_id = base.bootstrap.organization_id;
        let existing = ExistingOidcOwnerBinding {
            organization_id,
            principal_id: PrincipalId::new(),
            issuer: "https://login.example.test/tenant".into(),
            subject: "existing-subject".into(),
            display_name: "Initial Owner".into(),
        };
        let store = MemoryStore {
            existing: Some(existing),
            ..base
        };
        let error = BindOidcOwnerService::new(store)
            .execute(command(organization_id))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            BindOidcOwnerError::Store(StoreError::Conflict)
        ));
    }
}
