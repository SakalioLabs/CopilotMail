use crate::{ControlPlaneStore, ControlPlaneTransaction, PendingEvent, StoreError};
use chrono::{DateTime, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthenticationBinding,
    AuthorizationContext, DomainError, OrganizationId, Permission, PrincipalId, WebhookEventType,
    WebhookSubscription, WebhookSubscriptionId,
};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct CreateWebhookSubscriptionCommand {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
    pub signing_key_reference: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpdateWebhookSubscriptionCommand {
    pub organization_id: OrganizationId,
    pub subscription_id: WebhookSubscriptionId,
    pub expected_version: i64,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
    pub signing_key_reference: String,
    pub enabled: bool,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookSubscriptionMutationResult {
    pub subscription_id: WebhookSubscriptionId,
    pub principal_id: PrincipalId,
    pub enabled: bool,
    pub version: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookSubscriptionError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("webhook subscription operation is forbidden")]
    Forbidden,
    #[error("webhook subscription does not exist")]
    NotFound,
    #[error("webhook subscription changed concurrently")]
    Conflict,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct WebhookSubscriptionService<S> {
    store: S,
}

impl<S: ControlPlaneStore> WebhookSubscriptionService<S> {
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn create(
        &self,
        context: &AuthorizationContext,
        command: CreateWebhookSubscriptionCommand,
    ) -> Result<WebhookSubscriptionMutationResult, WebhookSubscriptionError> {
        let mut transaction = self.store.begin_authorized(context).await?;
        authorize_actor(&mut *transaction, context, command.organization_id).await?;
        authorize_owner(
            &mut *transaction,
            context,
            command.organization_id,
            command.principal_id,
        )
        .await?;
        let event_types = parse_event_types(command.event_types)?;
        let subscription = WebhookSubscription::new(
            command.organization_id,
            command.principal_id,
            command.endpoint_url,
            event_types,
            command.signing_key_reference,
            command.now,
        )?;
        let audit = subscription_audit(
            context.principal_id,
            "agent.webhook_subscription.created",
            &subscription,
            command.trace_id,
            json!({
                "principal_id": subscription.principal_id,
                "event_types": event_type_names(&subscription.event_types),
                "signing_key_reference": subscription.signing_key_reference,
                "enabled": subscription.enabled,
                "version": subscription.version,
            }),
            command.now,
            context.authentication.as_ref(),
        )?;
        let event = subscription_event(
            "agent.webhook_subscription.created.v1",
            &subscription,
            command.now,
        );
        transaction
            .insert_webhook_subscription(&subscription)
            .await?;
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(mutation_result(&subscription))
    }

    pub async fn update(
        &self,
        context: &AuthorizationContext,
        command: UpdateWebhookSubscriptionCommand,
    ) -> Result<WebhookSubscriptionMutationResult, WebhookSubscriptionError> {
        let mut transaction = self.store.begin_authorized(context).await?;
        authorize_actor(&mut *transaction, context, command.organization_id).await?;
        let mut subscription = transaction
            .find_webhook_subscription_for_update(command.organization_id, command.subscription_id)
            .await?
            .ok_or(WebhookSubscriptionError::NotFound)?;
        if subscription.principal_id != context.principal_id
            && !context.permits(command.organization_id, Permission::IdentityManage)
        {
            return Err(WebhookSubscriptionError::NotFound);
        }
        if subscription.version != command.expected_version {
            return Err(WebhookSubscriptionError::Conflict);
        }
        let previously_enabled = subscription.enabled;
        subscription.update(
            command.endpoint_url,
            parse_event_types(command.event_types)?,
            command.signing_key_reference,
            command.enabled,
            command.now,
        )?;
        let disabled = previously_enabled && !subscription.enabled;
        let action = if disabled {
            "agent.webhook_subscription.disabled"
        } else {
            "agent.webhook_subscription.updated"
        };
        let audit = subscription_audit(
            context.principal_id,
            action,
            &subscription,
            command.trace_id,
            json!({
                "principal_id": subscription.principal_id,
                "event_types": event_type_names(&subscription.event_types),
                "signing_key_reference": subscription.signing_key_reference,
                "enabled": subscription.enabled,
                "previous_version": command.expected_version,
                "version": subscription.version,
            }),
            command.now,
            context.authentication.as_ref(),
        )?;
        let event = subscription_event(
            if disabled {
                "agent.webhook_subscription.disabled.v1"
            } else {
                "agent.webhook_subscription.updated.v1"
            },
            &subscription,
            command.now,
        );
        if !transaction
            .update_webhook_subscription(&subscription, command.expected_version)
            .await?
        {
            return Err(WebhookSubscriptionError::Conflict);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(mutation_result(&subscription))
    }
}

async fn authorize_actor(
    transaction: &mut dyn ControlPlaneTransaction,
    context: &AuthorizationContext,
    organization_id: OrganizationId,
) -> Result<(), WebhookSubscriptionError> {
    if !context.permits(organization_id, Permission::AgentOperate)
        || !transaction
            .active_principal_exists(organization_id, context.principal_id)
            .await?
    {
        return Err(WebhookSubscriptionError::Forbidden);
    }
    Ok(())
}

async fn authorize_owner(
    transaction: &mut dyn ControlPlaneTransaction,
    context: &AuthorizationContext,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
) -> Result<(), WebhookSubscriptionError> {
    if principal_id != context.principal_id
        && !context.permits(organization_id, Permission::IdentityManage)
        || !transaction
            .active_agent_principal_exists(organization_id, principal_id)
            .await?
    {
        return Err(WebhookSubscriptionError::Forbidden);
    }
    Ok(())
}

fn parse_event_types(
    values: Vec<String>,
) -> Result<BTreeSet<WebhookEventType>, WebhookSubscriptionError> {
    let original_len = values.len();
    let event_types = values
        .into_iter()
        .map(|value| WebhookEventType::parse(&value))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if event_types.len() != original_len {
        return Err(DomainError::InvalidWebhookSubscription.into());
    }
    Ok(event_types)
}

fn event_type_names(event_types: &BTreeSet<WebhookEventType>) -> Vec<&'static str> {
    event_types.iter().map(|value| value.as_str()).collect()
}

fn subscription_audit(
    actor: PrincipalId,
    action: &'static str,
    subscription: &WebhookSubscription,
    trace_id: String,
    details: serde_json::Value,
    now: DateTime<Utc>,
    authentication: Option<&AuthenticationBinding>,
) -> Result<AuditEvent, DomainError> {
    AuditEvent::new(
        subscription.organization_id,
        AuditActor {
            principal_id: actor,
            on_behalf_of: None,
            credential_id: None,
        },
        action,
        AuditResource {
            kind: "agent.webhook_subscription".into(),
            id: subscription.id.to_string(),
        },
        AuditOutcome::Success,
        trace_id,
        details,
        now,
    )?
    .correlate_human_session(authentication)
}

fn subscription_event(
    event_type: &'static str,
    subscription: &WebhookSubscription,
    now: DateTime<Utc>,
) -> PendingEvent {
    PendingEvent::new(
        event_type,
        "agent.webhook_subscription",
        subscription.id.to_string(),
        json!({
            "organization_id": subscription.organization_id,
            "subscription_id": subscription.id,
            "principal_id": subscription.principal_id,
            "event_types": event_type_names(&subscription.event_types),
            "enabled": subscription.enabled,
            "version": subscription.version,
        }),
        now,
    )
}

fn mutation_result(subscription: &WebhookSubscription) -> WebhookSubscriptionMutationResult {
    WebhookSubscriptionMutationResult {
        subscription_id: subscription.id,
        principal_id: subscription.principal_id,
        enabled: subscription.enabled,
        version: subscription.version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemoryStore {
        state: Arc<Mutex<State>>,
    }

    #[derive(Clone, Default)]
    struct State {
        subscription: Option<WebhookSubscription>,
        actions: Vec<String>,
    }

    struct Transaction {
        shared: Arc<Mutex<State>>,
        working: State,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                shared: self.state.clone(),
                working: self.state.lock().unwrap().clone(),
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

        async fn active_principal_exists(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(true)
        }

        async fn active_agent_principal_exists(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(true)
        }

        async fn insert_webhook_subscription(
            &mut self,
            subscription: &WebhookSubscription,
        ) -> Result<(), StoreError> {
            self.working.subscription = Some(subscription.clone());
            self.working.actions.push("subscription".into());
            Ok(())
        }

        async fn find_webhook_subscription_for_update(
            &mut self,
            _: OrganizationId,
            _: WebhookSubscriptionId,
        ) -> Result<Option<WebhookSubscription>, StoreError> {
            Ok(self.working.subscription.clone())
        }

        async fn update_webhook_subscription(
            &mut self,
            subscription: &WebhookSubscription,
            expected_version: i64,
        ) -> Result<bool, StoreError> {
            if self
                .working
                .subscription
                .as_ref()
                .is_none_or(|current| current.version != expected_version)
            {
                return Ok(false);
            }
            self.working.subscription = Some(subscription.clone());
            self.working.actions.push("subscription_updated".into());
            Ok(true)
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            self.working.actions.push(format!("audit:{}", event.action));
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.working
                .actions
                .push(format!("outbox:{}", event.event_type));
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            let mut working = self.working;
            working.actions.push("commit".into());
            *self.shared.lock().unwrap() = working;
            Ok(())
        }
    }

    fn context(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        manage_identity: bool,
    ) -> AuthorizationContext {
        let mut permissions = BTreeSet::from([Permission::AgentOperate]);
        if manage_identity {
            permissions.insert(Permission::IdentityManage);
        }
        AuthorizationContext {
            principal_id,
            organization_id,
            permissions,
            authentication: None,
        }
    }

    fn create_command(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> CreateWebhookSubscriptionCommand {
        CreateWebhookSubscriptionCommand {
            organization_id,
            principal_id,
            endpoint_url: "https://agent.example.test/hook".into(),
            event_types: vec!["message.received.v1".into()],
            signing_key_reference: "hook-2026-01".into(),
            trace_id: "trace-create-hook".into(),
            now,
        }
    }

    #[tokio::test]
    async fn agent_can_create_only_its_own_allowlisted_subscription_atomically() {
        let store = MemoryStore::default();
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let now = Utc::now();
        let result = WebhookSubscriptionService::new(store.clone())
            .create(
                &context(organization_id, actor, false),
                create_command(organization_id, actor, now),
            )
            .await
            .unwrap();
        assert_eq!(result.version, 1);
        assert_eq!(
            store.state.lock().unwrap().actions,
            [
                "subscription",
                "audit:agent.webhook_subscription.created",
                "outbox:agent.webhook_subscription.created.v1",
                "commit",
            ]
        );

        let other = PrincipalId::new();
        let error = WebhookSubscriptionService::new(MemoryStore::default())
            .create(
                &context(organization_id, actor, false),
                create_command(organization_id, other, now),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, WebhookSubscriptionError::Forbidden));
    }

    #[tokio::test]
    async fn stale_update_cannot_disable_a_newer_subscription() {
        let store = MemoryStore::default();
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let now = Utc::now();
        let created = WebhookSubscriptionService::new(store.clone())
            .create(
                &context(organization_id, actor, false),
                create_command(organization_id, actor, now),
            )
            .await
            .unwrap();
        let error = WebhookSubscriptionService::new(store.clone())
            .update(
                &context(organization_id, actor, false),
                UpdateWebhookSubscriptionCommand {
                    organization_id,
                    subscription_id: created.subscription_id,
                    expected_version: 0,
                    endpoint_url: "https://agent.example.test/new".into(),
                    event_types: vec!["message.received.v1".into()],
                    signing_key_reference: "hook-2026-02".into(),
                    enabled: false,
                    trace_id: "trace-disable".into(),
                    now,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, WebhookSubscriptionError::Conflict));
    }
}
