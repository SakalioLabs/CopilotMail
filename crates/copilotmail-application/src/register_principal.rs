use crate::{ControlPlaneStore, PendingEvent, StoreError};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, DomainError, OrganizationId, Principal,
    PrincipalId, PrincipalKind,
};
use serde_json::json;

/// Input for registering a human, Agent, or service identity.
#[derive(Debug, Clone)]
pub struct RegisterPrincipalCommand {
    /// Organization receiving the principal.
    pub organization_id: OrganizationId,
    /// Existing principal performing the operation.
    pub actor_principal_id: PrincipalId,
    /// Optional principal represented by the actor.
    pub on_behalf_of: Option<PrincipalId>,
    /// New principal type.
    pub kind: PrincipalKind,
    /// Human-readable directory name.
    pub display_name: String,
    /// Correlation identifier propagated into audit and event records.
    pub trace_id: String,
    /// Authoritative server timestamp supplied through the application clock.
    pub now: DateTime<Utc>,
}

/// Failure returned by principal registration.
#[derive(Debug, thiserror::Error)]
pub enum RegisterPrincipalError {
    /// Domain validation rejected the command.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// Transactional persistence failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Registers principals while atomically writing audit and outbox records.
pub struct RegisterPrincipalService<S> {
    store: S,
}

impl<S> RegisterPrincipalService<S>
where
    S: ControlPlaneStore,
{
    /// Creates a principal registration service.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Executes the registration as one database transaction.
    pub async fn execute(
        &self,
        command: RegisterPrincipalCommand,
    ) -> Result<Principal, RegisterPrincipalError> {
        let principal = Principal::new(
            command.organization_id,
            command.kind,
            command.display_name,
            command.now,
        )?;
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: command.actor_principal_id,
                on_behalf_of: command.on_behalf_of,
                credential_id: None,
            },
            "identity.principal.created",
            AuditResource {
                kind: "identity.principal".into(),
                id: principal.id().to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "principal_kind": principal.kind(),
                "principal_status": principal.status(),
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "identity.principal.created.v1",
            "identity.principal",
            principal.id().to_string(),
            json!({
                "organization_id": command.organization_id,
                "principal_id": principal.id(),
                "kind": principal.kind(),
                "status": principal.status(),
            }),
            command.now,
        );

        let mut transaction = self.store.begin().await?;
        transaction.insert_principal(&principal).await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(principal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlPlaneTransaction, PendingEvent};
    use async_trait::async_trait;
    use copilotmail_domain::{AuditEvent, Organization, OrganizationSlug, Role, RoleAssignment};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemoryStore {
        writes: Arc<Mutex<Vec<&'static str>>>,
    }

    struct MemoryTransaction {
        writes: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(MemoryTransaction {
                writes: self.writes.clone(),
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for MemoryTransaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
            Ok(None)
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

    #[tokio::test]
    async fn registration_writes_state_audit_and_event_before_commit() {
        let store = MemoryStore::default();
        let service = RegisterPrincipalService::new(store.clone());
        let result = service
            .execute(RegisterPrincipalCommand {
                organization_id: OrganizationId::new(),
                actor_principal_id: PrincipalId::new(),
                on_behalf_of: None,
                kind: PrincipalKind::Agent,
                display_name: "Triage Agent".into(),
                trace_id: "trace-1".into(),
                now: Utc::now(),
            })
            .await;
        assert!(result.is_ok());
        assert_eq!(
            store.writes.lock().unwrap().as_slice(),
            ["principal", "audit", "outbox", "commit"]
        );
    }
}
