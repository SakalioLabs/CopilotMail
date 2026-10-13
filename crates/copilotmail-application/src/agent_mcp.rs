use crate::{ControlPlaneStore, PendingEvent, StoreError};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, DomainError,
    OrganizationId, Permission,
};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OpenAgentMcpSessionCommand {
    pub organization_id: OrganizationId,
    pub session_id: Uuid,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentMcpSessionError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("Agent MCP session access is forbidden")]
    Forbidden,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct AgentMcpSessionService<S> {
    store: S,
}

impl<S> AgentMcpSessionService<S>
where
    S: ControlPlaneStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Records the governed session-opened evidence after the transport has
    /// authenticated an active Agent and admitted the upgrade. A failed write
    /// fails the whole session open so an unaudited channel never exists.
    pub async fn open(
        &self,
        context: &AuthorizationContext,
        command: OpenAgentMcpSessionCommand,
    ) -> Result<(), AgentMcpSessionError> {
        if context.organization_id != command.organization_id
            || !context.permits(command.organization_id, Permission::AgentOperate)
        {
            return Err(AgentMcpSessionError::Forbidden);
        }
        let mut transaction = self.store.begin_authorized(context).await?;
        if !transaction
            .active_agent_principal_exists(command.organization_id, context.principal_id)
            .await?
        {
            return Err(AgentMcpSessionError::Forbidden);
        }
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.mcp_session.opened",
            AuditResource {
                kind: "agent.mcp_session".into(),
                id: command.session_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "session_id": command.session_id,
                "transport": "websocket",
                "subprotocol": "mcp",
                "maximum_session_seconds": 900,
            }),
            command.now,
        )?
        .correlate_human_session(context.authentication.as_ref())?;
        let event = PendingEvent::new(
            "agent.mcp_session.opened.v1",
            "agent.mcp_session",
            command.session_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "session_id": command.session_id,
                "principal_id": context.principal_id,
                "transport": "websocket",
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ControlPlaneTransaction;
    use async_trait::async_trait;
    use copilotmail_domain::{AuditEvent as DomainAuditEvent, PrincipalId};
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MemoryStore {
        agent_active: bool,
        actions: Arc<Mutex<Vec<String>>>,
    }

    struct Transaction {
        agent_active: bool,
        actions: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                agent_active: self.agent_active,
                actions: self.actions.clone(),
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for Transaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &copilotmail_domain::OrganizationSlug,
        ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_organization(
            &mut self,
            _: &copilotmail_domain::Organization,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_principal(
            &mut self,
            _: &copilotmail_domain::Principal,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role(&mut self, _: &copilotmail_domain::Role) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role_assignment(
            &mut self,
            _: &copilotmail_domain::RoleAssignment,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn record_organization_bootstrap(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn active_agent_principal_exists(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(self.agent_active)
        }

        async fn append_audit_event(&mut self, event: &DomainAuditEvent) -> Result<(), StoreError> {
            self.actions
                .lock()
                .expect("fixture lock")
                .push(format!("audit:{}", event.action));
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.actions
                .lock()
                .expect("fixture lock")
                .push(format!("outbox:{}", event.event_type));
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            self.actions
                .lock()
                .expect("fixture lock")
                .push("commit".into());
            Ok(())
        }
    }

    fn context(permission: Permission) -> AuthorizationContext {
        AuthorizationContext {
            principal_id: PrincipalId::new(),
            organization_id: OrganizationId::new(),
            permissions: [permission].into_iter().collect(),
            authentication: None,
        }
    }

    #[tokio::test]
    async fn open_records_session_audit_and_outbox_atomically() {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let store = MemoryStore {
            agent_active: true,
            actions: actions.clone(),
        };
        let context = context(Permission::AgentOperate);
        let command = OpenAgentMcpSessionCommand {
            organization_id: context.organization_id,
            session_id: Uuid::now_v7(),
            trace_id: "trace-mcp-open".into(),
            now: Utc::now(),
        };

        AgentMcpSessionService::new(store)
            .open(&context, command)
            .await
            .expect("session open is recorded");

        assert_eq!(
            actions.lock().expect("fixture lock").as_slice(),
            [
                "audit:agent.mcp_session.opened",
                "outbox:agent.mcp_session.opened.v1",
                "commit",
            ]
        );
    }

    #[tokio::test]
    async fn open_requires_agent_operate_and_an_active_agent() {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let store = MemoryStore {
            agent_active: false,
            actions: actions.clone(),
        };
        let denied = context(Permission::MailboxUse);
        let allowed = context(Permission::AgentOperate);
        let other_tenant = OrganizationId::new();
        let command =
            |_context: &AuthorizationContext, organization_id| OpenAgentMcpSessionCommand {
                organization_id,
                session_id: Uuid::now_v7(),
                trace_id: "trace".into(),
                now: Utc::now(),
            };
        let service = AgentMcpSessionService::new(store);

        assert!(matches!(
            service
                .open(&denied, command(&denied, denied.organization_id))
                .await,
            Err(AgentMcpSessionError::Forbidden)
        ));
        assert!(matches!(
            service
                .open(&allowed, command(&allowed, other_tenant))
                .await,
            Err(AgentMcpSessionError::Forbidden)
        ));
        assert!(matches!(
            service
                .open(&allowed, command(&allowed, allowed.organization_id))
                .await,
            Err(AgentMcpSessionError::Forbidden)
        ));
        assert!(actions.lock().expect("fixture lock").is_empty());
    }
}
