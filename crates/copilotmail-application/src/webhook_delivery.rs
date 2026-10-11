use crate::{ControlPlaneStore, PendingEvent, StoreError};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, DomainError,
    OrganizationId, Permission, WebhookSubscriptionId,
};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ReplayWebhookDeadLetterCommand {
    pub organization_id: OrganizationId,
    pub subscription_id: WebhookSubscriptionId,
    pub event_id: Uuid,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayWebhookDeadLetterResult {
    pub subscription_id: WebhookSubscriptionId,
    pub event_id: Uuid,
    pub attempt: i32,
    pub status: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplayWebhookDeadLetterError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("manual webhook replay is forbidden")]
    Forbidden,
    #[error("the delivery is not the latest dead letter")]
    NotReplayable,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct WebhookDeadLetterService<S> {
    store: S,
}

impl<S: ControlPlaneStore> WebhookDeadLetterService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn replay(
        &self,
        context: &AuthorizationContext,
        command: ReplayWebhookDeadLetterCommand,
    ) -> Result<ReplayWebhookDeadLetterResult, ReplayWebhookDeadLetterError> {
        if context.organization_id != command.organization_id
            || !context.permits(command.organization_id, Permission::SecurityAdmin)
        {
            return Err(ReplayWebhookDeadLetterError::Forbidden);
        }
        let mut transaction = self.store.begin_authorized(context).await?;
        if !transaction
            .authorize_human_permission(
                command.organization_id,
                context.principal_id,
                Permission::SecurityAdmin,
            )
            .await?
        {
            return Err(ReplayWebhookDeadLetterError::Forbidden);
        }
        let attempt = transaction
            .replay_dead_lettered_webhook_delivery(
                command.organization_id,
                command.subscription_id,
                command.event_id,
                command.now,
            )
            .await?
            .ok_or(ReplayWebhookDeadLetterError::NotReplayable)?;
        let resource_id = format!("{}:{}", command.subscription_id, command.event_id);
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.webhook_delivery.replayed",
            AuditResource {
                kind: "agent.webhook_delivery".into(),
                id: resource_id.clone(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "subscription_id": command.subscription_id,
                "event_id": command.event_id,
                "attempt": attempt,
                "reason_code": "manual_replay_requested",
            }),
            command.now,
        )?
        .correlate_human_session(context.authentication.as_ref())?;
        let event = PendingEvent::new(
            "agent.webhook_delivery.replayed.v1",
            "agent.webhook_delivery",
            resource_id,
            json!({
                "organization_id": command.organization_id,
                "subscription_id": command.subscription_id,
                "event_id": command.event_id,
                "attempt": attempt,
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(ReplayWebhookDeadLetterResult {
            subscription_id: command.subscription_id,
            event_id: command.event_id,
            attempt,
            status: "retrying",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ControlPlaneTransaction;
    use async_trait::async_trait;
    use copilotmail_domain::{OrganizationSlug, Principal, PrincipalId, Role, RoleAssignment};
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MemoryStore {
        replayable: bool,
        human_actor: bool,
        actions: Arc<Mutex<Vec<String>>>,
    }

    struct Transaction {
        replayable: bool,
        human_actor: bool,
        actions: Arc<Mutex<Vec<String>>>,
        pending: Vec<String>,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                replayable: self.replayable,
                human_actor: self.human_actor,
                actions: self.actions.clone(),
                pending: Vec::new(),
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for Transaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_organization(
            &mut self,
            _: &copilotmail_domain::Organization,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_principal(&mut self, _: &Principal) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role(&mut self, _: &Role) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role_assignment(&mut self, _: &RoleAssignment) -> Result<(), StoreError> {
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

        async fn authorize_human_permission(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            permission: Permission,
        ) -> Result<bool, StoreError> {
            assert_eq!(permission, Permission::SecurityAdmin);
            Ok(self.human_actor)
        }

        async fn replay_dead_lettered_webhook_delivery(
            &mut self,
            _: OrganizationId,
            _: WebhookSubscriptionId,
            _: Uuid,
            _: DateTime<Utc>,
        ) -> Result<Option<i32>, StoreError> {
            if !self.replayable {
                return Ok(None);
            }
            self.replayable = false;
            self.pending.push("delivery:retrying".into());
            Ok(Some(4))
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            self.pending.push(format!("audit:{}", event.action));
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.pending.push(format!("outbox:{}", event.event_type));
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            let mut actions = self.actions.lock().unwrap();
            actions.extend(self.pending);
            actions.push("commit".into());
            Ok(())
        }
    }

    fn context(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        security_admin: bool,
    ) -> AuthorizationContext {
        AuthorizationContext {
            organization_id,
            principal_id,
            permissions: if security_admin {
                BTreeSet::from([Permission::SecurityAdmin])
            } else {
                BTreeSet::new()
            },
            authentication: None,
        }
    }

    #[tokio::test]
    async fn replay_is_append_only_audited_and_human_admin_only() {
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let subscription_id = WebhookSubscriptionId::new();
        let event_id = Uuid::now_v7();
        let actions = Arc::new(Mutex::new(Vec::new()));
        let command = ReplayWebhookDeadLetterCommand {
            organization_id,
            subscription_id,
            event_id,
            trace_id: "trace-replay".into(),
            now: Utc::now(),
        };
        let result = WebhookDeadLetterService::new(MemoryStore {
            replayable: true,
            human_actor: true,
            actions: actions.clone(),
        })
        .replay(&context(organization_id, actor, true), command.clone())
        .await
        .unwrap();
        assert_eq!(result.attempt, 4);
        assert_eq!(
            *actions.lock().unwrap(),
            [
                "delivery:retrying",
                "audit:agent.webhook_delivery.replayed",
                "outbox:agent.webhook_delivery.replayed.v1",
                "commit",
            ]
        );

        let error = WebhookDeadLetterService::new(MemoryStore {
            replayable: true,
            human_actor: true,
            actions: Arc::new(Mutex::new(Vec::new())),
        })
        .replay(&context(organization_id, actor, false), command)
        .await
        .unwrap_err();
        assert!(matches!(error, ReplayWebhookDeadLetterError::Forbidden));

        let agent_error = WebhookDeadLetterService::new(MemoryStore {
            replayable: true,
            human_actor: false,
            actions: Arc::new(Mutex::new(Vec::new())),
        })
        .replay(
            &context(organization_id, actor, true),
            ReplayWebhookDeadLetterCommand {
                organization_id,
                subscription_id,
                event_id,
                trace_id: "trace-agent-replay".into(),
                now: Utc::now(),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(
            agent_error,
            ReplayWebhookDeadLetterError::Forbidden
        ));
    }
}
