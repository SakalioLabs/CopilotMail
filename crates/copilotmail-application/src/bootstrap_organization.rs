use crate::{ControlPlaneStore, PendingEvent, StoreError};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, DomainError, Organization, OrganizationId,
    Principal, PrincipalId, PrincipalKind, Role, RoleAssignment, SystemRole,
};
use serde_json::json;

/// Input to the offline, one-time organization bootstrap operation.
#[derive(Debug, Clone)]
pub struct BootstrapOrganizationCommand {
    /// Canonical organization key.
    pub slug: String,
    /// Human-readable organization name.
    pub display_name: String,
    /// Root service principal name used until enterprise identity is configured.
    pub root_principal_name: String,
    /// Correlation identifier written to audit.
    pub trace_id: String,
    /// Authoritative server timestamp.
    pub now: DateTime<Utc>,
}

/// Stable bootstrap result; repeated calls for a slug return the same IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapOrganizationResult {
    /// Organization identifier.
    pub organization_id: OrganizationId,
    /// Root service principal identifier.
    pub root_principal_id: PrincipalId,
    /// Canonical slug.
    pub slug: String,
    /// Organization display name.
    pub display_name: String,
    /// Whether this call created the bootstrap records.
    pub created: bool,
}

/// Failure returned by organization bootstrap.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapOrganizationError {
    /// Domain validation rejected the command.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// Transactional persistence failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Atomically creates the initial organization and its root service identity.
pub struct BootstrapOrganizationService<S> {
    store: S,
}

impl<S> BootstrapOrganizationService<S>
where
    S: ControlPlaneStore,
{
    /// Creates a bootstrap service.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Executes a concurrency-safe, idempotent organization bootstrap.
    pub async fn execute(
        &self,
        command: BootstrapOrganizationCommand,
    ) -> Result<BootstrapOrganizationResult, BootstrapOrganizationError> {
        let organization = Organization::new(command.slug, command.display_name, command.now)?;
        let mut transaction = self.store.begin().await?;

        if let Some(existing) = transaction
            .find_organization_bootstrap(organization.slug())
            .await?
        {
            // audit-coverage: no-op-idempotent-replay
            transaction.commit().await?;
            return Ok(BootstrapOrganizationResult {
                organization_id: existing.organization_id,
                root_principal_id: existing.root_principal_id,
                slug: existing.slug,
                display_name: existing.display_name,
                created: false,
            });
        }

        let mut root_principal = Principal::new(
            organization.id(),
            PrincipalKind::Service,
            command.root_principal_name,
            command.now,
        )?;
        root_principal.activate(command.now)?;
        let roles = SystemRole::CATALOG
            .into_iter()
            .map(|system_role| Role::built_in(organization.id(), system_role, command.now))
            .collect::<Vec<_>>();
        let owner_role = roles
            .iter()
            .find(|role| role.system_role() == SystemRole::OrgOwner)
            .expect("built-in role catalog contains organization owner");
        let agent_operator_role = roles
            .iter()
            .find(|role| role.system_role() == SystemRole::AgentOperator)
            .expect("built-in role catalog contains Agent operator");
        let owner_assignment = RoleAssignment::new(
            organization.id(),
            root_principal.id(),
            owner_role.id(),
            root_principal.id(),
            command.now,
            None,
            command.now,
        )?;
        let audit = AuditEvent::new(
            organization.id(),
            AuditActor {
                principal_id: root_principal.id(),
                on_behalf_of: None,
                credential_id: None,
            },
            "organization.bootstrapped",
            AuditResource {
                kind: "organization.tenant".into(),
                id: organization.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "organization_slug": organization.slug(),
                "root_principal_kind": root_principal.kind(),
                "initial_role": owner_role.system_role(),
                "agent_runtime_role": agent_operator_role.system_role(),
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "organization.bootstrapped.v1",
            "organization.tenant",
            organization.id().to_string(),
            json!({
                "organization_id": organization.id(),
                "organization_slug": organization.slug(),
                "root_principal_id": root_principal.id(),
                "owner_role_id": owner_role.id(),
                "agent_operator_role_id": agent_operator_role.id(),
            }),
            command.now,
        );

        transaction.insert_organization(&organization).await?;
        transaction.insert_principal(&root_principal).await?;
        for role in &roles {
            transaction.insert_role(role).await?;
        }
        transaction
            .insert_role_assignment(&owner_assignment)
            .await?;
        transaction
            .record_organization_bootstrap(organization.id(), root_principal.id(), command.now)
            .await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;

        Ok(BootstrapOrganizationResult {
            organization_id: organization.id(),
            root_principal_id: root_principal.id(),
            slug: organization.slug().to_string(),
            display_name: organization.display_name().into(),
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
        AuditEvent, Organization, OrganizationSlug, Principal, Role, RoleAssignment,
    };
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemoryStore {
        writes: Arc<Mutex<Vec<&'static str>>>,
        existing: Option<ExistingOrganizationBootstrap>,
    }

    struct MemoryTransaction {
        writes: Arc<Mutex<Vec<&'static str>>>,
        existing: Option<ExistingOrganizationBootstrap>,
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
    impl ControlPlaneTransaction for MemoryTransaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
            self.writes.lock().unwrap().push("lookup");
            Ok(self.existing.clone())
        }

        async fn insert_organization(&mut self, _: &Organization) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("organization");
            Ok(())
        }

        async fn insert_principal(&mut self, _: &Principal) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("principal");
            Ok(())
        }

        async fn insert_role(&mut self, _: &Role) -> Result<(), StoreError> {
            self.writes.lock().unwrap().push("role");
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
            self.writes.lock().unwrap().push("bootstrap");
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

    fn command() -> BootstrapOrganizationCommand {
        BootstrapOrganizationCommand {
            slug: "example-corp".into(),
            display_name: "Example Corporation".into(),
            root_principal_name: "CopilotMail Bootstrap".into(),
            trace_id: "bootstrap-test".into(),
            now: Utc::now(),
        }
    }

    #[tokio::test]
    async fn bootstrap_writes_identity_audit_and_event_atomically() {
        let store = MemoryStore::default();
        let result = BootstrapOrganizationService::new(store.clone())
            .execute(command())
            .await
            .unwrap();
        assert!(result.created);
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            [
                "lookup",
                "organization",
                "principal",
                "role",
                "role",
                "role",
                "role",
                "role",
                "role",
                "role",
                "role",
                "role",
                "assignment",
                "bootstrap",
                "audit",
                "outbox",
                "commit"
            ]
        );
    }

    #[tokio::test]
    async fn repeated_bootstrap_returns_existing_identity_without_writes() {
        let organization_id = OrganizationId::new();
        let root_principal_id = PrincipalId::new();
        let store = MemoryStore {
            existing: Some(ExistingOrganizationBootstrap {
                organization_id,
                root_principal_id,
                slug: "example-corp".into(),
                display_name: "Example Corporation".into(),
            }),
            ..MemoryStore::default()
        };
        let result = BootstrapOrganizationService::new(store.clone())
            .execute(command())
            .await
            .unwrap();
        assert!(!result.created);
        assert_eq!(result.organization_id, organization_id);
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            ["lookup", "commit"]
        );
    }
}
