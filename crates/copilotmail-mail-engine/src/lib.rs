//! Stalwart JMAP adapter for the application-owned mail data-plane ports.
//!
//! Application use cases own the vendor-neutral contracts in
//! `copilotmail_application::mail_ports`; this outward adapter implements them
//! using only documented JMAP surfaces and never exposes Stalwart persistence
//! details.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use copilotmail_application::mail_ports::*;
use copilotmail_domain::{MailDataPlaneId, MailDomainName, MailboxAddress};
use hmac::{Hmac, Mac};
use reqwest::{Client, Url, redirect::Policy};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Duration,
};

const JMAP_CORE: &str = "urn:ietf:params:jmap:core";
const JMAP_MAIL: &str = "urn:ietf:params:jmap:mail";
const JMAP_SUBMISSION: &str = "urn:ietf:params:jmap:submission";
const STALWART_JMAP: &str = "urn:stalwart:jmap";
const MAX_SESSION_BYTES: usize = 1024 * 1024;
const MAX_JMAP_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_STALWART_WEBHOOK_EVENTS: usize = 100;
const MAX_STALWART_WEBHOOK_BYTES: usize = 1024 * 1024;
const MAX_STALWART_EVENT_ID_BYTES: usize = 128;
const MAX_STALWART_MAILBOX_IDS: usize = 128;
const MAX_MAIL_PAGE_SIZE: u32 = 50;
const MAX_MAIL_PAGE_POSITION: u32 = 10_000;
const MAX_TEXT_BODY_BYTES: usize = 256 * 1024;

/// Authenticated parser for the documented Stalwart v0.16 telemetry WebHook
/// envelope. Stalwart signs the exact request body using HMAC-SHA256 and
/// Base64-encodes the tag in `X-Signature`.
pub struct StalwartWebhookParser;

impl StalwartWebhookParser {
    /// Verifies the signature before deserializing and accepts only SMTP
    /// message-ingest events explicitly configured by CopilotMail.
    pub fn verify_and_parse(
        body: &[u8],
        signature: &str,
        signature_key: &[u8],
    ) -> Result<Vec<InboundMailEngineEvent>, MailEngineWebhookError> {
        Self::verify_envelope(body, signature, signature_key)?
            .into_iter()
            .map(StalwartWebhookEvent::normalize_inbound)
            .collect()
    }

    /// Verifies and normalizes the configured inbound and outbound event
    /// families without retaining vendor payloads or SMTP response text.
    pub fn verify_and_parse_all(
        body: &[u8],
        signature: &str,
        signature_key: &[u8],
    ) -> Result<Vec<MailEngineWebhookEvent>, MailEngineWebhookError> {
        Self::verify_envelope(body, signature, signature_key)?
            .into_iter()
            .map(StalwartWebhookEvent::normalize)
            .collect()
    }

    fn verify_envelope(
        body: &[u8],
        signature: &str,
        signature_key: &[u8],
    ) -> Result<Vec<StalwartWebhookEvent>, MailEngineWebhookError> {
        if signature_key.len() < 32 || signature_key.len() > MAX_TOKEN_BYTES {
            return Err(MailEngineWebhookError::InvalidConfiguration);
        }
        if body.is_empty() || body.len() > MAX_STALWART_WEBHOOK_BYTES {
            return Err(MailEngineWebhookError::InvalidPayload);
        }
        if signature.len() != 44 {
            return Err(MailEngineWebhookError::InvalidSignature);
        }
        let signature = STANDARD
            .decode(signature)
            .map_err(|_| MailEngineWebhookError::InvalidSignature)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(signature_key)
            .map_err(|_| MailEngineWebhookError::InvalidConfiguration)?;
        mac.update(body);
        mac.verify_slice(&signature)
            .map_err(|_| MailEngineWebhookError::InvalidSignature)?;

        let envelope: StalwartWebhookEnvelope =
            serde_json::from_slice(body).map_err(|_| MailEngineWebhookError::InvalidPayload)?;
        if envelope.events.is_empty() || envelope.events.len() > MAX_STALWART_WEBHOOK_EVENTS {
            return Err(MailEngineWebhookError::InvalidPayload);
        }
        Ok(envelope.events)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StalwartWebhookEnvelope {
    events: Vec<StalwartWebhookEvent>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StalwartWebhookEvent {
    id: String,
    created_at: String,
    #[serde(rename = "type")]
    event_type: String,
    data: serde_json::Map<String, serde_json::Value>,
}

impl StalwartWebhookEvent {
    fn validate_identity(&self) -> Result<DateTime<Utc>, MailEngineWebhookError> {
        if !valid_opaque_ref(&self.id, MAX_STALWART_EVENT_ID_BYTES) {
            return Err(MailEngineWebhookError::InvalidPayload);
        }
        let occurred_at = DateTime::parse_from_rfc3339(&self.created_at)
            .map_err(|_| MailEngineWebhookError::InvalidPayload)?
            .with_timezone(&Utc);
        Ok(occurred_at)
    }

    fn normalize_inbound(self) -> Result<InboundMailEngineEvent, MailEngineWebhookError> {
        let source_occurred_at = self.validate_identity()?;
        let classification = match self.event_type.as_str() {
            "message-ingest.ham" => InboundMessageClassification::Ham,
            "message-ingest.spam" => InboundMessageClassification::Spam,
            _ => return Err(MailEngineWebhookError::UnsupportedEvent),
        };
        let account_ref = unsigned_ref(self.data.get("accountId"))?;
        let document_ref = unsigned_ref(self.data.get("documentId"))?;
        validate_mailbox_ids(self.data.get("mailboxId"))?;
        Ok(InboundMailEngineEvent {
            source_event_id: self.id,
            source_occurred_at,
            account_ref,
            document_ref,
            classification,
        })
    }

    fn normalize(self) -> Result<MailEngineWebhookEvent, MailEngineWebhookError> {
        if matches!(
            self.event_type.as_str(),
            "message-ingest.ham" | "message-ingest.spam"
        ) {
            return self
                .normalize_inbound()
                .map(MailEngineWebhookEvent::Inbound);
        }
        let source_occurred_at = self.validate_identity()?;
        let kind = match self.event_type.as_str() {
            "delivery.delivered" => OutboundDeliveryEventKind::Delivered,
            "delivery.failed" => OutboundDeliveryEventKind::PermanentFailure,
            "queue.rescheduled" => OutboundDeliveryEventKind::Rescheduled,
            "delivery.completed" => OutboundDeliveryEventKind::Completed,
            _ => return Err(MailEngineWebhookError::UnsupportedEvent),
        };
        let queue_ref = unsigned_ref(self.data.get("queueId"))?;
        let envelope_from = self
            .data
            .get("from")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| MailboxAddress::parse(value).ok())
            .ok_or(MailEngineWebhookError::InvalidPayload)?;
        let recipient = if matches!(
            kind,
            OutboundDeliveryEventKind::Delivered | OutboundDeliveryEventKind::PermanentFailure
        ) {
            Some(
                self.data
                    .get("to")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| MailboxAddress::parse(value).ok())
                    .ok_or(MailEngineWebhookError::InvalidPayload)?,
            )
        } else {
            None
        };
        let smtp_code = if kind == OutboundDeliveryEventKind::Delivered {
            Some(
                self.data
                    .get("code")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u16::try_from(value).ok())
                    .filter(|value| (200..=299).contains(value))
                    .ok_or(MailEngineWebhookError::InvalidPayload)?,
            )
        } else {
            None
        };
        let next_retry_at = if kind == OutboundDeliveryEventKind::Rescheduled {
            Some(
                self.data
                    .get("nextRetry")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.with_timezone(&Utc))
                    .ok_or(MailEngineWebhookError::InvalidPayload)?,
            )
        } else {
            None
        };
        Ok(MailEngineWebhookEvent::Outbound(OutboundMailEngineEvent {
            source_event_id: self.id,
            source_occurred_at,
            queue_ref,
            envelope_from,
            recipient,
            kind,
            smtp_code,
            next_retry_at,
        }))
    }
}

fn unsigned_ref(value: Option<&serde_json::Value>) -> Result<String, MailEngineWebhookError> {
    value
        .and_then(serde_json::Value::as_u64)
        .map(|value| value.to_string())
        .ok_or(MailEngineWebhookError::InvalidPayload)
}

fn validate_mailbox_ids(value: Option<&serde_json::Value>) -> Result<(), MailEngineWebhookError> {
    let values = value
        .and_then(serde_json::Value::as_array)
        .ok_or(MailEngineWebhookError::InvalidPayload)?;
    if values.is_empty()
        || values.len() > MAX_STALWART_MAILBOX_IDS
        || values.iter().any(|value| value.as_u64().is_none())
    {
        return Err(MailEngineWebhookError::InvalidPayload);
    }
    Ok(())
}

fn valid_opaque_ref(value: &str, maximum_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_bytes
        && value.is_ascii()
        && value.bytes().all(|byte| !byte.is_ascii_control())
}

fn valid_mail_search_text(value: &str) -> bool {
    value == value.trim()
        && (2..=128).contains(&value.chars().count())
        && value.len() <= 512
        && !value.chars().any(char::is_control)
}

/// Validated Stalwart/JMAP connection settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalwartJmapConfig {
    base_url: Url,
    api_token_file: PathBuf,
    timeout: Duration,
}

impl StalwartJmapConfig {
    /// Validates the configured origin before any credential is read or sent.
    pub fn new(
        base_url: impl AsRef<str>,
        api_token_file: PathBuf,
        timeout: Duration,
        allow_insecure_http: bool,
    ) -> Result<Self, MailEngineError> {
        let base_url =
            Url::parse(base_url.as_ref()).map_err(|_| MailEngineError::InvalidConfiguration)?;
        let valid_scheme =
            base_url.scheme() == "https" || (allow_insecure_http && base_url.scheme() == "http");
        if !valid_scheme
            || base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || !matches!(base_url.path(), "" | "/")
            || api_token_file.as_os_str().is_empty()
            || !(Duration::from_secs(1)..=Duration::from_secs(60)).contains(&timeout)
        {
            return Err(MailEngineError::InvalidConfiguration);
        }
        Ok(Self {
            base_url,
            api_token_file,
            timeout,
        })
    }
}

/// Authenticated Stalwart adapter using the standard JMAP session resource.
pub struct StalwartJmapClient {
    data_plane_id: MailDataPlaneId,
    config: StalwartJmapConfig,
    client: Client,
    discovery_url: Url,
}

impl MailDataPlanePort for StalwartJmapClient {
    fn data_plane_id(&self) -> MailDataPlaneId {
        self.data_plane_id
    }
}

impl StalwartJmapClient {
    /// Builds a hardened HTTP client without making a network request.
    pub fn new(
        data_plane_id: MailDataPlaneId,
        config: StalwartJmapConfig,
    ) -> Result<Self, MailEngineError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let discovery_url = config
            .base_url
            .join("/.well-known/jmap")
            .map_err(|_| MailEngineError::InvalidConfiguration)?;
        let client = Client::builder()
            .connect_timeout(config.timeout.min(Duration::from_secs(5)))
            .timeout(config.timeout)
            .redirect(Policy::none())
            .user_agent("CopilotMail-MailEngine/0.2")
            .build()
            .map_err(|_| MailEngineError::InvalidConfiguration)?;
        Ok(Self {
            data_plane_id,
            config,
            client,
            discovery_url,
        })
    }

    async fn load_token(&self) -> Result<SecretString, MailEngineError> {
        let metadata = tokio::fs::metadata(&self.config.api_token_file)
            .await
            .map_err(|_| MailEngineError::CredentialUnavailable)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TOKEN_BYTES as u64 {
            return Err(MailEngineError::CredentialUnavailable);
        }
        let token = tokio::fs::read_to_string(&self.config.api_token_file)
            .await
            .map_err(|_| MailEngineError::CredentialUnavailable)?;
        let token = token.trim();
        if token.is_empty()
            || token.len() > MAX_TOKEN_BYTES
            || token.chars().any(char::is_whitespace)
        {
            return Err(MailEngineError::CredentialUnavailable);
        }
        Ok(SecretString::from(token.to_owned()))
    }

    fn validate_api_url(&self, value: &str) -> Result<Url, MailEngineError> {
        let url = Url::parse(value).map_err(|_| MailEngineError::InvalidSession)?;
        if url.origin() != self.config.base_url.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(MailEngineError::InvalidSession);
        }
        Ok(url)
    }

    async fn authenticated_session(
        &self,
    ) -> Result<(SecretString, JmapSession, Url), MailEngineError> {
        let token = self.load_token().await?;
        let response = self
            .client
            .get(self.discovery_url.clone())
            .bearer_auth(token.expose_secret())
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_SESSION_BYTES).await?;
        let session: JmapSession =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidSession)?;
        if session.username.is_empty()
            || session.username.len() > 512
            || session.state.is_empty()
            || session.state.len() > 512
            || !session.capabilities.contains_key(JMAP_CORE)
            || !session.capabilities.contains_key(STALWART_JMAP)
        {
            return Err(MailEngineError::InvalidSession);
        }
        let api_url = self.validate_api_url(&session.api_url)?;
        Ok((token, session, api_url))
    }

    async fn authenticated_mail_session(
        &self,
        account_ref: &str,
    ) -> Result<(SecretString, JmapSession, Url), MailEngineError> {
        if !valid_opaque_ref(account_ref, 512) {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let token = self.load_token().await?;
        let response = self
            .client
            .get(self.discovery_url.clone())
            .bearer_auth(token.expose_secret())
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_SESSION_BYTES).await?;
        let session: JmapSession =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidSession)?;
        let account = session
            .accounts
            .get(account_ref)
            .ok_or(MailEngineError::InvalidSession)?;
        if session.username.is_empty()
            || session.username.len() > 512
            || session.state.is_empty()
            || session.state.len() > 512
            || !session.capabilities.contains_key(JMAP_CORE)
            || !session.capabilities.contains_key(JMAP_MAIL)
            || !account.account_capabilities.contains_key(JMAP_MAIL)
        {
            return Err(MailEngineError::InvalidSession);
        }
        let api_url = self.validate_api_url(&session.api_url)?;
        Ok((token, session, api_url))
    }

    async fn authenticated_submission_session(
        &self,
        account_ref: &str,
    ) -> Result<(SecretString, JmapSession, Url), MailEngineError> {
        let (token, session, api_url) = self.authenticated_mail_session(account_ref).await?;
        let account = session
            .accounts
            .get(account_ref)
            .ok_or(MailEngineError::InvalidSession)?;
        if !session.capabilities.contains_key(JMAP_SUBMISSION)
            || !account.account_capabilities.contains_key(JMAP_SUBMISSION)
        {
            return Err(MailEngineError::InvalidSession);
        }
        Ok((token, session, api_url))
    }

    async fn submission_call(
        &self,
        token: &SecretString,
        api_url: &Url,
        method: &str,
        arguments: serde_json::Value,
        call_id: &str,
    ) -> Result<serde_json::Value, MailEngineError> {
        let response = self
            .client
            .post(api_url.clone())
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL, JMAP_SUBMISSION],
                "methodCalls": [[method, arguments, call_id]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1 || envelope.session_state.is_empty() {
            return Err(MailEngineError::InvalidResponse);
        }
        parse_method_response(&envelope.method_responses[0], method, call_id).cloned()
    }

    async fn resolve_submission_identity(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
        expected_from: &MailboxAddress,
    ) -> Result<String, MailEngineError> {
        let value = self
            .submission_call(
                token,
                api_url,
                "Identity/get",
                serde_json::json!({
                    "accountId": account_ref,
                    "ids": null,
                    "properties": ["id", "email"]
                }),
                "i",
            )
            .await?;
        let response: IdentityGetResponse =
            serde_json::from_value(value).map_err(|_| MailEngineError::InvalidResponse)?;
        if response.account_id != account_ref
            || response.state.is_empty()
            || !response.not_found.is_empty()
            || response.list.len() > 10_000
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let mut exact = response
            .list
            .into_iter()
            .filter_map(|identity| {
                let address = MailboxAddress::parse(&identity.email).ok()?;
                (address == *expected_from && valid_opaque_ref(&identity.id, 512))
                    .then_some(identity.id)
            })
            .collect::<Vec<_>>();
        if exact.len() != 1 {
            return Err(MailEngineError::ProjectionConflict);
        }
        Ok(exact.pop().expect("length checked"))
    }

    async fn resolve_submission_mailboxes(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
    ) -> Result<(String, String), MailEngineError> {
        let value = self
            .submission_call(
                token,
                api_url,
                "Mailbox/get",
                serde_json::json!({
                    "accountId": account_ref,
                    "ids": null,
                    "properties": ["id", "role", "myRights"]
                }),
                "m",
            )
            .await?;
        let response: MailboxGetResponse =
            serde_json::from_value(value).map_err(|_| MailEngineError::InvalidResponse)?;
        if response.account_id != account_ref
            || response.state.is_empty()
            || !response.not_found.is_empty()
            || response.list.len() > 10_000
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let drafts = response
            .list
            .iter()
            .filter(|mailbox| {
                mailbox.role.as_deref() == Some("drafts")
                    && mailbox.my_rights.may_remove_items
                    && valid_opaque_ref(&mailbox.id, 512)
            })
            .map(|mailbox| mailbox.id.clone())
            .collect::<Vec<_>>();
        let sent = response
            .list
            .iter()
            .filter(|mailbox| {
                mailbox.role.as_deref() == Some("sent")
                    && mailbox.my_rights.may_add_items
                    && valid_opaque_ref(&mailbox.id, 512)
            })
            .map(|mailbox| mailbox.id.clone())
            .collect::<Vec<_>>();
        if drafts.len() != 1 || sent.len() != 1 || drafts[0] == sent[0] {
            return Err(MailEngineError::ProjectionConflict);
        }
        Ok((drafts[0].clone(), sent[0].clone()))
    }

    async fn query_submission_ids(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
        engine_email_ref: &str,
    ) -> Result<Vec<String>, MailEngineError> {
        let value = self
            .submission_call(
                token,
                api_url,
                "EmailSubmission/query",
                serde_json::json!({
                    "accountId": account_ref,
                    "filter": {"emailIds": [engine_email_ref]},
                    "sort": null,
                    "position": 0,
                    "limit": 2,
                    "calculateTotal": true
                }),
                "q",
            )
            .await?;
        let response: SubmissionQueryResponse =
            serde_json::from_value(value).map_err(|_| MailEngineError::InvalidResponse)?;
        if response.account_id != account_ref
            || response.query_state.is_empty()
            || response.position != 0
            || response.total != Some(response.ids.len() as u32)
            || response.ids.len() > 1
            || response.ids.iter().any(|id| !valid_opaque_ref(id, 512))
        {
            return Err(MailEngineError::ProjectionConflict);
        }
        Ok(response.ids)
    }

    async fn get_submission(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
        submission_ref: &str,
    ) -> Result<SubmissionWire, MailEngineError> {
        let value = self
            .submission_call(
                token,
                api_url,
                "EmailSubmission/get",
                serde_json::json!({
                    "accountId": account_ref,
                    "ids": [submission_ref],
                    "properties": ["id", "identityId", "emailId", "deliveryStatus"]
                }),
                "g",
            )
            .await?;
        let response: SubmissionGetResponse =
            serde_json::from_value(value).map_err(|_| MailEngineError::InvalidResponse)?;
        if response.account_id != account_ref
            || response.state.is_empty()
            || !response.not_found.is_empty()
            || response.list.len() != 1
            || response.list[0].id != submission_ref
        {
            return Err(MailEngineError::ProjectionConflict);
        }
        let mut submission = response.list.into_iter().next().expect("length checked");
        submission.source_state = response.state;
        Ok(submission)
    }

    async fn resolve_drafts_mailbox(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
    ) -> Result<String, MailEngineError> {
        let response = self
            .client
            .post(api_url.clone())
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Mailbox/get", {
                    "accountId": account_ref,
                    "ids": null,
                    "properties": ["id", "role", "myRights"]
                }, "m"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1 {
            return Err(MailEngineError::InvalidResponse);
        }
        let mailboxes = parse_method_response(&envelope.method_responses[0], "Mailbox/get", "m")?;
        let mailboxes: MailboxGetResponse = serde_json::from_value(mailboxes.clone())
            .map_err(|_| MailEngineError::InvalidResponse)?;
        if mailboxes.account_id != account_ref
            || mailboxes.state.is_empty()
            || !mailboxes.not_found.is_empty()
            || mailboxes.list.len() > 10_000
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let draft_ids = mailboxes
            .list
            .into_iter()
            .filter(|mailbox| mailbox.role.as_deref() == Some("drafts"))
            .filter(|mailbox| mailbox.my_rights.may_add_items)
            .map(|mailbox| mailbox.id)
            .collect::<Vec<_>>();
        if draft_ids.len() != 1 || !valid_opaque_ref(&draft_ids[0], 512) {
            return Err(MailEngineError::Rejected);
        }
        Ok(draft_ids.into_iter().next().expect("length checked"))
    }

    async fn resolve_readable_drafts_mailbox(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
    ) -> Result<String, MailEngineError> {
        let response = self
            .client
            .post(api_url.clone())
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Mailbox/get", {
                    "accountId": account_ref,
                    "ids": null,
                    "properties": ["id", "role", "myRights"]
                }, "m"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1
            || envelope.session_state.is_empty()
            || envelope.session_state.len() > 512
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let mailboxes = parse_method_response(&envelope.method_responses[0], "Mailbox/get", "m")?;
        let mailboxes: MailboxGetResponse = serde_json::from_value(mailboxes.clone())
            .map_err(|_| MailEngineError::InvalidResponse)?;
        select_readable_drafts_mailbox(mailboxes, account_ref)
    }

    async fn resolve_inbox_mailbox(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
    ) -> Result<String, MailEngineError> {
        let response = self
            .client
            .post(api_url.clone())
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Mailbox/get", {
                    "accountId": account_ref,
                    "ids": null,
                    "properties": ["id", "role", "myRights"]
                }, "m"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1 || envelope.session_state.is_empty() {
            return Err(MailEngineError::InvalidResponse);
        }
        let mailboxes = parse_method_response(&envelope.method_responses[0], "Mailbox/get", "m")?;
        let mailboxes: MailboxGetResponse = serde_json::from_value(mailboxes.clone())
            .map_err(|_| MailEngineError::InvalidResponse)?;
        if mailboxes.account_id != account_ref
            || mailboxes.state.is_empty()
            || !mailboxes.not_found.is_empty()
            || mailboxes.list.len() > 10_000
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let mut inboxes = mailboxes
            .list
            .into_iter()
            .filter(|mailbox| mailbox.role.as_deref() == Some("inbox"))
            .collect::<Vec<_>>();
        if inboxes.len() != 1 {
            return Err(MailEngineError::Rejected);
        }
        let inbox = inboxes.pop().expect("length checked");
        if !inbox.my_rights.may_read_items || !valid_opaque_ref(&inbox.id, 512) {
            return Err(MailEngineError::Rejected);
        }
        Ok(inbox.id)
    }

    async fn validate_previous_draft(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
        draft_mailbox_id: &str,
        previous_draft_id: &str,
        expected_from: &str,
    ) -> Result<(), MailEngineError> {
        let response = self
            .client
            .post(api_url.clone())
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Email/get", {
                    "accountId": account_ref,
                    "ids": [previous_draft_id],
                    "properties": ["id", "mailboxIds", "keywords", "from"]
                }, "p"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1 {
            return Err(MailEngineError::InvalidResponse);
        }
        let result = parse_method_response(&envelope.method_responses[0], "Email/get", "p")?;
        let result: PreviousDraftGetResponse =
            serde_json::from_value(result.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
        if result.account_id != account_ref
            || result.state.is_empty()
            || !result.not_found.is_empty()
            || result.list.len() != 1
        {
            return Err(MailEngineError::Rejected);
        }
        let previous = &result.list[0];
        let previous_from = previous
            .from
            .first()
            .filter(|_| previous.from.len() == 1)
            .and_then(|address| MailboxAddress::parse(&address.email).ok());
        if previous.id != previous_draft_id
            || previous.mailbox_ids.get(draft_mailbox_id) != Some(&true)
            || previous
                .mailbox_ids
                .values()
                .filter(|enabled| **enabled)
                .count()
                != 1
            || previous.keywords.get("$draft") != Some(&true)
            || previous_from.as_ref().map(MailboxAddress::as_str) != Some(expected_from)
        {
            return Err(MailEngineError::Rejected);
        }
        Ok(())
    }

    async fn resolve_reply_headers(
        &self,
        token: &SecretString,
        api_url: &Url,
        account_ref: &str,
        reply_to_email_id: &str,
    ) -> Result<ReplyHeaders, MailEngineError> {
        let response = self
            .client
            .post(api_url.clone())
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Email/get", {
                    "accountId": account_ref,
                    "ids": [reply_to_email_id],
                    "properties": [
                        "id", "messageId", "references",
                        "header:Auto-Submitted:asText"
                    ]
                }, "r"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1 {
            return Err(MailEngineError::InvalidResponse);
        }
        let result = parse_method_response(&envelope.method_responses[0], "Email/get", "r")?;
        let result: ReplySourceGetResponse =
            serde_json::from_value(result.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
        if result.account_id != account_ref
            || result.state.is_empty()
            || !result.not_found.is_empty()
            || result.list.len() != 1
            || result.list[0].id != reply_to_email_id
            || result.list[0].message_id.len() != 1
        {
            return Err(MailEngineError::Rejected);
        }
        let source = &result.list[0];
        let message_id = source.message_id[0].clone();
        if !valid_message_id(&message_id)
            || source.references.len() > 100
            || source
                .references
                .iter()
                .any(|value| !valid_message_id(value))
            || source
                .auto_submitted
                .as_deref()
                .is_some_and(|value| !valid_auto_submitted(value))
        {
            return Err(MailEngineError::Rejected);
        }
        let mut references = Vec::with_capacity(source.references.len() + 1);
        for value in source.references.iter().chain(std::iter::once(&message_id)) {
            if !references.contains(value) {
                references.push(value.clone());
            }
        }
        if references.iter().map(String::len).sum::<usize>() > 16 * 1024 {
            return Err(MailEngineError::Rejected);
        }
        Ok(ReplyHeaders {
            in_reply_to: vec![message_id],
            references,
            automatic: source
                .auto_submitted
                .as_deref()
                .is_some_and(|value| !value.eq_ignore_ascii_case("no")),
        })
    }

    async fn jmap_call(
        &self,
        method: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, MailEngineError> {
        let (token, _session, api_url) = self.authenticated_session().await?;
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, STALWART_JMAP],
                "methodCalls": [[method, arguments, "c1"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let value: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        let response = value
            .get("methodResponses")
            .and_then(serde_json::Value::as_array)
            .filter(|responses| responses.len() == 1)
            .and_then(|responses| responses.first())
            .and_then(serde_json::Value::as_array)
            .filter(|response| response.len() == 3)
            .ok_or(MailEngineError::InvalidResponse)?;
        if response[2].as_str() != Some("c1") {
            return Err(MailEngineError::InvalidResponse);
        }
        if response[0].as_str() == Some("error") {
            return Err(MailEngineError::Rejected);
        }
        if response[0].as_str() != Some(method) {
            return Err(MailEngineError::InvalidResponse);
        }
        Ok(response[1].clone())
    }

    async fn get_domain(&self, engine_ref: &str) -> Result<Option<EngineDomain>, MailEngineError> {
        validate_engine_ref(engine_ref)?;
        let arguments = self
            .jmap_call(
                "x:Domain/get",
                serde_json::json!({
                    "ids": [engine_ref],
                    "properties": ["id", "name", "isEnabled"]
                }),
            )
            .await?;
        let list = arguments
            .get("list")
            .and_then(serde_json::Value::as_array)
            .ok_or(MailEngineError::InvalidResponse)?;
        if list.len() > 1 {
            return Err(MailEngineError::InvalidResponse);
        }
        list.first()
            .map(|value| {
                serde_json::from_value(value.clone()).map_err(|_| MailEngineError::InvalidResponse)
            })
            .transpose()
    }
}

#[async_trait]
impl MailContentReader for StalwartJmapClient {
    async fn probe_content(&self) -> Result<(), MailEngineError> {
        let token = self.load_token().await?;
        let response = self
            .client
            .get(self.discovery_url.clone())
            .bearer_auth(token.expose_secret())
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_SESSION_BYTES).await?;
        let session: JmapSession =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidSession)?;
        if !session.capabilities.contains_key(JMAP_CORE)
            || !session.capabilities.contains_key(JMAP_MAIL)
            || !session
                .accounts
                .values()
                .any(|account| account.account_capabilities.contains_key(JMAP_MAIL))
        {
            return Err(MailEngineError::InvalidSession);
        }
        self.validate_api_url(&session.api_url)?;
        Ok(())
    }

    async fn list_messages(
        &self,
        account_ref: &str,
        position: u32,
        page_size: u32,
    ) -> Result<MailMessagePage, MailEngineError> {
        if !(1..=MAX_MAIL_PAGE_SIZE).contains(&page_size) || position > MAX_MAIL_PAGE_POSITION {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let (token, _session, api_url) = self.authenticated_mail_session(account_ref).await?;
        let inbox_id = self
            .resolve_inbox_mailbox(&token, &api_url, account_ref)
            .await?;
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [
                    ["Email/query", {
                        "accountId": account_ref,
                        "filter": {"inMailbox": inbox_id},
                        "position": position,
                        "limit": page_size,
                        "sort": [{"property": "receivedAt", "isAscending": false}]
                    }, "q"],
                    ["Email/get", {
                        "accountId": account_ref,
                        "#ids": {"resultOf": "q", "name": "Email/query", "path": "/ids"},
                        "properties": [
                            "id", "subject", "from", "receivedAt",
                            "preview", "keywords", "hasAttachment"
                        ]
                    }, "g"]
                ]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        parse_mail_message_page(response, account_ref, position, page_size).await
    }

    async fn search_messages(
        &self,
        account_ref: &str,
        text: &str,
        position: u32,
        page_size: u32,
    ) -> Result<MailMessagePage, MailEngineError> {
        if !valid_mail_search_text(text)
            || !(1..=MAX_MAIL_PAGE_SIZE).contains(&page_size)
            || position > MAX_MAIL_PAGE_POSITION
        {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let (token, _session, api_url) = self.authenticated_mail_session(account_ref).await?;
        let inbox_id = self
            .resolve_inbox_mailbox(&token, &api_url, account_ref)
            .await?;
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [
                    ["Email/query", {
                        "accountId": account_ref,
                        "filter": {
                            "operator": "AND",
                            "conditions": [
                                {"inMailbox": inbox_id},
                                {"text": text}
                            ]
                        },
                        "position": position,
                        "limit": page_size,
                        "sort": [{"property": "receivedAt", "isAscending": false}]
                    }, "q"],
                    ["Email/get", {
                        "accountId": account_ref,
                        "#ids": {"resultOf": "q", "name": "Email/query", "path": "/ids"},
                        "properties": [
                            "id", "subject", "from", "receivedAt",
                            "preview", "keywords", "hasAttachment"
                        ]
                    }, "g"]
                ]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        parse_mail_message_page(response, account_ref, position, page_size).await
    }

    async fn get_message(
        &self,
        account_ref: &str,
        message_id: &str,
    ) -> Result<Option<MailMessageDetail>, MailEngineError> {
        if !valid_opaque_ref(message_id, 512) {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let (token, _session, api_url) = self.authenticated_mail_session(account_ref).await?;
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Email/get", {
                    "accountId": account_ref,
                    "ids": [message_id],
                    "properties": [
                        "id", "subject", "from", "to", "cc",
                        "receivedAt", "preview", "keywords", "hasAttachment",
                        "textBody", "bodyValues", "attachments"
                    ],
                    "bodyProperties": ["partId", "type", "size", "name", "disposition"],
                    "fetchTextBodyValues": true,
                    "maxBodyValueBytes": 131072
                }, "d"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1
            || envelope.session_state.is_empty()
            || envelope.session_state.len() > 512
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let value = parse_method_response(&envelope.method_responses[0], "Email/get", "d")?;
        let get: EmailDetailGetResponse =
            serde_json::from_value(value.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
        if get.account_id != account_ref
            || get.state.is_empty()
            || get.state.len() > 512
            || get.list.len() > 1
            || get.not_found.len() > 1
            || !get.list.is_empty() && !get.not_found.is_empty()
        {
            return Err(MailEngineError::InvalidResponse);
        }
        if get.list.is_empty() {
            return if get.not_found.first().map(String::as_str) == Some(message_id) {
                Ok(None)
            } else {
                Err(MailEngineError::InvalidResponse)
            };
        }
        let detail = get
            .list
            .into_iter()
            .next()
            .ok_or(MailEngineError::InvalidResponse)?
            .normalize()?;
        if detail.summary.id != message_id {
            return Err(MailEngineError::InvalidResponse);
        }
        Ok(Some(detail))
    }

    async fn set_message_state(
        &self,
        account_ref: &str,
        message_id: &str,
        is_read: Option<bool>,
        is_flagged: Option<bool>,
    ) -> Result<MailMessageState, MailEngineError> {
        if !valid_opaque_ref(message_id, 512) || is_read.is_none() && is_flagged.is_none() {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let mut patch = serde_json::Map::new();
        if let Some(value) = is_read {
            patch.insert(
                "keywords/$seen".into(),
                if value {
                    serde_json::Value::Bool(true)
                } else {
                    serde_json::Value::Null
                },
            );
        }
        if let Some(value) = is_flagged {
            patch.insert(
                "keywords/$flagged".into(),
                if value {
                    serde_json::Value::Bool(true)
                } else {
                    serde_json::Value::Null
                },
            );
        }
        let update = BTreeMap::from([(message_id, serde_json::Value::Object(patch))]);
        let (token, _session, api_url) = self.authenticated_mail_session(account_ref).await?;
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [
                    ["Email/set", {
                        "accountId": account_ref,
                        "update": update
                    }, "s"],
                    ["Email/get", {
                        "accountId": account_ref,
                        "ids": [message_id],
                        "properties": ["id", "keywords"]
                    }, "k"]
                ]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 2 {
            return Err(MailEngineError::InvalidResponse);
        }
        let set = parse_method_response(&envelope.method_responses[0], "Email/set", "s")?;
        let set: EmailSetResponse =
            serde_json::from_value(set.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
        if set.account_id != account_ref
            || set.old_state.is_empty()
            || set.new_state.is_empty()
            || set.updated.len() != 1
            || !set.updated.contains_key(message_id)
            || !set.not_updated.is_empty()
        {
            return Err(MailEngineError::Rejected);
        }
        let get = parse_method_response(&envelope.method_responses[1], "Email/get", "k")?;
        let get: EmailStateGetResponse =
            serde_json::from_value(get.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
        if get.account_id != account_ref
            || get.state.is_empty()
            || get.list.len() != 1
            || !get.not_found.is_empty()
            || get.list[0].id != message_id
        {
            return Err(MailEngineError::InvalidResponse);
        }
        Ok(MailMessageState {
            is_read: get.list[0].keywords.get("$seen").copied().unwrap_or(false),
            is_flagged: get.list[0]
                .keywords
                .get("$flagged")
                .copied()
                .unwrap_or(false),
        })
    }
}

#[async_trait]
impl MailDraftContentReader for StalwartJmapClient {
    async fn read_exact_text_draft(
        &self,
        account_ref: &str,
        email_ref: &str,
        expected_thread_ref: &str,
        expected_from: &MailboxAddress,
    ) -> Result<Option<MailDraftContent>, MailEngineError> {
        if !valid_opaque_ref(email_ref, 512) || !valid_opaque_ref(expected_thread_ref, 512) {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let (token, _session, api_url) = self.authenticated_mail_session(account_ref).await?;
        let drafts_mailbox_ref = self
            .resolve_readable_drafts_mailbox(&token, &api_url, account_ref)
            .await?;
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Email/get", {
                    "accountId": account_ref,
                    "ids": [email_ref],
                    "properties": [
                        "id", "threadId", "mailboxIds", "keywords", "from",
                        "to", "cc", "bcc", "subject", "hasAttachment",
                        "textBody", "htmlBody", "bodyValues", "attachments"
                    ],
                    "bodyProperties": ["partId", "type", "size", "name", "disposition"],
                    "fetchTextBodyValues": true,
                    "fetchHTMLBodyValues": false,
                    "maxBodyValueBytes": MAX_TEXT_BODY_BYTES
                }, "x"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        parse_exact_draft_content(
            &body,
            account_ref,
            email_ref,
            expected_thread_ref,
            expected_from,
            &drafts_mailbox_ref,
        )
    }
}

#[async_trait]
impl MailDraftStore for StalwartJmapClient {
    async fn probe_drafts(&self) -> Result<(), MailEngineError> {
        let token = self.load_token().await?;
        let response = self
            .client
            .get(self.discovery_url.clone())
            .bearer_auth(token.expose_secret())
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_SESSION_BYTES).await?;
        let session: JmapSession =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidSession)?;
        let account_ref = session
            .accounts
            .iter()
            .find(|(_, account)| account.account_capabilities.contains_key(JMAP_MAIL))
            .map(|(id, _)| id.as_str())
            .ok_or(MailEngineError::InvalidSession)?;
        if !session.capabilities.contains_key(JMAP_CORE)
            || !session.capabilities.contains_key(JMAP_MAIL)
        {
            return Err(MailEngineError::InvalidSession);
        }
        let api_url = self.validate_api_url(&session.api_url)?;
        self.resolve_drafts_mailbox(&token, &api_url, account_ref)
            .await?;
        Ok(())
    }

    async fn save_text_draft(
        &self,
        account_ref: &str,
        previous_draft_id: Option<&str>,
        input: &TextDraftInput,
    ) -> Result<MailDraft, MailEngineError> {
        let input = normalize_draft_input(input)?;
        if previous_draft_id.is_some_and(|id| !valid_opaque_ref(id, 512)) {
            return Err(MailEngineError::InvalidConfiguration);
        }
        if input
            .reply_to_email_id
            .as_deref()
            .is_some_and(|id| !valid_opaque_ref(id, 512))
        {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let (token, _session, api_url) = self.authenticated_mail_session(account_ref).await?;
        let draft_mailbox_id = self
            .resolve_drafts_mailbox(&token, &api_url, account_ref)
            .await?;
        let reply_headers = if let Some(reply_to_email_id) = input.reply_to_email_id.as_deref() {
            Some(
                self.resolve_reply_headers(&token, &api_url, account_ref, reply_to_email_id)
                    .await?,
            )
        } else {
            None
        };
        if let Some(previous_draft_id) = previous_draft_id {
            self.validate_previous_draft(
                &token,
                &api_url,
                account_ref,
                &draft_mailbox_id,
                previous_draft_id,
                &input.from,
            )
            .await?;
        }
        let mailbox_ids = BTreeMap::from([(draft_mailbox_id, true)]);
        let mut create = serde_json::Map::new();
        let mut draft = serde_json::json!({
            "mailboxIds": mailbox_ids,
            "keywords": {"$draft": true, "$seen": true},
            "from": [{"email": input.from}],
            "to": input.to.into_iter().map(|email| serde_json::json!({"email": email})).collect::<Vec<_>>(),
            "cc": input.cc.into_iter().map(|email| serde_json::json!({"email": email})).collect::<Vec<_>>(),
            "bcc": input.bcc.into_iter().map(|email| serde_json::json!({"email": email})).collect::<Vec<_>>(),
            "subject": input.subject,
            "bodyStructure": {"type": "text/plain", "partId": "body"},
            "bodyValues": {"body": {"value": input.text_body, "isTruncated": false}}
        });
        if input.automatic {
            draft
                .as_object_mut()
                .ok_or(MailEngineError::InvalidConfiguration)?
                .insert(
                    "header:Auto-Submitted:asText".into(),
                    serde_json::json!(if input.reply_to_email_id.is_some() {
                        "auto-replied"
                    } else {
                        "auto-generated"
                    }),
                );
        }
        let reply_source_automatic = reply_headers
            .as_ref()
            .is_some_and(|headers| headers.automatic);
        if let Some(reply_headers) = reply_headers {
            let draft = draft
                .as_object_mut()
                .ok_or(MailEngineError::InvalidConfiguration)?;
            draft.insert(
                "inReplyTo".into(),
                serde_json::json!(reply_headers.in_reply_to),
            );
            draft.insert(
                "references".into(),
                serde_json::json!(reply_headers.references),
            );
        }
        create.insert("draft".into(), draft);
        let request = serde_json::json!({
            "accountId": account_ref,
            "create": create,
            "destroy": previous_draft_id.into_iter().collect::<Vec<_>>()
        });
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL],
                "methodCalls": [["Email/set", request, "d"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 1 {
            return Err(MailEngineError::InvalidResponse);
        }
        let result = parse_method_response(&envelope.method_responses[0], "Email/set", "d")?;
        let result: DraftSetResponse =
            serde_json::from_value(result.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
        if result.account_id != account_ref
            || result.old_state.is_empty()
            || result.new_state.is_empty()
            || !result.not_created.is_empty()
        {
            return Err(MailEngineError::Rejected);
        }
        let created = result
            .created
            .get("draft")
            .ok_or(MailEngineError::InvalidResponse)?;
        if !valid_opaque_ref(&created.id, 512) || !valid_opaque_ref(&created.thread_id, 512) {
            return Err(MailEngineError::InvalidResponse);
        }
        let previous_draft_destroyed = previous_draft_id.is_none_or(|previous| {
            result
                .destroyed
                .iter()
                .any(|destroyed| destroyed == previous)
                && !result.not_destroyed.contains_key(previous)
        });
        Ok(MailDraft {
            id: created.id.clone(),
            thread_id: created.thread_id.clone(),
            reply_source_automatic,
            previous_draft_destroyed,
        })
    }
}

#[async_trait]
impl MailSubmissionAdapter for StalwartJmapClient {
    async fn probe_submissions(&self) -> Result<(), MailEngineError> {
        let token = self.load_token().await?;
        let response = self
            .client
            .get(self.discovery_url.clone())
            .bearer_auth(token.expose_secret())
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_SESSION_BYTES).await?;
        let session: JmapSession =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidSession)?;
        let account_ref = session
            .accounts
            .iter()
            .find(|(_, account)| {
                account.account_capabilities.contains_key(JMAP_MAIL)
                    && account.account_capabilities.contains_key(JMAP_SUBMISSION)
            })
            .map(|(id, _)| id.as_str())
            .ok_or(MailEngineError::InvalidSession)?;
        if !session.capabilities.contains_key(JMAP_CORE)
            || !session.capabilities.contains_key(JMAP_MAIL)
            || !session.capabilities.contains_key(JMAP_SUBMISSION)
        {
            return Err(MailEngineError::InvalidSession);
        }
        let api_url = self.validate_api_url(&session.api_url)?;
        let value = self
            .submission_call(
                &token,
                &api_url,
                "Identity/get",
                serde_json::json!({
                    "accountId": account_ref,
                    "ids": null,
                    "properties": ["id", "email"]
                }),
                "i",
            )
            .await?;
        let identities: IdentityGetResponse =
            serde_json::from_value(value).map_err(|_| MailEngineError::InvalidResponse)?;
        if identities.account_id != account_ref
            || identities.state.is_empty()
            || identities.list.is_empty()
            || identities.list.len() > 10_000
            || !identities.not_found.is_empty()
            || identities.list.iter().any(|identity| {
                !valid_opaque_ref(&identity.id, 512)
                    || MailboxAddress::parse(&identity.email).is_err()
            })
        {
            return Err(MailEngineError::InvalidResponse);
        }
        Ok(())
    }

    async fn reconcile_submission(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        expected_from: &str,
    ) -> Result<MailSubmissionReconciliation, MailEngineError> {
        if !valid_opaque_ref(engine_email_ref, 512) {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let expected_from = MailboxAddress::parse(expected_from)
            .map_err(|_| MailEngineError::InvalidConfiguration)?;
        let (token, _session, api_url) = self.authenticated_submission_session(account_ref).await?;
        let identity_ref = self
            .resolve_submission_identity(&token, &api_url, account_ref, &expected_from)
            .await?;
        let ids = self
            .query_submission_ids(&token, &api_url, account_ref, engine_email_ref)
            .await?;
        let Some(submission_ref) = ids.into_iter().next() else {
            return Ok(MailSubmissionReconciliation::NotFound { identity_ref });
        };
        let submission = self
            .get_submission(&token, &api_url, account_ref, &submission_ref)
            .await?;
        if submission.email_id != engine_email_ref || submission.identity_id != identity_ref {
            return Err(MailEngineError::ProjectionConflict);
        }
        Ok(MailSubmissionReconciliation::Accepted {
            identity_ref,
            submission_ref,
        })
    }

    async fn create_submission(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        identity_ref: &str,
    ) -> Result<CreatedMailSubmission, MailEngineError> {
        if !valid_opaque_ref(engine_email_ref, 512) || !valid_opaque_ref(identity_ref, 512) {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let (token, _session, api_url) = self.authenticated_submission_session(account_ref).await?;
        let (drafts_mailbox_ref, sent_mailbox_ref) = self
            .resolve_submission_mailboxes(&token, &api_url, account_ref)
            .await?;
        let success_patch = BTreeMap::from([
            (
                format!("mailboxIds/{}", escape_patch_segment(&drafts_mailbox_ref)),
                serde_json::Value::Null,
            ),
            (
                format!("mailboxIds/{}", escape_patch_segment(&sent_mailbox_ref)),
                serde_json::Value::Bool(true),
            ),
            ("keywords/$draft".into(), serde_json::Value::Null),
        ]);
        let response = self
            .client
            .post(api_url)
            .bearer_auth(token.expose_secret())
            .json(&serde_json::json!({
                "using": [JMAP_CORE, JMAP_MAIL, JMAP_SUBMISSION],
                "methodCalls": [["EmailSubmission/set", {
                    "accountId": account_ref,
                    "create": {
                        "submission": {
                            "identityId": identity_ref,
                            "emailId": engine_email_ref
                        }
                    },
                    "onSuccessUpdateEmail": {
                        "#submission": success_patch
                    }
                }, "s"]]
            }))
            .send()
            .await
            .map_err(|_| MailEngineError::Unavailable)?;
        let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
        let envelope: MailMethodEnvelope =
            serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
        if envelope.method_responses.len() != 2 || envelope.session_state.is_empty() {
            return Err(MailEngineError::InvalidResponse);
        }
        let value =
            parse_method_response(&envelope.method_responses[0], "EmailSubmission/set", "s")?;
        let response: SubmissionSetResponse =
            serde_json::from_value(value.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
        if response.account_id != account_ref
            || response.old_state.is_empty()
            || response.new_state.is_empty()
            || !response.not_created.is_empty()
            || response.created.len() != 1
        {
            return Err(MailEngineError::Rejected);
        }
        let created = response
            .created
            .get("submission")
            .ok_or(MailEngineError::InvalidResponse)?;
        if !valid_opaque_ref(&created.id, 512) {
            return Err(MailEngineError::InvalidResponse);
        }
        let implicit =
            parse_implicit_method_response(&envelope.method_responses[1], "Email/set", "s")?;
        let implicit: EmailSetResponse = serde_json::from_value(implicit.clone())
            .map_err(|_| MailEngineError::InvalidResponse)?;
        if implicit.account_id != account_ref
            || implicit.old_state.is_empty()
            || implicit.new_state.is_empty()
            || implicit.updated.len() != 1
            || !implicit.updated.contains_key(engine_email_ref)
            || !implicit.not_updated.is_empty()
        {
            return Err(MailEngineError::InvalidResponse);
        }
        Ok(CreatedMailSubmission {
            identity_ref: identity_ref.to_owned(),
            submission_ref: created.id.clone(),
        })
    }

    async fn get_delivery_snapshot(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        identity_ref: &str,
        submission_ref: &str,
    ) -> Result<MailDeliverySnapshot, MailEngineError> {
        if !valid_opaque_ref(engine_email_ref, 512)
            || !valid_opaque_ref(identity_ref, 512)
            || !valid_opaque_ref(submission_ref, 512)
        {
            return Err(MailEngineError::InvalidConfiguration);
        }
        let (token, _session, api_url) = self.authenticated_submission_session(account_ref).await?;
        let submission = self
            .get_submission(&token, &api_url, account_ref, submission_ref)
            .await?;
        let source_state = submission.source_state.clone();
        if submission.email_id != engine_email_ref || submission.identity_id != identity_ref {
            return Err(MailEngineError::ProjectionConflict);
        }
        let recipients = submission.normalize_delivery_status()?;
        Ok(MailDeliverySnapshot {
            submission_ref: submission_ref.to_owned(),
            source_state,
            recipients,
        })
    }
}

fn normalize_draft_input(input: &TextDraftInput) -> Result<TextDraftInput, MailEngineError> {
    if input.subject.len() > 998
        || input.subject.chars().any(char::is_control)
        || input.text_body.len() > MAX_TEXT_BODY_BYTES
        || input.text_body.contains('\0')
        || input.to.len() + input.cc.len() + input.bcc.len() == 0
        || input.to.len() + input.cc.len() + input.bcc.len() > 100
    {
        return Err(MailEngineError::InvalidConfiguration);
    }
    let normalize = |values: &[String]| {
        values
            .iter()
            .map(|value| {
                MailboxAddress::parse(value)
                    .map(|address| address.to_string())
                    .map_err(|_| MailEngineError::InvalidConfiguration)
            })
            .collect::<Result<Vec<_>, _>>()
    };
    Ok(TextDraftInput {
        from: MailboxAddress::parse(&input.from)
            .map_err(|_| MailEngineError::InvalidConfiguration)?
            .to_string(),
        to: normalize(&input.to)?,
        cc: normalize(&input.cc)?,
        bcc: normalize(&input.bcc)?,
        subject: input.subject.clone(),
        text_body: input.text_body.clone(),
        reply_to_email_id: input.reply_to_email_id.clone(),
        automatic: input.automatic,
    })
}

fn valid_message_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 998
        && value.is_ascii()
        && value.contains('@')
        && !value.bytes().any(|byte| {
            byte.is_ascii_whitespace() || byte.is_ascii_control() || b"<>,".contains(&byte)
        })
}

fn valid_auto_submitted(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value == value.trim()
        && value
            .bytes()
            .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
}

#[async_trait]
impl MailEngine for StalwartJmapClient {
    async fn probe(&self) -> Result<MailEngineReadiness, MailEngineError> {
        let (_token, session, api_url) = self.authenticated_session().await?;
        Ok(MailEngineReadiness {
            username: session.username,
            session_state: session.state,
            api_url,
            mail_capability_advertised: session.capabilities.contains_key(JMAP_MAIL),
        })
    }
}

#[async_trait]
impl MailDomainProjector for StalwartJmapClient {
    async fn reconcile_domain(
        &self,
        name: &MailDomainName,
        current_engine_ref: Option<&str>,
        enabled: bool,
    ) -> Result<MailEngineDomainProjection, MailEngineError> {
        let mut created = false;
        let domain = if let Some(engine_ref) = current_engine_ref {
            self.get_domain(engine_ref)
                .await?
                .ok_or(MailEngineError::ProjectionConflict)?
        } else {
            let query = self
                .jmap_call(
                    "x:Domain/query",
                    serde_json::json!({"filter": {"name": name.as_str()}}),
                )
                .await?;
            let ids = query
                .get("ids")
                .and_then(serde_json::Value::as_array)
                .ok_or(MailEngineError::InvalidResponse)?;
            if ids.len() > 64 {
                return Err(MailEngineError::InvalidResponse);
            }
            let mut exact = Vec::new();
            for engine_ref in ids {
                let engine_ref = engine_ref
                    .as_str()
                    .ok_or(MailEngineError::InvalidResponse)?;
                if let Some(domain) = self.get_domain(engine_ref).await?
                    && domain.name == name.as_str()
                {
                    exact.push(domain);
                }
            }
            match exact.len() {
                0 => {
                    let response = self
                        .jmap_call(
                            "x:Domain/set",
                            serde_json::json!({
                                "create": {
                                    "copilotmail": {
                                        "name": name.as_str(),
                                        "aliases": {},
                                        "isEnabled": enabled,
                                        "certificateManagement": {"@type": "Manual"},
                                        "dkimManagement": {"@type": "Automatic"},
                                        "dnsManagement": {"@type": "Manual"},
                                        "subAddressing": {"@type": "Enabled"},
                                        "allowRelaying": false
                                    }
                                }
                            }),
                        )
                        .await?;
                    reject_set_failures(&response)?;
                    let engine_ref = response
                        .pointer("/created/copilotmail/id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or(MailEngineError::InvalidResponse)?;
                    validate_engine_ref(engine_ref)?;
                    created = true;
                    EngineDomain {
                        id: engine_ref.to_owned(),
                        name: name.to_string(),
                        is_enabled: enabled,
                    }
                }
                1 => exact.pop().expect("length checked"),
                _ => return Err(MailEngineError::ProjectionConflict),
            }
        };
        if domain.name != name.as_str() {
            return Err(MailEngineError::ProjectionConflict);
        }
        if !created && domain.is_enabled != enabled {
            let response = self
                .jmap_call(
                    "x:Domain/set",
                    serde_json::json!({
                        "update": {domain.id.clone(): {"isEnabled": enabled}}
                    }),
                )
                .await?;
            reject_set_failures(&response)?;
            if response
                .get("updated")
                .and_then(|value| value.get(&domain.id))
                .is_none()
            {
                return Err(MailEngineError::InvalidResponse);
            }
        }
        Ok(MailEngineDomainProjection {
            engine_ref: domain.id,
            name: domain.name,
            enabled,
            created,
        })
    }
}

async fn bounded_response(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, MailEngineError> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > max_bytes as u64)
    {
        return Err(MailEngineError::Unavailable);
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| MailEngineError::Unavailable)?;
    if body.len() > max_bytes {
        return Err(MailEngineError::InvalidResponse);
    }
    Ok(body.to_vec())
}

fn select_readable_drafts_mailbox(
    response: MailboxGetResponse,
    account_ref: &str,
) -> Result<String, MailEngineError> {
    if response.account_id != account_ref {
        return Err(MailEngineError::ProjectionConflict);
    }
    if response.state.is_empty()
        || response.state.len() > 512
        || !response.not_found.is_empty()
        || response.list.len() > 10_000
    {
        return Err(MailEngineError::InvalidResponse);
    }
    let mut drafts = response
        .list
        .into_iter()
        .filter(|mailbox| mailbox.role.as_deref() == Some("drafts"))
        .collect::<Vec<_>>();
    if drafts.len() != 1 {
        return Err(MailEngineError::ProjectionConflict);
    }
    let draft = drafts.pop().expect("length checked");
    if !valid_opaque_ref(&draft.id, 512) {
        return Err(MailEngineError::InvalidResponse);
    }
    if !draft.my_rights.may_read_items {
        return Err(MailEngineError::Rejected);
    }
    Ok(draft.id)
}

fn parse_exact_draft_content(
    body: &[u8],
    account_ref: &str,
    email_ref: &str,
    expected_thread_ref: &str,
    expected_from: &MailboxAddress,
    drafts_mailbox_ref: &str,
) -> Result<Option<MailDraftContent>, MailEngineError> {
    let envelope: MailMethodEnvelope =
        serde_json::from_slice(body).map_err(|_| MailEngineError::InvalidResponse)?;
    if envelope.method_responses.len() != 1
        || envelope.session_state.is_empty()
        || envelope.session_state.len() > 512
    {
        return Err(MailEngineError::InvalidResponse);
    }
    let value = parse_method_response(&envelope.method_responses[0], "Email/get", "x")?;
    let response: DraftContentGetResponse =
        serde_json::from_value(value.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
    if response.account_id != account_ref {
        return Err(MailEngineError::ProjectionConflict);
    }
    if response.state.is_empty()
        || response.state.len() > 512
        || response.list.len() > 1
        || response.not_found.len() > 1
        || !response.list.is_empty() && !response.not_found.is_empty()
    {
        return Err(MailEngineError::InvalidResponse);
    }
    if response.list.is_empty() {
        return if response.not_found.first().map(String::as_str) == Some(email_ref) {
            Ok(None)
        } else {
            Err(MailEngineError::InvalidResponse)
        };
    }
    response
        .list
        .into_iter()
        .next()
        .expect("length checked")
        .normalize(
            email_ref,
            expected_thread_ref,
            expected_from,
            drafts_mailbox_ref,
        )
        .map(Some)
}

async fn parse_mail_message_page(
    response: reqwest::Response,
    account_ref: &str,
    position: u32,
    page_size: u32,
) -> Result<MailMessagePage, MailEngineError> {
    let body = bounded_response(response, MAX_JMAP_RESPONSE_BYTES).await?;
    let envelope: MailMethodEnvelope =
        serde_json::from_slice(&body).map_err(|_| MailEngineError::InvalidResponse)?;
    if envelope.method_responses.len() != 2
        || envelope.session_state.is_empty()
        || envelope.session_state.len() > 512
    {
        return Err(MailEngineError::InvalidResponse);
    }
    let query = parse_method_response(&envelope.method_responses[0], "Email/query", "q")?;
    let get = parse_method_response(&envelope.method_responses[1], "Email/get", "g")?;
    let query: EmailQueryResponse =
        serde_json::from_value(query.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
    let get: EmailGetResponse =
        serde_json::from_value(get.clone()).map_err(|_| MailEngineError::InvalidResponse)?;
    if query.account_id != account_ref
        || get.account_id != account_ref
        || query.position != position
        || query.query_state.is_empty()
        || query.query_state.len() > 512
        || get.state.is_empty()
        || get.state.len() > 512
        || query.ids.len() > page_size as usize
        || get.list.len() != query.ids.len()
        || !get.not_found.is_empty()
    {
        return Err(MailEngineError::InvalidResponse);
    }
    let mut by_id = get
        .list
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let mut items = Vec::with_capacity(query.ids.len());
    for id in query.ids {
        let item = by_id.remove(&id).ok_or(MailEngineError::InvalidResponse)?;
        items.push(item.normalize()?);
    }
    let consumed = position
        .checked_add(items.len() as u32)
        .ok_or(MailEngineError::InvalidResponse)?;
    let next_position = match query.total {
        Some(total) if consumed < total => Some(consumed),
        Some(_) => None,
        None if items.len() == page_size as usize => Some(consumed),
        None => None,
    };
    Ok(MailMessagePage {
        query_state: query.query_state,
        items,
        next_position,
    })
}

fn validate_engine_ref(value: &str) -> Result<(), MailEngineError> {
    if value.is_empty()
        || value.len() > 512
        || value.chars().any(char::is_control)
        || value.contains('/')
    {
        return Err(MailEngineError::InvalidResponse);
    }
    Ok(())
}

fn reject_set_failures(arguments: &serde_json::Value) -> Result<(), MailEngineError> {
    for key in ["notCreated", "notUpdated", "notDestroyed"] {
        if arguments
            .get(key)
            .and_then(serde_json::Value::as_object)
            .is_some_and(|failures| !failures.is_empty())
        {
            return Err(MailEngineError::Rejected);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EngineDomain {
    id: String,
    name: String,
    is_enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JmapSession {
    capabilities: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    accounts: BTreeMap<String, JmapAccount>,
    api_url: String,
    username: String,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JmapAccount {
    account_capabilities: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailMethodEnvelope {
    method_responses: Vec<Vec<serde_json::Value>>,
    session_state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailQueryResponse {
    account_id: String,
    query_state: String,
    #[serde(rename = "canCalculateChanges")]
    _can_calculate_changes: bool,
    ids: Vec<String>,
    total: Option<u32>,
    position: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailGetResponse {
    account_id: String,
    list: Vec<EmailSummaryWire>,
    #[serde(default)]
    not_found: Vec<String>,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailDetailGetResponse {
    account_id: String,
    list: Vec<EmailDetailWire>,
    #[serde(default)]
    not_found: Vec<String>,
    state: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailSetResponse {
    account_id: String,
    old_state: String,
    new_state: String,
    #[serde(default)]
    updated: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    not_updated: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailStateGetResponse {
    account_id: String,
    state: String,
    list: Vec<EmailStateWire>,
    #[serde(default)]
    not_found: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailStateWire {
    id: String,
    #[serde(default)]
    keywords: BTreeMap<String, bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailAddressWire {
    name: Option<String>,
    email: String,
}

impl From<MailAddressWire> for MailAddress {
    fn from(value: MailAddressWire) -> Self {
        Self {
            name: value.name,
            email: value.email,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviousDraftGetResponse {
    account_id: String,
    state: String,
    list: Vec<PreviousDraftWire>,
    #[serde(default)]
    not_found: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviousDraftWire {
    id: String,
    #[serde(default)]
    mailbox_ids: BTreeMap<String, bool>,
    #[serde(default)]
    keywords: BTreeMap<String, bool>,
    #[serde(default)]
    from: Vec<MailAddressWire>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftContentGetResponse {
    account_id: String,
    state: String,
    list: Vec<DraftContentWire>,
    #[serde(default)]
    not_found: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftContentWire {
    id: String,
    thread_id: String,
    mailbox_ids: BTreeMap<String, bool>,
    keywords: BTreeMap<String, bool>,
    from: Vec<MailAddressWire>,
    to: Vec<MailAddressWire>,
    cc: Vec<MailAddressWire>,
    bcc: Vec<MailAddressWire>,
    subject: String,
    has_attachment: bool,
    text_body: Vec<EmailBodyPartWire>,
    html_body: Vec<EmailBodyPartWire>,
    body_values: BTreeMap<String, EmailBodyValueWire>,
    attachments: Vec<EmailBodyPartWire>,
}

impl DraftContentWire {
    fn normalize(
        self,
        expected_email_ref: &str,
        expected_thread_ref: &str,
        expected_from: &MailboxAddress,
        drafts_mailbox_ref: &str,
    ) -> Result<MailDraftContent, MailEngineError> {
        if !valid_opaque_ref(&self.id, 512)
            || !valid_opaque_ref(&self.thread_id, 512)
            || self
                .mailbox_ids
                .iter()
                .any(|(id, member)| !valid_opaque_ref(id, 512) || !member)
        {
            return Err(MailEngineError::InvalidResponse);
        }
        if self.id != expected_email_ref
            || self.thread_id != expected_thread_ref
            || self.mailbox_ids.len() != 1
            || self.mailbox_ids.get(drafts_mailbox_ref) != Some(&true)
            || self.keywords.get("$draft") != Some(&true)
        {
            return Err(MailEngineError::ProjectionConflict);
        }
        if self.from.len() != 1 {
            return Err(MailEngineError::ProjectionConflict);
        }
        let from = normalize_draft_address(self.from.into_iter().next().expect("length checked"))?;
        if from != *expected_from {
            return Err(MailEngineError::ProjectionConflict);
        }
        if self.subject.len() > 998 || self.subject.chars().any(char::is_control) {
            return Err(MailEngineError::InvalidResponse);
        }

        let to = normalize_draft_addresses(self.to)?;
        let cc = normalize_draft_addresses(self.cc)?;
        let bcc = normalize_draft_addresses(self.bcc)?;
        let recipient_count = to.len() + cc.len() + bcc.len();
        if !(1..=100).contains(&recipient_count) {
            return Err(MailEngineError::InvalidResponse);
        }
        let mut recipients = BTreeSet::new();
        if to
            .iter()
            .chain(&cc)
            .chain(&bcc)
            .any(|address| !recipients.insert(address))
        {
            return Err(MailEngineError::InvalidResponse);
        }

        if self.has_attachment
            || !self.attachments.is_empty()
            || !self.html_body.is_empty()
            || self.text_body.len() != 1
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let part = self.text_body.into_iter().next().expect("length checked");
        let part_id = part
            .part_id
            .filter(|value| valid_opaque_ref(value, 512))
            .ok_or(MailEngineError::InvalidResponse)?;
        if !part.media_type.eq_ignore_ascii_case("text/plain")
            || part.size > MAX_TEXT_BODY_BYTES as u64
            || part.name.is_some()
            || part.disposition.is_some()
            || self.body_values.len() != 1
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let body = self
            .body_values
            .get(&part_id)
            .ok_or(MailEngineError::InvalidResponse)?;
        if body.is_encoding_problem
            || body.is_truncated
            || body.value.len() > MAX_TEXT_BODY_BYTES
            || body.value.contains('\0')
            || part.size != body.value.len() as u64
        {
            return Err(MailEngineError::InvalidResponse);
        }
        Ok(MailDraftContent {
            to,
            cc,
            bcc,
            subject: self.subject,
            text_body: body.value.clone(),
        })
    }
}

fn normalize_draft_addresses(
    addresses: Vec<MailAddressWire>,
) -> Result<Vec<String>, MailEngineError> {
    addresses
        .into_iter()
        .map(normalize_draft_address)
        .map(|result| result.map(|address| address.to_string()))
        .collect()
}

fn normalize_draft_address(address: MailAddressWire) -> Result<MailboxAddress, MailEngineError> {
    if address
        .name
        .as_deref()
        .is_some_and(|name| name.len() > 512 || name.chars().any(char::is_control))
    {
        return Err(MailEngineError::InvalidResponse);
    }
    MailboxAddress::parse(address.email).map_err(|_| MailEngineError::InvalidResponse)
}

#[derive(Debug)]
struct ReplyHeaders {
    in_reply_to: Vec<String>,
    references: Vec<String>,
    automatic: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplySourceGetResponse {
    account_id: String,
    state: String,
    list: Vec<ReplySourceWire>,
    #[serde(default)]
    not_found: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplySourceWire {
    id: String,
    #[serde(default)]
    message_id: Vec<String>,
    #[serde(default)]
    references: Vec<String>,
    #[serde(rename = "header:Auto-Submitted:asText")]
    auto_submitted: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailboxGetResponse {
    account_id: String,
    state: String,
    list: Vec<MailboxWire>,
    #[serde(default)]
    not_found: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailboxWire {
    id: String,
    role: Option<String>,
    my_rights: MailboxRightsWire,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MailboxRightsWire {
    #[serde(default)]
    may_read_items: bool,
    #[serde(default)]
    may_add_items: bool,
    #[serde(default)]
    may_remove_items: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftSetResponse {
    account_id: String,
    old_state: String,
    new_state: String,
    #[serde(default)]
    created: BTreeMap<String, DraftCreatedWire>,
    #[serde(default)]
    not_created: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    destroyed: Vec<String>,
    #[serde(default)]
    not_destroyed: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DraftCreatedWire {
    id: String,
    thread_id: String,
    #[serde(rename = "blobId")]
    _blob_id: String,
    #[serde(rename = "size")]
    _size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdentityGetResponse {
    account_id: String,
    state: String,
    list: Vec<IdentityWire>,
    #[serde(default)]
    not_found: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IdentityWire {
    id: String,
    email: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionQueryResponse {
    account_id: String,
    query_state: String,
    #[serde(rename = "canCalculateChanges")]
    _can_calculate_changes: bool,
    ids: Vec<String>,
    total: Option<u32>,
    position: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionGetResponse {
    account_id: String,
    state: String,
    list: Vec<SubmissionWire>,
    #[serde(default)]
    not_found: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionWire {
    id: String,
    identity_id: String,
    email_id: String,
    #[serde(skip)]
    source_state: String,
    #[serde(default)]
    delivery_status: BTreeMap<String, SubmissionDeliveryWire>,
}

impl SubmissionWire {
    fn normalize_delivery_status(self) -> Result<Vec<RecipientDeliveryEvidence>, MailEngineError> {
        if self.delivery_status.is_empty() || self.delivery_status.len() > 100 {
            return Err(MailEngineError::InvalidResponse);
        }
        let mut recipients = Vec::with_capacity(self.delivery_status.len());
        for (raw_recipient, delivery) in self.delivery_status {
            let recipient = MailboxAddress::parse(raw_recipient)
                .map_err(|_| MailEngineError::InvalidResponse)?;
            if recipients
                .iter()
                .any(|existing: &RecipientDeliveryEvidence| existing.recipient == recipient)
                || delivery.smtp_reply.is_empty()
                || delivery.smtp_reply.len() > 1024
                || delivery.smtp_reply.chars().any(char::is_control)
            {
                return Err(MailEngineError::InvalidResponse);
            }
            recipients.push(RecipientDeliveryEvidence {
                recipient,
                state: delivery.delivered.into(),
                smtp_reply: delivery.smtp_reply,
            });
        }
        recipients.sort_by(|left, right| left.recipient.as_str().cmp(right.recipient.as_str()));
        Ok(recipients)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionDeliveryWire {
    delivered: SubmissionDeliveredWire,
    smtp_reply: String,
    #[serde(rename = "displayed")]
    _displayed: SubmissionDisplayedWire,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SubmissionDeliveredWire {
    Queued,
    Yes,
    No,
    Unknown,
}

impl From<SubmissionDeliveredWire> for RecipientDeliveryState {
    fn from(value: SubmissionDeliveredWire) -> Self {
        match value {
            SubmissionDeliveredWire::Queued => Self::Queued,
            SubmissionDeliveredWire::Yes => Self::Delivered,
            SubmissionDeliveredWire::No => Self::PermanentFailure,
            SubmissionDeliveredWire::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SubmissionDisplayedWire {
    Yes,
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionSetResponse {
    account_id: String,
    old_state: String,
    new_state: String,
    #[serde(default)]
    created: BTreeMap<String, SubmissionCreatedWire>,
    #[serde(default)]
    not_created: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionCreatedWire {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailSummaryWire {
    id: String,
    subject: Option<String>,
    #[serde(default)]
    from: Vec<MailAddressWire>,
    received_at: String,
    preview: String,
    #[serde(default)]
    keywords: BTreeMap<String, bool>,
    has_attachment: bool,
}

impl EmailSummaryWire {
    fn normalize(self) -> Result<MailMessageSummary, MailEngineError> {
        if !valid_opaque_ref(&self.id, 512)
            || self
                .subject
                .as_deref()
                .is_some_and(|value| value.len() > 998)
            || self.preview.len() > 4096
            || self.from.len() > 128
            || self.from.iter().any(|address| {
                address.email.is_empty()
                    || address.email.len() > 320
                    || address.name.as_deref().is_some_and(|name| name.len() > 512)
            })
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let received_at = DateTime::parse_from_rfc3339(&self.received_at)
            .map_err(|_| MailEngineError::InvalidResponse)?
            .with_timezone(&Utc);
        Ok(MailMessageSummary {
            id: self.id,
            subject: self.subject.unwrap_or_default(),
            from: self.from.into_iter().map(MailAddress::from).collect(),
            received_at,
            preview: self.preview,
            is_read: self.keywords.get("$seen").copied().unwrap_or(false),
            is_flagged: self.keywords.get("$flagged").copied().unwrap_or(false),
            has_attachment: self.has_attachment,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailDetailWire {
    id: String,
    subject: Option<String>,
    #[serde(default)]
    from: Vec<MailAddressWire>,
    #[serde(default)]
    to: Vec<MailAddressWire>,
    #[serde(default)]
    cc: Vec<MailAddressWire>,
    received_at: String,
    preview: String,
    #[serde(default)]
    keywords: BTreeMap<String, bool>,
    has_attachment: bool,
    #[serde(default)]
    text_body: Vec<EmailBodyPartWire>,
    #[serde(default)]
    body_values: BTreeMap<String, EmailBodyValueWire>,
    #[serde(default)]
    attachments: Vec<EmailBodyPartWire>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailBodyPartWire {
    part_id: Option<String>,
    #[serde(rename = "type")]
    media_type: String,
    size: u64,
    name: Option<String>,
    disposition: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EmailBodyValueWire {
    value: String,
    is_encoding_problem: bool,
    is_truncated: bool,
}

impl EmailDetailWire {
    fn normalize(self) -> Result<MailMessageDetail, MailEngineError> {
        let summary = EmailSummaryWire {
            id: self.id,
            subject: self.subject,
            from: self.from,
            received_at: self.received_at,
            preview: self.preview,
            keywords: self.keywords,
            has_attachment: self.has_attachment,
        }
        .normalize()?;
        if self.to.len() > 256
            || self.cc.len() > 256
            || !valid_addresses(&self.to)
            || !valid_addresses(&self.cc)
            || self.text_body.len() > 32
            || self.attachments.len() > 256
        {
            return Err(MailEngineError::InvalidResponse);
        }
        let mut text_body = String::new();
        let mut text_body_truncated = false;
        for part in self.text_body {
            if !part.media_type.eq_ignore_ascii_case("text/plain")
                || part.size > MAX_TEXT_BODY_BYTES as u64
            {
                return Err(MailEngineError::InvalidResponse);
            }
            let part_id = part
                .part_id
                .as_deref()
                .filter(|value| valid_opaque_ref(value, 512))
                .ok_or(MailEngineError::InvalidResponse)?;
            let value = self
                .body_values
                .get(part_id)
                .ok_or(MailEngineError::InvalidResponse)?;
            if value.is_encoding_problem {
                return Err(MailEngineError::InvalidResponse);
            }
            if !text_body.is_empty() {
                text_body.push_str("\n\n");
            }
            text_body.push_str(&value.value);
            text_body_truncated |= value.is_truncated;
            if text_body.len() > MAX_TEXT_BODY_BYTES {
                return Err(MailEngineError::InvalidResponse);
            }
        }
        let attachments = self
            .attachments
            .into_iter()
            .map(|part| {
                if part.media_type.is_empty()
                    || part.media_type.len() > 255
                    || part.name.as_deref().is_some_and(|name| name.len() > 1024)
                    || part
                        .disposition
                        .as_deref()
                        .is_some_and(|value| value.len() > 64)
                {
                    return Err(MailEngineError::InvalidResponse);
                }
                Ok(MailAttachmentSummary {
                    name: part.name,
                    media_type: part.media_type,
                    size_bytes: part.size,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(MailMessageDetail {
            summary,
            to: self.to.into_iter().map(MailAddress::from).collect(),
            cc: self.cc.into_iter().map(MailAddress::from).collect(),
            text_body,
            text_body_truncated,
            attachments,
        })
    }
}

fn valid_addresses(addresses: &[MailAddressWire]) -> bool {
    addresses.iter().all(|address| {
        !address.email.is_empty()
            && address.email.len() <= 320
            && address.name.as_deref().is_none_or(|name| name.len() <= 512)
    })
}

fn escape_patch_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn parse_method_response<'a>(
    response: &'a [serde_json::Value],
    expected_method: &str,
    expected_call_id: &str,
) -> Result<&'a serde_json::Value, MailEngineError> {
    if response.len() != 3 || response[2].as_str() != Some(expected_call_id) {
        return Err(MailEngineError::InvalidResponse);
    }
    if response[0].as_str() == Some("error") {
        return Err(MailEngineError::Rejected);
    }
    if response[0].as_str() != Some(expected_method) {
        return Err(MailEngineError::InvalidResponse);
    }
    Ok(&response[1])
}

fn parse_implicit_method_response<'a>(
    response: &'a [serde_json::Value],
    expected_method: &str,
    expected_call_id: &str,
) -> Result<&'a serde_json::Value, MailEngineError> {
    if response.len() != 3
        || response[2].as_str() != Some(expected_call_id)
        || response[0].as_str() != Some(expected_method)
    {
        // The submission may already have succeeded even when its implicit
        // Email update failed or the response was malformed. Force a later
        // reconciliation instead of classifying this as a safe rejection.
        return Err(MailEngineError::InvalidResponse);
    }
    Ok(&response[1])
}

/// Stable failures from the authenticated telemetry ingress boundary.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
pub enum MailEngineWebhookError {
    #[error("mail engine webhook verification is not safely configured")]
    InvalidConfiguration,
    #[error("mail engine webhook signature is invalid")]
    InvalidSignature,
    #[error("mail engine webhook payload is invalid")]
    InvalidPayload,
    #[error("mail engine webhook event type is not accepted")]
    UnsupportedEvent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::json;
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Mutex};

    fn test_mail_data_plane_id() -> MailDataPlaneId {
        MailDataPlaneId::from_uuid(uuid::Uuid::from_u128(
            0x018f_0000_0000_7000_8000_0000_0000_00d1,
        ))
    }

    async fn server(
        include_mail_capability: bool,
        expected_token: &'static str,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_for_handler = Arc::clone(&observed);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base_url = format!("http://{address}");
        let mut response = valid_session(&format!("{base_url}/jmap"));
        if include_mail_capability {
            response["capabilities"]
                .as_object_mut()
                .unwrap()
                .insert(JMAP_MAIL.into(), json!({}));
        }
        let app = Router::new().route(
            "/.well-known/jmap",
            get(move |headers: HeaderMap| {
                let response = response.clone();
                let observed = Arc::clone(&observed_for_handler);
                async move {
                    let authorization = headers
                        .get("authorization")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_owned();
                    observed.lock().await.push(authorization.clone());
                    if authorization != format!("Bearer {expected_token}") {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    Json(response).into_response()
                }
            }),
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base_url, observed)
    }

    fn valid_session(api_url: &str) -> serde_json::Value {
        json!({
            "capabilities": {
                JMAP_CORE: {},
                STALWART_JMAP: {}
            },
            "apiUrl": api_url,
            "username": "probe@example.test",
            "state": "session-1"
        })
    }

    async fn token_file(token: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("copilotmail-jmap-{}.token", uuid::Uuid::now_v7()));
        tokio::fs::write(&path, token).await.unwrap();
        path
    }

    async fn mail_content_server(
        expected_token: &'static str,
    ) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        mail_content_server_with_inboxes(
            expected_token,
            vec![inbox_mailbox_fixture("inbox-box", true)],
        )
        .await
    }

    fn inbox_mailbox_fixture(id: &str, may_read_items: bool) -> serde_json::Value {
        json!({
            "id": id,
            "role": "inbox",
            "myRights": {
                "mayReadItems": may_read_items,
                "mayAddItems": false,
                "mayRemoveItems": false,
                "maySetSeen": may_read_items,
                "maySetKeywords": may_read_items,
                "mayCreateChild": false,
                "mayRename": false,
                "mayDelete": false,
                "maySubmit": false
            }
        })
    }

    fn drafts_mailbox_fixture(id: &str, may_read_items: bool) -> serde_json::Value {
        json!({
            "id": id,
            "role": "drafts",
            "myRights": {
                "mayReadItems": may_read_items,
                "mayAddItems": true,
                "mayRemoveItems": true
            }
        })
    }

    fn valid_draft_content_envelope() -> serde_json::Value {
        json!({
            "methodResponses": [["Email/get", {
                "accountId": "account-42",
                "state": "email-12",
                "list": [{
                    "id": "draft-new",
                    "threadId": "thread-draft",
                    "mailboxIds": {"drafts-box": true},
                    "keywords": {"$draft": true, "$seen": true},
                    "from": [{"name": null, "email": "alice@example.test"}],
                    "to": [{"name": "Bob", "email": "bob@example.test"}],
                    "cc": [{"name": null, "email": "carol@example.test"}],
                    "bcc": [{"name": null, "email": "hidden@example.test"}],
                    "subject": "Status",
                    "hasAttachment": false,
                    "textBody": [{
                        "partId": "body", "type": "text/plain", "size": 15,
                        "name": null, "disposition": null
                    }],
                    "htmlBody": [],
                    "bodyValues": {
                        "body": {
                            "value": "Plain text only",
                            "isEncodingProblem": false,
                            "isTruncated": false
                        }
                    },
                    "attachments": []
                }],
                "notFound": []
            }, "x"]],
            "sessionState": "session-mail-1"
        })
    }

    fn parse_draft_fixture(
        envelope: serde_json::Value,
    ) -> Result<Option<MailDraftContent>, MailEngineError> {
        parse_exact_draft_content(
            &serde_json::to_vec(&envelope).unwrap(),
            "account-42",
            "draft-new",
            "thread-draft",
            &MailboxAddress::parse("alice@example.test").unwrap(),
            "drafts-box",
        )
    }

    async fn mail_content_server_with_inboxes(
        expected_token: &'static str,
        inboxes: Vec<serde_json::Value>,
    ) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base_url = format!("http://{address}");
        let mut role_mailboxes = vec![json!({
            "id": "drafts-box",
            "role": "drafts",
            "myRights": {
                "mayReadItems": true,
                "mayAddItems": true,
                "mayRemoveItems": true,
                "maySetSeen": true,
                "maySetKeywords": true,
                "mayCreateChild": false,
                "mayRename": false,
                "mayDelete": false,
                "maySubmit": true
            }
        })];
        role_mailboxes.extend(inboxes);
        let session = json!({
            "capabilities": {JMAP_CORE: {}, JMAP_MAIL: {}},
            "accounts": {
                "account-42": {
                    "name": "Alice",
                    "isPersonal": true,
                    "isReadOnly": false,
                    "accountCapabilities": {JMAP_MAIL: {}}
                }
            },
            "primaryAccounts": {JMAP_MAIL: "account-42"},
            "apiUrl": format!("{base_url}/jmap"),
            "username": "mail-reader@example.test",
            "state": "session-mail-1"
        });
        let observed_for_jmap = Arc::clone(&observed);
        let role_mailboxes_for_jmap = role_mailboxes;
        let app = Router::new()
            .route(
                "/.well-known/jmap",
                get(move |headers: HeaderMap| {
                    let session = session.clone();
                    async move {
                        if headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            != Some(&format!("Bearer {expected_token}"))
                        {
                            return StatusCode::UNAUTHORIZED.into_response();
                        }
                        Json(session).into_response()
                    }
                }),
            )
            .route(
                "/jmap",
                post(move |Json(body): Json<serde_json::Value>| {
                    let observed = Arc::clone(&observed_for_jmap);
                    let role_mailboxes = role_mailboxes_for_jmap.clone();
                    async move {
                        let method = body["methodCalls"][0][0].as_str().unwrap_or_default();
                        let detail = method == "Email/get" && body["methodCalls"][0][2] == "d";
                        let previous_draft_get =
                            method == "Email/get" && body["methodCalls"][0][2] == "p";
                        let reply_source_get =
                            method == "Email/get" && body["methodCalls"][0][2] == "r";
                        let draft_content_get =
                            method == "Email/get" && body["methodCalls"][0][2] == "x";
                        let state_update = body["methodCalls"][0][2] == "s";
                        let mailbox_get = method == "Mailbox/get";
                        let draft_set = method == "Email/set" && body["methodCalls"][0][2] == "d";
                        let destroyed = body["methodCalls"][0][1]["destroy"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();
                        observed.lock().await.push(body);
                        let response = if mailbox_get {
                            json!({
                                "methodResponses": [["Mailbox/get", {
                                    "accountId": "account-42",
                                    "state": "mailboxes-1",
                                    "list": role_mailboxes,
                                    "notFound": []
                                }, "m"]],
                                "sessionState": "session-mail-1"
                            })
                        } else if draft_content_get {
                            json!({
                                "methodResponses": [["Email/get", {
                                    "accountId": "account-42",
                                    "state": "email-12",
                                    "list": [{
                                        "id": "draft-new",
                                        "threadId": "thread-draft",
                                        "mailboxIds": {"drafts-box": true},
                                        "keywords": {"$draft": true, "$seen": true},
                                        "from": [{"name": null, "email": "Alice@Example.test"}],
                                        "to": [{"name": "Bob", "email": "bob@example.test"}],
                                        "cc": [{"name": null, "email": "carol@example.test"}],
                                        "bcc": [{"name": null, "email": "hidden@example.test"}],
                                        "subject": "Status",
                                        "hasAttachment": false,
                                        "textBody": [{
                                            "partId": "body", "type": "text/plain",
                                            "size": 15, "name": null, "disposition": null
                                        }],
                                        "htmlBody": [],
                                        "bodyValues": {
                                            "body": {
                                                "value": "Plain text only",
                                                "isEncodingProblem": false,
                                                "isTruncated": false
                                            }
                                        },
                                        "attachments": []
                                    }],
                                    "notFound": []
                                }, "x"]],
                                "sessionState": "session-mail-1"
                            })
                        } else if previous_draft_get {
                            json!({
                                "methodResponses": [["Email/get", {
                                    "accountId": "account-42",
                                    "state": "email-11",
                                    "list": [{
                                        "id": "draft-old",
                                        "mailboxIds": {"drafts-box": true},
                                        "keywords": {"$draft": true},
                                        "from": [{"name": null, "email": "alice@example.test"}]
                                    }],
                                    "notFound": []
                                }, "p"]],
                                "sessionState": "session-mail-1"
                            })
                        } else if reply_source_get {
                            json!({
                                "methodResponses": [["Email/get", {
                                    "accountId": "account-42",
                                    "state": "email-11",
                                    "list": [{
                                        "id": "message-new",
                                        "messageId": ["source-message@example.test"],
                                        "references": [
                                            "root-message@example.test",
                                            "source-message@example.test"
                                        ],
                                        "header:Auto-Submitted:asText": "auto-replied"
                                    }],
                                    "notFound": []
                                }, "r"]],
                                "sessionState": "session-mail-1"
                            })
                        } else if draft_set {
                            json!({
                                "methodResponses": [["Email/set", {
                                    "accountId": "account-42",
                                    "oldState": "email-11",
                                    "newState": "email-12",
                                    "created": {
                                        "draft": {
                                            "id": "draft-new",
                                            "blobId": "must-not-cross",
                                            "threadId": "thread-draft",
                                            "size": 123
                                        }
                                    },
                                    "notCreated": {},
                                    "destroyed": destroyed,
                                    "notDestroyed": {}
                                }, "d"]],
                                "sessionState": "session-mail-1"
                            })
                        } else if state_update {
                            json!({
                                "methodResponses": [
                                    ["Email/set", {
                                        "accountId": "account-42",
                                        "oldState": "email-10",
                                        "newState": "email-11",
                                        "updated": {"message-new": null},
                                        "notUpdated": {}
                                    }, "s"],
                                    ["Email/get", {
                                        "accountId": "account-42",
                                        "state": "email-11",
                                        "list": [{
                                            "id": "message-new",
                                            "keywords": {"$seen": true, "$flagged": true}
                                        }],
                                        "notFound": []
                                    }, "k"]
                                ],
                                "sessionState": "session-mail-1"
                            })
                        } else if detail {
                            json!({
                                "methodResponses": [["Email/get", {
                                    "accountId": "account-42",
                                    "state": "email-10",
                                    "list": [{
                                        "id": "message-new",
                                        "subject": "New",
                                        "from": [{"name": "Sender", "email": "new@example.test"}],
                                        "to": [{"name": "Alice", "email": "alice@example.test"}],
                                        "cc": [],
                                        "receivedAt": "2026-07-26T08:00:00Z",
                                        "preview": "new preview",
                                        "keywords": {"$flagged": true},
                                        "hasAttachment": true,
                                        "textBody": [{
                                            "partId": "text-1", "type": "text/plain",
                                            "size": 12, "name": null, "disposition": null
                                        }],
                                        "bodyValues": {
                                            "text-1": {
                                                "value": "Safe body",
                                                "isEncodingProblem": false,
                                                "isTruncated": false
                                            }
                                        },
                                        "attachments": [{
                                            "partId": "attachment-1",
                                            "type": "application/pdf", "size": 42,
                                            "name": "report.pdf", "disposition": "attachment"
                                        }]
                                    }],
                                    "notFound": []
                                }, "d"]],
                                "sessionState": "session-mail-1"
                            })
                        } else {
                            json!({
                            "methodResponses": [
                                ["Email/query", {
                                    "accountId": "account-42",
                                    "queryState": "query-7",
                                    "canCalculateChanges": true,
                                    "position": 0,
                                    "ids": ["message-new", "message-old"],
                                    "total": 3
                                }, "q"],
                                ["Email/get", {
                                    "accountId": "account-42",
                                    "state": "email-9",
                                    "list": [{
                                        "id": "message-old",
                                        "subject": "Older",
                                        "from": [{"name": null, "email": "old@example.test"}],
                                        "receivedAt": "2026-07-25T08:00:00Z",
                                        "preview": "older preview",
                                        "keywords": {"$seen": true},
                                        "hasAttachment": false
                                    }, {
                                        "id": "message-new",
                                        "subject": "New",
                                        "from": [{"name": "Sender", "email": "new@example.test"}],
                                        "receivedAt": "2026-07-26T08:00:00Z",
                                        "preview": "new preview",
                                        "keywords": {"$flagged": true},
                                        "hasAttachment": true
                                    }],
                                    "notFound": []
                                }, "g"]
                            ],
                            "sessionState": "session-mail-1"
                        })
                        };
                        Json(response)
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base_url, observed)
    }

    async fn submission_server(
        identity_count: usize,
        submission_count: usize,
    ) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base_url = format!("http://{address}");
        let session = json!({
            "capabilities": {
                JMAP_CORE: {},
                JMAP_MAIL: {},
                JMAP_SUBMISSION: {}
            },
            "accounts": {
                "account-42": {
                    "accountCapabilities": {
                        JMAP_MAIL: {},
                        JMAP_SUBMISSION: {}
                    }
                }
            },
            "apiUrl": format!("{base_url}/jmap"),
            "username": "submission-service",
            "state": "session-submission-1"
        });
        let observed_for_jmap = Arc::clone(&observed);
        let app = Router::new()
            .route(
                "/.well-known/jmap",
                get(move |headers: HeaderMap| {
                    let session = session.clone();
                    async move {
                        if headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            != Some("Bearer submission-secret")
                        {
                            return StatusCode::UNAUTHORIZED.into_response();
                        }
                        Json(session).into_response()
                    }
                }),
            )
            .route(
                "/jmap",
                post(move |Json(body): Json<serde_json::Value>| {
                    let observed = Arc::clone(&observed_for_jmap);
                    async move {
                        observed.lock().await.push(body.clone());
                        let call = &body["methodCalls"][0];
                        let method = call[0].as_str().unwrap();
                        let call_id = call[2].as_str().unwrap();
                        if method == "EmailSubmission/set" {
                            return Json(json!({
                                "methodResponses": [
                                    ["EmailSubmission/set", {
                                        "accountId": "account-42",
                                        "oldState": "submission-1",
                                        "newState": "submission-2",
                                        "created": {"submission": {"id": "submission-new"}},
                                        "notCreated": {}
                                    }, call_id],
                                    ["Email/set", {
                                        "accountId": "account-42",
                                        "oldState": "email-1",
                                        "newState": "email-2",
                                        "updated": {"draft-frozen": null},
                                        "notUpdated": {}
                                    }, call_id]
                                ],
                                "sessionState": "session-submission-1"
                            }));
                        }
                        let result = match method {
                            "Identity/get" => {
                                let list = (0..identity_count)
                                    .map(|index| {
                                        json!({
                                            "id": format!("identity-{index}"),
                                            "email": "alice@example.test"
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                json!({
                                    "accountId": "account-42",
                                    "state": "identity-1",
                                    "list": list,
                                    "notFound": []
                                })
                            }
                            "EmailSubmission/query" => {
                                let ids = (0..submission_count)
                                    .take(2)
                                    .map(|index| format!("submission-{index}"))
                                    .collect::<Vec<_>>();
                                json!({
                                    "accountId": "account-42",
                                    "queryState": "submission-query-1",
                                    "canCalculateChanges": true,
                                    "position": 0,
                                    "ids": ids,
                                    "total": submission_count
                                })
                            }
                            "EmailSubmission/get" => json!({
                                "accountId": "account-42",
                                "state": "submission-1",
                                "list": [{
                                    "id": "submission-0",
                                    "identityId": "identity-0",
                                    "emailId": "draft-frozen",
                                    "deliveryStatus": {
                                        "queued@example.test": {
                                            "delivered": "queued",
                                            "smtpReply": "250 2.1.5 Queued",
                                            "displayed": "unknown"
                                        },
                                        "failed@example.test": {
                                            "delivered": "no",
                                            "smtpReply": "550 5.1.1 User unknown",
                                            "displayed": "unknown"
                                        },
                                        "done@example.test": {
                                            "delivered": "yes",
                                            "smtpReply": "250 2.0.0 Accepted",
                                            "displayed": "unknown"
                                        }
                                    }
                                }],
                                "notFound": []
                            }),
                            "Mailbox/get" => json!({
                                "accountId": "account-42",
                                "state": "mailbox-1",
                                "list": [{
                                    "id": "drafts/box",
                                    "role": "drafts",
                                    "myRights": {
                                        "mayAddItems": false,
                                        "mayRemoveItems": true
                                    }
                                }, {
                                    "id": "sent~box",
                                    "role": "sent",
                                    "myRights": {
                                        "mayAddItems": true,
                                        "mayRemoveItems": false
                                    }
                                }],
                                "notFound": []
                            }),
                            _ => panic!("unexpected submission method {method}"),
                        };
                        Json(json!({
                            "methodResponses": [[method, result, call_id]],
                            "sessionState": "session-submission-1"
                        }))
                    }
                }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base_url, observed)
    }

    #[tokio::test]
    async fn submission_reconciliation_resolves_one_identity_before_create() {
        let (base_url, observed) = submission_server(1, 0).await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("submission-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();

        client.probe_submissions().await.unwrap();
        let result = client
            .reconcile_submission("account-42", "draft-frozen", "Alice@Example.Test")
            .await
            .unwrap();
        assert_eq!(
            result,
            MailSubmissionReconciliation::NotFound {
                identity_ref: "identity-0".into()
            }
        );
        let calls = observed.lock().await;
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1]["methodCalls"][0][0], "Identity/get");
        assert_eq!(
            calls[2]["methodCalls"][0][1]["filter"]["emailIds"],
            json!(["draft-frozen"])
        );
        assert_eq!(calls[2]["methodCalls"][0][1]["limit"], 2);
    }

    #[tokio::test]
    async fn submission_reconciliation_returns_only_exact_existing_binding() {
        let (base_url, observed) = submission_server(1, 1).await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("submission-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let result = client
            .reconcile_submission("account-42", "draft-frozen", "alice@example.test")
            .await
            .unwrap();
        assert_eq!(
            result,
            MailSubmissionReconciliation::Accepted {
                identity_ref: "identity-0".into(),
                submission_ref: "submission-0".into(),
            }
        );
        assert_eq!(observed.lock().await.len(), 3);
    }

    #[tokio::test]
    async fn submission_create_omits_envelope_and_returns_server_reference() {
        let (base_url, observed) = submission_server(1, 0).await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("submission-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let created = client
            .create_submission("account-42", "draft-frozen", "identity-0")
            .await
            .unwrap();
        assert_eq!(
            created,
            CreatedMailSubmission {
                identity_ref: "identity-0".into(),
                submission_ref: "submission-new".into(),
            }
        );
        let calls = observed.lock().await;
        assert_eq!(calls[0]["methodCalls"][0][0], "Mailbox/get");
        let create = &calls[1]["methodCalls"][0][1]["create"]["submission"];
        assert_eq!(create["emailId"], "draft-frozen");
        assert_eq!(create["identityId"], "identity-0");
        assert!(create.get("envelope").is_none());
        let success = &calls[1]["methodCalls"][0][1]["onSuccessUpdateEmail"]["#submission"];
        assert_eq!(success["mailboxIds/drafts~1box"], serde_json::Value::Null);
        assert_eq!(success["mailboxIds/sent~0box"], true);
        assert_eq!(success["keywords/$draft"], serde_json::Value::Null);
        assert!(
            calls[1]["using"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == JMAP_SUBMISSION)
        );
    }

    #[tokio::test]
    async fn delivery_snapshot_is_bound_to_submission_and_normalized_per_recipient() {
        let (base_url, observed) = submission_server(1, 1).await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("submission-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let snapshot = client
            .get_delivery_snapshot("account-42", "draft-frozen", "identity-0", "submission-0")
            .await
            .unwrap();
        assert_eq!(snapshot.submission_ref, "submission-0");
        assert_eq!(snapshot.source_state, "submission-1");
        assert_eq!(
            snapshot
                .recipients
                .iter()
                .map(|recipient| (recipient.recipient.as_str(), recipient.state))
                .collect::<Vec<_>>(),
            [
                ("done@example.test", RecipientDeliveryState::Delivered),
                (
                    "failed@example.test",
                    RecipientDeliveryState::PermanentFailure
                ),
                ("queued@example.test", RecipientDeliveryState::Queued),
            ]
        );
        let calls = observed.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["methodCalls"][0][0], "EmailSubmission/get");
        assert_eq!(
            calls[0]["methodCalls"][0][1]["properties"],
            json!(["id", "identityId", "emailId", "deliveryStatus"])
        );
    }

    #[tokio::test]
    async fn submission_reconciliation_rejects_ambiguous_remote_state() {
        for (identity_count, submission_count) in [(2, 0), (1, 2)] {
            let (base_url, _observed) = submission_server(identity_count, submission_count).await;
            let client = StalwartJmapClient::new(
                test_mail_data_plane_id(),
                StalwartJmapConfig::new(
                    &base_url,
                    token_file("submission-secret").await,
                    Duration::from_secs(2),
                    true,
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                client
                    .reconcile_submission("account-42", "draft-frozen", "alice@example.test",)
                    .await,
                Err(MailEngineError::ProjectionConflict)
            );
        }
    }

    #[test]
    fn implicit_sent_mailbox_update_failure_is_not_a_safe_rejection() {
        let response = vec![json!("error"), json!({"type": "serverFail"}), json!("s")];
        assert_eq!(
            parse_implicit_method_response(&response, "Email/set", "s"),
            Err(MailEngineError::InvalidResponse)
        );
        assert_eq!(escape_patch_segment("drafts/a~b"), "drafts~1a~0b");
    }

    #[tokio::test]
    async fn lists_only_bounded_envelopes_for_the_exact_bound_account() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let token_path = token_file("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(&base_url, token_path, Duration::from_secs(2), true).unwrap(),
        )
        .unwrap();

        assert_eq!(
            client.list_messages("another-account", 0, 20).await,
            Err(MailEngineError::InvalidSession)
        );
        let page = client.list_messages("account-42", 0, 2).await.unwrap();
        assert_eq!(page.query_state, "query-7");
        assert_eq!(page.next_position, Some(2));
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["message-new", "message-old"]
        );
        assert!(page.items[0].is_flagged);
        assert!(!page.items[0].is_read);
        assert!(page.items[0].has_attachment);
        assert!(page.items[1].is_read);

        let calls = observed.lock().await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["methodCalls"][0][0], "Mailbox/get");
        assert_eq!(
            calls[0]["methodCalls"][0][1]["properties"],
            json!(["id", "role", "myRights"])
        );
        assert_eq!(calls[1]["using"], json!([JMAP_CORE, JMAP_MAIL]));
        assert_eq!(calls[1]["methodCalls"][0][0], "Email/query");
        assert_eq!(
            calls[1]["methodCalls"][0][1]["filter"],
            json!({"inMailbox": "inbox-box"})
        );
        assert_eq!(calls[1]["methodCalls"][0][1]["accountId"], "account-42");
        let serialized = calls[1].to_string();
        assert!(!serialized.contains("bodyValues"));
        assert!(!serialized.contains("blobId"));
        assert!(!serialized.contains("threadId"));
    }

    #[tokio::test]
    async fn search_uses_a_bounded_jmap_body_filter_without_requesting_content() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();

        let page = client
            .search_messages("account-42", "季度 report", 0, 2)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);
        let calls = observed.lock().await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["methodCalls"][0][0], "Mailbox/get");
        assert_eq!(calls[1]["methodCalls"][0][0], "Email/query");
        assert_eq!(
            calls[1]["methodCalls"][0][1]["filter"],
            json!({
                "operator": "AND",
                "conditions": [
                    {"inMailbox": "inbox-box"},
                    {"text": "季度 report"}
                ]
            })
        );
        let serialized = calls[1].to_string();
        assert!(!serialized.contains("bodyValues"));
        assert!(!serialized.contains("blobId"));
        assert!(!serialized.contains("threadId"));
    }

    #[tokio::test]
    async fn inbox_reads_fail_closed_for_missing_ambiguous_or_unreadable_role_mailboxes() {
        let cases = [
            Vec::new(),
            vec![
                inbox_mailbox_fixture("inbox-one", true),
                inbox_mailbox_fixture("inbox-two", true),
            ],
            vec![inbox_mailbox_fixture("inbox-box", false)],
        ];
        for inboxes in cases {
            let (base_url, observed) =
                mail_content_server_with_inboxes("mail-secret", inboxes).await;
            let client = StalwartJmapClient::new(
                test_mail_data_plane_id(),
                StalwartJmapConfig::new(
                    &base_url,
                    token_file("mail-secret").await,
                    Duration::from_secs(2),
                    true,
                )
                .unwrap(),
            )
            .unwrap();

            assert_eq!(
                client.list_messages("account-42", 0, 2).await,
                Err(MailEngineError::Rejected)
            );
            let calls = observed.lock().await;
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0]["methodCalls"][0][0], "Mailbox/get");
        }
    }

    #[tokio::test]
    async fn rejects_unbounded_mail_list_requests_before_network_access() {
        let config = StalwartJmapConfig::new(
            "http://127.0.0.1:9",
            PathBuf::from("unused"),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        let client = StalwartJmapClient::new(test_mail_data_plane_id(), config).unwrap();
        assert_eq!(
            client.list_messages("account-42", 0, 51).await,
            Err(MailEngineError::InvalidConfiguration)
        );
        assert_eq!(
            client.list_messages("account-42", 10_001, 20).await,
            Err(MailEngineError::InvalidConfiguration)
        );
        for text in ["x", " padded", "line\nbreak"] {
            assert_eq!(
                client.search_messages("account-42", text, 0, 20).await,
                Err(MailEngineError::InvalidConfiguration)
            );
        }
    }

    #[tokio::test]
    async fn detail_returns_only_bounded_plain_text_and_attachment_metadata() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let detail = client
            .get_message("account-42", "message-new")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(detail.text_body, "Safe body");
        assert!(!detail.text_body_truncated);
        assert_eq!(detail.to[0].email, "alice@example.test");
        assert_eq!(detail.attachments[0].name.as_deref(), Some("report.pdf"));
        assert_eq!(detail.attachments[0].size_bytes, 42);
        let request = observed.lock().await[0].to_string();
        assert!(request.contains("fetchTextBodyValues"));
        assert!(!request.contains("fetchHTMLBodyValues"));
        assert!(!request.contains("blobId"));
        assert!(!request.contains("threadId"));
    }

    #[tokio::test]
    async fn state_update_uses_jmap_patch_and_returns_authoritative_keywords() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let state = client
            .set_message_state("account-42", "message-new", Some(true), Some(false))
            .await
            .unwrap();
        assert_eq!(
            state,
            MailMessageState {
                is_read: true,
                is_flagged: true
            }
        );
        let request = &observed.lock().await[0];
        assert_eq!(
            request["methodCalls"][0][1]["update"]["message-new"]["keywords/$seen"],
            true
        );
        assert!(
            request["methodCalls"][0][1]["update"]["message-new"]["keywords/$flagged"].is_null()
        );
        assert_eq!(request["methodCalls"][1][0], "Email/get");
    }

    #[tokio::test]
    async fn exact_draft_restore_reads_one_object_without_enumerating_email() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();

        let content = client
            .read_exact_text_draft(
                "account-42",
                "draft-new",
                "thread-draft",
                &MailboxAddress::parse("alice@example.test").unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            content,
            MailDraftContent {
                to: vec!["bob@example.test".into()],
                cc: vec!["carol@example.test".into()],
                bcc: vec!["hidden@example.test".into()],
                subject: "Status".into(),
                text_body: "Plain text only".into(),
            }
        );

        let calls = observed.lock().await;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["methodCalls"][0][0], "Mailbox/get");
        let request = &calls[1];
        assert_eq!(request["methodCalls"][0][0], "Email/get");
        assert_eq!(request["methodCalls"][0][1]["ids"], json!(["draft-new"]));
        assert_eq!(
            request["methodCalls"][0][1]["maxBodyValueBytes"],
            MAX_TEXT_BODY_BYTES
        );
        assert_eq!(request["methodCalls"][0][1]["fetchTextBodyValues"], true);
        assert_eq!(request["methodCalls"][0][1]["fetchHTMLBodyValues"], false);
        let serialized = calls
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<String>();
        assert!(!serialized.contains("Email/query"));
        assert!(!serialized.contains("Email/set"));
        assert!(!serialized.contains("blobId"));
        assert!(serialized.contains("threadId"));
        assert!(serialized.contains("bcc"));
    }

    #[test]
    fn exact_draft_restore_accepts_only_the_requested_not_found_identity() {
        let mut missing = valid_draft_content_envelope();
        missing["methodResponses"][0][1]["list"] = json!([]);
        missing["methodResponses"][0][1]["notFound"] = json!(["draft-new"]);
        assert_eq!(parse_draft_fixture(missing), Ok(None));

        let mut wrong = valid_draft_content_envelope();
        wrong["methodResponses"][0][1]["list"] = json!([]);
        wrong["methodResponses"][0][1]["notFound"] = json!(["other-draft"]);
        assert_eq!(
            parse_draft_fixture(wrong),
            Err(MailEngineError::InvalidResponse)
        );
    }

    #[test]
    fn exact_draft_restore_rejects_every_control_plane_binding_mismatch() {
        let mut cases = Vec::new();

        let mut account = valid_draft_content_envelope();
        account["methodResponses"][0][1]["accountId"] = json!("other-account");
        cases.push(account);

        let mut email = valid_draft_content_envelope();
        email["methodResponses"][0][1]["list"][0]["id"] = json!("other-draft");
        cases.push(email);

        let mut thread = valid_draft_content_envelope();
        thread["methodResponses"][0][1]["list"][0]["threadId"] = json!("other-thread");
        cases.push(thread);

        let mut sender = valid_draft_content_envelope();
        sender["methodResponses"][0][1]["list"][0]["from"][0]["email"] =
            json!("mallory@example.test");
        cases.push(sender);

        let mut multiple_senders = valid_draft_content_envelope();
        multiple_senders["methodResponses"][0][1]["list"][0]["from"] = json!([
            {"name": null, "email": "alice@example.test"},
            {"name": null, "email": "mallory@example.test"}
        ]);
        cases.push(multiple_senders);

        let mut multiple_mailboxes = valid_draft_content_envelope();
        multiple_mailboxes["methodResponses"][0][1]["list"][0]["mailboxIds"] =
            json!({"drafts-box": true, "other-box": true});
        cases.push(multiple_mailboxes);

        let mut wrong_mailbox = valid_draft_content_envelope();
        wrong_mailbox["methodResponses"][0][1]["list"][0]["mailboxIds"] =
            json!({"other-box": true});
        cases.push(wrong_mailbox);

        let mut not_a_draft = valid_draft_content_envelope();
        not_a_draft["methodResponses"][0][1]["list"][0]["keywords"]["$draft"] = json!(false);
        cases.push(not_a_draft);

        for case in cases {
            assert_eq!(
                parse_draft_fixture(case),
                Err(MailEngineError::ProjectionConflict)
            );
        }
    }

    #[test]
    fn exact_draft_restore_rejects_unsafe_or_lossy_content() {
        let mut cases = Vec::new();

        let mut duplicate_recipient = valid_draft_content_envelope();
        duplicate_recipient["methodResponses"][0][1]["list"][0]["bcc"][0]["email"] =
            json!("BOB@example.test");
        cases.push(duplicate_recipient);

        let mut invalid_recipient = valid_draft_content_envelope();
        invalid_recipient["methodResponses"][0][1]["list"][0]["bcc"][0]["email"] =
            json!("not-an-address");
        cases.push(invalid_recipient);

        let mut no_recipients = valid_draft_content_envelope();
        for field in ["to", "cc", "bcc"] {
            no_recipients["methodResponses"][0][1]["list"][0][field] = json!([]);
        }
        cases.push(no_recipients);

        let mut injected_subject = valid_draft_content_envelope();
        injected_subject["methodResponses"][0][1]["list"][0]["subject"] =
            json!("Subject\r\nBcc: victim@example.test");
        cases.push(injected_subject);

        let mut attachment_flag = valid_draft_content_envelope();
        attachment_flag["methodResponses"][0][1]["list"][0]["hasAttachment"] = json!(true);
        cases.push(attachment_flag);

        let mut attachment = valid_draft_content_envelope();
        attachment["methodResponses"][0][1]["list"][0]["attachments"] = json!([{
            "partId": "attachment", "type": "application/pdf", "size": 1,
            "name": "file.pdf", "disposition": "attachment"
        }]);
        cases.push(attachment);

        let mut html = valid_draft_content_envelope();
        html["methodResponses"][0][1]["list"][0]["htmlBody"] = json!([{
            "partId": "html", "type": "text/html", "size": 1,
            "name": null, "disposition": null
        }]);
        cases.push(html);

        let mut no_plain_part = valid_draft_content_envelope();
        no_plain_part["methodResponses"][0][1]["list"][0]["textBody"] = json!([]);
        cases.push(no_plain_part);

        let mut wrong_media_type = valid_draft_content_envelope();
        wrong_media_type["methodResponses"][0][1]["list"][0]["textBody"][0]["type"] =
            json!("text/html");
        cases.push(wrong_media_type);

        let mut named_body = valid_draft_content_envelope();
        named_body["methodResponses"][0][1]["list"][0]["textBody"][0]["name"] = json!("body.txt");
        cases.push(named_body);

        let mut truncated = valid_draft_content_envelope();
        truncated["methodResponses"][0][1]["list"][0]["bodyValues"]["body"]["isTruncated"] =
            json!(true);
        cases.push(truncated);

        let mut encoding_problem = valid_draft_content_envelope();
        encoding_problem["methodResponses"][0][1]["list"][0]["bodyValues"]["body"]["isEncodingProblem"] =
            json!(true);
        cases.push(encoding_problem);

        let mut size_mismatch = valid_draft_content_envelope();
        size_mismatch["methodResponses"][0][1]["list"][0]["textBody"][0]["size"] = json!(14);
        cases.push(size_mismatch);

        let mut oversized_body = valid_draft_content_envelope();
        oversized_body["methodResponses"][0][1]["list"][0]["textBody"][0]["size"] =
            json!(MAX_TEXT_BODY_BYTES + 1);
        oversized_body["methodResponses"][0][1]["list"][0]["bodyValues"]["body"]["value"] =
            json!("x".repeat(MAX_TEXT_BODY_BYTES + 1));
        cases.push(oversized_body);

        let mut extra_body_value = valid_draft_content_envelope();
        extra_body_value["methodResponses"][0][1]["list"][0]["bodyValues"]["other"] =
            json!({"value": "hidden", "isEncodingProblem": false, "isTruncated": false});
        cases.push(extra_body_value);

        for case in cases {
            assert_eq!(
                parse_draft_fixture(case),
                Err(MailEngineError::InvalidResponse)
            );
        }
    }

    #[test]
    fn exact_draft_restore_requires_one_readable_drafts_mailbox() {
        let response = |list| {
            serde_json::from_value::<MailboxGetResponse>(json!({
                "accountId": "account-42",
                "state": "mailboxes-1",
                "list": list,
                "notFound": []
            }))
            .unwrap()
        };
        assert_eq!(
            select_readable_drafts_mailbox(
                response(vec![drafts_mailbox_fixture("drafts-box", true)]),
                "account-42"
            ),
            Ok("drafts-box".into())
        );
        assert_eq!(
            select_readable_drafts_mailbox(
                response(vec![json!({
                    "id": "drafts-minimal",
                    "role": "drafts",
                    "myRights": {"mayReadItems": true}
                })]),
                "account-42"
            ),
            Ok("drafts-minimal".into())
        );
        assert_eq!(
            select_readable_drafts_mailbox(
                response(vec![drafts_mailbox_fixture("drafts-box", false)]),
                "account-42"
            ),
            Err(MailEngineError::Rejected)
        );
        assert_eq!(
            select_readable_drafts_mailbox(
                response(vec![
                    drafts_mailbox_fixture("drafts-a", true),
                    drafts_mailbox_fixture("drafts-b", false)
                ]),
                "account-42"
            ),
            Err(MailEngineError::ProjectionConflict)
        );
    }

    #[tokio::test]
    async fn exact_draft_restore_rejects_invalid_remote_refs_before_network_access() {
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                "http://127.0.0.1:9",
                PathBuf::from("unused"),
                Duration::from_secs(1),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            client
                .read_exact_text_draft(
                    "account-42",
                    "draft\nnew",
                    "thread-draft",
                    &MailboxAddress::parse("alice@example.test").unwrap(),
                )
                .await,
            Err(MailEngineError::InvalidConfiguration)
        );
    }

    #[tokio::test]
    async fn draft_probe_requires_a_writable_role_mailbox_without_mutation() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();

        client.probe_drafts().await.unwrap();
        let calls = observed.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["methodCalls"][0][0], "Mailbox/get");
        assert!(calls[0].to_string().contains("myRights"));
        assert!(!calls[0].to_string().contains("Email/set"));
    }

    #[tokio::test]
    async fn draft_save_resolves_role_and_replaces_previous_body_immutably() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let draft = client
            .save_text_draft(
                "account-42",
                Some("draft-old"),
                &TextDraftInput {
                    from: "alice@example.test".into(),
                    to: vec!["bob@example.test".into()],
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    subject: "Status".into(),
                    text_body: "Plain text only".into(),
                    reply_to_email_id: None,
                    automatic: false,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            draft,
            MailDraft {
                id: "draft-new".into(),
                thread_id: "thread-draft".into(),
                reply_source_automatic: false,
                previous_draft_destroyed: true,
            }
        );
        let requests = observed.lock().await;
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1]["methodCalls"][0][0], "Email/get");
        assert_eq!(
            requests[1]["methodCalls"][0][1]["properties"],
            json!(["id", "mailboxIds", "keywords", "from"])
        );
        let create = &requests[2]["methodCalls"][0][1]["create"]["draft"];
        assert_eq!(create["mailboxIds"]["drafts-box"], true);
        assert_eq!(create["keywords"]["$draft"], true);
        assert_eq!(create["bodyStructure"]["type"], "text/plain");
        assert_eq!(requests[2]["methodCalls"][0][1]["destroy"][0], "draft-old");
        let serialized = requests[2].to_string();
        assert!(!serialized.contains("blobId"));
        assert!(!serialized.contains("text/html"));
        assert!(!serialized.contains("Auto-Submitted"));
    }

    #[tokio::test]
    async fn draft_replacement_cannot_destroy_an_unverified_message_id() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let result = client
            .save_text_draft(
                "account-42",
                Some("message-new"),
                &TextDraftInput {
                    from: "alice@example.test".into(),
                    to: vec!["bob@example.test".into()],
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    subject: "Status".into(),
                    text_body: "Plain text only".into(),
                    reply_to_email_id: None,
                    automatic: false,
                },
            )
            .await;

        assert_eq!(result, Err(MailEngineError::Rejected));
        let calls = observed.lock().await;
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|call| call["methodCalls"][0][2] != "d"));
    }

    #[tokio::test]
    async fn agent_authored_new_mail_is_marked_auto_generated() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        client
            .save_text_draft(
                "account-42",
                None,
                &TextDraftInput {
                    from: "agent@example.test".into(),
                    to: vec!["bob@example.test".into()],
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    subject: "Status".into(),
                    text_body: "Generated body".into(),
                    reply_to_email_id: None,
                    automatic: true,
                },
            )
            .await
            .unwrap();
        let calls = observed.lock().await;
        let created = &calls[1]["methodCalls"][0][1]["create"]["draft"];
        assert_eq!(created["header:Auto-Submitted:asText"], "auto-generated");
    }

    #[tokio::test]
    async fn explicit_reply_headers_are_derived_from_the_exact_source_email() {
        let (base_url, observed) = mail_content_server("mail-secret").await;
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                &base_url,
                token_file("mail-secret").await,
                Duration::from_secs(2),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        let draft = client
            .save_text_draft(
                "account-42",
                None,
                &TextDraftInput {
                    from: "alice@example.test".into(),
                    to: vec!["bob@example.test".into()],
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    subject: "Re: Status".into(),
                    text_body: "Reply body".into(),
                    reply_to_email_id: Some("message-new".into()),
                    automatic: true,
                },
            )
            .await
            .unwrap();
        assert!(draft.reply_source_automatic);

        let calls = observed.lock().await;
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1]["methodCalls"][0][0], "Email/get");
        assert_eq!(
            calls[1]["methodCalls"][0][1]["properties"],
            json!([
                "id",
                "messageId",
                "references",
                "header:Auto-Submitted:asText"
            ])
        );
        let created = &calls[2]["methodCalls"][0][1]["create"]["draft"];
        assert_eq!(created["inReplyTo"], json!(["source-message@example.test"]));
        assert_eq!(
            created["references"],
            json!(["root-message@example.test", "source-message@example.test"])
        );
        assert_eq!(created["header:Auto-Submitted:asText"], "auto-replied");
    }

    #[tokio::test]
    async fn draft_input_rejects_missing_recipients_and_header_controls() {
        let client = StalwartJmapClient::new(
            test_mail_data_plane_id(),
            StalwartJmapConfig::new(
                "http://127.0.0.1:9",
                PathBuf::from("unused"),
                Duration::from_secs(1),
                true,
            )
            .unwrap(),
        )
        .unwrap();
        for input in [
            TextDraftInput {
                from: "alice@example.test".into(),
                to: Vec::new(),
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "No recipients".into(),
                text_body: "body".into(),
                reply_to_email_id: None,
                automatic: false,
            },
            TextDraftInput {
                from: "alice@example.test".into(),
                to: vec!["bob@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Injected\r\nBcc: victim@example.test".into(),
                text_body: "body".into(),
                reply_to_email_id: None,
                automatic: false,
            },
        ] {
            assert_eq!(
                client.save_text_draft("account-42", None, &input).await,
                Err(MailEngineError::InvalidConfiguration)
            );
        }
    }

    fn signed_webhook(value: serde_json::Value, key: &[u8]) -> (Vec<u8>, String) {
        let body = serde_json::to_vec(&value).unwrap();
        let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
        mac.update(&body);
        (body, STANDARD.encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn verifies_and_minimizes_stalwart_inbound_events() {
        let key = b"0123456789abcdef0123456789abcdef";
        let (body, signature) = signed_webhook(
            json!({
                "events": [{
                    "id": "17850123450001283",
                    "createdAt": "2026-07-26T08:00:00Z",
                    "type": "message-ingest.spam",
                    "data": {
                        "accountId": 42,
                        "documentId": 991,
                        "mailboxId": [7, 8],
                        "messageId": "<sensitive@example.test>",
                        "blobId": "must-not-cross-boundary",
                        "size": 123456
                    }
                }]
            }),
            key,
        );
        let events = StalwartWebhookParser::verify_and_parse(&body, &signature, key).unwrap();
        assert_eq!(
            events,
            vec![InboundMailEngineEvent {
                source_event_id: "17850123450001283".into(),
                source_occurred_at: "2026-07-26T08:00:00Z".parse().unwrap(),
                account_ref: "42".into(),
                document_ref: "991".into(),
                classification: InboundMessageClassification::Spam,
            }]
        );
        let serialized = format!("{events:?}");
        assert!(!serialized.contains("sensitive"));
        assert!(!serialized.contains("blob"));
        assert!(!serialized.contains("123456"));
    }

    #[test]
    fn verifies_queue_scoped_delivery_events_without_retaining_response_text() {
        let key = b"0123456789abcdef0123456789abcdef";
        let (body, signature) = signed_webhook(
            json!({
                "events": [{
                    "id": "delivery-1",
                    "createdAt": "2026-07-26T08:00:00Z",
                    "type": "delivery.delivered",
                    "data": {
                        "spanId": 90,
                        "queueId": 771,
                        "from": "Sender@Example.Test",
                        "to": "Recipient@External.Test",
                        "code": 250,
                        "details": "250 2.0.0 sensitive remote text",
                        "hostname": "mx.external.test"
                    }
                }, {
                    "id": "delivery-2",
                    "createdAt": "2026-07-26T08:01:00Z",
                    "type": "queue.rescheduled",
                    "data": {
                        "spanId": 91,
                        "queueId": 771,
                        "from": "sender@example.test",
                        "to": ["recipient@external.test"],
                        "nextRetry": "2026-07-26T08:06:00Z",
                        "expires": "2026-07-27T08:00:00Z"
                    }
                }, {
                    "id": "delivery-3",
                    "createdAt": "2026-07-27T08:00:00Z",
                    "type": "delivery.failed",
                    "data": {
                        "spanId": 92,
                        "queueId": 772,
                        "from": "sender@example.test",
                        "to": "failed@external.test",
                        "reason": "must-not-cross-boundary"
                    }
                }, {
                    "id": "delivery-4",
                    "createdAt": "2026-07-27T08:00:01Z",
                    "type": "delivery.completed",
                    "data": {
                        "spanId": 92,
                        "queueId": 772,
                        "from": "sender@example.test",
                        "to": ["failed@external.test"]
                    }
                }]
            }),
            key,
        );
        let events = StalwartWebhookParser::verify_and_parse_all(&body, &signature, key).unwrap();
        assert_eq!(events.len(), 4);
        let MailEngineWebhookEvent::Outbound(delivered) = &events[0] else {
            panic!("expected outbound delivery event");
        };
        assert_eq!(delivered.queue_ref, "771");
        assert_eq!(delivered.envelope_from.as_str(), "sender@example.test");
        assert_eq!(
            delivered.recipient.as_ref().unwrap().as_str(),
            "recipient@external.test"
        );
        assert_eq!(delivered.kind, OutboundDeliveryEventKind::Delivered);
        assert_eq!(delivered.smtp_code, Some(250));
        let MailEngineWebhookEvent::Outbound(rescheduled) = &events[1] else {
            panic!("expected queue reschedule event");
        };
        assert_eq!(
            rescheduled.next_retry_at,
            Some("2026-07-26T08:06:00Z".parse().unwrap())
        );
        assert_eq!(rescheduled.recipient, None);
        let serialized = format!("{events:?}");
        assert!(!serialized.contains("sensitive remote text"));
        assert!(!serialized.contains("must-not-cross-boundary"));
        assert!(!serialized.contains("mx.external.test"));
    }

    #[test]
    fn rejects_tampering_unknown_types_and_malformed_resource_keys() {
        let key = b"0123456789abcdef0123456789abcdef";
        let valid = json!({
            "events": [{
                "id": "event-1",
                "createdAt": "2026-07-26T08:00:00Z",
                "type": "message-ingest.ham",
                "data": {
                    "accountId": 42,
                    "documentId": 991,
                    "mailboxId": [7]
                }
            }]
        });
        let (mut body, signature) = signed_webhook(valid.clone(), key);
        body.push(b' ');
        assert_eq!(
            StalwartWebhookParser::verify_and_parse(&body, &signature, key),
            Err(MailEngineWebhookError::InvalidSignature)
        );

        let mut unsupported = valid.clone();
        unsupported["events"][0]["type"] = json!("delivery.delivered");
        let (body, signature) = signed_webhook(unsupported, key);
        assert_eq!(
            StalwartWebhookParser::verify_and_parse(&body, &signature, key),
            Err(MailEngineWebhookError::UnsupportedEvent)
        );

        let mut malformed = valid;
        malformed["events"][0]["data"]["accountId"] = json!("42");
        let (body, signature) = signed_webhook(malformed, key);
        assert_eq!(
            StalwartWebhookParser::verify_and_parse(&body, &signature, key),
            Err(MailEngineWebhookError::InvalidPayload)
        );
    }

    #[test]
    fn rejects_weak_keys_and_non_base64_signatures_before_parsing() {
        assert_eq!(
            StalwartWebhookParser::verify_and_parse(b"not-json", "not-base64", b"short"),
            Err(MailEngineWebhookError::InvalidConfiguration)
        );
        assert_eq!(
            StalwartWebhookParser::verify_and_parse(
                b"not-json",
                "not-base64",
                b"0123456789abcdef0123456789abcdef"
            ),
            Err(MailEngineWebhookError::InvalidSignature)
        );
    }

    async fn projection_server(
        expected_token: &'static str,
        initial: Option<EngineDomain>,
    ) -> (String, Arc<Mutex<Option<EngineDomain>>>) {
        let state = Arc::new(Mutex::new(initial));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let base_url = format!("http://{address}");
        let session = valid_session(&format!("{base_url}/jmap"));
        let jmap_state = Arc::clone(&state);
        let app = Router::new()
            .route(
                "/.well-known/jmap",
                get(move |headers: HeaderMap| {
                    let session = session.clone();
                    async move {
                        if headers.get("authorization").and_then(|v| v.to_str().ok())
                            != Some(&format!("Bearer {expected_token}"))
                        {
                            return StatusCode::UNAUTHORIZED.into_response();
                        }
                        Json(session).into_response()
                    }
                }),
            )
            .route(
                "/jmap",
                post(
                    move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                        let state = Arc::clone(&jmap_state);
                        async move {
                            if headers.get("authorization").and_then(|v| v.to_str().ok())
                                != Some(&format!("Bearer {expected_token}"))
                            {
                                return StatusCode::UNAUTHORIZED.into_response();
                            }
                            let call = &body["methodCalls"][0];
                            let method = call[0].as_str().unwrap();
                            let arguments = &call[1];
                            let response = match method {
                                "x:Domain/query" => {
                                    let domain = state.lock().await.clone();
                                    let ids = domain
                                        .filter(|domain| {
                                            arguments["filter"]["name"].as_str()
                                                == Some(domain.name.as_str())
                                        })
                                        .map(|domain| vec![domain.id])
                                        .unwrap_or_default();
                                    json!({"ids": ids})
                                }
                                "x:Domain/get" => {
                                    let requested = arguments["ids"][0].as_str().unwrap();
                                    let list = state
                                        .lock()
                                        .await
                                        .clone()
                                        .filter(|domain| domain.id == requested)
                                        .map(|domain| {
                                            vec![json!({
                                                "id": domain.id,
                                                "name": domain.name,
                                                "isEnabled": domain.is_enabled
                                            })]
                                        })
                                        .unwrap_or_default();
                                    json!({"list": list, "notFound": []})
                                }
                                "x:Domain/set" if arguments.get("create").is_some() => {
                                    let created = &arguments["create"]["copilotmail"];
                                    assert_eq!(created["certificateManagement"]["@type"], "Manual");
                                    assert_eq!(created["dkimManagement"]["@type"], "Automatic");
                                    assert_eq!(created["dnsManagement"]["@type"], "Manual");
                                    assert_eq!(created["allowRelaying"], false);
                                    *state.lock().await = Some(EngineDomain {
                                        id: "domain-1".into(),
                                        name: created["name"].as_str().unwrap().into(),
                                        is_enabled: created["isEnabled"].as_bool().unwrap(),
                                    });
                                    json!({"created": {"copilotmail": {"id": "domain-1"}}})
                                }
                                "x:Domain/set" => {
                                    let (id, patch) = arguments["update"]
                                        .as_object()
                                        .unwrap()
                                        .iter()
                                        .next()
                                        .unwrap();
                                    let mut guard = state.lock().await;
                                    let domain = guard.as_mut().unwrap();
                                    assert_eq!(&domain.id, id);
                                    domain.is_enabled = patch["isEnabled"].as_bool().unwrap();
                                    json!({"updated": {id: null}})
                                }
                                _ => panic!("unexpected method {method}"),
                            };
                            Json(json!({"methodResponses": [[method, response, "c1"]]}))
                                .into_response()
                        }
                    },
                ),
            );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base_url, state)
    }

    #[tokio::test]
    async fn probes_required_capabilities_with_bearer_token() {
        let (base_url, observed) = server(true, "secret").await;
        let token_path = token_file("secret\n").await;
        let config =
            StalwartJmapConfig::new(&base_url, token_path.clone(), Duration::from_secs(2), true)
                .unwrap();
        let readiness = StalwartJmapClient::new(test_mail_data_plane_id(), config)
            .unwrap()
            .probe()
            .await
            .unwrap();
        assert_eq!(readiness.username, "probe@example.test");
        assert!(readiness.mail_capability_advertised);
        assert_eq!(observed.lock().await.as_slice(), ["Bearer secret"]);
        tokio::fs::remove_file(token_path).await.unwrap();
    }

    #[tokio::test]
    async fn accepts_management_key_without_mail_capability() {
        let (base_url, _) = server(false, "secret").await;
        let token_path = token_file("secret").await;
        let config =
            StalwartJmapConfig::new(&base_url, token_path.clone(), Duration::from_secs(2), true)
                .unwrap();
        let readiness = StalwartJmapClient::new(test_mail_data_plane_id(), config)
            .unwrap()
            .probe()
            .await
            .unwrap();
        assert!(!readiness.mail_capability_advertised);
        tokio::fs::remove_file(token_path).await.unwrap();
    }

    #[tokio::test]
    async fn domain_projection_is_idempotent_and_suspension_disables_without_destroying() {
        let (base_url, state) = projection_server("secret", None).await;
        let token_path = token_file("secret").await;
        let config =
            StalwartJmapConfig::new(&base_url, token_path.clone(), Duration::from_secs(2), true)
                .unwrap();
        let client = StalwartJmapClient::new(test_mail_data_plane_id(), config).unwrap();
        let name = MailDomainName::parse("mail.example.test").unwrap();

        let created = client.reconcile_domain(&name, None, true).await.unwrap();
        assert!(created.created);
        assert_eq!(created.engine_ref, "domain-1");
        assert!(state.lock().await.as_ref().unwrap().is_enabled);

        let replay = client.reconcile_domain(&name, None, true).await.unwrap();
        assert!(!replay.created);
        assert_eq!(replay.engine_ref, created.engine_ref);

        let disabled = client
            .reconcile_domain(&name, Some(&created.engine_ref), false)
            .await
            .unwrap();
        assert!(!disabled.created);
        assert!(!disabled.enabled);
        assert!(!state.lock().await.as_ref().unwrap().is_enabled);
        tokio::fs::remove_file(token_path).await.unwrap();
    }

    #[tokio::test]
    async fn domain_projection_rejects_engine_reference_bound_to_another_name() {
        let (base_url, _) = projection_server(
            "secret",
            Some(EngineDomain {
                id: "domain-1".into(),
                name: "other.example.test".into(),
                is_enabled: true,
            }),
        )
        .await;
        let token_path = token_file("secret").await;
        let config =
            StalwartJmapConfig::new(&base_url, token_path.clone(), Duration::from_secs(2), true)
                .unwrap();
        let error = StalwartJmapClient::new(test_mail_data_plane_id(), config)
            .unwrap()
            .reconcile_domain(
                &MailDomainName::parse("target.example.test").unwrap(),
                Some("domain-1"),
                true,
            )
            .await
            .unwrap_err();
        assert_eq!(error, MailEngineError::ProjectionConflict);
        tokio::fs::remove_file(token_path).await.unwrap();
    }

    #[test]
    fn client_exposes_configured_data_plane_id() {
        let data_plane_id = test_mail_data_plane_id();
        let config = StalwartJmapConfig::new(
            "https://mail.example.test",
            PathBuf::from("token"),
            Duration::from_secs(5),
            false,
        )
        .unwrap();
        let client = StalwartJmapClient::new(data_plane_id, config).unwrap();

        assert_eq!(client.data_plane_id(), data_plane_id);
    }

    #[test]
    fn rejects_insecure_or_path_scoped_origins() {
        assert_eq!(
            StalwartJmapConfig::new(
                "http://mail.example.test",
                PathBuf::from("token"),
                Duration::from_secs(5),
                false
            ),
            Err(MailEngineError::InvalidConfiguration)
        );
        assert_eq!(
            StalwartJmapConfig::new(
                "https://mail.example.test/admin",
                PathBuf::from("token"),
                Duration::from_secs(5),
                false
            ),
            Err(MailEngineError::InvalidConfiguration)
        );
    }

    #[test]
    fn rejects_cross_origin_or_credential_bearing_api_urls() {
        let config = StalwartJmapConfig::new(
            "https://mail.example.test",
            PathBuf::from("token"),
            Duration::from_secs(5),
            false,
        )
        .unwrap();
        let client = StalwartJmapClient::new(test_mail_data_plane_id(), config).unwrap();
        assert_eq!(
            client.validate_api_url("https://attacker.example/jmap"),
            Err(MailEngineError::InvalidSession)
        );
        assert_eq!(
            client.validate_api_url("https://user:password@mail.example.test/jmap"),
            Err(MailEngineError::InvalidSession)
        );
    }
}
