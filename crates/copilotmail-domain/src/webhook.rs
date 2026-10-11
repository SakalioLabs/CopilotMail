use crate::{DomainError, OrganizationId, PrincipalId, WebhookSubscriptionId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use url::Url;

/// Explicit allowlist of events safe to leave the control-plane trust boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WebhookEventType {
    #[serde(rename = "message.received.v1")]
    MessageReceivedV1,
    #[serde(rename = "message.delivery_status_changed.v1")]
    MessageDeliveryStatusChangedV1,
    #[serde(rename = "agent.message.dead_lettered.v1")]
    AgentMessageDeadLetteredV1,
}

impl WebhookEventType {
    pub const CATALOG: [Self; 3] = [
        Self::MessageReceivedV1,
        Self::MessageDeliveryStatusChangedV1,
        Self::AgentMessageDeadLetteredV1,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MessageReceivedV1 => "message.received.v1",
            Self::MessageDeliveryStatusChangedV1 => "message.delivery_status_changed.v1",
            Self::AgentMessageDeadLetteredV1 => "agent.message.dead_lettered.v1",
        }
    }

    pub fn parse(value: &str) -> Result<Self, DomainError> {
        match value {
            "message.received.v1" => Ok(Self::MessageReceivedV1),
            "message.delivery_status_changed.v1" => Ok(Self::MessageDeliveryStatusChangedV1),
            "agent.message.dead_lettered.v1" => Ok(Self::AgentMessageDeadLetteredV1),
            _ => Err(DomainError::InvalidWebhookSubscription),
        }
    }
}

/// Tenant-bound, Agent-owned outbound event subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookSubscription {
    pub id: WebhookSubscriptionId,
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub endpoint_url: String,
    pub event_types: BTreeSet<WebhookEventType>,
    pub signing_key_reference: String,
    pub enabled: bool,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WebhookSubscription {
    pub fn new(
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        endpoint_url: String,
        event_types: BTreeSet<WebhookEventType>,
        signing_key_reference: String,
        now: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        validate_configuration(&endpoint_url, &event_types, &signing_key_reference)?;
        Ok(Self {
            id: WebhookSubscriptionId::new(),
            organization_id,
            principal_id,
            endpoint_url,
            event_types,
            signing_key_reference,
            enabled: true,
            version: 1,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update(
        &mut self,
        endpoint_url: String,
        event_types: BTreeSet<WebhookEventType>,
        signing_key_reference: String,
        enabled: bool,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        validate_configuration(&endpoint_url, &event_types, &signing_key_reference)?;
        if now < self.updated_at
            || self.version == i64::MAX
            || self.endpoint_url == endpoint_url
                && self.event_types == event_types
                && self.signing_key_reference == signing_key_reference
                && self.enabled == enabled
        {
            return Err(DomainError::InvalidWebhookSubscription);
        }
        self.endpoint_url = endpoint_url;
        self.event_types = event_types;
        self.signing_key_reference = signing_key_reference;
        self.enabled = enabled;
        self.version += 1;
        self.updated_at = now;
        Ok(())
    }
}

fn validate_configuration(
    endpoint_url: &str,
    event_types: &BTreeSet<WebhookEventType>,
    signing_key_reference: &str,
) -> Result<(), DomainError> {
    let endpoint = Url::parse(endpoint_url).map_err(|_| DomainError::InvalidWebhookSubscription)?;
    if endpoint_url.len() > 2048
        || endpoint.scheme() != "https"
        || endpoint.host().is_none()
        || endpoint.username() != ""
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || event_types.is_empty()
        || event_types.len() > WebhookEventType::CATALOG.len()
        || signing_key_reference.is_empty()
        || signing_key_reference.len() > 100
        || !signing_key_reference
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(DomainError::InvalidWebhookSubscription);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events() -> BTreeSet<WebhookEventType> {
        [WebhookEventType::MessageReceivedV1].into_iter().collect()
    }

    #[test]
    fn only_https_secret_free_endpoints_and_safe_events_are_accepted() {
        let now = Utc::now();
        let valid = WebhookSubscription::new(
            OrganizationId::new(),
            PrincipalId::new(),
            "https://agent.example.test/hooks/copilotmail".into(),
            events(),
            "hook-2026-01".into(),
            now,
        )
        .unwrap();
        assert_eq!(valid.version, 1);

        for endpoint in [
            "http://agent.example.test/hook",
            "https://user:secret@agent.example.test/hook",
            "https://agent.example.test/hook?token=secret",
            "https://agent.example.test/hook#fragment",
        ] {
            let error = WebhookSubscription::new(
                OrganizationId::new(),
                PrincipalId::new(),
                endpoint.into(),
                events(),
                "hook-2026-01".into(),
                now,
            )
            .unwrap_err();
            let rendered = format!("{error} {error:?}");
            assert_eq!(
                rendered,
                "invalid Agent webhook subscription InvalidWebhookSubscription"
            );
            assert!(!rendered.contains(endpoint), "{endpoint}");
            assert!(!rendered.contains("secret"));
        }
        assert!(WebhookEventType::parse("identity.principal.created.v1").is_err());
    }

    #[test]
    fn updates_are_versioned_and_no_op_replays_are_rejected() {
        let now = Utc::now();
        let mut subscription = WebhookSubscription::new(
            OrganizationId::new(),
            PrincipalId::new(),
            "https://agent.example.test/hook".into(),
            events(),
            "hook-2026-01".into(),
            now,
        )
        .unwrap();
        assert!(
            subscription
                .update(
                    subscription.endpoint_url.clone(),
                    subscription.event_types.clone(),
                    subscription.signing_key_reference.clone(),
                    true,
                    now,
                )
                .is_err()
        );
        subscription
            .update(
                "https://agent.example.test/new-hook".into(),
                events(),
                "hook-2026-02".into(),
                false,
                now,
            )
            .unwrap();
        assert_eq!(subscription.version, 2);
        assert!(!subscription.enabled);
    }
}
