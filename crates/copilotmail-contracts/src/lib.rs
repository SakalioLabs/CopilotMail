//! Stable transport contracts shared by the V2 control-plane interfaces.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Liveness response that does not depend on external services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Always `ok` while the HTTP process can serve requests.
    pub status: String,
    /// Stable service name.
    pub service: String,
    /// Build version.
    pub version: String,
}

/// Readiness response including required dependency status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessResponse {
    /// `ready` only when every required dependency is available.
    pub status: String,
    /// Dependency name to health state.
    pub dependencies: BTreeMap<String, String>,
}

/// Public build and API compatibility information.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemInfoResponse {
    /// Versioned HTTP contract exposed by the service.
    pub api_version: String,
    /// Build version of the running binary.
    pub build_version: String,
    /// Identity model implemented by the V2 control plane.
    pub principal_model: String,
}

/// Current server-side Human browser session. No provider or session token is
/// ever reflected to browser JavaScript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanSessionResponse {
    pub principal_id: String,
    pub organization_id: String,
    pub client: String,
    pub expires_at: String,
}

/// Offline organization-bootstrap result printed by the control-plane CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationBootstrapResponse {
    /// Stable organization identifier.
    pub organization_id: String,
    /// Root service principal used to configure the first enterprise identity.
    pub root_principal_id: String,
    /// Canonical organization slug.
    pub slug: String,
    /// Human-readable organization name.
    pub display_name: String,
    /// True when records were created; false for an idempotent replay.
    pub created: bool,
}

/// Offline OIDC owner handover result printed by the control-plane CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcOwnerBindingResponse {
    /// Organization handed to the enterprise human.
    pub organization_id: String,
    /// Active human principal mapped to the OIDC subject.
    pub principal_id: String,
    /// Bootstrap service principal retired by the operation.
    pub retired_bootstrap_principal_id: String,
    /// Exact configured OIDC issuer.
    pub issuer: String,
    /// Exact stable OIDC subject.
    pub subject: String,
    /// True when the handover was created; false for an exact replay.
    pub created: bool,
}

/// One-time output returned after issuing a SCIM provisioning credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScimTokenIssuedResponse {
    pub organization_id: String,
    pub token_id: String,
    pub label: String,
    pub token: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScimTokenRevokedResponse {
    pub organization_id: String,
    pub token_id: String,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRegisteredResponse {
    pub organization_id: String,
    pub client_id: String,
    pub credential_id: String,
    pub owner_principal_id: String,
    pub allowed_scopes: Vec<String>,
    pub credential_expires_at: String,
    pub token_endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCredentialRevokedResponse {
    pub organization_id: String,
    pub credential_id: String,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCredentialAddedResponse {
    pub organization_id: String,
    pub client_id: String,
    pub credential_id: String,
    pub allowed_scopes: Vec<String>,
    pub credential_expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMtlsCredentialAddedResponse {
    pub organization_id: String,
    pub client_id: String,
    pub credential_id: String,
    pub credential_kind: String,
    pub certificate_fingerprint: String,
    pub allowed_scopes: Vec<String>,
    pub credential_expires_at: String,
}

/// Authorized organization metadata projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationResponse {
    /// Stable organization identifier.
    pub organization_id: String,
    /// Canonical organization slug.
    pub slug: String,
    /// Human-readable organization name.
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxResponse {
    pub mailbox_id: String,
    pub organization_id: String,
    pub kind: String,
    pub primary_address: String,
    pub created_at: String,
}

/// Minimal mailbox selector item disclosed to the Human Mail application.
///
/// Tenant, provider, binding, owner, and delegation internals deliberately do
/// not cross this contract boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanMailboxDirectoryItemResponse {
    pub mailbox_id: String,
    pub kind: String,
    pub primary_address: String,
    pub can_send: bool,
}

/// Cursor page for the authenticated Human's usable mailboxes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanMailboxDirectoryResponse {
    pub items: Vec<HumanMailboxDirectoryItemResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxAddressResponse {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxMessageSummaryResponse {
    /// Externally safe CopilotMail message UUID. Provider Email and Thread
    /// identifiers never cross this contract.
    pub message_id: String,
    pub subject: String,
    pub from: Vec<InboxAddressResponse>,
    pub received_at: String,
    pub preview: String,
    pub is_read: bool,
    pub is_flagged: bool,
    pub has_attachment: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxPageResponse {
    pub items: Vec<InboxMessageSummaryResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxSearchRequest {
    pub text: String,
    pub cursor: Option<String>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxAttachmentResponse {
    pub name: Option<String>,
    pub media_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageDetailResponse {
    pub message: InboxMessageSummaryResponse,
    /// Safe inbound UUID accepted by the explicit-reply draft API. It is null
    /// until the authenticated inbound event has been committed.
    pub reply_to_message_id: Option<String>,
    pub to: Vec<InboxAddressResponse>,
    pub cc: Vec<InboxAddressResponse>,
    pub text_body: String,
    pub text_body_truncated: bool,
    pub attachments: Vec<InboxAttachmentResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMailContentResponse {
    pub message_id: String,
    pub subject: String,
    pub from: Vec<InboxAddressResponse>,
    pub received_at: String,
    pub text_body: String,
    pub text_body_truncated: bool,
    pub has_attachment: bool,
    pub untrusted_external_content: bool,
    pub html_omitted: bool,
    pub attachments_omitted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProcessingSubscriptionResponse {
    pub subscription_id: String,
    pub organization_id: String,
    pub agent_id: String,
    pub mailbox_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProcessingSubscriptionChangeRequest {
    pub expected_status: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProcessingSubscriptionChangeResponse {
    pub operation_id: String,
    pub subscription_id: String,
    pub organization_id: String,
    pub agent_id: String,
    pub mailbox_id: String,
    pub previous_status: String,
    pub status: String,
    pub replayed: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMessageClaimRequest {
    pub lease_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMessageLeaseExtensionRequest {
    pub lease_id: String,
    pub lease_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMessageAcknowledgementRequest {
    pub lease_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMessageNackRequest {
    pub lease_id: String,
    pub reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessageProcessingResponse {
    pub operation_id: String,
    pub subscription_id: String,
    pub message_id: String,
    pub lease_id: String,
    pub operation: String,
    pub state: String,
    pub attempt_count: u32,
    pub lease_expires_at: Option<String>,
    pub reason_code: Option<String>,
    pub replayed: bool,
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProcessingDeadLetterResponse {
    pub message_id: String,
    pub attempt_count: u32,
    pub reason_code: String,
    pub failed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProcessingDeadLetterListResponse {
    pub items: Vec<AgentProcessingDeadLetterResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageStateUpdateRequest {
    pub is_read: Option<bool>,
    pub is_flagged: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageStateResponse {
    pub message_id: String,
    pub is_read: bool,
    pub is_flagged: bool,
}

/// Saves a bounded plain-text draft. The server derives `From` from the
/// authorized mailbox, so callers cannot inject or impersonate a sender.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextDraftSaveRequest {
    pub draft_id: Option<String>,
    pub expected_version: Option<i64>,
    /// Externally safe CopilotMail UUID of the inbound message being replied
    /// to. The server derives all provider and RFC reply identifiers.
    pub reply_to_message_id: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextDraftResponse {
    pub draft_id: String,
    pub version: i64,
    pub reply_to_message_id: Option<String>,
    pub previous_draft_destroyed: bool,
}

/// Minimal control-plane metadata for one active draft owned by the
/// authenticated Human. Message content, addresses, and provider identifiers
/// are deliberately omitted from list responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanDraftListItemResponse {
    pub draft_id: String,
    pub version: i64,
    pub reply_to_message_id: Option<String>,
    pub recipient_count: u16,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanDraftListResponse {
    pub items: Vec<HumanDraftListItemResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Latest immutable delivery evidence reduced to counters that are safe for a
/// Human's own sent-directory. Recipient identities, provider references, and
/// SMTP replies are intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanSentDeliverySummaryResponse {
    pub observed_at: String,
    pub terminal: bool,
    pub recipient_count: u16,
    pub delivered_count: u16,
    pub failed_count: u16,
    pub queued_count: u16,
    pub unknown_count: u16,
}

/// Content-free accepted-submission metadata for the authenticated Human.
/// This is deliberately not interchangeable with `MailSubmissionResponse`,
/// which is an internal-status DTO containing recipient evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanSentListItemResponse {
    pub submission_id: String,
    pub accepted_at: String,
    pub recipient_count: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<HumanSentDeliverySummaryResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanSentListResponse {
    pub items: Vec<HumanSentListItemResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Restores only the caller's exact active draft version. The server resolves
/// all provider references from its control-plane provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanDraftRestoreRequest {
    pub expected_version: i64,
}

/// Plain-text draft content returned only after provider-integrity checks and
/// atomic Audit/Outbox evidence succeed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanDraftRestoreResponse {
    pub draft_id: String,
    pub version: i64,
    pub reply_to_message_id: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub text_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailSubmissionCreateRequest {
    pub draft_id: String,
    pub expected_draft_version: i64,
    /// Required only when a production Agent addresses at least one contact
    /// that has no prior committed admission.
    pub first_contact_approval_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailSandboxCreateRequest {
    pub draft_id: String,
    pub expected_draft_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMailSandboxResponse {
    pub sandbox_submission_id: String,
    pub mailbox_id: String,
    pub draft_id: String,
    pub draft_version: i64,
    pub recipient_count: u16,
    pub delivery_suppressed: bool,
    pub captured_at: String,
    pub replayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailSubmissionResponse {
    pub submission_id: String,
    pub mailbox_id: String,
    pub draft_id: String,
    pub draft_version: i64,
    pub status: String,
    pub state_version: i64,
    pub created_at: String,
    pub updated_at: String,
    pub delivery: Option<MailDeliveryResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDeliveryResponse {
    pub observed_at: String,
    pub terminal: bool,
    pub recipient_count: usize,
    pub delivered_count: usize,
    pub failed_count: usize,
    pub queued_count: usize,
    pub unknown_count: usize,
    pub recipients: Vec<MailDeliveryRecipientResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDeliveryRecipientResponse {
    pub recipient_ref: String,
    pub status: String,
    pub smtp_reply: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailboxCreateRequest {
    pub kind: String,
    pub primary_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxCreateResponse {
    pub mailbox_id: String,
    pub organization_id: String,
    pub owner_principal_id: String,
    pub primary_address: String,
}

/// Human MailAdmin-only CAS update of the data-plane account mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailEngineMailboxBindingUpdateRequest {
    pub account_ref: String,
    pub expected_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailEngineMailboxBindingResponse {
    pub organization_id: String,
    pub mailbox_id: String,
    pub data_plane_id: String,
    pub account_ref: String,
    pub version: i64,
    pub updated_at: String,
}

/// Minimal acknowledgement returned to Stalwart; event data is never echoed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailEventIngressResponse {
    pub accepted: usize,
    pub inserted: usize,
    pub duplicates: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailDomainCreateRequest {
    pub name: String,
    pub route_mode: String,
}

/// Registration response. `ownership_token` is returned once and must be
/// handled as a secret by both Human and Agent clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainCreateResponse {
    pub mail_domain_id: String,
    pub organization_id: String,
    pub name: String,
    pub status: String,
    pub ownership_challenge_version: i64,
    pub route_version: i64,
    pub ownership_record_name: String,
    pub ownership_token: String,
}

/// Optimistic concurrency precondition for replacing an ownership challenge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailDomainChallengeRotationRequest {
    pub expected_version: i64,
}

/// One-time response returned after replacing a domain ownership challenge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainChallengeRotationResponse {
    pub mail_domain_id: String,
    pub status: String,
    pub ownership_challenge_version: i64,
    pub ownership_record_name: String,
    pub ownership_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailDomainDnsVerificationRequest {
    pub domain: String,
    pub ownership_token: String,
    pub dkim_selector: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainDnsVerificationResponse {
    pub mail_domain_id: String,
    pub status: String,
    pub activation_ready: bool,
    pub valid_until: String,
    pub failure_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainActionResponse {
    pub mail_domain_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailDomainSuspendRequest {
    pub reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailDomainRouteUpdateRequest {
    pub expected_version: i64,
    pub inbound_enabled: bool,
    pub outbound_enabled: bool,
    pub smart_host_ref: Option<String>,
    pub dkim_key_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainRouteResponse {
    pub mail_domain_id: String,
    pub version: i64,
    pub inbound_enabled: bool,
    pub outbound_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainResponse {
    pub mail_domain_id: String,
    pub organization_id: String,
    pub name: String,
    pub status: String,
    pub route_mode: String,
    pub ownership_challenge_version: i64,
    pub route_version: i64,
    pub inbound_enabled: bool,
    pub outbound_enabled: bool,
    pub smart_host_ref: Option<String>,
    pub dkim_key_ref: Option<String>,
    pub mail_engine_domain_ref: Option<String>,
    pub mail_engine_projection_status: String,
    pub mail_engine_projected_enabled: Option<bool>,
    pub mail_engine_projection_last_attempt_at: Option<String>,
    pub mail_engine_projection_succeeded_at: Option<String>,
    pub mail_engine_projection_error_code: Option<String>,
    pub latest_dns_observed_at: Option<String>,
    pub latest_dns_valid_until: Option<String>,
    pub latest_dns_activation_ready: bool,
    pub latest_dns_failure_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainListResponse {
    pub items: Vec<MailDomainResponse>,
    pub next_after: Option<String>,
}

/// Creates an outbound Agent webhook subscription. The signing field is a
/// server-side key reference; raw signing secrets are never accepted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookSubscriptionCreateRequest {
    pub principal_id: String,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
    pub signing_key_reference: String,
}

/// Optimistically replaces the mutable settings of one webhook subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookSubscriptionUpdateRequest {
    pub expected_version: i64,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
    pub signing_key_reference: String,
    pub enabled: bool,
}

/// Governed outbound webhook subscription projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookSubscriptionResponse {
    pub subscription_id: String,
    pub organization_id: String,
    pub principal_id: String,
    pub endpoint_url: String,
    pub event_types: Vec<String>,
    pub signing_key_reference: String,
    pub enabled: bool,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookSubscriptionListResponse {
    pub items: Vec<WebhookSubscriptionResponse>,
    pub next_after: Option<String>,
}

/// Sanitized latest dead-letter evidence. Receiver bodies and exception text
/// are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookDeadLetterResponse {
    pub delivery_id: String,
    pub subscription_id: String,
    pub principal_id: String,
    pub event_id: String,
    pub event_type: String,
    pub attempt: i32,
    pub response_status: Option<i32>,
    pub error_code: String,
    pub failed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookDeadLetterListResponse {
    pub items: Vec<WebhookDeadLetterResponse>,
    pub next_after: Option<String>,
}

/// Empty by design: replay is bound entirely to the path's immutable evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebhookDeadLetterReplayRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookDeadLetterReplayResponse {
    pub subscription_id: String,
    pub event_id: String,
    pub attempt: i32,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEventAggregateResponse {
    #[serde(rename = "type")]
    pub aggregate_type: String,
    pub id: String,
}

/// The same stable event envelope used for signed webhook delivery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEventEnvelopeResponse {
    pub id: String,
    pub organization_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub schema_version: i32,
    pub occurred_at: String,
    pub aggregate: AgentEventAggregateResponse,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEventCursorPageResponse {
    pub items: Vec<AgentEventEnvelopeResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEventResponse {
    pub event_id: String,
    pub organization_id: String,
    pub actor_principal_id: String,
    pub actor_kind: String,
    pub on_behalf_of_principal_id: Option<String>,
    pub credential_id: Option<String>,
    pub subject_principal_id: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub outcome: String,
    pub trace_id: String,
    pub request_id: String,
    pub source_ip: Option<String>,
    pub client_id: Option<String>,
    pub session_id: Option<String>,
    pub token_jti: Option<String>,
    pub policy_id: Option<String>,
    pub policy_version_id: Option<String>,
    pub approval_request_id: Option<String>,
    pub ticket_reference: Option<String>,
    pub environment: Option<String>,
    pub region: Option<String>,
    pub details: Value,
    pub occurred_at: String,
    pub received_at: String,
    /// Decimal PostgreSQL BIGINT encoded as text so JavaScript clients never
    /// lose evidence precision above Number.MAX_SAFE_INTEGER.
    pub chain_sequence: String,
    pub hash_version: i16,
    pub previous_hash: Option<String>,
    pub event_hash: String,
    pub signature_batch_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEventSearchResponse {
    pub query_id: String,
    pub occurred_from: String,
    pub occurred_to: String,
    pub items: Vec<AuditEventResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailboxDelegationCreateRequest {
    pub grantee_principal_id: String,
    pub capabilities: Vec<String>,
    pub reason: String,
    pub valid_until: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxDelegationResponse {
    pub delegation_id: String,
    pub mailbox_id: String,
    pub grantor_principal_id: String,
    pub grantee_principal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxDelegationRevokedResponse {
    pub delegation_id: String,
    pub revoked: bool,
}

/// Creates an approval request bound to the authenticated Principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRequestCreateRequest {
    pub policy_key: String,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub reason: String,
    pub ticket_reference: Option<String>,
    /// RFC 3339 instant; the server also enforces the policy's maximum window.
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequestResponse {
    pub approval_request_id: String,
    pub status: String,
}

/// Appends one irreversible approval decision by the authenticated Principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalDecisionRequest {
    pub decision: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecisionResponse {
    pub approval_request_id: String,
    pub status: String,
    pub approval_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequestSummaryResponse {
    pub approval_request_id: String,
    pub organization_id: String,
    pub policy_version_id: String,
    pub action: String,
    pub resource_kind: String,
    pub resource_id: String,
    pub requested_by: String,
    pub reason: String,
    pub ticket_reference: Option<String>,
    pub required_approvals: u8,
    pub approval_count: u8,
    pub rejection_count: u8,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub completed_at: Option<String>,
    pub cancelled_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalStepResponse {
    pub approval_step_id: String,
    pub approver_principal_id: String,
    pub decision: String,
    pub reason: String,
    pub decided_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequestDetailResponse {
    pub request: ApprovalRequestSummaryResponse,
    pub steps: Vec<ApprovalStepResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequestListResponse {
    pub items: Vec<ApprovalRequestSummaryResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Empty body: cancellation is bound to the authenticated original requester.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalCancellationRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleResponse {
    pub role_id: String,
    pub organization_id: String,
    pub system_role: String,
    pub display_name: String,
    pub permissions: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleListResponse {
    pub items: Vec<RoleResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleAssignmentGrantRequest {
    pub principal_id: String,
    pub role_id: String,
    pub valid_until: String,
    pub approval_request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAssignmentResponse {
    pub role_assignment_id: String,
    pub organization_id: String,
    pub principal_id: String,
    pub principal_kind: String,
    pub principal_status: String,
    pub principal_display_name: String,
    pub role_id: String,
    pub system_role: String,
    pub role_display_name: String,
    pub created_by: String,
    pub approval_request_id: Option<String>,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub revoked_at: Option<String>,
    pub created_at: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleAssignmentListResponse {
    pub items: Vec<RoleAssignmentResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Empty body: the assignment and revoking actor come from the protected path
/// and authenticated identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleAssignmentRevocationRequest {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComplianceCaseCreateRequest {
    pub external_reference: String,
    pub title: String,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplianceCaseResponse {
    pub case_id: String,
    pub organization_id: String,
    pub external_reference: String,
    pub title: String,
    pub purpose: String,
    pub status: String,
    pub created_by: String,
    pub created_at: String,
    pub closed_by: Option<String>,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplianceCaseListResponse {
    pub items: Vec<ComplianceCaseResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegalHoldApplyRequest {
    pub case_id: String,
    pub approval_request_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegalHoldReleaseRequest {
    pub approval_request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegalHoldResponse {
    pub legal_hold_id: String,
    pub organization_id: String,
    pub case_id: String,
    pub archive_id: String,
    pub reason: String,
    pub status: String,
    pub apply_approval_request_id: String,
    pub created_by: String,
    pub created_at: String,
    pub release_approval_request_id: Option<String>,
    pub released_by: Option<String>,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegalHoldListResponse {
    pub items: Vec<LegalHoldResponse>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

/// Authorizes retrieval of one exact immutable audit archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditExportGrantCreateRequest {
    pub approval_request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditExportGrantResponse {
    pub audit_export_grant_id: String,
    pub organization_id: String,
    pub archive_id: String,
    pub approval_request_id: String,
    pub requested_by: String,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicyCreateRequest {
    pub policy_key: String,
    pub display_name: String,
    pub resource_kind: String,
    pub minimum_retention_days: u32,
    pub archive_after_days: Option<u32>,
    pub delete_after_days: Option<u32>,
    pub rule: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicyVersionCreateRequest {
    pub minimum_retention_days: u32,
    pub archive_after_days: Option<u32>,
    pub delete_after_days: Option<u32>,
    pub rule: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicyActivationRequest {
    pub approval_request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailPolicyActivationRequest {
    pub approval_request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMailPolicyActivationResponse {
    pub activation_id: String,
    pub organization_id: String,
    pub agent_id: String,
    pub policy_version: u32,
    pub approval_request_id: String,
    pub activated_by: String,
    pub activated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailPolicyVersionCreateRequest {
    pub allowed_recipient_domains: Vec<String>,
    pub max_recipients_per_message: u16,
    pub max_messages_per_hour: u32,
    pub max_messages_per_recipient_per_day: u16,
    pub require_first_contact_approval: bool,
    pub max_auto_replies_per_thread_per_day: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMailPolicyVersionResponse {
    pub policy_version_id: String,
    pub organization_id: String,
    pub agent_id: String,
    pub version: u32,
    pub allowed_recipient_domains: Vec<String>,
    pub max_recipients_per_message: u16,
    pub max_messages_per_hour: u32,
    pub max_messages_per_recipient_per_day: u16,
    pub require_first_contact_approval: bool,
    pub max_auto_replies_per_thread_per_day: u8,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnAgentMailPolicyResponse {
    pub agent_id: String,
    pub environment: String,
    pub admission_state: String,
    pub effective_permissions: Vec<String>,
    pub active_policy: Option<OwnAgentMailPolicyLimitsResponse>,
    pub hourly_usage: OwnAgentMailHourlyUsageResponse,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnAgentMailPolicyLimitsResponse {
    pub version: u32,
    pub allowed_recipient_domains: Vec<String>,
    pub max_recipients_per_message: u16,
    pub max_messages_per_hour: u32,
    pub max_messages_per_recipient_per_day: u16,
    pub require_first_contact_approval: bool,
    pub max_auto_replies_per_thread_per_day: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnAgentMailHourlyUsageResponse {
    pub window_started_at: String,
    pub admitted_messages: u32,
    pub remaining_messages: Option<u32>,
}

/// Tenant-bound, privileged control-plane observation of one Agent.
///
/// This is deliberately not a runtime-health or execution-trace response.
/// It contains no credential material, mailbox grants, delegation data, or
/// mail content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInspectorResponse {
    pub agent_id: String,
    pub display_name: String,
    pub principal_status: String,
    pub owner: AgentInspectorOwnerResponse,
    pub profile: AgentInspectorProfileResponse,
    pub token_access: AgentInspectorTokenAccessResponse,
    pub control_plane: AgentInspectorControlPlaneResponse,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInspectorOwnerResponse {
    pub principal_id: String,
    pub display_name: String,
    pub principal_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInspectorProfileResponse {
    pub environment: String,
    pub risk_level: String,
    pub purpose: String,
    pub review_due_at: String,
    pub review_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInspectorTokenAccessResponse {
    pub state: String,
    pub effective_scopes: Vec<String>,
    pub active_credential_count: u32,
    pub next_credential_expiry_at: Option<String>,
    pub last_authenticated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentInspectorControlPlaneResponse {
    pub mail_admission_state: String,
    pub active_processing_subscription_count: u32,
    pub paused_processing_subscription_count: u32,
    pub retired_processing_subscription_count: u32,
    pub last_processing_activity_at: Option<String>,
}

/// Human-only, exact evidence chain for one production Agent submission.
///
/// This is not a submission directory, recipient view, runtime signal,
/// execution trace, delegation record, or delivery-success assertion. The
/// response carries only immutable admission/approval facts, optional latest
/// aggregate delivery observation, and minimal audit references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailSubmissionEvidenceResponse {
    pub agent_id: String,
    pub submission: AgentMailSubmissionEvidenceSubmissionResponse,
    pub admission: AgentMailSubmissionEvidenceAdmissionResponse,
    pub delivery: Option<AgentMailSubmissionEvidenceDeliveryResponse>,
    pub audit_references: Vec<AgentMailSubmissionEvidenceAuditReferenceResponse>,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailSubmissionEvidenceSubmissionResponse {
    pub submission_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailSubmissionEvidenceAdmissionResponse {
    pub admission_id: String,
    pub admitted_at: String,
    pub policy: AgentMailSubmissionEvidencePolicyResponse,
    pub recipient_count: u16,
    pub first_contact_count: u16,
    pub first_contact_approval_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailSubmissionEvidencePolicyResponse {
    pub activation_id: String,
    pub activated_at: String,
    pub version_id: String,
    pub version: u32,
    pub approval_request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailSubmissionEvidenceDeliveryResponse {
    pub observed_at: String,
    pub terminal: bool,
    pub recipient_count: u16,
    pub delivered_count: u16,
    pub failed_count: u16,
    pub queued_count: u16,
    pub unknown_count: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentMailSubmissionEvidenceAuditReferenceResponse {
    pub event_id: String,
    pub action: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetentionPolicyVersionResponse {
    pub retention_policy_version_id: String,
    pub version: u32,
    pub minimum_retention_days: u32,
    pub archive_after_days: Option<u32>,
    pub delete_after_days: Option<u32>,
    pub rule: serde_json::Value,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetentionPolicyResponse {
    pub retention_policy_id: String,
    pub organization_id: String,
    pub policy_key: String,
    pub display_name: String,
    pub resource_kind: String,
    pub active_version: Option<RetentionPolicyVersionResponse>,
    pub latest_version: RetentionPolicyVersionResponse,
    pub created_by: String,
    pub created_at: String,
}

/// Consistent machine-readable API error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Stable code suitable for programmatic handling.
    pub code: String,
    /// Human-readable summary safe to display.
    pub message: String,
    /// Identifier echoed in the `x-request-id` response header.
    pub request_id: String,
    /// Distributed trace identifier; equal to request ID until tracing is sampled.
    pub trace_id: String,
    /// Whether retrying the same request later may succeed.
    pub retryable: bool,
    /// Optional non-secret structured context.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub details: Value,
}

#[cfg(test)]
mod tests {
    use super::{
        AgentInspectorResponse, AgentMailPolicyActivationRequest,
        AgentMailPolicyVersionCreateRequest, AgentMailSandboxCreateRequest,
        AgentMailSubmissionEvidenceResponse, AgentMessageAcknowledgementRequest,
        AgentMessageClaimRequest, AgentMessageLeaseExtensionRequest, AgentMessageNackRequest,
        AgentProcessingSubscriptionChangeRequest, ApprovalCancellationRequest, AuditEventResponse,
        AuditExportGrantCreateRequest, HumanDraftListResponse, HumanDraftRestoreRequest,
        HumanDraftRestoreResponse, HumanMailboxDirectoryResponse, HumanSentListResponse,
        InboxMessageSummaryResponse, InboxSearchRequest, MailEngineMailboxBindingUpdateRequest,
        MailSubmissionCreateRequest, MessageDetailResponse, MessageStateResponse,
        RetentionPolicyActivationRequest, RetentionPolicyCreateRequest, RoleAssignmentGrantRequest,
        RoleAssignmentRevocationRequest, TextDraftSaveRequest,
    };
    use serde_json::json;

    #[test]
    fn audit_chain_sequence_round_trips_above_javascript_safe_integer() {
        let value = json!({
            "event_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "organization_id": "018f13b9-2222-7bc8-965d-48a28866f073",
            "actor_principal_id": "018f13b9-3333-7bc8-965d-48a28866f073",
            "actor_kind": "human",
            "on_behalf_of_principal_id": null,
            "credential_id": null,
            "subject_principal_id": null,
            "action": "audit.events.queried",
            "resource_type": "audit.event_query",
            "resource_id": "018f13b9-4444-7bc8-965d-48a28866f073",
            "outcome": "success",
            "trace_id": "trace-1",
            "request_id": "request-1",
            "source_ip": null,
            "client_id": null,
            "session_id": null,
            "token_jti": null,
            "policy_id": null,
            "policy_version_id": null,
            "approval_request_id": null,
            "ticket_reference": null,
            "environment": "testing",
            "region": null,
            "details": {},
            "occurred_at": "2026-07-26T00:00:00Z",
            "received_at": "2026-07-26T00:00:00Z",
            "chain_sequence": "9007199254740993",
            "hash_version": 1,
            "previous_hash": null,
            "event_hash": "00".repeat(32),
            "signature_batch_id": null
        });

        let event: AuditEventResponse = serde_json::from_value(value).unwrap();
        assert_eq!(event.chain_sequence, "9007199254740993");
        assert_eq!(
            serde_json::to_value(event).unwrap()["chain_sequence"],
            "9007199254740993"
        );
    }

    #[test]
    fn agent_inspector_response_rejects_unexpected_sensitive_fields() {
        let response = json!({
            "agent_id": "019b0000-0000-7000-8000-000000000101",
            "display_name": "Inbound triage",
            "principal_status": "active",
            "owner": {
                "principal_id": "019b0000-0000-7000-8000-000000000102",
                "display_name": "Owner",
                "principal_status": "active"
            },
            "profile": {
                "environment": "production",
                "risk_level": "medium",
                "purpose": "Triage inbound support mail",
                "review_due_at": "2026-09-01T00:00:00Z",
                "review_state": "scheduled"
            },
            "token_access": {
                "state": "effective",
                "effective_scopes": ["organization.read"],
                "active_credential_count": 1,
                "next_credential_expiry_at": "2026-09-15T00:00:00Z",
                "last_authenticated_at": null
            },
            "control_plane": {
                "mail_admission_state": "active",
                "active_processing_subscription_count": 1,
                "paused_processing_subscription_count": 0,
                "retired_processing_subscription_count": 0,
                "last_processing_activity_at": null
            },
            "observed_at": "2026-08-23T09:01:00Z",
            "credential_fingerprint": "must-not-be-contractual"
        });
        assert!(serde_json::from_value::<AgentInspectorResponse>(response).is_err());
    }

    #[test]
    fn agent_mail_submission_evidence_rejects_sensitive_or_unknown_fields() {
        let base = json!({
            "agent_id": "019b0000-0000-7000-8000-000000000101",
            "submission": {
                "submission_id": "019b0000-0000-7000-8000-000000000102",
                "status": "accepted",
                "created_at": "2026-08-24T09:00:00Z",
                "updated_at": "2026-08-24T09:01:00Z"
            },
            "admission": {
                "admission_id": "019b0000-0000-7000-8000-000000000103",
                "admitted_at": "2026-08-24T09:00:01Z",
                "policy": {
                    "activation_id": "019b0000-0000-7000-8000-000000000104",
                    "activated_at": "2026-08-24T08:00:00Z",
                    "version_id": "019b0000-0000-7000-8000-000000000105",
                    "version": 2,
                    "approval_request_id": "019b0000-0000-7000-8000-000000000106"
                },
                "recipient_count": 2,
                "first_contact_count": 1,
                "first_contact_approval_request_id": "019b0000-0000-7000-8000-000000000107"
            },
            "delivery": {
                "observed_at": "2026-08-24T09:02:00Z",
                "terminal": false,
                "recipient_count": 2,
                "delivered_count": 1,
                "failed_count": 0,
                "queued_count": 1,
                "unknown_count": 0
            },
            "audit_references": [{
                "event_id": "019b0000-0000-7000-8000-000000000108",
                "action": "mail.submission.requested",
                "occurred_at": "2026-08-24T09:00:00Z"
            }, {
                "event_id": "019b0000-0000-7000-8000-000000000109",
                "action": "agent.mail.admitted",
                "occurred_at": "2026-08-24T09:00:01Z"
            }, {
                "event_id": "019b0000-0000-7000-8000-000000000110",
                "action": "mail.submission.accepted",
                "occurred_at": "2026-08-24T09:01:00Z"
            }],
            "observed_at": "2026-08-24T09:03:00Z"
        });
        assert!(
            serde_json::from_value::<AgentMailSubmissionEvidenceResponse>(base.clone()).is_ok()
        );

        for leaked_field in [
            "recipient",
            "smtp_reply",
            "provider",
            "mailbox_id",
            "execution",
        ] {
            let mut leaked = base.clone();
            leaked[leaked_field] = json!("must-not-cross-contract");
            assert!(
                serde_json::from_value::<AgentMailSubmissionEvidenceResponse>(leaked).is_err(),
                "{leaked_field} must not cross the Agent submission evidence contract"
            );
        }
    }

    #[test]
    fn audit_export_grant_request_rejects_unrecognized_fields() {
        let value = json!({
            "approval_request_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "archive_id": "018f13b9-2222-7bc8-965d-48a28866f073"
        });
        assert!(serde_json::from_value::<AuditExportGrantCreateRequest>(value).is_err());
    }

    #[test]
    fn mail_engine_binding_rejects_caller_selected_data_plane() {
        let valid = json!({"account_ref": "42", "expected_version": 0});
        assert!(serde_json::from_value::<MailEngineMailboxBindingUpdateRequest>(valid).is_ok());

        let caller_selected_source =
            json!({"source": "stalwart", "account_ref": "42", "expected_version": 0});
        assert!(
            serde_json::from_value::<MailEngineMailboxBindingUpdateRequest>(caller_selected_source)
                .is_err()
        );

        let caller_selected_data_plane = json!({
            "data_plane_id": "018f305d-3d0f-7b2a-9d68-7f4a3f6e2101",
            "account_ref": "42",
            "expected_version": 0
        });
        assert!(
            serde_json::from_value::<MailEngineMailboxBindingUpdateRequest>(
                caller_selected_data_plane
            )
            .is_err()
        );
    }

    #[test]
    fn human_mailbox_directory_is_a_minimized_closed_contract() {
        let valid = json!({
            "items": [{
                "mailbox_id": "018f13b9-1111-7bc8-965d-48a28866f073",
                "kind": "shared",
                "primary_address": "support@example.test",
                "can_send": true
            }],
            "next_cursor": "v1.018f13b9-2222-7bc8-965d-48a28866f073",
            "has_more": true
        });
        assert!(serde_json::from_value::<HumanMailboxDirectoryResponse>(valid).is_ok());

        for leaked_field in ["organization_id", "provider_id", "delegation_id"] {
            let mut leaked = json!({
                "items": [{
                    "mailbox_id": "018f13b9-1111-7bc8-965d-48a28866f073",
                    "kind": "shared",
                    "primary_address": "support@example.test",
                    "can_send": true
                }],
                "next_cursor": null,
                "has_more": false
            });
            leaked["items"][0][leaked_field] = json!("must-not-cross-contract");
            assert!(
                serde_json::from_value::<HumanMailboxDirectoryResponse>(leaked).is_err(),
                "{leaked_field} must not be accepted by the Human mailbox directory contract"
            );
        }
    }

    #[test]
    fn agent_processing_mutations_reject_unrecognized_fields() {
        let lifecycle = json!({
            "expected_status": "active",
            "status": "paused",
            "principal_id": "caller-controlled"
        });
        assert!(
            serde_json::from_value::<AgentProcessingSubscriptionChangeRequest>(lifecycle).is_err()
        );
        let claim = json!({"lease_seconds": 60, "message_id": "caller-controlled"});
        assert!(serde_json::from_value::<AgentMessageClaimRequest>(claim).is_err());
        let extension = json!({
            "lease_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "lease_seconds": 60,
            "expires_at": "caller-controlled"
        });
        assert!(serde_json::from_value::<AgentMessageLeaseExtensionRequest>(extension).is_err());
        let acknowledgement = json!({
            "lease_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "processed_by": "caller-controlled"
        });
        assert!(
            serde_json::from_value::<AgentMessageAcknowledgementRequest>(acknowledgement).is_err()
        );
        let nack = json!({
            "lease_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "reason_code": "temporary_failure",
            "raw_error": "must not cross the API boundary"
        });
        assert!(serde_json::from_value::<AgentMessageNackRequest>(nack).is_err());
    }

    #[test]
    fn retention_mutations_reject_unrecognized_fields() {
        let create = json!({
            "policy_key": "audit.standard",
            "display_name": "Standard",
            "resource_kind": "audit_evidence",
            "minimum_retention_days": 2555,
            "archive_after_days": 30,
            "delete_after_days": null,
            "rule": {},
            "active": true
        });
        assert!(serde_json::from_value::<RetentionPolicyCreateRequest>(create).is_err());
        let activation = json!({
            "approval_request_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "policy_version_id": "018f13b9-2222-7bc8-965d-48a28866f073"
        });
        assert!(serde_json::from_value::<RetentionPolicyActivationRequest>(activation).is_err());
        let agent_activation = json!({
            "approval_request_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "agent_id": "018f13b9-2222-7bc8-965d-48a28866f073"
        });
        assert!(
            serde_json::from_value::<AgentMailPolicyActivationRequest>(agent_activation).is_err()
        );
        let agent_version = json!({
            "allowed_recipient_domains": ["internal.example"],
            "max_recipients_per_message": 5,
            "max_messages_per_hour": 100,
            "max_messages_per_recipient_per_day": 10,
            "require_first_contact_approval": true,
            "max_auto_replies_per_thread_per_day": 2,
            "active": true
        });
        assert!(
            serde_json::from_value::<AgentMailPolicyVersionCreateRequest>(agent_version).is_err()
        );
    }

    #[test]
    fn draft_request_rejects_caller_selected_sender_and_unknown_fields() {
        let value = json!({
            "draft_id": null,
            "expected_version": null,
            "from": "attacker@example.test",
            "to": ["recipient@example.test"],
            "cc": [],
            "bcc": [],
            "subject": "Status",
            "text_body": "Ready."
        });
        assert!(serde_json::from_value::<TextDraftSaveRequest>(value).is_err());
    }

    #[test]
    fn human_draft_restore_contracts_exclude_provider_and_sender_fields() {
        let request = json!({"expected_version": 4});
        assert!(serde_json::from_value::<HumanDraftRestoreRequest>(request).is_ok());
        assert!(
            serde_json::from_value::<HumanDraftRestoreRequest>(json!({
                "expected_version": 4,
                "engine_email_ref": "provider-secret"
            }))
            .is_err()
        );

        let list = json!({
            "items": [{
                "draft_id": "018f13b9-1111-7bc8-965d-48a28866f073",
                "version": 4,
                "reply_to_message_id": null,
                "recipient_count": 2,
                "created_at": "2026-08-13T00:00:00Z",
                "updated_at": "2026-08-13T00:01:00Z"
            }],
            "next_cursor": null,
            "has_more": false
        });
        assert!(serde_json::from_value::<HumanDraftListResponse>(list).is_ok());

        let mut restored = json!({
            "draft_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "version": 4,
            "reply_to_message_id": null,
            "to": ["recipient@example.test"],
            "cc": [],
            "bcc": [],
            "subject": "Status",
            "text_body": "Ready."
        });
        assert!(serde_json::from_value::<HumanDraftRestoreResponse>(restored.clone()).is_ok());
        for leaked_field in [
            "from",
            "engine_account_ref",
            "engine_email_ref",
            "thread_id",
        ] {
            restored[leaked_field] = json!("must-not-cross-contract");
            assert!(
                serde_json::from_value::<HumanDraftRestoreResponse>(restored.clone()).is_err(),
                "{leaked_field} must not cross the Human draft restore contract"
            );
            restored.as_object_mut().unwrap().remove(leaked_field);
        }
    }

    #[test]
    fn human_sent_directory_contract_excludes_content_and_delivery_evidence() {
        let directory = json!({
            "items": [{
                "submission_id": "018f13b9-1111-7bc8-965d-48a28866f073",
                "accepted_at": "2026-08-22T00:00:00Z",
                "recipient_count": 2,
                "delivery": {
                    "observed_at": "2026-08-22T00:01:00Z",
                    "terminal": false,
                    "recipient_count": 2,
                    "delivered_count": 1,
                    "failed_count": 0,
                    "queued_count": 1,
                    "unknown_count": 0
                }
            }],
            "next_cursor": null,
            "has_more": false
        });
        assert!(serde_json::from_value::<HumanSentListResponse>(directory.clone()).is_ok());

        for leaked_field in [
            "draft_id",
            "draft_version",
            "subject",
            "recipient_ref",
            "smtp_reply",
            "engine_account_ref",
            "mail_data_plane_id",
        ] {
            let mut leaked = directory.clone();
            leaked["items"][0][leaked_field] = json!("must-not-cross-contract");
            assert!(
                serde_json::from_value::<HumanSentListResponse>(leaked).is_err(),
                "{leaked_field} must not cross the Human sent-directory contract"
            );
        }
    }

    #[test]
    fn inbox_search_request_rejects_caller_selected_engine_fields() {
        let valid = serde_json::from_value::<InboxSearchRequest>(json!({
            "text": "quarterly report",
            "cursor": null,
            "page_size": 30
        }))
        .unwrap();
        assert_eq!(valid.text, "quarterly report");
        assert!(
            serde_json::from_value::<InboxSearchRequest>(json!({
                "text": "quarterly report",
                "cursor": null,
                "page_size": 30,
                "account_ref": "engine-secret"
            }))
            .is_err()
        );
    }

    #[test]
    fn inbox_summary_exposes_only_the_safe_message_uuid() {
        let value = serde_json::to_value(InboxMessageSummaryResponse {
            message_id: "018f13b9-1111-7bc8-965d-48a28866f073".into(),
            subject: "Status".into(),
            from: vec![],
            received_at: "2026-07-27T00:00:00Z".into(),
            preview: "Ready.".into(),
            is_read: false,
            is_flagged: false,
            has_attachment: false,
        })
        .unwrap();

        assert_eq!(value["message_id"], "018f13b9-1111-7bc8-965d-48a28866f073");
        assert!(value.get("id").is_none());
        assert!(value.get("thread_id").is_none());
    }

    #[test]
    fn human_mail_responses_reject_unknown_provider_fields() {
        let summary = json!({
            "message_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "subject": "Status",
            "from": [],
            "received_at": "2026-07-27T00:00:00Z",
            "preview": "Ready.",
            "is_read": false,
            "is_flagged": false,
            "has_attachment": false
        });
        let mut provider_summary = summary.clone();
        provider_summary["threadId"] = json!("provider-thread");
        assert!(serde_json::from_value::<InboxMessageSummaryResponse>(provider_summary).is_err());

        let mut detail = json!({
            "message": summary,
            "reply_to_message_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "to": [],
            "cc": [],
            "text_body": "Ready.",
            "text_body_truncated": false,
            "attachments": []
        });
        detail["blobId"] = json!("provider-blob");
        assert!(serde_json::from_value::<MessageDetailResponse>(detail).is_err());

        assert!(
            serde_json::from_value::<MessageStateResponse>(json!({
                "message_id": "018f13b9-1111-7bc8-965d-48a28866f073",
                "is_read": true,
                "is_flagged": false,
                "accountId": "provider-account"
            }))
            .is_err()
        );
    }

    #[test]
    fn submission_request_rejects_body_idempotency_keys() {
        let value = json!({
            "draft_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "expected_draft_version": 2,
            "idempotency_key": "caller-must-use-the-header"
        });
        assert!(serde_json::from_value::<MailSubmissionCreateRequest>(value).is_err());
        let valid = serde_json::from_value::<MailSubmissionCreateRequest>(json!({
            "draft_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "expected_draft_version": 2,
            "first_contact_approval_request_id":
                "018f13b9-2222-7bc8-965d-48a28866f073"
        }))
        .unwrap();
        assert_eq!(
            valid.first_contact_approval_request_id.as_deref(),
            Some("018f13b9-2222-7bc8-965d-48a28866f073")
        );
    }

    #[test]
    fn sandbox_request_accepts_only_an_exact_draft_version_binding() {
        let valid = serde_json::from_value::<AgentMailSandboxCreateRequest>(json!({
            "draft_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "expected_draft_version": 2
        }))
        .unwrap();
        assert_eq!(valid.expected_draft_version, 2);
        assert!(
            serde_json::from_value::<AgentMailSandboxCreateRequest>(json!({
                "draft_id": "018f13b9-1111-7bc8-965d-48a28866f073",
                "expected_draft_version": 2,
                "deliver": true
            }))
            .is_err()
        );
    }

    #[test]
    fn approval_cancellation_body_cannot_change_the_bound_actor_or_request() {
        assert!(serde_json::from_value::<ApprovalCancellationRequest>(json!({})).is_ok());
        assert!(
            serde_json::from_value::<ApprovalCancellationRequest>(json!({
                "requested_by": "018f13b9-1111-7bc8-965d-48a28866f073"
            }))
            .is_err()
        );
    }

    #[test]
    fn role_assignment_mutations_reject_caller_selected_governance_fields() {
        let grant = json!({
            "principal_id": "018f13b9-1111-7bc8-965d-48a28866f073",
            "role_id": "018f13b9-2222-7bc8-965d-48a28866f073",
            "valid_until": "2026-08-01T00:00:00.000Z",
            "approval_request_id": "018f13b9-3333-7bc8-965d-48a28866f073",
            "granted_by": "018f13b9-4444-7bc8-965d-48a28866f073"
        });
        assert!(serde_json::from_value::<RoleAssignmentGrantRequest>(grant).is_err());
        assert!(serde_json::from_value::<RoleAssignmentRevocationRequest>(json!({})).is_ok());
        assert!(
            serde_json::from_value::<RoleAssignmentRevocationRequest>(json!({
                "revoked_at": "2026-08-01T00:00:00Z"
            }))
            .is_err()
        );
    }
}
