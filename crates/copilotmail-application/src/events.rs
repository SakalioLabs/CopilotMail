use chrono::{DateTime, Utc};
use copilotmail_domain::MailDataPlaneId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Event written transactionally with an application state change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingEvent {
    /// Stable event identifier used for deduplication.
    pub id: Uuid,
    /// Versioned dotted event type.
    pub event_type: String,
    /// Aggregate type that produced the event.
    pub aggregate_type: String,
    /// Aggregate identifier.
    pub aggregate_id: String,
    /// Event schema version.
    pub schema_version: u32,
    /// Sanitized event body.
    pub payload: Value,
    /// Optional exact mail data-plane route. General control-plane events are
    /// intentionally unrouted; mail work and provider-originated evidence must
    /// always carry the data plane that owns their remote references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_plane_id: Option<MailDataPlaneId>,
    /// Business timestamp.
    pub occurred_at: DateTime<Utc>,
}

impl PendingEvent {
    /// Creates a version-one application event with a UUIDv7 identifier.
    #[must_use]
    pub fn new(
        event_type: impl Into<String>,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        payload: Value,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            event_type: event_type.into(),
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            schema_version: 1,
            payload,
            data_plane_id: None,
            occurred_at,
        }
    }

    /// Creates a version-one event routed to one exact mail data plane.
    #[must_use]
    pub fn for_mail_data_plane(
        data_plane_id: MailDataPlaneId,
        event_type: impl Into<String>,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        payload: Value,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        let mut event = Self::new(
            event_type,
            aggregate_type,
            aggregate_id,
            payload,
            occurred_at,
        );
        event.data_plane_id = Some(data_plane_id);
        event
    }

    /// Creates an event with an already allocated stable identifier. This is
    /// used when an authenticated external event and its Outbox envelope must
    /// share one transactionally deduplicated identity.
    #[must_use]
    pub fn with_id(
        id: Uuid,
        event_type: impl Into<String>,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        payload: Value,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            event_type: event_type.into(),
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            schema_version: 1,
            payload,
            data_plane_id: None,
            occurred_at,
        }
    }

    /// Creates a stably identified event routed to one exact mail data plane.
    #[must_use]
    pub fn with_id_for_mail_data_plane(
        id: Uuid,
        data_plane_id: MailDataPlaneId,
        event_type: impl Into<String>,
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        payload: Value,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        let mut event = Self::with_id(
            id,
            event_type,
            aggregate_type,
            aggregate_id,
            payload,
            occurred_at,
        );
        event.data_plane_id = Some(data_plane_id);
        event
    }
}
