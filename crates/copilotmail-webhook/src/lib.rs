//! Signed, bounded, SSRF-resistant webhook delivery adapter.
//!
//! The transactional Outbox remains the event source. This crate owns only the
//! external HTTP boundary and never decides which tenant subscription receives
//! an event or when an event becomes terminal.

#![forbid(unsafe_code)]

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use copilotmail_dns::{DnsLookup, is_public_mail_address};
use hmac::{Hmac, Mac};
use reqwest::redirect::Policy;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::{
    collections::BTreeMap,
    fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};
use url::{Host, Url};
use uuid::Uuid;

const MAX_EVENT_BODY_BYTES: usize = 1024 * 1024;
const MAX_ENDPOINT_ADDRESSES: usize = 16;
const MAX_KEY_SET_BYTES: u64 = 64 * 1024;
const MAX_SECRET_FILE_BYTES: u64 = 512;

/// File-backed key ring. Subscriptions select a key by stable reference so an
/// old and new secret can overlap during rotation.
pub struct WebhookSigningKeySet {
    keys: BTreeMap<String, SecretString>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeySetManifest {
    keys: Vec<KeySetEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeySetEntry {
    key_id: String,
    secret_file: PathBuf,
}

impl WebhookSigningKeySet {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, WebhookError> {
        let path = path.as_ref();
        let metadata = fs::metadata(path).map_err(|_| WebhookError::SigningKeySetUnavailable)?;
        if metadata.len() == 0 || metadata.len() > MAX_KEY_SET_BYTES {
            return Err(WebhookError::InvalidSigningKeySet);
        }
        let bytes = fs::read(path).map_err(|_| WebhookError::SigningKeySetUnavailable)?;
        let manifest: KeySetManifest =
            serde_json::from_slice(&bytes).map_err(|_| WebhookError::InvalidSigningKeySet)?;
        if manifest.keys.is_empty() || manifest.keys.len() > 100 {
            return Err(WebhookError::InvalidSigningKeySet);
        }
        let base = path.parent().unwrap_or_else(|| Path::new("."));
        let mut keys = BTreeMap::new();
        for entry in manifest.keys {
            validate_key_id(&entry.key_id)?;
            let secret_path = if entry.secret_file.is_absolute() {
                entry.secret_file
            } else {
                base.join(entry.secret_file)
            };
            let metadata =
                fs::metadata(&secret_path).map_err(|_| WebhookError::SigningKeySetUnavailable)?;
            if metadata.len() == 0 || metadata.len() > MAX_SECRET_FILE_BYTES {
                return Err(WebhookError::InvalidSigningKeySet);
            }
            let secret =
                fs::read_to_string(secret_path).map_err(|_| WebhookError::InvalidSigningKeySet)?;
            let secret = secret.trim();
            if !(32..=128).contains(&secret.len())
                || !secret.bytes().all(|byte| byte.is_ascii_graphic())
                || keys
                    .insert(entry.key_id, SecretString::from(secret.to_owned()))
                    .is_some()
            {
                return Err(WebhookError::InvalidSigningKeySet);
            }
        }
        Ok(Self { keys })
    }

    #[must_use]
    pub fn get(&self, key_id: &str) -> Option<&SecretString> {
        self.keys.get(key_id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Durable event data transformed into the stable external envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct WebhookEvent {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub event_type: String,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub schema_version: i32,
    pub payload: Value,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct WebhookEnvelope<'a> {
    id: Uuid,
    organization_id: Uuid,
    #[serde(rename = "type")]
    event_type: &'a str,
    schema_version: i32,
    occurred_at: DateTime<Utc>,
    aggregate: WebhookAggregate<'a>,
    data: &'a Value,
}

#[derive(Debug, Serialize)]
struct WebhookAggregate<'a> {
    #[serde(rename = "type")]
    aggregate_type: &'a str,
    id: &'a str,
}

/// Network and policy bounds for one delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookDeliveryConfig {
    pub timeout: Duration,
    /// Development/test only. Production callers must keep this false.
    pub allow_insecure_http: bool,
    /// Development/test only. Disables the public-address SSRF boundary.
    pub allow_private_network: bool,
}

impl WebhookDeliveryConfig {
    pub fn validate(&self) -> Result<(), WebhookError> {
        if !(Duration::from_secs(1)..=Duration::from_secs(30)).contains(&self.timeout)
            || self.allow_private_network && !self.allow_insecure_http
        {
            return Err(WebhookError::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Successful receiver acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebhookDeliveryReceipt {
    pub response_status: u16,
}

/// Stable failure taxonomy persisted by the Worker without response bodies.
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("webhook delivery configuration is invalid")]
    InvalidConfiguration,
    #[error("webhook endpoint URL is invalid")]
    InvalidEndpoint,
    #[error("webhook endpoint did not resolve to an allowed address")]
    EndpointNotPublic,
    #[error("webhook event is invalid or exceeds the size limit")]
    InvalidEvent,
    #[error("webhook signing key is invalid")]
    InvalidSigningKey,
    #[error("webhook signing key reference was not found")]
    SigningKeyNotFound,
    #[error("webhook signing key set is unavailable")]
    SigningKeySetUnavailable,
    #[error("webhook signing key set is invalid")]
    InvalidSigningKeySet,
    #[error("webhook DNS lookup failed")]
    DnsUnavailable,
    #[error("webhook HTTP request failed")]
    Transport,
    #[error("webhook receiver returned HTTP {0}")]
    Rejected(u16),
}

impl WebhookError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfiguration => "webhook_invalid_configuration",
            Self::InvalidEndpoint => "webhook_invalid_endpoint",
            Self::EndpointNotPublic => "webhook_endpoint_not_public",
            Self::InvalidEvent => "webhook_invalid_event",
            Self::InvalidSigningKey => "webhook_invalid_signing_key",
            Self::SigningKeyNotFound => "webhook_signing_key_not_found",
            Self::SigningKeySetUnavailable => "webhook_signing_key_set_unavailable",
            Self::InvalidSigningKeySet => "webhook_invalid_signing_key_set",
            Self::DnsUnavailable => "webhook_dns_unavailable",
            Self::Transport => "webhook_transport_error",
            Self::Rejected(status) if retryable_status(*status) => "webhook_receiver_retryable",
            Self::Rejected(_) => "webhook_receiver_rejected",
        }
    }

    #[must_use]
    pub const fn retryable(&self) -> bool {
        match self {
            Self::DnsUnavailable
            | Self::Transport
            | Self::SigningKeySetUnavailable
            | Self::SigningKeyNotFound => true,
            Self::Rejected(status) => retryable_status(*status),
            _ => false,
        }
    }
}

/// Real HTTP publisher with a replaceable DNS port for deterministic testing.
pub struct SignedWebhookClient<L> {
    dns: L,
    config: WebhookDeliveryConfig,
}

impl<L> SignedWebhookClient<L>
where
    L: DnsLookup,
{
    pub fn new(dns: L, config: WebhookDeliveryConfig) -> Result<Self, WebhookError> {
        config.validate()?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        Ok(Self { dns, config })
    }

    pub async fn deliver(
        &self,
        endpoint: &str,
        key_id: &str,
        signing_secret: &SecretString,
        event: &WebhookEvent,
        attempted_at: DateTime<Utc>,
    ) -> Result<WebhookDeliveryReceipt, WebhookError> {
        validate_key_id(key_id)?;
        let body = serialize_event(event)?;
        let signature = sign(
            signing_secret.expose_secret().as_bytes(),
            attempted_at.timestamp(),
            event.id,
            &body,
        )?;
        let (url, host, addresses) = self.resolve_endpoint(endpoint).await?;
        let mut builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(self.config.timeout)
            .connect_timeout(self.config.timeout)
            .user_agent("CopilotMail-Webhook/1")
            .https_only(!self.config.allow_insecure_http);
        if let Some(host) = host {
            builder = builder.resolve_to_addrs(&host, &addresses);
        }
        let client = builder.build().map_err(|_| WebhookError::Transport)?;
        let response = client
            .post(url)
            .header("content-type", "application/json")
            .header("copilotmail-webhook-id", event.id.to_string())
            .header(
                "copilotmail-webhook-timestamp",
                attempted_at.timestamp().to_string(),
            )
            .header("copilotmail-webhook-key-id", key_id)
            .header("copilotmail-webhook-signature", format!("v1={signature}"))
            .body(body)
            .send()
            .await
            .map_err(|_| WebhookError::Transport)?;
        let status = response.status();
        if status.is_success() {
            Ok(WebhookDeliveryReceipt {
                response_status: status.as_u16(),
            })
        } else {
            Err(WebhookError::Rejected(status.as_u16()))
        }
    }

    async fn resolve_endpoint(
        &self,
        endpoint: &str,
    ) -> Result<(Url, Option<String>, Vec<SocketAddr>), WebhookError> {
        let url = Url::parse(endpoint).map_err(|_| WebhookError::InvalidEndpoint)?;
        let scheme_allowed =
            url.scheme() == "https" || self.config.allow_insecure_http && url.scheme() == "http";
        if !scheme_allowed
            || url.username() != ""
            || url.password().is_some()
            || url.fragment().is_some()
            || url.query().is_some()
        {
            return Err(WebhookError::InvalidEndpoint);
        }
        let host = url.host().ok_or(WebhookError::InvalidEndpoint)?;
        let port = url
            .port_or_known_default()
            .ok_or(WebhookError::InvalidEndpoint)?;
        match host {
            Host::Ipv4(address) => {
                self.ensure_allowed_address(IpAddr::V4(address))?;
                Ok((url, None, Vec::new()))
            }
            Host::Ipv6(address) => {
                self.ensure_allowed_address(IpAddr::V6(address))?;
                Ok((url, None, Vec::new()))
            }
            Host::Domain(host) => {
                let host = host.to_ascii_lowercase();
                if host.is_empty() || host.len() > 253 {
                    return Err(WebhookError::InvalidEndpoint);
                }
                let lookup = self
                    .dns
                    .ip(&format!("{host}."))
                    .await
                    .map_err(|_| WebhookError::DnsUnavailable)?;
                if lookup.values.is_empty() || lookup.values.len() > MAX_ENDPOINT_ADDRESSES {
                    return Err(WebhookError::EndpointNotPublic);
                }
                let addresses = lookup
                    .values
                    .into_iter()
                    .map(|address| {
                        self.ensure_allowed_address(address)?;
                        Ok(SocketAddr::new(address, port))
                    })
                    .collect::<Result<Vec<_>, WebhookError>>()?;
                Ok((url, Some(host), addresses))
            }
        }
    }

    fn ensure_allowed_address(&self, address: IpAddr) -> Result<(), WebhookError> {
        if self.config.allow_private_network || is_public_mail_address(address) {
            Ok(())
        } else {
            Err(WebhookError::EndpointNotPublic)
        }
    }
}

fn serialize_event(event: &WebhookEvent) -> Result<Vec<u8>, WebhookError> {
    if event.schema_version <= 0
        || !valid_dotted_name(&event.event_type)
        || !valid_dotted_name(&event.aggregate_type)
        || event.aggregate_id.is_empty()
        || event.aggregate_id.len() > 512
        || event.aggregate_id.chars().any(char::is_control)
    {
        return Err(WebhookError::InvalidEvent);
    }
    let bytes = serde_json::to_vec(&WebhookEnvelope {
        id: event.id,
        organization_id: event.organization_id,
        event_type: &event.event_type,
        schema_version: event.schema_version,
        occurred_at: event.occurred_at,
        aggregate: WebhookAggregate {
            aggregate_type: &event.aggregate_type,
            id: &event.aggregate_id,
        },
        data: &event.payload,
    })
    .map_err(|_| WebhookError::InvalidEvent)?;
    if bytes.len() > MAX_EVENT_BODY_BYTES {
        return Err(WebhookError::InvalidEvent);
    }
    Ok(bytes)
}

fn sign(
    secret: &[u8],
    timestamp: i64,
    event_id: Uuid,
    body: &[u8],
) -> Result<String, WebhookError> {
    if !(32..=128).contains(&secret.len()) {
        return Err(WebhookError::InvalidSigningKey);
    }
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret).map_err(|_| WebhookError::InvalidSigningKey)?;
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(event_id.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn validate_key_id(value: &str) -> Result<(), WebhookError> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Err(WebhookError::InvalidSigningKey)
    } else {
        Ok(())
    }
}

fn valid_dotted_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 64
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

const fn retryable_status(status: u16) -> bool {
    status == 408 || status == 425 || status == 429 || status >= 500
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use chrono::TimeZone;
    use copilotmail_dns::{DnsError, DnsRecordSet};
    use std::{net::Ipv4Addr, sync::Arc};
    use tokio::sync::Mutex;

    #[derive(Clone)]
    struct StaticDns(Vec<IpAddr>);

    #[async_trait]
    impl DnsLookup for StaticDns {
        async fn txt(&self, _: &str) -> Result<DnsRecordSet<String>, DnsError> {
            unreachable!()
        }

        async fn mx(&self, _: &str) -> Result<DnsRecordSet<copilotmail_dns::MxRecord>, DnsError> {
            unreachable!()
        }

        async fn ip(&self, _: &str) -> Result<DnsRecordSet<IpAddr>, DnsError> {
            Ok(DnsRecordSet {
                values: self.0.clone(),
                valid_until: Utc::now() + chrono::Duration::minutes(1),
            })
        }
    }

    fn event() -> WebhookEvent {
        WebhookEvent {
            id: Uuid::parse_str("018f4f6d-7b42-7cc0-9f7e-9d3fbe0db123").unwrap(),
            organization_id: Uuid::parse_str("018f4f6d-7b42-7cc0-9f7e-9d3fbe0db456").unwrap(),
            event_type: "message.received.v1".into(),
            aggregate_type: "message".into(),
            aggregate_id: "018f4f6d-7b42-7cc0-9f7e-9d3fbe0db789".into(),
            schema_version: 1,
            payload: serde_json::json!({"mailbox_id": "mailbox-1"}),
            occurred_at: Utc.timestamp_opt(1_720_000_000, 0).unwrap(),
        }
    }

    #[test]
    fn signature_is_deterministic_and_binds_timestamp_event_and_body() {
        let event = event();
        let body = serialize_event(&event).unwrap();
        let secret = [7_u8; 32];
        let first = sign(&secret, 1_720_000_001, event.id, &body).unwrap();
        let second = sign(&secret, 1_720_000_001, event.id, &body).unwrap();
        assert_eq!(first, second);
        assert_ne!(
            first,
            sign(&secret, 1_720_000_002, event.id, &body).unwrap()
        );
    }

    #[test]
    fn file_key_set_resolves_relative_secret_files_and_rejects_duplicates() {
        let directory =
            std::env::temp_dir().join(format!("copilotmail-webhook-{}", Uuid::new_v4()));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("hook.secret"), "z".repeat(32)).unwrap();
        let manifest = directory.join("keys.json");
        fs::write(
            &manifest,
            r#"{"keys":[{"key_id":"hook-2026-01","secret_file":"hook.secret"}]}"#,
        )
        .unwrap();
        let keys = WebhookSigningKeySet::load(&manifest).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys.get("hook-2026-01").unwrap().expose_secret(),
            &"z".repeat(32)
        );

        fs::write(
            &manifest,
            r#"{"keys":[
                {"key_id":"duplicate","secret_file":"hook.secret"},
                {"key_id":"duplicate","secret_file":"hook.secret"}
            ]}"#,
        )
        .unwrap();
        assert!(matches!(
            WebhookSigningKeySet::load(&manifest),
            Err(WebhookError::InvalidSigningKeySet)
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn production_policy_rejects_http_and_private_dns_answers() {
        let client = SignedWebhookClient::new(
            StaticDns(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
            WebhookDeliveryConfig {
                timeout: Duration::from_secs(2),
                allow_insecure_http: false,
                allow_private_network: false,
            },
        )
        .unwrap();
        assert!(matches!(
            client
                .resolve_endpoint("http://hooks.example.test/event")
                .await,
            Err(WebhookError::InvalidEndpoint)
        ));
        assert!(matches!(
            client
                .resolve_endpoint("https://hooks.example.test/event")
                .await,
            Err(WebhookError::EndpointNotPublic)
        ));
    }

    #[tokio::test]
    async fn posts_exact_signed_body_and_never_follows_redirects() {
        type CapturedRequest = Option<(HeaderMap, Vec<u8>)>;

        #[derive(Clone, Default)]
        struct Capture(Arc<Mutex<CapturedRequest>>);

        async fn receive(
            State(capture): State<Capture>,
            headers: HeaderMap,
            body: axum::body::Bytes,
        ) -> StatusCode {
            *capture.0.lock().await = Some((headers, body.to_vec()));
            StatusCode::NO_CONTENT
        }

        async fn redirect() -> (StatusCode, [(&'static str, &'static str); 1]) {
            (StatusCode::FOUND, [("location", "/event")])
        }

        let capture = Capture::default();
        let app = Router::new()
            .route("/event", post(receive))
            .route("/redirect", post(redirect))
            .with_state(capture.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = SignedWebhookClient::new(
            StaticDns(vec![address.ip()]),
            WebhookDeliveryConfig {
                timeout: Duration::from_secs(2),
                allow_insecure_http: true,
                allow_private_network: true,
            },
        )
        .unwrap();
        let attempted_at = Utc.timestamp_opt(1_720_000_001, 0).unwrap();
        let receipt = client
            .deliver(
                &format!("http://localhost:{}/event", address.port()),
                "agent-hook-2026-01",
                &SecretString::from("x".repeat(32)),
                &event(),
                attempted_at,
            )
            .await
            .unwrap();
        assert_eq!(receipt.response_status, 204);
        let (headers, body) = capture.0.lock().await.clone().unwrap();
        assert_eq!(
            headers["copilotmail-webhook-timestamp"],
            attempted_at.timestamp().to_string()
        );
        assert!(
            headers["copilotmail-webhook-signature"]
                .to_str()
                .unwrap()
                .starts_with("v1=")
        );
        assert_eq!(body, serialize_event(&event()).unwrap());

        *capture.0.lock().await = None;
        let redirect_result = client
            .deliver(
                &format!("http://localhost:{}/redirect", address.port()),
                "agent-hook-2026-01",
                &SecretString::from("x".repeat(32)),
                &event(),
                attempted_at,
            )
            .await;
        assert!(matches!(redirect_result, Err(WebhookError::Rejected(302))));
        assert!(capture.0.lock().await.is_none());
    }

    #[test]
    fn retry_taxonomy_is_stable() {
        for status in [408, 425, 429, 500, 503] {
            let error = WebhookError::Rejected(status);
            assert!(error.retryable(), "{status}");
            assert_eq!(error.code(), "webhook_receiver_retryable");
        }
        let rejected = WebhookError::Rejected(400);
        assert!(!rejected.retryable());
        assert_eq!(rejected.code(), "webhook_receiver_rejected");
    }
}
