use crate::{
    AgentCursorEvent, AgentEventCursorQueryStore, AgentEventStreamLease,
    AgentEventStreamLeaseAcquisition, ControlPlaneStore, PendingEvent, StoreError,
    WebhookSubscriptionQueryStore,
};
use chrono::{DateTime, Duration, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, DomainError,
    OrganizationId, Permission, WebhookSubscriptionId,
};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PullAgentEventsCommand {
    pub organization_id: OrganizationId,
    pub subscription_id: WebhookSubscriptionId,
    pub after_event_id: Option<Uuid>,
    pub limit: i64,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct OpenAgentEventStreamCommand {
    pub organization_id: OrganizationId,
    pub subscription_id: WebhookSubscriptionId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
    pub lease_id: Uuid,
    pub tenant_connection_limit: u32,
    pub principal_connection_limit: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentEventCursorPage {
    pub items: Vec<AgentCursorEvent>,
    pub next_event_id: Option<Uuid>,
    pub has_more: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentEventCursorError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("Agent event cursor access is forbidden")]
    Forbidden,
    #[error("the webhook subscription does not exist")]
    NotFound,
    #[error("the event cursor is invalid, expired, or belongs to another subscription")]
    InvalidCursor,
    #[error("the cursor page size is invalid")]
    InvalidPageSize,
    #[error("the Agent event stream quota is invalid")]
    InvalidQuota,
    #[error("the tenant Agent event stream capacity is reached")]
    TenantCapacityReached,
    #[error("the Agent event stream capacity is reached")]
    PrincipalCapacityReached,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct AgentEventCursorService<S> {
    store: S,
}

impl<S> AgentEventCursorService<S>
where
    S: ControlPlaneStore + WebhookSubscriptionQueryStore + AgentEventCursorQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn pull(
        &self,
        context: &AuthorizationContext,
        command: PullAgentEventsCommand,
    ) -> Result<AgentEventCursorPage, AgentEventCursorError> {
        if context.organization_id != command.organization_id
            || !context.permits(command.organization_id, Permission::AgentOperate)
        {
            return Err(AgentEventCursorError::Forbidden);
        }
        if !(1..=100).contains(&command.limit) {
            return Err(AgentEventCursorError::InvalidPageSize);
        }
        let subscription = self
            .store
            .get_webhook_subscription(command.organization_id, command.subscription_id)
            .await?
            .filter(|subscription| {
                subscription.enabled && subscription.principal_id == context.principal_id
            })
            .ok_or(AgentEventCursorError::NotFound)?;
        if let Some(after_event_id) = command.after_event_id
            && !self
                .store
                .webhook_cursor_event_exists(
                    command.organization_id,
                    command.subscription_id,
                    subscription.principal_id,
                    after_event_id,
                )
                .await?
        {
            return Err(AgentEventCursorError::InvalidCursor);
        }
        let mut items = self
            .store
            .list_webhook_cursor_events(
                command.organization_id,
                command.subscription_id,
                subscription.principal_id,
                command.after_event_id,
                command.limit + 1,
            )
            .await?;
        let has_more = items.len() > command.limit as usize;
        if has_more {
            items.pop();
        }
        let next_event_id = items
            .last()
            .map(|event| event.event_id)
            .or(command.after_event_id);
        if let (Some(first), Some(last)) = (items.first(), items.last()) {
            let mut transaction = self.store.begin_authorized(context).await?;
            if !transaction
                .active_agent_principal_exists(command.organization_id, context.principal_id)
                .await?
            {
                return Err(AgentEventCursorError::Forbidden);
            }
            let audit = AuditEvent::new(
                command.organization_id,
                AuditActor {
                    principal_id: context.principal_id,
                    on_behalf_of: None,
                    credential_id: None,
                },
                "agent.event_cursor.read",
                AuditResource {
                    kind: "agent.event_cursor".into(),
                    id: command.subscription_id.to_string(),
                },
                AuditOutcome::Success,
                command.trace_id,
                json!({
                    "subscription_id": command.subscription_id,
                    "event_count": items.len(),
                    "first_event_id": first.event_id,
                    "last_event_id": last.event_id,
                    "has_more": has_more,
                }),
                command.now,
            )?
            .correlate_human_session(context.authentication.as_ref())?;
            let event = PendingEvent::new(
                "agent.event_cursor.read.v1",
                "agent.event_cursor",
                command.subscription_id.to_string(),
                json!({
                    "organization_id": command.organization_id,
                    "subscription_id": command.subscription_id,
                    "principal_id": context.principal_id,
                    "event_count": items.len(),
                    "first_event_id": first.event_id,
                    "last_event_id": last.event_id,
                }),
                command.now,
            );
            transaction.append_audit_event(&audit).await?;
            transaction.enqueue_event(&event).await?;
            transaction.commit().await?;
        }
        Ok(AgentEventCursorPage {
            items,
            next_event_id,
            has_more,
        })
    }

    /// Records a successful SSE connection after its initial cursor has been
    /// validated by [`Self::pull`]. Long-lived delivery continues to re-use
    /// `pull`, so disabled subscriptions and retired Agents fail closed.
    pub async fn open_stream(
        &self,
        context: &AuthorizationContext,
        command: OpenAgentEventStreamCommand,
    ) -> Result<AgentEventStreamLease, AgentEventCursorError> {
        if context.organization_id != command.organization_id
            || !context.permits(command.organization_id, Permission::AgentOperate)
        {
            return Err(AgentEventCursorError::Forbidden);
        }
        if command.tenant_connection_limit == 0
            || command.principal_connection_limit == 0
            || command.principal_connection_limit > command.tenant_connection_limit
        {
            return Err(AgentEventCursorError::InvalidQuota);
        }
        self.store
            .get_webhook_subscription(command.organization_id, command.subscription_id)
            .await?
            .filter(|subscription| {
                subscription.enabled && subscription.principal_id == context.principal_id
            })
            .ok_or(AgentEventCursorError::NotFound)?;

        let mut transaction = self.store.begin_authorized(context).await?;
        if !transaction
            .active_agent_principal_exists(command.organization_id, context.principal_id)
            .await?
        {
            return Err(AgentEventCursorError::Forbidden);
        }
        let lease = AgentEventStreamLease {
            lease_id: command.lease_id,
            organization_id: command.organization_id,
            principal_id: context.principal_id,
            subscription_id: command.subscription_id,
            opened_at: command.now,
            expires_at: command.now + Duration::minutes(5),
        };
        match transaction
            .acquire_agent_event_stream_lease(
                &lease,
                command.tenant_connection_limit,
                command.principal_connection_limit,
            )
            .await?
        {
            AgentEventStreamLeaseAcquisition::Acquired(acquired) if acquired == lease => {}
            AgentEventStreamLeaseAcquisition::Acquired(_) => {
                return Err(AgentEventCursorError::Store(StoreError::Conflict));
            }
            AgentEventStreamLeaseAcquisition::TenantCapacityReached => {
                return Err(AgentEventCursorError::TenantCapacityReached);
            }
            AgentEventStreamLeaseAcquisition::PrincipalCapacityReached => {
                return Err(AgentEventCursorError::PrincipalCapacityReached);
            }
        }
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "agent.event_stream.opened",
            AuditResource {
                kind: "agent.event_stream".into(),
                id: command.subscription_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "subscription_id": command.subscription_id,
                "maximum_connection_seconds": 300,
                "lease_id": lease.lease_id,
                "tenant_connection_limit": command.tenant_connection_limit,
                "principal_connection_limit": command.principal_connection_limit,
            }),
            command.now,
        )?
        .correlate_human_session(context.authentication.as_ref())?;
        let event = PendingEvent::new(
            "agent.event_stream.opened.v1",
            "agent.event_stream",
            command.subscription_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "subscription_id": command.subscription_id,
                "principal_id": context.principal_id,
                "maximum_connection_seconds": 300,
                "lease_id": lease.lease_id,
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(lease)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ControlPlaneTransaction, ExistingOrganizationBootstrap, WebhookSubscriptionSummary,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{
        Organization, OrganizationSlug, Principal, PrincipalId, Role, RoleAssignment,
        WebhookEventType,
    };
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MemoryStore {
        subscription: WebhookSubscriptionSummary,
        events: Vec<AgentCursorEvent>,
        actions: Arc<Mutex<Vec<String>>>,
        capacity_rejection: Option<AgentEventStreamLeaseAcquisition>,
    }

    struct Transaction {
        actions: Arc<Mutex<Vec<String>>>,
        pending: Vec<String>,
        capacity_rejection: Option<AgentEventStreamLeaseAcquisition>,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                actions: self.actions.clone(),
                pending: Vec::new(),
                capacity_rejection: self.capacity_rejection.clone(),
            }))
        }
    }

    #[async_trait]
    impl WebhookSubscriptionQueryStore for MemoryStore {
        async fn get_webhook_subscription(
            &self,
            organization_id: OrganizationId,
            subscription_id: WebhookSubscriptionId,
        ) -> Result<Option<WebhookSubscriptionSummary>, StoreError> {
            Ok((self.subscription.organization_id == organization_id
                && self.subscription.subscription_id == subscription_id)
                .then(|| self.subscription.clone()))
        }

        async fn list_webhook_subscriptions(
            &self,
            _: OrganizationId,
            _: Option<PrincipalId>,
            _: Option<WebhookSubscriptionId>,
            _: i64,
        ) -> Result<Vec<WebhookSubscriptionSummary>, StoreError> {
            Err(StoreError::Unavailable)
        }
    }

    #[async_trait]
    impl AgentEventCursorQueryStore for MemoryStore {
        async fn webhook_cursor_event_exists(
            &self,
            organization_id: OrganizationId,
            subscription_id: WebhookSubscriptionId,
            principal_id: PrincipalId,
            event_id: Uuid,
        ) -> Result<bool, StoreError> {
            Ok(self.subscription.organization_id == organization_id
                && self.subscription.subscription_id == subscription_id
                && self.subscription.principal_id == principal_id
                && self.events.iter().any(|event| event.event_id == event_id))
        }

        async fn list_webhook_cursor_events(
            &self,
            organization_id: OrganizationId,
            subscription_id: WebhookSubscriptionId,
            principal_id: PrincipalId,
            after_event_id: Option<Uuid>,
            limit: i64,
        ) -> Result<Vec<AgentCursorEvent>, StoreError> {
            if self.subscription.organization_id != organization_id
                || self.subscription.subscription_id != subscription_id
                || self.subscription.principal_id != principal_id
                || !self.subscription.enabled
            {
                return Ok(Vec::new());
            }
            Ok(self
                .events
                .iter()
                .filter(|event| after_event_id.is_none_or(|after| event.event_id > after))
                .take(limit as usize)
                .cloned()
                .collect())
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for Transaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_organization(&mut self, _: &Organization) -> Result<(), StoreError> {
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

        async fn active_agent_principal_exists(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(true)
        }

        async fn acquire_agent_event_stream_lease(
            &mut self,
            lease: &AgentEventStreamLease,
            _: u32,
            _: u32,
        ) -> Result<AgentEventStreamLeaseAcquisition, StoreError> {
            if let Some(result) = &self.capacity_rejection {
                return Ok(result.clone());
            }
            self.pending.push("lease:acquired".into());
            Ok(AgentEventStreamLeaseAcquisition::Acquired(lease.clone()))
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

    fn fixture() -> (MemoryStore, AuthorizationContext, WebhookSubscriptionId) {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let subscription_id = WebhookSubscriptionId::new();
        let now = Utc::now();
        let mut events = (0..2)
            .map(|index| AgentCursorEvent {
                event_id: Uuid::now_v7(),
                organization_id,
                event_type: WebhookEventType::MessageReceivedV1,
                aggregate_type: "message".into(),
                aggregate_id: format!("message-{index}"),
                schema_version: 1,
                payload: json!({"mailbox_id": "mailbox-safe-reference"}),
                occurred_at: now,
            })
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.event_id);
        (
            MemoryStore {
                subscription: WebhookSubscriptionSummary {
                    subscription_id,
                    organization_id,
                    principal_id,
                    endpoint_url: "https://agent.example.test/events".into(),
                    event_types: vec![WebhookEventType::MessageReceivedV1],
                    signing_key_reference: "agent-hook-1".into(),
                    enabled: true,
                    version: 1,
                    created_at: now,
                    updated_at: now,
                },
                events,
                actions: Arc::new(Mutex::new(Vec::new())),
                capacity_rejection: None,
            },
            AuthorizationContext {
                organization_id,
                principal_id,
                permissions: BTreeSet::from([Permission::AgentOperate]),
                authentication: None,
            },
            subscription_id,
        )
    }

    #[tokio::test]
    async fn cursor_pages_are_owner_scoped_continuous_and_audited_without_payload() {
        let (store, context, subscription_id) = fixture();
        let page = AgentEventCursorService::new(store.clone())
            .pull(
                &context,
                PullAgentEventsCommand {
                    organization_id: context.organization_id,
                    subscription_id,
                    after_event_id: None,
                    limit: 1,
                    trace_id: "trace-cursor".into(),
                    now: Utc::now(),
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.has_more);
        assert_eq!(page.next_event_id, Some(page.items[0].event_id));
        assert_eq!(
            *store.actions.lock().unwrap(),
            [
                "audit:agent.event_cursor.read",
                "outbox:agent.event_cursor.read.v1",
                "commit",
            ]
        );
        let second_page = AgentEventCursorService::new(store.clone())
            .pull(
                &context,
                PullAgentEventsCommand {
                    organization_id: context.organization_id,
                    subscription_id,
                    after_event_id: page.next_event_id,
                    limit: 1,
                    trace_id: "trace-cursor-next".into(),
                    now: Utc::now(),
                },
            )
            .await
            .unwrap();
        assert_eq!(second_page.items.len(), 1);
        assert_ne!(second_page.items[0].event_id, page.items[0].event_id);
        assert!(!second_page.has_more);
        assert_eq!(
            second_page.next_event_id,
            Some(second_page.items[0].event_id)
        );
        let empty = AgentEventCursorService::new(store.clone())
            .pull(
                &context,
                PullAgentEventsCommand {
                    organization_id: context.organization_id,
                    subscription_id,
                    after_event_id: second_page.next_event_id,
                    limit: 1,
                    trace_id: "trace-cursor-empty".into(),
                    now: Utc::now(),
                },
            )
            .await
            .unwrap();
        assert!(empty.items.is_empty());
        assert_eq!(empty.next_event_id, second_page.next_event_id);
        assert_eq!(store.actions.lock().unwrap().len(), 6);

        let invalid = AgentEventCursorService::new(store.clone())
            .pull(
                &context,
                PullAgentEventsCommand {
                    organization_id: context.organization_id,
                    subscription_id,
                    after_event_id: Some(Uuid::now_v7()),
                    limit: 10,
                    trace_id: "trace-invalid-cursor".into(),
                    now: Utc::now(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(invalid, AgentEventCursorError::InvalidCursor));

        let mut other = context.clone();
        other.principal_id = PrincipalId::new();
        let hidden = AgentEventCursorService::new(store)
            .pull(
                &other,
                PullAgentEventsCommand {
                    organization_id: other.organization_id,
                    subscription_id,
                    after_event_id: None,
                    limit: 10,
                    trace_id: "trace-other-agent".into(),
                    now: Utc::now(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(hidden, AgentEventCursorError::NotFound));
    }

    #[tokio::test]
    async fn stream_open_is_owner_scoped_and_audited_without_event_payload() {
        let (store, context, subscription_id) = fixture();
        AgentEventCursorService::new(store.clone())
            .open_stream(
                &context,
                OpenAgentEventStreamCommand {
                    organization_id: context.organization_id,
                    subscription_id,
                    trace_id: "trace-stream".into(),
                    now: Utc::now(),
                    lease_id: Uuid::now_v7(),
                    tenant_connection_limit: 100,
                    principal_connection_limit: 5,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            *store.actions.lock().unwrap(),
            [
                "lease:acquired",
                "audit:agent.event_stream.opened",
                "outbox:agent.event_stream.opened.v1",
                "commit",
            ]
        );

        let mut other = context;
        other.principal_id = PrincipalId::new();
        let error = AgentEventCursorService::new(store)
            .open_stream(
                &other,
                OpenAgentEventStreamCommand {
                    organization_id: other.organization_id,
                    subscription_id,
                    trace_id: "trace-other-stream".into(),
                    now: Utc::now(),
                    lease_id: Uuid::now_v7(),
                    tenant_connection_limit: 100,
                    principal_connection_limit: 5,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, AgentEventCursorError::NotFound));
    }

    #[tokio::test]
    async fn stream_capacity_rejection_commits_no_open_evidence() {
        let (mut store, context, subscription_id) = fixture();
        store.capacity_rejection = Some(AgentEventStreamLeaseAcquisition::PrincipalCapacityReached);
        let error = AgentEventCursorService::new(store.clone())
            .open_stream(
                &context,
                OpenAgentEventStreamCommand {
                    organization_id: context.organization_id,
                    subscription_id,
                    trace_id: "trace-capacity".into(),
                    now: Utc::now(),
                    lease_id: Uuid::now_v7(),
                    tenant_connection_limit: 100,
                    principal_connection_limit: 5,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            AgentEventCursorError::PrincipalCapacityReached
        ));
        assert!(store.actions.lock().unwrap().is_empty());
    }
}
