use crate::{
    AgentMessageClaimWrite, AgentMessageProcessingOperation, AgentMessageProcessingResult,
    AgentMessageProcessingState, AgentMessageTransitionWrite, AgentProcessingDeadLetterQuery,
    AgentProcessingDeadLetterRecord, AgentProcessingQueryStore,
    AgentProcessingSubscriptionChangeResult, AgentProcessingSubscriptionChangeWrite,
    AgentProcessingSubscriptionRecord, AgentProcessingSubscriptionStatus, ControlPlaneStore,
    MailboxAuthorizationStore, PendingEvent, StoreError,
};
use chrono::{DateTime, Duration, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, DomainError,
    LeaseId, MailboxCapability, MailboxId, Permission, SubscriptionId,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MIN_LEASE_SECONDS: u32 = 30;
const MAX_LEASE_SECONDS: u32 = 300;
const MAX_PROCESSING_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone)]
pub struct EnsureAgentProcessingSubscriptionCommand {
    pub context: AuthorizationContext,
    pub mailbox_id: MailboxId,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ClaimAgentMessageCommand {
    pub context: AuthorizationContext,
    pub subscription_id: SubscriptionId,
    pub idempotency_key: String,
    pub lease_seconds: u32,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ExtendAgentMessageLeaseCommand {
    pub context: AuthorizationContext,
    pub subscription_id: SubscriptionId,
    pub message_id: Uuid,
    pub lease_id: LeaseId,
    pub idempotency_key: String,
    pub lease_seconds: u32,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AcknowledgeAgentMessageCommand {
    pub context: AuthorizationContext,
    pub subscription_id: SubscriptionId,
    pub message_id: Uuid,
    pub lease_id: LeaseId,
    pub idempotency_key: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NackAgentMessageCommand {
    pub context: AuthorizationContext,
    pub subscription_id: SubscriptionId,
    pub message_id: Uuid,
    pub lease_id: LeaseId,
    pub idempotency_key: String,
    pub reason_code: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ListAgentProcessingDeadLettersCommand {
    pub context: AuthorizationContext,
    pub subscription_id: SubscriptionId,
    pub before_message_id: Option<Uuid>,
    pub limit: u16,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ChangeAgentProcessingSubscriptionCommand {
    pub context: AuthorizationContext,
    pub subscription_id: SubscriptionId,
    pub expected_status: AgentProcessingSubscriptionStatus,
    pub target_status: AgentProcessingSubscriptionStatus,
    pub idempotency_key: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcessingDeadLetterPage {
    pub items: Vec<AgentProcessingDeadLetterRecord>,
    pub next_before_message_id: Option<Uuid>,
    pub has_more: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentProcessingError {
    #[error("Agent processing request is invalid")]
    InvalidRequest,
    #[error("Agent processing operation is forbidden")]
    Forbidden,
    #[error("Agent processing resource was not found")]
    NotFound,
    #[error("Agent processing state or idempotency key conflicts")]
    Conflict,
    #[error("Agent processing persistence is unavailable")]
    Unavailable,
    #[error(transparent)]
    Domain(#[from] DomainError),
}

pub struct AgentProcessingService<S> {
    store: S,
}

impl<S> AgentProcessingService<S>
where
    S: ControlPlaneStore + AgentProcessingQueryStore + MailboxAuthorizationStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn ensure_subscription(
        &self,
        command: EnsureAgentProcessingSubscriptionCommand,
    ) -> Result<AgentProcessingSubscriptionRecord, AgentProcessingError> {
        ensure_agent_permissions(&command.context)?;
        self.ensure_mailbox_read(&command.context, command.mailbox_id, command.now)
            .await?;

        let record = AgentProcessingSubscriptionRecord {
            subscription_id: SubscriptionId::new(),
            organization_id: command.context.organization_id,
            principal_id: command.context.principal_id,
            mailbox_id: command.mailbox_id,
            status: AgentProcessingSubscriptionStatus::Active,
            created_at: command.now,
            updated_at: command.now,
        };
        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(map_store_error)?;
        if !transaction
            .active_principal_exists(
                command.context.organization_id,
                command.context.principal_id,
            )
            .await
            .map_err(map_store_error)?
        {
            return Err(AgentProcessingError::Forbidden);
        }
        let (stored, created) = transaction
            .ensure_agent_processing_subscription(&record)
            .await
            .map_err(map_store_error)?;
        validate_subscription(&command.context, command.mailbox_id, &stored)?;
        if !created {
            return Ok(stored);
        }

        append_processing_evidence(
            &mut *transaction,
            &command.context,
            "agent.processing_subscription.created",
            "agent.processing_subscription.created.v1",
            "agent.processing_subscription",
            stored.subscription_id.to_string(),
            json!({
                "organization_id": stored.organization_id,
                "subscription_id": stored.subscription_id,
                "mailbox_id": stored.mailbox_id,
            }),
            command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(stored)
    }

    pub async fn change_subscription(
        &self,
        command: ChangeAgentProcessingSubscriptionCommand,
    ) -> Result<AgentProcessingSubscriptionChangeResult, AgentProcessingError> {
        ensure_agent_permissions(&command.context)?;
        validate_idempotency_key(&command.idempotency_key)?;
        validate_subscription_status_transition(command.expected_status, command.target_status)?;
        let subscription = self
            .owned_subscription(&command.context, command.subscription_id, command.now)
            .await?;
        validate_subscription(&command.context, subscription.mailbox_id, &subscription)?;

        let idempotency_key_sha256 = Sha256::digest(command.idempotency_key.as_bytes()).into();
        let request_sha256 = subscription_change_request_digest(
            &command.context,
            command.subscription_id,
            command.expected_status,
            command.target_status,
        );
        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(map_store_error)?;
        let result = transaction
            .change_agent_processing_subscription(&AgentProcessingSubscriptionChangeWrite {
                operation_id: Uuid::new_v4(),
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                subscription_id: command.subscription_id,
                expected_status: command.expected_status,
                target_status: command.target_status,
                idempotency_key_sha256,
                request_sha256,
                now: command.now,
            })
            .await
            .map_err(map_store_error)?
            .ok_or(AgentProcessingError::Conflict)?;
        validate_subscription_change_result(&command.context, &result)?;
        if result.replayed {
            return Ok(result);
        }

        let (audit_action, event_type) = match command.target_status {
            AgentProcessingSubscriptionStatus::Paused => (
                "agent.processing_subscription.paused",
                "agent.processing_subscription.paused.v1",
            ),
            AgentProcessingSubscriptionStatus::Active => (
                "agent.processing_subscription.resumed",
                "agent.processing_subscription.resumed.v1",
            ),
            AgentProcessingSubscriptionStatus::Retired => (
                "agent.processing_subscription.retired",
                "agent.processing_subscription.retired.v1",
            ),
        };
        append_processing_evidence(
            &mut *transaction,
            &command.context,
            audit_action,
            event_type,
            "agent.processing_subscription",
            result.subscription.subscription_id.to_string(),
            json!({
                "organization_id": result.subscription.organization_id,
                "subscription_id": result.subscription.subscription_id,
                "mailbox_id": result.subscription.mailbox_id,
                "operation_id": result.operation_id,
                "previous_status": result.previous_status.as_str(),
                "status": result.subscription.status.as_str(),
            }),
            command.trace_id,
            result.occurred_at,
        )
        .await?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(result)
    }

    pub async fn claim(
        &self,
        command: ClaimAgentMessageCommand,
    ) -> Result<Option<AgentMessageProcessingResult>, AgentProcessingError> {
        ensure_agent_permissions(&command.context)?;
        validate_idempotency_key(&command.idempotency_key)?;
        validate_lease_seconds(command.lease_seconds)?;
        let subscription = self
            .owned_subscription(&command.context, command.subscription_id, command.now)
            .await?;
        require_subscription_active(&subscription)?;

        let idempotency_key_sha256 = Sha256::digest(command.idempotency_key.as_bytes()).into();
        let request_sha256 = processing_request_digest(
            &command.context,
            command.subscription_id,
            None,
            AgentMessageProcessingOperation::Claimed,
            None,
            Some(command.lease_seconds),
            None,
        );
        let lease_id = deterministic_lease_id(request_sha256, idempotency_key_sha256);
        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(map_store_error)?;
        let result = transaction
            .claim_next_agent_message(&AgentMessageClaimWrite {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                subscription_id: command.subscription_id,
                lease_id,
                idempotency_key_sha256,
                request_sha256,
                lease_expires_at: command.now + Duration::seconds(i64::from(command.lease_seconds)),
                now: command.now,
            })
            .await
            .map_err(map_store_error)?;
        let Some(result) = result else {
            return Ok(None);
        };
        validate_processing_result(&command.context, &subscription, &result)?;
        if result.replayed {
            return Ok(Some(result));
        }
        append_result_evidence(
            &mut *transaction,
            &command.context,
            &result,
            "agent.message.claimed",
            "agent.message.claimed.v1",
            command.trace_id,
        )
        .await?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(Some(result))
    }

    pub async fn extend_lease(
        &self,
        command: ExtendAgentMessageLeaseCommand,
    ) -> Result<AgentMessageProcessingResult, AgentProcessingError> {
        validate_lease_seconds(command.lease_seconds)?;
        self.transition(
            command.context,
            command.subscription_id,
            command.message_id,
            command.lease_id,
            command.idempotency_key,
            AgentMessageProcessingOperation::LeaseExtended,
            Some(command.lease_seconds),
            None,
            command.trace_id,
            command.now,
        )
        .await
    }

    pub async fn acknowledge(
        &self,
        command: AcknowledgeAgentMessageCommand,
    ) -> Result<AgentMessageProcessingResult, AgentProcessingError> {
        self.transition(
            command.context,
            command.subscription_id,
            command.message_id,
            command.lease_id,
            command.idempotency_key,
            AgentMessageProcessingOperation::Acknowledged,
            None,
            None,
            command.trace_id,
            command.now,
        )
        .await
    }

    pub async fn nack(
        &self,
        command: NackAgentMessageCommand,
    ) -> Result<AgentMessageProcessingResult, AgentProcessingError> {
        validate_reason_code(&command.reason_code)?;
        self.transition(
            command.context,
            command.subscription_id,
            command.message_id,
            command.lease_id,
            command.idempotency_key,
            AgentMessageProcessingOperation::Nacked,
            None,
            Some(command.reason_code),
            command.trace_id,
            command.now,
        )
        .await
    }

    pub async fn list_dead_letters(
        &self,
        command: ListAgentProcessingDeadLettersCommand,
    ) -> Result<AgentProcessingDeadLetterPage, AgentProcessingError> {
        ensure_agent_permissions(&command.context)?;
        let limit = bounded_dead_letter_limit(command.limit)?;
        let subscription = self
            .owned_subscription(&command.context, command.subscription_id, command.now)
            .await?;
        let batch = self
            .store
            .list_agent_processing_dead_letters(&AgentProcessingDeadLetterQuery {
                organization_id: command.context.organization_id,
                principal_id: command.context.principal_id,
                subscription_id: command.subscription_id,
                before_message_id: command.before_message_id,
                observed_at: command.now,
                limit: i64::from(limit) + 1,
            })
            .await
            .map_err(map_store_error)?;
        if !batch.cursor_valid {
            return Err(AgentProcessingError::InvalidRequest);
        }
        if batch.items.iter().any(|item| {
            item.organization_id != command.context.organization_id
                || item.principal_id != command.context.principal_id
                || item.subscription_id != subscription.subscription_id
                || item.attempt_count == 0
                || validate_reason_code(&item.reason_code).is_err()
        }) {
            return Err(AgentProcessingError::Unavailable);
        }
        let mut items = batch.items;
        let has_more = items.len() > usize::from(limit);
        items.truncate(usize::from(limit));
        let next_before_message_id = has_more
            .then(|| items.last().map(|item| item.message_id))
            .flatten();

        let mut transaction = self
            .store
            .begin_authorized(&command.context)
            .await
            .map_err(map_store_error)?;
        append_processing_evidence(
            &mut *transaction,
            &command.context,
            "agent.message_dead_letters.queried",
            "agent.message_dead_letters.queried.v1",
            "agent.message_dead_letter_query",
            command.subscription_id.to_string(),
            json!({
                "organization_id": command.context.organization_id,
                "subscription_id": command.subscription_id,
                "item_count": items.len(),
                "first_message_id": items.first().map(|item| item.message_id),
                "last_message_id": items.last().map(|item| item.message_id),
                "has_more": has_more,
            }),
            command.trace_id,
            command.now,
        )
        .await?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(AgentProcessingDeadLetterPage {
            items,
            next_before_message_id,
            has_more,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn transition(
        &self,
        context: AuthorizationContext,
        subscription_id: SubscriptionId,
        message_id: Uuid,
        lease_id: LeaseId,
        idempotency_key: String,
        operation: AgentMessageProcessingOperation,
        lease_seconds: Option<u32>,
        reason_code: Option<String>,
        trace_id: String,
        now: DateTime<Utc>,
    ) -> Result<AgentMessageProcessingResult, AgentProcessingError> {
        ensure_agent_permissions(&context)?;
        validate_idempotency_key(&idempotency_key)?;
        let subscription = self
            .owned_subscription(&context, subscription_id, now)
            .await?;
        let idempotency_key_sha256 = Sha256::digest(idempotency_key.as_bytes()).into();
        let request_sha256 = processing_request_digest(
            &context,
            subscription_id,
            Some(message_id),
            operation,
            Some(lease_id),
            lease_seconds,
            reason_code.as_deref(),
        );
        let mut transaction = self
            .store
            .begin_authorized(&context)
            .await
            .map_err(map_store_error)?;
        let result = transaction
            .transition_agent_message(&AgentMessageTransitionWrite {
                organization_id: context.organization_id,
                principal_id: context.principal_id,
                subscription_id,
                message_id,
                lease_id,
                operation,
                idempotency_key_sha256,
                request_sha256,
                lease_expires_at: lease_seconds
                    .map(|seconds| now + Duration::seconds(i64::from(seconds))),
                reason_code,
                max_attempts: MAX_PROCESSING_ATTEMPTS,
                now,
            })
            .await
            .map_err(map_store_error)?
            .ok_or(AgentProcessingError::Conflict)?;
        validate_processing_result(&context, &subscription, &result)?;
        if result.replayed {
            return Ok(result);
        }

        let (audit_action, event_type) = match (operation, result.state) {
            (
                AgentMessageProcessingOperation::LeaseExtended,
                AgentMessageProcessingState::Leased,
            ) => (
                "agent.message.lease_extended",
                "agent.message.lease_extended.v1",
            ),
            (
                AgentMessageProcessingOperation::Acknowledged,
                AgentMessageProcessingState::Acknowledged,
            ) => (
                "agent.message.acknowledged",
                "agent.message.acknowledged.v1",
            ),
            (AgentMessageProcessingOperation::Nacked, AgentMessageProcessingState::Pending) => {
                ("agent.message.nacked", "agent.message.nacked.v1")
            }
            (
                AgentMessageProcessingOperation::Nacked,
                AgentMessageProcessingState::DeadLettered,
            ) => (
                "agent.message.dead_lettered",
                "agent.message.dead_lettered.v1",
            ),
            _ => return Err(AgentProcessingError::Unavailable),
        };
        append_result_evidence(
            &mut *transaction,
            &context,
            &result,
            audit_action,
            event_type,
            trace_id,
        )
        .await?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(result)
    }

    async fn owned_subscription(
        &self,
        context: &AuthorizationContext,
        subscription_id: SubscriptionId,
        now: DateTime<Utc>,
    ) -> Result<AgentProcessingSubscriptionRecord, AgentProcessingError> {
        let subscription = self
            .store
            .get_agent_processing_subscription(
                context.organization_id,
                context.principal_id,
                subscription_id,
            )
            .await
            .map_err(map_store_error)?
            .ok_or(AgentProcessingError::NotFound)?;
        validate_subscription(context, subscription.mailbox_id, &subscription)?;
        self.ensure_mailbox_read(context, subscription.mailbox_id, now)
            .await?;
        Ok(subscription)
    }

    async fn ensure_mailbox_read(
        &self,
        context: &AuthorizationContext,
        mailbox_id: MailboxId,
        now: DateTime<Utc>,
    ) -> Result<(), AgentProcessingError> {
        let access = self
            .store
            .resolve_mailbox_access(context.principal_id, mailbox_id, now)
            .await
            .map_err(map_store_error)?
            .ok_or(AgentProcessingError::NotFound)?;
        if access
            .authorize(
                context.organization_id,
                mailbox_id,
                MailboxCapability::Read,
                None,
            )
            .is_none()
        {
            return Err(AgentProcessingError::NotFound);
        }
        Ok(())
    }
}

fn ensure_agent_permissions(context: &AuthorizationContext) -> Result<(), AgentProcessingError> {
    if context.permits(context.organization_id, Permission::AgentOperate)
        && context.permits(context.organization_id, Permission::MailboxUse)
    {
        Ok(())
    } else {
        Err(AgentProcessingError::Forbidden)
    }
}

fn validate_subscription(
    context: &AuthorizationContext,
    mailbox_id: MailboxId,
    subscription: &AgentProcessingSubscriptionRecord,
) -> Result<(), AgentProcessingError> {
    if subscription.organization_id == context.organization_id
        && subscription.principal_id == context.principal_id
        && subscription.mailbox_id == mailbox_id
    {
        Ok(())
    } else {
        Err(AgentProcessingError::Unavailable)
    }
}

fn require_subscription_active(
    subscription: &AgentProcessingSubscriptionRecord,
) -> Result<(), AgentProcessingError> {
    if subscription.status == AgentProcessingSubscriptionStatus::Active {
        Ok(())
    } else {
        Err(AgentProcessingError::Conflict)
    }
}

fn validate_subscription_status_transition(
    expected: AgentProcessingSubscriptionStatus,
    target: AgentProcessingSubscriptionStatus,
) -> Result<(), AgentProcessingError> {
    if matches!(
        (expected, target),
        (
            AgentProcessingSubscriptionStatus::Active,
            AgentProcessingSubscriptionStatus::Paused
        ) | (
            AgentProcessingSubscriptionStatus::Paused,
            AgentProcessingSubscriptionStatus::Active
        ) | (
            AgentProcessingSubscriptionStatus::Active | AgentProcessingSubscriptionStatus::Paused,
            AgentProcessingSubscriptionStatus::Retired
        )
    ) {
        Ok(())
    } else {
        Err(AgentProcessingError::InvalidRequest)
    }
}

fn validate_subscription_change_result(
    context: &AuthorizationContext,
    result: &AgentProcessingSubscriptionChangeResult,
) -> Result<(), AgentProcessingError> {
    if result.subscription.organization_id == context.organization_id
        && result.subscription.principal_id == context.principal_id
        && result.subscription.status != result.previous_status
        && result.subscription.updated_at == result.occurred_at
    {
        Ok(())
    } else {
        Err(AgentProcessingError::Unavailable)
    }
}

fn validate_processing_result(
    context: &AuthorizationContext,
    subscription: &AgentProcessingSubscriptionRecord,
    result: &AgentMessageProcessingResult,
) -> Result<(), AgentProcessingError> {
    if result.organization_id == context.organization_id
        && result.principal_id == context.principal_id
        && result.subscription_id == subscription.subscription_id
        && result.attempt_count > 0
        && ((result.state == AgentMessageProcessingState::Leased)
            == result.lease_expires_at.is_some())
    {
        Ok(())
    } else {
        Err(AgentProcessingError::Unavailable)
    }
}

fn validate_idempotency_key(value: &str) -> Result<(), AgentProcessingError> {
    if (22..=200).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Ok(())
    } else {
        Err(AgentProcessingError::InvalidRequest)
    }
}

fn validate_lease_seconds(value: u32) -> Result<(), AgentProcessingError> {
    if (MIN_LEASE_SECONDS..=MAX_LEASE_SECONDS).contains(&value) {
        Ok(())
    } else {
        Err(AgentProcessingError::InvalidRequest)
    }
}

fn bounded_dead_letter_limit(value: u16) -> Result<u16, AgentProcessingError> {
    if (1..=100).contains(&value) {
        Ok(value)
    } else {
        Err(AgentProcessingError::InvalidRequest)
    }
}

fn validate_reason_code(value: &str) -> Result<(), AgentProcessingError> {
    if (1..=100).contains(&value.len())
        && value.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase()
            } else {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'.' | b'-')
            }
        })
    {
        Ok(())
    } else {
        Err(AgentProcessingError::InvalidRequest)
    }
}

#[allow(clippy::too_many_arguments)]
fn processing_request_digest(
    context: &AuthorizationContext,
    subscription_id: SubscriptionId,
    message_id: Option<Uuid>,
    operation: AgentMessageProcessingOperation,
    lease_id: Option<LeaseId>,
    lease_seconds: Option<u32>,
    reason_code: Option<&str>,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.agent.processing.v1");
    digest.update(context.organization_id.as_uuid().as_bytes());
    digest.update(context.principal_id.as_uuid().as_bytes());
    digest.update(subscription_id.as_uuid().as_bytes());
    digest.update(operation.as_str().as_bytes());
    update_optional_uuid(&mut digest, message_id);
    update_optional_uuid(&mut digest, lease_id.map(LeaseId::as_uuid));
    if let Some(seconds) = lease_seconds {
        digest.update([1]);
        digest.update(seconds.to_be_bytes());
    } else {
        digest.update([0]);
    }
    if let Some(reason) = reason_code {
        digest.update([1]);
        digest.update(reason.as_bytes());
    } else {
        digest.update([0]);
    }
    digest.finalize().into()
}

fn subscription_change_request_digest(
    context: &AuthorizationContext,
    subscription_id: SubscriptionId,
    expected_status: AgentProcessingSubscriptionStatus,
    target_status: AgentProcessingSubscriptionStatus,
) -> [u8; 32] {
    Sha256::new()
        .chain_update(b"copilotmail.agent.processing_subscription.change.v1")
        .chain_update(context.organization_id.as_uuid().as_bytes())
        .chain_update(context.principal_id.as_uuid().as_bytes())
        .chain_update(subscription_id.as_uuid().as_bytes())
        .chain_update(expected_status.as_str().as_bytes())
        .chain_update(target_status.as_str().as_bytes())
        .finalize()
        .into()
}

fn update_optional_uuid(digest: &mut Sha256, value: Option<Uuid>) {
    if let Some(value) = value {
        digest.update([1]);
        digest.update(value.as_bytes());
    } else {
        digest.update([0]);
    }
}

fn deterministic_lease_id(request_sha256: [u8; 32], idempotency_key_sha256: [u8; 32]) -> LeaseId {
    let digest = Sha256::new()
        .chain_update(b"copilotmail.agent.processing.lease.v1")
        .chain_update(request_sha256)
        .chain_update(idempotency_key_sha256)
        .finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    LeaseId::from_uuid(Uuid::from_bytes(bytes))
}

#[allow(clippy::too_many_arguments)]
async fn append_processing_evidence(
    transaction: &mut dyn crate::ControlPlaneTransaction,
    context: &AuthorizationContext,
    audit_action: &'static str,
    event_type: &'static str,
    resource_kind: &'static str,
    resource_id: String,
    payload: serde_json::Value,
    trace_id: String,
    now: DateTime<Utc>,
) -> Result<(), AgentProcessingError> {
    let audit = AuditEvent::new(
        context.organization_id,
        AuditActor {
            principal_id: context.principal_id,
            on_behalf_of: None,
            credential_id: None,
        },
        audit_action,
        AuditResource {
            kind: resource_kind.into(),
            id: resource_id.clone(),
        },
        AuditOutcome::Success,
        trace_id,
        payload.clone(),
        now,
    )?
    .correlate_human_session(context.authentication.as_ref())?;
    let event = PendingEvent::new(event_type, resource_kind, resource_id, payload, now);
    transaction
        .append_audit_event(&audit)
        .await
        .map_err(map_store_error)?;
    transaction
        .enqueue_event(&event)
        .await
        .map_err(map_store_error)?;
    Ok(())
}

async fn append_result_evidence(
    transaction: &mut dyn crate::ControlPlaneTransaction,
    context: &AuthorizationContext,
    result: &AgentMessageProcessingResult,
    audit_action: &'static str,
    event_type: &'static str,
    trace_id: String,
) -> Result<(), AgentProcessingError> {
    append_processing_evidence(
        transaction,
        context,
        audit_action,
        event_type,
        "agent.message_processing",
        result.message_id.to_string(),
        json!({
            "organization_id": result.organization_id,
            "subscription_id": result.subscription_id,
            "message_id": result.message_id,
            "operation_id": result.operation_id,
            "state": result.state.as_str(),
            "attempt_count": result.attempt_count,
            "lease_expires_at": result.lease_expires_at,
            "reason_code": result.reason_code,
        }),
        trace_id,
        result.occurred_at,
    )
    .await
}

fn map_store_error(error: StoreError) -> AgentProcessingError {
    match error {
        StoreError::Conflict => AgentProcessingError::Conflict,
        StoreError::MissingReference => AgentProcessingError::NotFound,
        StoreError::AuthenticationInvalid => AgentProcessingError::Forbidden,
        StoreError::Unavailable | StoreError::AgentMailAdmission(_) => {
            AgentProcessingError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn validators_keep_leases_and_failure_reasons_bounded() {
        assert!(validate_lease_seconds(30).is_ok());
        assert!(validate_lease_seconds(300).is_ok());
        assert!(validate_lease_seconds(29).is_err());
        assert!(validate_lease_seconds(301).is_err());
        assert!(validate_reason_code("temporary.upstream_failure").is_ok());
        assert!(validate_reason_code("Secret response body").is_err());
        assert!(validate_idempotency_key("PXfM8c6jEwM6e1V0hJf3mQv_4N2s8a9L").is_ok());
        assert_eq!(bounded_dead_letter_limit(1).unwrap(), 1);
        assert_eq!(bounded_dead_letter_limit(100).unwrap(), 100);
        assert!(bounded_dead_letter_limit(0).is_err());
        assert!(bounded_dead_letter_limit(101).is_err());
        assert!(
            validate_subscription_status_transition(
                AgentProcessingSubscriptionStatus::Active,
                AgentProcessingSubscriptionStatus::Paused,
            )
            .is_ok()
        );
        assert!(
            validate_subscription_status_transition(
                AgentProcessingSubscriptionStatus::Paused,
                AgentProcessingSubscriptionStatus::Active,
            )
            .is_ok()
        );
        assert!(
            validate_subscription_status_transition(
                AgentProcessingSubscriptionStatus::Paused,
                AgentProcessingSubscriptionStatus::Retired,
            )
            .is_ok()
        );
        assert!(
            validate_subscription_status_transition(
                AgentProcessingSubscriptionStatus::Retired,
                AgentProcessingSubscriptionStatus::Active,
            )
            .is_err()
        );
        assert!(
            validate_subscription_status_transition(
                AgentProcessingSubscriptionStatus::Active,
                AgentProcessingSubscriptionStatus::Active,
            )
            .is_err()
        );
    }

    #[test]
    fn claim_lease_is_stable_for_exact_request_and_changes_with_parameters() {
        let organization_id = copilotmail_domain::OrganizationId::new();
        let principal_id = copilotmail_domain::PrincipalId::new();
        let subscription_id = SubscriptionId::new();
        let context = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: BTreeSet::from([Permission::AgentOperate, Permission::MailboxUse]),
            authentication: None,
        };
        let first = processing_request_digest(
            &context,
            subscription_id,
            None,
            AgentMessageProcessingOperation::Claimed,
            None,
            Some(60),
            None,
        );
        let replay = processing_request_digest(
            &context,
            subscription_id,
            None,
            AgentMessageProcessingOperation::Claimed,
            None,
            Some(60),
            None,
        );
        let changed = processing_request_digest(
            &context,
            subscription_id,
            None,
            AgentMessageProcessingOperation::Claimed,
            None,
            Some(120),
            None,
        );
        let key = Sha256::digest(b"stable-idempotency-key").into();
        let other_key = Sha256::digest(b"different-idempotency-key").into();
        assert_eq!(
            deterministic_lease_id(first, key),
            deterministic_lease_id(replay, key)
        );
        assert_ne!(
            deterministic_lease_id(first, key),
            deterministic_lease_id(changed, key)
        );
        assert_ne!(
            deterministic_lease_id(first, key),
            deterministic_lease_id(first, other_key)
        );
    }
}
