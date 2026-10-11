use crate::mail_ports::{
    InboundMailEngineEvent, InboundMessageClassification, MailEngineWebhookEvent,
    OutboundDeliveryEventKind, OutboundMailEngineEvent,
};
use crate::{
    ControlPlaneStore, InboundMailEventInsert, InboundMailEventRecord, MailEngineMailboxBinding,
    MailEngineMailboxBindingWrite, MailEventIngestionStore, OutboundMailEventInsert,
    OutboundMailEventKind, OutboundMailEventRecord, PendingEvent, StoreError,
};
use chrono::{DateTime, Duration, Utc};
use copilotmail_domain::{
    AuditActor, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext, DomainError,
    MailDataPlaneId, MailboxId, OrganizationId, Permission,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_SOURCE_FUTURE_SKEW: Duration = Duration::minutes(5);
const MAX_SOURCE_AGE: Duration = Duration::days(7);

/// Selects the byte-level identity scheme for evidence that does not need a
/// legacy provider token (for example delivery snapshots and recipient refs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailDataPlaneFingerprintScheme {
    /// Preserves the fingerprint bytes emitted before typed data-plane IDs.
    LegacyV1,
    /// Domain-separates evidence by kind and typed data-plane ID.
    DataPlaneV2,
}

/// A validated legacy provider token used only to reproduce historical event
/// fingerprints. The private field prevents unvalidated tokens from entering
/// the compatibility branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMailEventSource(String);

impl LegacyMailEventSource {
    pub fn try_new(value: impl Into<String>) -> Result<Self, MailEventIngestionError> {
        let value = value.into();
        if !valid_legacy_source(&value) {
            return Err(MailEventIngestionError::InvalidBinding);
        }
        Ok(Self(value))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Selects the byte-level identity scheme for inbound and outbound source
/// events. Legacy source values are composition-root configuration, never
/// request input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailEventFingerprintScheme {
    LegacySource(LegacyMailEventSource),
    DataPlaneV2,
}

impl MailEventFingerprintScheme {
    pub fn try_legacy_source(source: impl Into<String>) -> Result<Self, MailEventIngestionError> {
        Ok(Self::LegacySource(LegacyMailEventSource::try_new(source)?))
    }

    #[must_use]
    pub const fn data_plane_v2() -> Self {
        Self::DataPlaneV2
    }
}

#[derive(Debug, Clone)]
pub struct BindMailEngineMailboxCommand {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub account_ref: String,
    pub expected_version: i64,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindMailEngineMailboxResult {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub data_plane_id: MailDataPlaneId,
    pub account_ref: String,
    pub version: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct IngestInboundMailEventsCommand {
    pub events: Vec<InboundMailEngineEvent>,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct IngestMailEngineEventsCommand {
    pub events: Vec<MailEngineWebhookEvent>,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestInboundMailEventsResult {
    pub accepted: usize,
    pub inserted: usize,
    pub duplicates: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum MailEventIngestionError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("mail-engine mailbox binding is forbidden")]
    Forbidden,
    #[error("mail-engine mailbox binding input is invalid")]
    InvalidBinding,
    #[error("mail-engine event timestamp is outside the accepted window")]
    InvalidTimestamp,
    #[error("mail-engine account is not mapped to a tenant mailbox")]
    UnmappedMailbox,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct MailEngineMailboxBindingService<S> {
    store: S,
    data_plane_id: MailDataPlaneId,
    legacy_source: LegacyMailEventSource,
}

impl<S: ControlPlaneStore> MailEngineMailboxBindingService<S> {
    /// Builds the server-controlled compatibility boundary for mailbox
    /// binding events. The source token is never accepted from an API caller.
    pub fn try_new(
        store: S,
        data_plane_id: MailDataPlaneId,
        legacy_source: impl Into<String>,
    ) -> Result<Self, MailEventIngestionError> {
        let legacy_source = LegacyMailEventSource::try_new(legacy_source)?;
        Ok(Self {
            store,
            data_plane_id,
            legacy_source,
        })
    }

    pub async fn bind_authorized(
        &self,
        context: &AuthorizationContext,
        command: BindMailEngineMailboxCommand,
    ) -> Result<BindMailEngineMailboxResult, MailEventIngestionError> {
        if !context.permits(command.organization_id, Permission::MailAdmin)
            || command.expected_version < 0
            || !valid_account_ref(&command.account_ref)
        {
            return if !context.permits(command.organization_id, Permission::MailAdmin) {
                Err(MailEventIngestionError::Forbidden)
            } else {
                Err(MailEventIngestionError::InvalidBinding)
            };
        }
        let mut transaction = self.store.begin_authorized(context).await?;
        if !transaction
            .authorize_human_permission(
                command.organization_id,
                context.principal_id,
                Permission::MailAdmin,
            )
            .await?
        {
            return Err(MailEventIngestionError::Forbidden);
        }
        if !transaction
            .tenant_mailbox_exists(command.organization_id, command.mailbox_id)
            .await?
        {
            return Err(StoreError::MissingReference.into());
        }
        let binding = transaction
            .upsert_mail_engine_mailbox_binding(&MailEngineMailboxBindingWrite {
                organization_id: command.organization_id,
                mailbox_id: command.mailbox_id,
                data_plane_id: self.data_plane_id,
                account_ref: command.account_ref.clone(),
                expected_version: command.expected_version,
                updated_by: context.principal_id,
                updated_at: command.now,
            })
            .await?
            .filter(|binding| {
                binding.organization_id == command.organization_id
                    && binding.mailbox_id == command.mailbox_id
                    && binding.data_plane_id == self.data_plane_id
                    && binding.account_ref == command.account_ref
            })
            .ok_or(StoreError::Conflict)?;
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "mail_engine.mailbox_binding.updated",
            AuditResource {
                kind: "mail_engine.mailbox_binding".into(),
                id: command.mailbox_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "mailbox_id": command.mailbox_id,
                "data_plane_id": binding.data_plane_id,
                "account_ref": binding.account_ref,
                "version": binding.version,
            }),
            command.now,
        )?
        .correlate_human_session(context.authentication.as_ref())?;
        // Keep the published v1 schema stable for rolling consumers. The v2
        // event introduces the provider-neutral typed identity.
        let compatibility_event = PendingEvent::for_mail_data_plane(
            self.data_plane_id,
            "mail_engine.mailbox_binding.updated.v1",
            "mail_engine.mailbox_binding",
            command.mailbox_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mailbox_id": command.mailbox_id,
                "source": self.legacy_source.as_str(),
                "version": binding.version,
            }),
            command.now,
        );
        let event = PendingEvent::for_mail_data_plane(
            self.data_plane_id,
            "mail_engine.mailbox_binding.updated.v2",
            "mail_engine.mailbox_binding",
            command.mailbox_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "mailbox_id": command.mailbox_id,
                "data_plane_id": binding.data_plane_id,
                "version": binding.version,
            }),
            command.now,
        );
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&compatibility_event).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;
        Ok(binding.into())
    }
}

impl From<MailEngineMailboxBinding> for BindMailEngineMailboxResult {
    fn from(value: MailEngineMailboxBinding) -> Self {
        Self {
            organization_id: value.organization_id,
            mailbox_id: value.mailbox_id,
            data_plane_id: value.data_plane_id,
            account_ref: value.account_ref,
            version: value.version,
            updated_at: value.updated_at,
        }
    }
}

pub struct InboundMailEventIngestionService<S> {
    store: S,
    data_plane_id: MailDataPlaneId,
    fingerprint_scheme: MailEventFingerprintScheme,
}

impl<S: MailEventIngestionStore> InboundMailEventIngestionService<S> {
    #[must_use]
    pub const fn new(
        store: S,
        data_plane_id: MailDataPlaneId,
        fingerprint_scheme: MailEventFingerprintScheme,
    ) -> Self {
        Self {
            store,
            data_plane_id,
            fingerprint_scheme,
        }
    }

    pub async fn ingest(
        &self,
        command: IngestInboundMailEventsCommand,
    ) -> Result<IngestInboundMailEventsResult, MailEventIngestionError> {
        if command.events.is_empty() {
            return Err(MailEventIngestionError::InvalidBinding);
        }
        let mut transaction = self.store.begin_mail_event_ingestion().await?;
        let mut inserted = 0;
        let mut duplicates = 0;
        for source_event in &command.events {
            validate_source_timestamp(source_event.source_occurred_at, command.received_at)?;
            let binding = transaction
                .resolve_mail_engine_mailbox_binding(self.data_plane_id, &source_event.account_ref)
                .await?
                .filter(|binding| {
                    binding.data_plane_id == self.data_plane_id
                        && binding.account_ref == source_event.account_ref
                })
                .ok_or(MailEventIngestionError::UnmappedMailbox)?;
            let event_id = Uuid::now_v7();
            let message_id = Uuid::now_v7();
            let classification = classification(source_event.classification);
            let record = InboundMailEventRecord {
                event_id,
                message_id,
                organization_id: binding.organization_id,
                mailbox_id: binding.mailbox_id,
                data_plane_id: self.data_plane_id,
                source_event_id: source_event.source_event_id.clone(),
                source_event_sha256: source_event_fingerprint(
                    self.data_plane_id,
                    &self.fingerprint_scheme,
                    source_event,
                ),
                account_ref: source_event.account_ref.clone(),
                document_ref: source_event.document_ref.clone(),
                classification: classification.into(),
                source_occurred_at: source_event.source_occurred_at,
                received_at: command.received_at,
            };
            match transaction.insert_inbound_mail_event(&record).await? {
                InboundMailEventInsert::Inserted => {
                    transaction.fanout_agent_message_states(&record).await?;
                    let event = PendingEvent::with_id_for_mail_data_plane(
                        event_id,
                        self.data_plane_id,
                        "message.received.v1",
                        "message",
                        message_id.to_string(),
                        json!({
                            "organization_id": binding.organization_id,
                            "mailbox_id": binding.mailbox_id,
                            "message_id": message_id,
                            "classification": classification,
                        }),
                        source_event.source_occurred_at,
                    );
                    transaction.enqueue_event(&event).await?;
                    inserted += 1;
                }
                InboundMailEventInsert::Duplicate => duplicates += 1,
            }
        }
        // audit-coverage: operational-source-evidence
        // Machine telemetry is evidenced by the immutable source receipt and
        // matching Outbox event; attributing it to a Human audit actor would
        // be false provenance.
        transaction.commit().await?;
        Ok(IngestInboundMailEventsResult {
            accepted: command.events.len(),
            inserted,
            duplicates,
        })
    }
}

pub struct MailEngineEventIngestionService<S> {
    store: S,
    data_plane_id: MailDataPlaneId,
    fingerprint_scheme: MailEventFingerprintScheme,
}

impl<S: MailEventIngestionStore> MailEngineEventIngestionService<S> {
    #[must_use]
    pub const fn new(
        store: S,
        data_plane_id: MailDataPlaneId,
        fingerprint_scheme: MailEventFingerprintScheme,
    ) -> Self {
        Self {
            store,
            data_plane_id,
            fingerprint_scheme,
        }
    }

    pub async fn ingest(
        &self,
        command: IngestMailEngineEventsCommand,
    ) -> Result<IngestInboundMailEventsResult, MailEventIngestionError> {
        if command.events.is_empty() {
            return Err(MailEventIngestionError::InvalidBinding);
        }
        let mut transaction = self.store.begin_mail_event_ingestion().await?;
        let mut inserted = 0;
        let mut duplicates = 0;
        for source_event in &command.events {
            let insertion = match source_event {
                MailEngineWebhookEvent::Inbound(source_event) => {
                    validate_source_timestamp(
                        source_event.source_occurred_at,
                        command.received_at,
                    )?;
                    let binding = transaction
                        .resolve_mail_engine_mailbox_binding(
                            self.data_plane_id,
                            &source_event.account_ref,
                        )
                        .await?
                        .filter(|binding| binding.data_plane_id == self.data_plane_id)
                        .ok_or(MailEventIngestionError::UnmappedMailbox)?;
                    let event_id = Uuid::now_v7();
                    let message_id = Uuid::now_v7();
                    let classification = classification(source_event.classification);
                    let record = InboundMailEventRecord {
                        event_id,
                        message_id,
                        organization_id: binding.organization_id,
                        mailbox_id: binding.mailbox_id,
                        data_plane_id: self.data_plane_id,
                        source_event_id: source_event.source_event_id.clone(),
                        source_event_sha256: source_event_fingerprint(
                            self.data_plane_id,
                            &self.fingerprint_scheme,
                            source_event,
                        ),
                        account_ref: source_event.account_ref.clone(),
                        document_ref: source_event.document_ref.clone(),
                        classification: classification.into(),
                        source_occurred_at: source_event.source_occurred_at,
                        received_at: command.received_at,
                    };
                    match transaction.insert_inbound_mail_event(&record).await? {
                        InboundMailEventInsert::Inserted => {
                            transaction
                                .enqueue_event(&PendingEvent::with_id_for_mail_data_plane(
                                    event_id,
                                    self.data_plane_id,
                                    "message.received.v1",
                                    "message",
                                    message_id.to_string(),
                                    json!({
                                        "organization_id": binding.organization_id,
                                        "mailbox_id": binding.mailbox_id,
                                        "message_id": message_id,
                                        "classification": classification,
                                    }),
                                    source_event.source_occurred_at,
                                ))
                                .await?;
                            true
                        }
                        InboundMailEventInsert::Duplicate => false,
                    }
                }
                MailEngineWebhookEvent::Outbound(source_event) => {
                    validate_source_timestamp(
                        source_event.source_occurred_at,
                        command.received_at,
                    )?;
                    validate_outbound_event(source_event)?;
                    let binding = transaction
                        .resolve_mail_engine_mailbox_by_primary_address(
                            self.data_plane_id,
                            &source_event.envelope_from,
                        )
                        .await?
                        .filter(|binding| binding.data_plane_id == self.data_plane_id)
                        .ok_or(MailEventIngestionError::UnmappedMailbox)?;
                    let event_id = Uuid::now_v7();
                    let record = normalize_outbound_record(
                        event_id,
                        &binding,
                        &self.fingerprint_scheme,
                        source_event,
                        command.received_at,
                    );
                    match transaction.insert_outbound_mail_event(&record).await? {
                        OutboundMailEventInsert::Inserted => {
                            transaction
                                .enqueue_event(&PendingEvent::with_id_for_mail_data_plane(
                                    event_id,
                                    self.data_plane_id,
                                    "mail_engine.delivery_trace.observed.v1",
                                    "mail_engine.delivery_trace",
                                    event_id.to_string(),
                                    json!({
                                        "organization_id": binding.organization_id,
                                        "mailbox_id": binding.mailbox_id,
                                        "trace_event_id": event_id,
                                        "kind": outbound_kind_name(record.kind),
                                        "terminal": matches!(
                                            record.kind,
                                            OutboundMailEventKind::PermanentFailure
                                                | OutboundMailEventKind::Completed
                                        ),
                                    }),
                                    source_event.source_occurred_at,
                                ))
                                .await?;
                            true
                        }
                        OutboundMailEventInsert::Duplicate => false,
                    }
                }
            };
            if insertion {
                inserted += 1;
            } else {
                duplicates += 1;
            }
        }
        // audit-coverage: operational-source-evidence
        // Authenticated data-plane evidence and its minimized Outbox event are
        // committed atomically without fabricating a Human audit actor.
        transaction.commit().await?;
        Ok(IngestInboundMailEventsResult {
            accepted: command.events.len(),
            inserted,
            duplicates,
        })
    }
}

fn normalize_outbound_record(
    event_id: Uuid,
    binding: &MailEngineMailboxBinding,
    fingerprint_scheme: &MailEventFingerprintScheme,
    event: &OutboundMailEngineEvent,
    received_at: DateTime<Utc>,
) -> OutboundMailEventRecord {
    let recipient_ref = event.recipient.as_ref().map(|recipient| {
        let mut digest = Sha256::new();
        match fingerprint_scheme {
            MailEventFingerprintScheme::LegacySource(_) => {
                digest.update(b"copilotmail.mail.delivery.trace.recipient.v1");
            }
            MailEventFingerprintScheme::DataPlaneV2 => {
                digest.update(b"copilotmail.mail.delivery.trace.recipient.v2");
                digest.update(binding.data_plane_id.as_uuid().as_bytes());
            }
        }
        digest.update(event.queue_ref.as_bytes());
        digest.update([0]);
        digest.update(recipient.as_str().as_bytes());
        digest.finalize().into()
    });
    OutboundMailEventRecord {
        event_id,
        organization_id: binding.organization_id,
        mailbox_id: binding.mailbox_id,
        data_plane_id: binding.data_plane_id,
        source_event_id: event.source_event_id.clone(),
        source_event_sha256: outbound_source_event_fingerprint(
            binding.data_plane_id,
            fingerprint_scheme,
            event,
        ),
        queue_ref: event.queue_ref.clone(),
        kind: match event.kind {
            OutboundDeliveryEventKind::Delivered => OutboundMailEventKind::Delivered,
            OutboundDeliveryEventKind::PermanentFailure => OutboundMailEventKind::PermanentFailure,
            OutboundDeliveryEventKind::Rescheduled => OutboundMailEventKind::Rescheduled,
            OutboundDeliveryEventKind::Completed => OutboundMailEventKind::Completed,
        },
        recipient_ref,
        smtp_code: event.smtp_code,
        next_retry_at: event.next_retry_at,
        source_occurred_at: event.source_occurred_at,
        received_at,
    }
}

fn valid_account_ref(account_ref: &str) -> bool {
    !account_ref.is_empty()
        && account_ref.len() <= 512
        && account_ref.trim() == account_ref
        && !account_ref.chars().any(char::is_control)
}

fn valid_legacy_source(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value.trim() == value
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn validate_source_timestamp(
    source_occurred_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
) -> Result<(), MailEventIngestionError> {
    if source_occurred_at > received_at + MAX_SOURCE_FUTURE_SKEW
        || source_occurred_at < received_at - MAX_SOURCE_AGE
    {
        return Err(MailEventIngestionError::InvalidTimestamp);
    }
    Ok(())
}

fn validate_outbound_event(event: &OutboundMailEngineEvent) -> Result<(), MailEventIngestionError> {
    let valid_shape = match event.kind {
        OutboundDeliveryEventKind::Delivered => {
            event.recipient.is_some() && event.smtp_code.is_some() && event.next_retry_at.is_none()
        }
        OutboundDeliveryEventKind::PermanentFailure => {
            event.recipient.is_some() && event.smtp_code.is_none() && event.next_retry_at.is_none()
        }
        OutboundDeliveryEventKind::Rescheduled => {
            event.recipient.is_none()
                && event.smtp_code.is_none()
                && event
                    .next_retry_at
                    .is_some_and(|next_retry_at| next_retry_at > event.source_occurred_at)
        }
        OutboundDeliveryEventKind::Completed => {
            event.recipient.is_none() && event.smtp_code.is_none() && event.next_retry_at.is_none()
        }
    };
    if !valid_shape {
        return Err(MailEventIngestionError::InvalidBinding);
    }
    Ok(())
}

const fn classification(value: InboundMessageClassification) -> &'static str {
    match value {
        InboundMessageClassification::Ham => "ham",
        InboundMessageClassification::Spam => "spam",
    }
}

fn source_event_fingerprint(
    data_plane_id: MailDataPlaneId,
    fingerprint_scheme: &MailEventFingerprintScheme,
    event: &InboundMailEngineEvent,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    match fingerprint_scheme {
        MailEventFingerprintScheme::LegacySource(source) => {
            digest.update(source.as_str().as_bytes());
            digest.update([0]);
        }
        MailEventFingerprintScheme::DataPlaneV2 => {
            digest.update(b"copilotmail.mail.event.inbound.v2");
            digest.update([0]);
            digest.update(data_plane_id.as_uuid().as_bytes());
            digest.update([0]);
        }
    }
    for part in [
        event.source_event_id.as_str(),
        event.account_ref.as_str(),
        event.document_ref.as_str(),
        classification(event.classification),
    ] {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    digest.update(event.source_occurred_at.timestamp_micros().to_be_bytes());
    digest.finalize().into()
}

const fn outbound_kind_name(kind: OutboundMailEventKind) -> &'static str {
    match kind {
        OutboundMailEventKind::Delivered => "delivered",
        OutboundMailEventKind::PermanentFailure => "permanent_failure",
        OutboundMailEventKind::Rescheduled => "rescheduled",
        OutboundMailEventKind::Completed => "completed",
    }
}

fn outbound_source_event_fingerprint(
    data_plane_id: MailDataPlaneId,
    fingerprint_scheme: &MailEventFingerprintScheme,
    event: &OutboundMailEngineEvent,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    match fingerprint_scheme {
        MailEventFingerprintScheme::LegacySource(source) => {
            digest.update(source.as_str().as_bytes());
            digest.update([0]);
        }
        MailEventFingerprintScheme::DataPlaneV2 => {
            digest.update(b"copilotmail.mail.event.outbound.v2");
            digest.update([0]);
            digest.update(data_plane_id.as_uuid().as_bytes());
            digest.update([0]);
        }
    }
    for part in [
        event.source_event_id.as_str(),
        event.queue_ref.as_str(),
        event.envelope_from.as_str(),
        event.recipient.as_ref().map_or("", |value| value.as_str()),
        match event.kind {
            OutboundDeliveryEventKind::Delivered => "delivered",
            OutboundDeliveryEventKind::PermanentFailure => "permanent_failure",
            OutboundDeliveryEventKind::Rescheduled => "rescheduled",
            OutboundDeliveryEventKind::Completed => "completed",
        },
    ] {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    digest.update(event.source_occurred_at.timestamp_micros().to_be_bytes());
    digest.update(event.smtp_code.unwrap_or_default().to_be_bytes());
    digest.update(
        event
            .next_retry_at
            .map_or(i64::MIN, |value| value.timestamp_micros())
            .to_be_bytes(),
    );
    digest.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MailEventIngestionTransaction, PendingEvent};
    use async_trait::async_trait;
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct MemoryStore {
        state: Arc<Mutex<State>>,
    }

    #[derive(Clone, Default)]
    struct State {
        bindings: BTreeMap<(MailDataPlaneId, String), MailEngineMailboxBinding>,
        address_bindings: BTreeMap<(MailDataPlaneId, String), MailEngineMailboxBinding>,
        records: Vec<InboundMailEventRecord>,
        outbound_records: Vec<OutboundMailEventRecord>,
        events: Vec<PendingEvent>,
    }

    struct MemoryTransaction {
        shared: Arc<Mutex<State>>,
        local: State,
    }

    #[async_trait]
    impl MailEventIngestionStore for MemoryStore {
        async fn begin_mail_event_ingestion(
            &self,
        ) -> Result<Box<dyn MailEventIngestionTransaction>, StoreError> {
            Ok(Box::new(MemoryTransaction {
                shared: Arc::clone(&self.state),
                local: self.state.lock().unwrap().clone(),
            }))
        }
    }

    #[async_trait]
    impl MailEventIngestionTransaction for MemoryTransaction {
        async fn resolve_mail_engine_mailbox_binding(
            &mut self,
            data_plane_id: MailDataPlaneId,
            account_ref: &str,
        ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
            Ok(self
                .local
                .bindings
                .get(&(data_plane_id, account_ref.to_owned()))
                .cloned())
        }

        async fn insert_inbound_mail_event(
            &mut self,
            record: &InboundMailEventRecord,
        ) -> Result<InboundMailEventInsert, StoreError> {
            if let Some(existing) = self.local.records.iter().find(|existing| {
                existing.data_plane_id == record.data_plane_id
                    && existing.source_event_id == record.source_event_id
            }) {
                return if existing.source_event_sha256 == record.source_event_sha256 {
                    Ok(InboundMailEventInsert::Duplicate)
                } else {
                    Err(StoreError::Conflict)
                };
            }
            if self.local.records.iter().any(|existing| {
                existing.data_plane_id == record.data_plane_id
                    && existing.account_ref == record.account_ref
                    && existing.document_ref == record.document_ref
            }) {
                return Ok(InboundMailEventInsert::Duplicate);
            }
            self.local.records.push(record.clone());
            Ok(InboundMailEventInsert::Inserted)
        }

        async fn fanout_agent_message_states(
            &mut self,
            _record: &InboundMailEventRecord,
        ) -> Result<u64, StoreError> {
            Ok(0)
        }

        async fn resolve_mail_engine_mailbox_by_primary_address(
            &mut self,
            data_plane_id: MailDataPlaneId,
            primary_address: &copilotmail_domain::MailboxAddress,
        ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
            Ok(self
                .local
                .address_bindings
                .get(&(data_plane_id, primary_address.as_str().to_owned()))
                .cloned())
        }

        async fn insert_outbound_mail_event(
            &mut self,
            record: &OutboundMailEventRecord,
        ) -> Result<OutboundMailEventInsert, StoreError> {
            if let Some(existing) = self.local.outbound_records.iter().find(|existing| {
                existing.data_plane_id == record.data_plane_id
                    && existing.source_event_id == record.source_event_id
            }) {
                return if existing.source_event_sha256 == record.source_event_sha256 {
                    Ok(OutboundMailEventInsert::Duplicate)
                } else {
                    Err(StoreError::Conflict)
                };
            }
            self.local.outbound_records.push(record.clone());
            Ok(OutboundMailEventInsert::Inserted)
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.local.events.push(event.clone());
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            *self.shared.lock().unwrap() = self.local;
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct BindingState {
        events: Vec<PendingEvent>,
        audits: Vec<String>,
    }

    #[derive(Clone, Default)]
    struct BindingStore {
        state: Arc<Mutex<BindingState>>,
    }

    struct BindingTransaction {
        shared: Arc<Mutex<BindingState>>,
        local: BindingState,
    }

    #[async_trait]
    impl ControlPlaneStore for BindingStore {
        async fn begin(&self) -> Result<Box<dyn crate::ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(BindingTransaction {
                shared: Arc::clone(&self.state),
                local: self.state.lock().unwrap().clone(),
            }))
        }
    }

    #[async_trait]
    impl crate::ControlPlaneTransaction for BindingTransaction {
        async fn find_organization_bootstrap(
            &mut self,
            _slug: &copilotmail_domain::OrganizationSlug,
        ) -> Result<Option<crate::ExistingOrganizationBootstrap>, StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_organization(
            &mut self,
            _organization: &copilotmail_domain::Organization,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_principal(
            &mut self,
            _principal: &copilotmail_domain::Principal,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role(
            &mut self,
            _role: &copilotmail_domain::Role,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn insert_role_assignment(
            &mut self,
            _assignment: &copilotmail_domain::RoleAssignment,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn record_organization_bootstrap(
            &mut self,
            _organization_id: OrganizationId,
            _root_principal_id: copilotmail_domain::PrincipalId,
            _created_at: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Err(StoreError::Unavailable)
        }

        async fn authorize_human_permission(
            &mut self,
            _organization_id: OrganizationId,
            _principal_id: copilotmail_domain::PrincipalId,
            permission: Permission,
        ) -> Result<bool, StoreError> {
            assert_eq!(permission, Permission::MailAdmin);
            Ok(true)
        }

        async fn tenant_mailbox_exists(
            &mut self,
            _organization_id: OrganizationId,
            _mailbox_id: MailboxId,
        ) -> Result<bool, StoreError> {
            Ok(true)
        }

        async fn upsert_mail_engine_mailbox_binding(
            &mut self,
            write: &MailEngineMailboxBindingWrite,
        ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
            Ok(Some(MailEngineMailboxBinding {
                organization_id: write.organization_id,
                mailbox_id: write.mailbox_id,
                data_plane_id: write.data_plane_id,
                account_ref: write.account_ref.clone(),
                version: write.expected_version + 1,
                updated_at: write.updated_at,
            }))
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            self.local.audits.push(event.action.clone());
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            self.local.events.push(event.clone());
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            *self.shared.lock().unwrap() = self.local;
            Ok(())
        }
    }

    fn store_with_binding(account_ref: &str) -> (MemoryStore, OrganizationId, MailboxId) {
        let store = MemoryStore::default();
        let organization_id = OrganizationId::new();
        let mailbox_id = MailboxId::new();
        let data_plane_id = crate::mail_ports::test_mail_data_plane_id();
        store.state.lock().unwrap().bindings.insert(
            (data_plane_id, account_ref.to_owned()),
            MailEngineMailboxBinding {
                organization_id,
                mailbox_id,
                data_plane_id,
                account_ref: account_ref.into(),
                version: 1,
                updated_at: Utc::now(),
            },
        );
        (store, organization_id, mailbox_id)
    }

    fn inbound(
        source_event_id: &str,
        account_ref: &str,
        document_ref: &str,
        classification: InboundMessageClassification,
        occurred_at: DateTime<Utc>,
    ) -> InboundMailEngineEvent {
        InboundMailEngineEvent {
            source_event_id: source_event_id.into(),
            source_occurred_at: occurred_at,
            account_ref: account_ref.into(),
            document_ref: document_ref.into(),
            classification,
        }
    }

    fn legacy_event_fingerprint_scheme() -> MailEventFingerprintScheme {
        MailEventFingerprintScheme::try_legacy_source("legacy-adapter").unwrap()
    }

    fn hex_lower(bytes: &[u8]) -> String {
        use std::fmt::Write as _;

        bytes.iter().fold(String::new(), |mut output, byte| {
            write!(&mut output, "{byte:02x}").unwrap();
            output
        })
    }

    #[test]
    fn legacy_event_fingerprints_match_historical_fixed_vectors() {
        let occurred_at = DateTime::parse_from_rfc3339("2024-01-02T03:04:05.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        let scheme = MailEventFingerprintScheme::try_legacy_source("legacy-adapter").unwrap();
        let inbound = inbound(
            "evt-42",
            "acct-7",
            "doc-9",
            InboundMessageClassification::Ham,
            occurred_at,
        );
        assert_eq!(
            hex_lower(&source_event_fingerprint(
                crate::mail_ports::test_mail_data_plane_id(),
                &scheme,
                &inbound,
            )),
            "5f834fe62a507e864552a8067ca8b8ca35644f25e86cdb4dc7a83573b6247869"
        );

        let outbound = OutboundMailEngineEvent {
            source_event_id: "delivery-42".into(),
            source_occurred_at: occurred_at,
            queue_ref: "queue-7".into(),
            envelope_from: copilotmail_domain::MailboxAddress::parse("sender@example.test")
                .unwrap(),
            recipient: Some(
                copilotmail_domain::MailboxAddress::parse("recipient@example.test").unwrap(),
            ),
            kind: OutboundDeliveryEventKind::Delivered,
            smtp_code: Some(250),
            next_retry_at: None,
        };
        assert_eq!(
            hex_lower(&outbound_source_event_fingerprint(
                crate::mail_ports::test_mail_data_plane_id(),
                &scheme,
                &outbound,
            )),
            "9477e70e9bd185cf79bc8808c16428a0746107019d6d58b141fe14074179e97c"
        );
        let binding = MailEngineMailboxBinding {
            organization_id: OrganizationId::new(),
            mailbox_id: MailboxId::new(),
            data_plane_id: crate::mail_ports::test_mail_data_plane_id(),
            account_ref: "acct-7".into(),
            version: 1,
            updated_at: occurred_at,
        };
        let record =
            normalize_outbound_record(Uuid::nil(), &binding, &scheme, &outbound, occurred_at);
        assert_eq!(
            hex_lower(record.recipient_ref.as_ref().unwrap()),
            "de24d622ef30ccef6b40b2f6d837d14f28794dc80c2a5e5f62b4e258c12bdfeb"
        );
    }

    #[test]
    fn v2_event_fingerprints_are_scoped_to_the_data_plane() {
        let occurred_at = DateTime::parse_from_rfc3339("2024-01-02T03:04:05.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        let inbound = inbound(
            "evt-42",
            "acct-7",
            "doc-9",
            InboundMessageClassification::Ham,
            occurred_at,
        );
        let first_id = crate::mail_ports::test_mail_data_plane_id();
        let second_id = crate::mail_ports::other_test_mail_data_plane_id();
        let scheme = MailEventFingerprintScheme::DataPlaneV2;
        assert_ne!(
            source_event_fingerprint(first_id, &scheme, &inbound),
            source_event_fingerprint(second_id, &scheme, &inbound)
        );

        let outbound = OutboundMailEngineEvent {
            source_event_id: "delivery-42".into(),
            source_occurred_at: occurred_at,
            queue_ref: "queue-7".into(),
            envelope_from: copilotmail_domain::MailboxAddress::parse("sender@example.test")
                .unwrap(),
            recipient: Some(
                copilotmail_domain::MailboxAddress::parse("recipient@example.test").unwrap(),
            ),
            kind: OutboundDeliveryEventKind::Delivered,
            smtp_code: Some(250),
            next_retry_at: None,
        };
        assert_ne!(
            outbound_source_event_fingerprint(first_id, &scheme, &outbound),
            outbound_source_event_fingerprint(second_id, &scheme, &outbound)
        );
        let binding = |data_plane_id| MailEngineMailboxBinding {
            organization_id: OrganizationId::new(),
            mailbox_id: MailboxId::new(),
            data_plane_id,
            account_ref: "acct-7".into(),
            version: 1,
            updated_at: occurred_at,
        };
        let first = normalize_outbound_record(
            Uuid::nil(),
            &binding(first_id),
            &scheme,
            &outbound,
            occurred_at,
        );
        let second = normalize_outbound_record(
            Uuid::nil(),
            &binding(second_id),
            &scheme,
            &outbound,
            occurred_at,
        );
        assert_ne!(first.recipient_ref, second.recipient_ref);
    }

    #[test]
    fn binding_compatibility_source_is_server_controlled_and_canonical() {
        let store = BindingStore::default();
        let data_plane_id = crate::mail_ports::test_mail_data_plane_id();
        assert!(
            MailEngineMailboxBindingService::try_new(
                store.clone(),
                data_plane_id,
                "legacy-adapter"
            )
            .is_ok()
        );
        for invalid in [
            "",
            " Legacy-adapter",
            "Legacy-adapter",
            "legacy adapter",
            "legacy/adapter",
            "legacy\nadapter",
        ] {
            assert!(matches!(
                MailEngineMailboxBindingService::try_new(store.clone(), data_plane_id, invalid),
                Err(MailEventIngestionError::InvalidBinding)
            ));
        }
    }

    #[tokio::test]
    async fn binding_dual_publishes_stable_v1_and_typed_v2_on_the_same_route() {
        let store = BindingStore::default();
        let organization_id = OrganizationId::new();
        let mailbox_id = MailboxId::new();
        let data_plane_id = crate::mail_ports::test_mail_data_plane_id();
        let principal_id = copilotmail_domain::PrincipalId::new();
        let context = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: BTreeSet::from([Permission::MailAdmin]),
            authentication: None,
        };

        let result = MailEngineMailboxBindingService::try_new(
            store.clone(),
            data_plane_id,
            "legacy-adapter",
        )
        .unwrap()
        .bind_authorized(
            &context,
            BindMailEngineMailboxCommand {
                organization_id,
                mailbox_id,
                account_ref: "account-42".into(),
                expected_version: 0,
                trace_id: "binding-dual-publish".into(),
                now: Utc::now(),
            },
        )
        .await
        .unwrap();

        assert_eq!(result.data_plane_id, data_plane_id);
        let state = store.state.lock().unwrap();
        assert_eq!(state.audits, ["mail_engine.mailbox_binding.updated"]);
        assert_eq!(state.events.len(), 2);
        let v1 = &state.events[0];
        assert_eq!(v1.event_type, "mail_engine.mailbox_binding.updated.v1");
        assert_eq!(v1.data_plane_id, Some(data_plane_id));
        assert_eq!(v1.payload["source"], "legacy-adapter");
        assert!(v1.payload.get("data_plane_id").is_none());
        let v2 = &state.events[1];
        assert_eq!(v2.event_type, "mail_engine.mailbox_binding.updated.v2");
        assert_eq!(v2.data_plane_id, Some(data_plane_id));
        assert_eq!(v2.payload["data_plane_id"], data_plane_id.to_string());
        assert!(v2.payload.get("source").is_none());
    }

    #[tokio::test]
    async fn emits_one_minimal_external_event_and_deduplicates_exact_replay() {
        let (store, organization_id, mailbox_id) = store_with_binding("42");
        let now = Utc::now();
        let event = inbound(
            "source-1",
            "42",
            "991",
            InboundMessageClassification::Ham,
            now,
        );
        let first = InboundMailEventIngestionService::new(
            store.clone(),
            crate::mail_ports::test_mail_data_plane_id(),
            legacy_event_fingerprint_scheme(),
        )
        .ingest(IngestInboundMailEventsCommand {
            events: vec![event.clone()],
            received_at: now,
        })
        .await
        .unwrap();
        assert_eq!(first.inserted, 1);
        let replay = InboundMailEventIngestionService::new(
            store.clone(),
            crate::mail_ports::test_mail_data_plane_id(),
            legacy_event_fingerprint_scheme(),
        )
        .ingest(IngestInboundMailEventsCommand {
            events: vec![event],
            received_at: now,
        })
        .await
        .unwrap();
        assert_eq!(replay.duplicates, 1);
        let state = store.state.lock().unwrap();
        assert_eq!(state.records.len(), 1);
        assert_eq!(state.events.len(), 1);
        assert_eq!(state.events[0].event_type, "message.received.v1");
        assert_eq!(
            state.events[0].data_plane_id,
            Some(crate::mail_ports::test_mail_data_plane_id())
        );
        assert_eq!(
            state.events[0].payload["organization_id"],
            organization_id.to_string()
        );
        assert_eq!(
            state.events[0].payload["mailbox_id"],
            mailbox_id.to_string()
        );
        assert_eq!(state.events[0].payload["classification"], "ham");
        assert!(state.events[0].payload.get("account_ref").is_none());
        assert!(state.events[0].payload.get("document_ref").is_none());
    }

    #[tokio::test]
    async fn unknown_binding_rolls_back_the_entire_batch() {
        let (store, _, _) = store_with_binding("42");
        let now = Utc::now();
        let result = InboundMailEventIngestionService::new(
            store.clone(),
            crate::mail_ports::test_mail_data_plane_id(),
            legacy_event_fingerprint_scheme(),
        )
        .ingest(IngestInboundMailEventsCommand {
            events: vec![
                inbound(
                    "source-1",
                    "42",
                    "991",
                    InboundMessageClassification::Ham,
                    now,
                ),
                inbound(
                    "source-2",
                    "404",
                    "992",
                    InboundMessageClassification::Ham,
                    now,
                ),
            ],
            received_at: now,
        })
        .await;
        assert!(matches!(
            result,
            Err(MailEventIngestionError::UnmappedMailbox)
        ));
        let state = store.state.lock().unwrap();
        assert!(state.records.is_empty());
        assert!(state.events.is_empty());
    }

    #[tokio::test]
    async fn rejects_same_source_id_with_changed_normalized_content() {
        let (store, _, _) = store_with_binding("42");
        let now = Utc::now();
        let service = InboundMailEventIngestionService::new(
            store.clone(),
            crate::mail_ports::test_mail_data_plane_id(),
            legacy_event_fingerprint_scheme(),
        );
        service
            .ingest(IngestInboundMailEventsCommand {
                events: vec![inbound(
                    "source-1",
                    "42",
                    "991",
                    InboundMessageClassification::Ham,
                    now,
                )],
                received_at: now,
            })
            .await
            .unwrap();
        let result = service
            .ingest(IngestInboundMailEventsCommand {
                events: vec![inbound(
                    "source-1",
                    "42",
                    "991",
                    InboundMessageClassification::Spam,
                    now,
                )],
                received_at: now,
            })
            .await;
        assert!(matches!(
            result,
            Err(MailEventIngestionError::Store(StoreError::Conflict))
        ));
        assert_eq!(store.state.lock().unwrap().events.len(), 1);
    }

    #[tokio::test]
    async fn rejects_stale_or_future_source_timestamps_before_commit() {
        let (store, _, _) = store_with_binding("42");
        let now = Utc::now();
        for occurred_at in [now - Duration::days(8), now + Duration::minutes(6)] {
            let result = InboundMailEventIngestionService::new(
                store.clone(),
                crate::mail_ports::test_mail_data_plane_id(),
                legacy_event_fingerprint_scheme(),
            )
            .ingest(IngestInboundMailEventsCommand {
                events: vec![inbound(
                    "source-1",
                    "42",
                    "991",
                    InboundMessageClassification::Ham,
                    occurred_at,
                )],
                received_at: now,
            })
            .await;
            assert!(matches!(
                result,
                Err(MailEventIngestionError::InvalidTimestamp)
            ));
        }
        assert!(store.state.lock().unwrap().events.is_empty());
    }

    #[tokio::test]
    async fn outbound_trace_is_tenant_mapped_minimized_and_replay_safe() {
        let (store, organization_id, mailbox_id) = store_with_binding("42");
        let from = copilotmail_domain::MailboxAddress::parse("sender@example.test").unwrap();
        let binding = store
            .state
            .lock()
            .unwrap()
            .bindings
            .get(&(crate::mail_ports::test_mail_data_plane_id(), "42".into()))
            .unwrap()
            .clone();
        store.state.lock().unwrap().address_bindings.insert(
            (
                crate::mail_ports::test_mail_data_plane_id(),
                from.as_str().to_owned(),
            ),
            binding,
        );
        let now = Utc::now();
        let source_event = OutboundMailEngineEvent {
            source_event_id: "delivery-1".into(),
            source_occurred_at: now,
            queue_ref: "771".into(),
            envelope_from: from,
            recipient: Some(
                copilotmail_domain::MailboxAddress::parse("recipient@external.test").unwrap(),
            ),
            kind: OutboundDeliveryEventKind::Delivered,
            smtp_code: Some(250),
            next_retry_at: None,
        };
        let service = MailEngineEventIngestionService::new(
            store.clone(),
            crate::mail_ports::test_mail_data_plane_id(),
            legacy_event_fingerprint_scheme(),
        );
        let first = service
            .ingest(IngestMailEngineEventsCommand {
                events: vec![MailEngineWebhookEvent::Outbound(source_event.clone())],
                received_at: now,
            })
            .await
            .unwrap();
        assert_eq!(first.inserted, 1);
        let replay = service
            .ingest(IngestMailEngineEventsCommand {
                events: vec![MailEngineWebhookEvent::Outbound(source_event.clone())],
                received_at: now,
            })
            .await
            .unwrap();
        assert_eq!(replay.duplicates, 1);
        {
            let state = store.state.lock().unwrap();
            assert_eq!(state.outbound_records.len(), 1);
            assert_eq!(state.outbound_records[0].organization_id, organization_id);
            assert_eq!(state.outbound_records[0].mailbox_id, mailbox_id);
            assert!(state.outbound_records[0].recipient_ref.is_some());
            assert_eq!(state.events.len(), 1);
            assert_eq!(
                state.events[0].event_type,
                "mail_engine.delivery_trace.observed.v1"
            );
            assert_eq!(
                state.events[0].data_plane_id,
                Some(crate::mail_ports::test_mail_data_plane_id())
            );
            // Compare the complete, typed envelope rather than searching its
            // serialized bytes: a randomly generated UUID may legitimately
            // happen to contain a digit sequence from an omitted queue ref.
            // Exact equality proves that no source address, queue reference,
            // recipient, SMTP code, or provider event identifier crossed the
            // minimized Outbox boundary.
            assert_eq!(
                state.events[0].payload,
                json!({
                    "organization_id": organization_id,
                    "mailbox_id": mailbox_id,
                    "trace_event_id": state.outbound_records[0].event_id,
                    "kind": "delivered",
                    "terminal": false,
                })
            );
        }

        let mut tampered = source_event;
        tampered.smtp_code = Some(251);
        let conflict = service
            .ingest(IngestMailEngineEventsCommand {
                events: vec![MailEngineWebhookEvent::Outbound(tampered)],
                received_at: now,
            })
            .await;
        assert!(matches!(
            conflict,
            Err(MailEventIngestionError::Store(StoreError::Conflict))
        ));
    }

    #[tokio::test]
    async fn outbound_trace_rejects_invalid_kind_shape_before_storage() {
        let (store, _, _) = store_with_binding("42");
        let now = Utc::now();
        let source_event = OutboundMailEngineEvent {
            source_event_id: "reschedule-1".into(),
            source_occurred_at: now,
            queue_ref: "771".into(),
            envelope_from: copilotmail_domain::MailboxAddress::parse("sender@example.test")
                .unwrap(),
            recipient: None,
            kind: OutboundDeliveryEventKind::Rescheduled,
            smtp_code: None,
            next_retry_at: Some(now),
        };
        let result = MailEngineEventIngestionService::new(
            store.clone(),
            crate::mail_ports::test_mail_data_plane_id(),
            legacy_event_fingerprint_scheme(),
        )
        .ingest(IngestMailEngineEventsCommand {
            events: vec![MailEngineWebhookEvent::Outbound(source_event)],
            received_at: now,
        })
        .await;
        assert!(matches!(
            result,
            Err(MailEventIngestionError::InvalidBinding)
        ));
        let state = store.state.lock().unwrap();
        assert!(state.outbound_records.is_empty());
        assert!(state.events.is_empty());
    }
}
