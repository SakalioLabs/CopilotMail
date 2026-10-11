use crate::{
    AuditEventQueryStore, AuditEventSearch, AuditEventSummary, ControlPlaneStore, PendingEvent,
    StoreError,
};
use chrono::{DateTime, Duration, Utc};
use copilotmail_domain::{
    AUDIT_EVENT_CATALOG, AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext,
    DomainError, OrganizationId, Permission, PrincipalId,
};
use serde_json::json;
use uuid::Uuid;

const MAX_AUDIT_QUERY_WINDOW: Duration = Duration::days(31);
const MAX_FUTURE_SKEW: Duration = Duration::minutes(5);

#[derive(Debug, Clone)]
pub struct SearchAuditEventsCommand {
    pub organization_id: OrganizationId,
    pub occurred_from: DateTime<Utc>,
    pub occurred_to: DateTime<Utc>,
    pub action: Option<String>,
    pub actor_principal_id: Option<PrincipalId>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub outcome: Option<AuditOutcome>,
    pub request_id: Option<String>,
    pub before_chain_sequence: Option<i64>,
    pub limit: i64,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEventSearchPage {
    pub query_id: Uuid,
    pub items: Vec<AuditEventSummary>,
    pub next_chain_sequence: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AuditEventSearchError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("audit event search is forbidden")]
    Forbidden,
    #[error("the audit event search filters are invalid")]
    InvalidFilter,
    #[error("the audit event search cursor is invalid or unavailable")]
    InvalidCursor,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct AuditEventSearchService<S> {
    store: S,
}

impl<S> AuditEventSearchService<S>
where
    S: ControlPlaneStore + AuditEventQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn search(
        &self,
        context: &AuthorizationContext,
        command: SearchAuditEventsCommand,
    ) -> Result<AuditEventSearchPage, AuditEventSearchError> {
        if context.organization_id != command.organization_id
            || !context.permits(command.organization_id, Permission::AuditRead)
        {
            return Err(AuditEventSearchError::Forbidden);
        }
        validate_search_command(&command)?;
        if !self
            .store
            .active_human_audit_reader_exists(command.organization_id, context.principal_id)
            .await?
        {
            return Err(AuditEventSearchError::Forbidden);
        }
        let mut search = AuditEventSearch {
            organization_id: command.organization_id,
            occurred_from: command.occurred_from,
            occurred_to: command.occurred_to,
            action: command.action,
            actor_principal_id: command.actor_principal_id,
            resource_type: command.resource_type,
            resource_id: command.resource_id,
            outcome: command.outcome,
            request_id: command.request_id,
            before_chain_sequence: command.before_chain_sequence,
            limit: command.limit + 1,
        };
        if let Some(sequence) = command.before_chain_sequence
            && !self
                .store
                .audit_event_search_anchor_exists(&search, sequence)
                .await?
        {
            return Err(AuditEventSearchError::InvalidCursor);
        }

        let mut items = self.store.search_audit_events(&search).await?;
        let has_more = items.len() > command.limit as usize;
        if has_more {
            items.pop();
        }
        let next_chain_sequence = items
            .last()
            .map(|event| event.chain_sequence)
            .or(command.before_chain_sequence);
        let query_id = Uuid::now_v7();

        // Audit reads fail closed: no result page is returned unless the query
        // itself is durably appended to both Audit and Outbox.
        let mut transaction = self.store.begin_authorized(context).await?;
        if !transaction
            .authorize_human_permission(
                command.organization_id,
                context.principal_id,
                Permission::AuditRead,
            )
            .await?
        {
            return Err(AuditEventSearchError::Forbidden);
        }
        search.limit = command.limit;
        let details = json!({
            "query_id": query_id,
            "occurred_from": search.occurred_from,
            "occurred_to": search.occurred_to,
            "action": search.action,
            "actor_principal_id": search.actor_principal_id,
            "resource_type": search.resource_type,
            "resource_id": search.resource_id,
            "outcome": search.outcome,
            "request_id_filter": search.request_id,
            "before_chain_sequence": search.before_chain_sequence,
            "limit": search.limit,
            "result_count": items.len(),
            "first_chain_sequence": items.first().map(|event| event.chain_sequence),
            "last_chain_sequence": items.last().map(|event| event.chain_sequence),
            "has_more": has_more,
        });
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "audit.events.queried",
            AuditResource {
                kind: "audit.query".into(),
                id: query_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            details,
            command.now,
        )?
        .correlate_human_session(context.authentication.as_ref())?;
        let event = PendingEvent::new(
            "audit.events.queried.v1",
            "audit.query",
            query_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "query_id": query_id,
                "actor_principal_id": context.principal_id,
                "result_count": items.len(),
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;

        Ok(AuditEventSearchPage {
            query_id,
            items,
            next_chain_sequence,
            has_more,
        })
    }
}

fn validate_search_command(
    command: &SearchAuditEventsCommand,
) -> Result<(), AuditEventSearchError> {
    let action_definition = command
        .action
        .as_deref()
        .map(|action| {
            AUDIT_EVENT_CATALOG
                .iter()
                .find(|definition| definition.action == action)
                .ok_or(AuditEventSearchError::InvalidFilter)
        })
        .transpose()?;
    if command.occurred_from > command.occurred_to
        || command.occurred_to - command.occurred_from > MAX_AUDIT_QUERY_WINDOW
        || command.occurred_to > command.now + MAX_FUTURE_SKEW
        || !(1..=100).contains(&command.limit)
        || command
            .before_chain_sequence
            .is_some_and(|sequence| sequence <= 0)
        || command.resource_type.as_deref().is_some_and(|kind| {
            !AUDIT_EVENT_CATALOG
                .iter()
                .any(|definition| definition.resource_kind == kind)
        })
        || action_definition.is_some_and(|definition| {
            command
                .resource_type
                .as_deref()
                .is_some_and(|kind| kind != definition.resource_kind)
        })
        || command
            .resource_id
            .as_deref()
            .is_some_and(|value| !valid_filter_text(value, 512))
        || command.resource_id.is_some() && command.resource_type.is_none()
        || command
            .request_id
            .as_deref()
            .is_some_and(|value| !valid_filter_text(value, 200))
        || !valid_filter_text(&command.trace_id, 200)
    {
        return Err(AuditEventSearchError::InvalidFilter);
    }
    Ok(())
}

fn valid_filter_text(value: &str, max_len: usize) -> bool {
    !value.is_empty() && value.len() <= max_len && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlPlaneTransaction, ExistingOrganizationBootstrap};
    use async_trait::async_trait;
    use copilotmail_domain::{
        AuditEventId, Organization, OrganizationSlug, PolicyId, PolicyVersionId, Principal, Role,
        RoleAssignment,
    };
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MemoryStore {
        items: Vec<AuditEventSummary>,
        actions: Arc<Mutex<Vec<String>>>,
        anchor_exists: bool,
        active_human: bool,
    }

    struct Transaction {
        store: MemoryStore,
        pending: Vec<String>,
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(Transaction {
                store: self.clone(),
                pending: Vec::new(),
            }))
        }
    }

    #[async_trait]
    impl AuditEventQueryStore for MemoryStore {
        async fn active_human_audit_reader_exists(
            &self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(self.active_human)
        }

        async fn audit_event_search_anchor_exists(
            &self,
            _: &AuditEventSearch,
            _: i64,
        ) -> Result<bool, StoreError> {
            Ok(self.anchor_exists)
        }

        async fn search_audit_events(
            &self,
            search: &AuditEventSearch,
        ) -> Result<Vec<AuditEventSummary>, StoreError> {
            Ok(self
                .items
                .iter()
                .filter(|event| {
                    search
                        .before_chain_sequence
                        .is_none_or(|before| event.chain_sequence < before)
                })
                .take(search.limit as usize)
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

        async fn authorize_human_permission(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            permission: Permission,
        ) -> Result<bool, StoreError> {
            assert_eq!(permission, Permission::AuditRead);
            Ok(self.store.active_human)
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
            let mut actions = self.store.actions.lock().unwrap();
            actions.extend(self.pending);
            actions.push("commit".into());
            Ok(())
        }
    }

    fn event(
        organization_id: OrganizationId,
        actor_principal_id: PrincipalId,
        sequence: i64,
    ) -> AuditEventSummary {
        AuditEventSummary {
            event_id: AuditEventId::new().as_uuid(),
            organization_id,
            actor_principal_id,
            actor_kind: "human".into(),
            on_behalf_of_principal_id: None,
            credential_id: None,
            subject_principal_id: None,
            action: "organization.bootstrapped".into(),
            resource_type: "organization.tenant".into(),
            resource_id: organization_id.to_string(),
            outcome: AuditOutcome::Success,
            trace_id: "trace-source".into(),
            request_id: "trace-source".into(),
            source_ip: None,
            client_id: None,
            session_id: None,
            token_jti: None,
            policy_id: None::<PolicyId>,
            policy_version_id: None::<PolicyVersionId>,
            approval_request_id: None,
            ticket_reference: None,
            environment: None,
            region: None,
            details: json!({"safe": true}),
            occurred_at: Utc::now(),
            received_at: Utc::now(),
            chain_sequence: sequence,
            hash_version: 1,
            previous_hash_hex: None,
            event_hash_hex: "00".repeat(32),
            signature_batch_id: None,
        }
    }

    #[tokio::test]
    async fn search_is_bounded_human_only_and_audited_before_return() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let actions = Arc::new(Mutex::new(Vec::new()));
        let store = MemoryStore {
            items: vec![
                event(organization_id, principal_id, 3),
                event(organization_id, principal_id, 2),
            ],
            actions: actions.clone(),
            anchor_exists: true,
            active_human: true,
        };
        let context = AuthorizationContext {
            organization_id,
            principal_id,
            permissions: BTreeSet::from([Permission::AuditRead]),
            authentication: None,
        };
        let now = Utc::now();
        let page = AuditEventSearchService::new(store.clone())
            .search(
                &context,
                SearchAuditEventsCommand {
                    organization_id,
                    occurred_from: now - Duration::hours(24),
                    occurred_to: now,
                    action: None,
                    actor_principal_id: None,
                    resource_type: None,
                    resource_id: None,
                    outcome: None,
                    request_id: None,
                    before_chain_sequence: None,
                    limit: 1,
                    trace_id: "trace-query".into(),
                    now,
                },
            )
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.has_more);
        assert_eq!(page.next_chain_sequence, Some(3));
        assert_eq!(
            *actions.lock().unwrap(),
            [
                "audit:audit.events.queried",
                "outbox:audit.events.queried.v1",
                "commit",
            ]
        );

        let mut inactive = store;
        inactive.active_human = false;
        let error = AuditEventSearchService::new(inactive)
            .search(
                &context,
                SearchAuditEventsCommand {
                    organization_id,
                    occurred_from: now - Duration::hours(24),
                    occurred_to: now,
                    action: None,
                    actor_principal_id: None,
                    resource_type: None,
                    resource_id: None,
                    outcome: None,
                    request_id: None,
                    before_chain_sequence: Some(3),
                    limit: 10,
                    trace_id: "trace-query-inactive".into(),
                    now,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(error, AuditEventSearchError::Forbidden));
    }

    #[tokio::test]
    async fn rejects_unbounded_filters_and_fabricated_cursor_anchors() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let store = MemoryStore {
            items: Vec::new(),
            actions: Arc::new(Mutex::new(Vec::new())),
            anchor_exists: false,
            active_human: true,
        };
        let context = AuthorizationContext {
            organization_id,
            principal_id,
            permissions: BTreeSet::from([Permission::AuditRead]),
            authentication: None,
        };
        let now = Utc::now();
        let service = AuditEventSearchService::new(store);
        let invalid_window = service
            .search(
                &context,
                SearchAuditEventsCommand {
                    organization_id,
                    occurred_from: now - Duration::days(32),
                    occurred_to: now,
                    action: None,
                    actor_principal_id: None,
                    resource_type: None,
                    resource_id: None,
                    outcome: None,
                    request_id: None,
                    before_chain_sequence: None,
                    limit: 10,
                    trace_id: "trace-invalid-window".into(),
                    now,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            invalid_window,
            AuditEventSearchError::InvalidFilter
        ));

        let invalid_cursor = service
            .search(
                &context,
                SearchAuditEventsCommand {
                    organization_id,
                    occurred_from: now - Duration::hours(1),
                    occurred_to: now,
                    action: None,
                    actor_principal_id: None,
                    resource_type: None,
                    resource_id: None,
                    outcome: None,
                    request_id: None,
                    before_chain_sequence: Some(99),
                    limit: 10,
                    trace_id: "trace-invalid-cursor".into(),
                    now,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            invalid_cursor,
            AuditEventSearchError::InvalidCursor
        ));
    }
}
