//! V2 HTTP control plane and durable background worker.

#![forbid(unsafe_code)]

mod tls;

use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    Extension, Form, Json, Router,
    body::to_bytes,
    extract::{
        FromRequestParts, OriginalUri, Path, Query, Request, State, rejection::FormRejection,
    },
    http::{HeaderMap, HeaderName, Method, StatusCode, header, request::Parts},
    middleware::{self, Next},
    response::{
        IntoResponse, Redirect, Response, Sse,
        sse::{Event as SseEvent, KeepAlive},
    },
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use copilotmail_agent_auth::{
    AgentOAuthConfig, AgentSigningKeyInput, AgentSigningKeyManager, AgentTokenService, TokenError,
    TokenRequest, rsa_public_key_fingerprint,
};
use copilotmail_application::mail_ports::{
    MailAddress, MailContentReader, MailDomainProjector, MailDraftContentReader, MailDraftStore,
    MailEngine, MailEngineError, MailSubmissionAdapter,
};
use copilotmail_application::{
    AccessTokenAuthenticator, AcknowledgeAgentMessageCommand, ActivateAgentMailPolicyCommand,
    ActivateMailDomainCommand, ActivateRetentionPolicyVersionCommand, AddAgentCredentialCommand,
    AddAgentCredentialService, AddAgentMtlsCredentialCommand, AddAgentMtlsCredentialService,
    AgentCursorEvent, AgentEventCursorError, AgentEventCursorQueryStore, AgentEventCursorService,
    AgentEventStreamLease, AgentIngressRateLimitDecision, AgentIngressRateLimitError,
    AgentIngressRateLimiter, AgentMailAdmissionFailure, AgentMailContentError,
    AgentMailContentService, AgentMailPolicyError, AgentMailPolicyQueryError,
    AgentMailPolicyQueryService, AgentMailPolicyRuntimeSnapshot, AgentMailPolicyService,
    AgentMailSandboxError, AgentMailSandboxService, AgentMessageProcessingResult,
    AgentProcessingError, AgentProcessingService, AgentProcessingSubscriptionStatus,
    AppendApprovalPolicyVersionCommand, AppendRetentionPolicyVersionCommand, ApplyLegalHoldCommand,
    ApprovalPolicyService, ApprovalProof, ApprovalQueryStore, ApprovalRequestDetail,
    ApprovalRequestSummary, ApprovalRequestView, ApprovalWorkflowError, ApprovalWorkflowService,
    ArchivedObjectRecord, AuditArchiveRecord, AuditCheckpointQueryStore, AuditEventSearchError,
    AuditEventSearchService, AuditEventSummary, AuditExportArtifact, AuditExportAuthorizationError,
    AuditExportAuthorizationService, AuditExportDownloadError, AuditExportDownloadService,
    AuditExportQueryStore, AuthenticatedIdentity, AuthenticatedPrincipalKind,
    AuthenticationContext, AuthenticationError, AuthorizationStore, AuthorizeAuditExportCommand,
    AuthorizeMailboxActionService, BeginHumanLoginCommand, BindMailEngineMailboxCommand,
    BindOidcOwnerCommand, BindOidcOwnerService, BootstrapOrganizationCommand,
    BootstrapOrganizationService, CancelApprovalRequestCommand, CaptureAgentMailSandboxCommand,
    ChangeAgentProcessingSubscriptionCommand, ClaimAgentMessageCommand, CloseComplianceCaseCommand,
    ComplianceGovernanceService, ComplianceQueryStore, ComplianceReadKind,
    ConsumeHumanLoginCommand, CreateAgentMailPolicyVersionCommand, CreateApprovalPolicyCommand,
    CreateAuditCheckpointCommand, CreateAuditCheckpointService, CreateComplianceCaseCommand,
    CreateHumanSessionCommand, CreateMailboxCommand, CreateRetentionPolicyCommand,
    CreateWebhookSubscriptionCommand, DecideApprovalCommand, DraftError, DraftService,
    EnsureAgentProcessingSubscriptionCommand, ExecuteMailSubmissionCommand,
    ExtendAgentMessageLeaseCommand, GetAgentMailPolicyVersionCommand, GetApprovalRequestCommand,
    GetMessageCommand, GetOwnAgentMailPolicyCommand, GetRetentionPolicyCommand,
    GrantMailboxDelegationCommand, GrantRoleAssignmentCommand, HumanDraftError, HumanDraftService,
    HumanInboxMessageSummary, HumanMailboxDirectoryError, HumanMailboxDirectoryService,
    HumanSessionClient, HumanSessionError, HumanSessionRevocationCause, HumanSessionService,
    InboxError, InboxService, IngestMailEngineEventsCommand, IssueScimTokenCommand,
    IssueScimTokenService, LegalHoldObjectEvidence, ListAgentProcessingDeadLettersCommand,
    ListApprovalRequestsCommand, ListHumanDraftsCommand, ListHumanMailboxDirectoryCommand,
    ListInboxCommand, ListRoleAssignmentsCommand, ListRolesCommand, MailDataPlaneFingerprintScheme,
    MailDeliveryQueryStore, MailDeliveryRecipientState, MailDeliveryReconciliationResult,
    MailDeliveryReconciliationService, MailDomainGovernanceError, MailDomainGovernanceService,
    MailDomainQueryStore, MailDomainSummary, MailEngineEventIngestionService,
    MailEngineMailboxBindingService, MailEventFingerprintScheme, MailEventIngestionError,
    MailSubmissionError, MailSubmissionExecutionResult, MailSubmissionExecutor,
    MailSubmissionQueryStore, MailSubmissionService, MailSubmissionStatus,
    MailboxDelegationService, MailboxManagementService, MailboxQueryStore, NackAgentMessageCommand,
    OpenAgentEventStreamCommand, OrganizationQueryStore, PullAgentEventsCommand,
    ReadAgentMailContentCommand, ReconcileMailDeliveryCommand, RecordAuditArchiveCommand,
    RecordAuditArchiveService, RecordAuditExportDownloadCommand, RecordComplianceReadCommand,
    RecordMailDomainDnsCommand, RegisterAgentCommand, RegisterAgentService,
    RegisterMailDomainCommand, ReleaseLegalHoldCommand, ReplayWebhookDeadLetterCommand,
    ReplayWebhookDeadLetterError, RequestApprovalCommand, RequestMailSubmissionCommand,
    ResolveAuditExportObjectCommand, ResolveHumanSessionCommand, RestoreHumanDraftCommand,
    RetentionPolicyError, RetentionPolicyService, RevokeAgentCredentialCommand,
    RevokeAgentCredentialService, RevokeHumanSessionCommand, RevokeMailboxDelegationCommand,
    RevokeRoleAssignmentCommand, RevokeScimTokenCommand, RevokeScimTokenService,
    RoleAssignmentListQuery, RoleAssignmentSummary, RoleGovernanceError, RoleGovernanceQueryStore,
    RoleGovernanceService, RoleSummary, RotateMailDomainChallengeCommand, SaveTextDraftCommand,
    SearchAuditEventsCommand, SearchInboxCommand, SuspendMailDomainCommand,
    UpdateMailDomainRouteCommand, UpdateMessageStateCommand, UpdateWebhookSubscriptionCommand,
    WebhookDeadLetterQueryStore, WebhookDeadLetterService, WebhookDeadLetterSummary,
    WebhookSubscriptionError, WebhookSubscriptionQueryStore, WebhookSubscriptionService,
    WebhookSubscriptionSummary, role_assignment_approval_resource_id,
};
use copilotmail_audit::{
    AuditExportManifest, AuditExportRecord, AuditExportVerificationReport, AuditExportVerifier,
    RsaAuditCheckpointSigner,
};
use copilotmail_contracts::{
    AgentCredentialAddedResponse, AgentCredentialRevokedResponse, AgentEventAggregateResponse,
    AgentEventCursorPageResponse, AgentEventEnvelopeResponse, AgentMailContentResponse,
    AgentMailPolicyActivationRequest, AgentMailPolicyActivationResponse,
    AgentMailPolicyVersionCreateRequest, AgentMailPolicyVersionResponse,
    AgentMailSandboxCreateRequest, AgentMailSandboxResponse, AgentMessageAcknowledgementRequest,
    AgentMessageClaimRequest, AgentMessageLeaseExtensionRequest, AgentMessageNackRequest,
    AgentMessageProcessingResponse, AgentMtlsCredentialAddedResponse,
    AgentProcessingDeadLetterListResponse, AgentProcessingDeadLetterResponse,
    AgentProcessingSubscriptionChangeRequest, AgentProcessingSubscriptionChangeResponse,
    AgentProcessingSubscriptionResponse, AgentRegisteredResponse, ApprovalCancellationRequest,
    ApprovalDecisionRequest, ApprovalDecisionResponse, ApprovalRequestCreateRequest,
    ApprovalRequestDetailResponse, ApprovalRequestListResponse, ApprovalRequestResponse,
    ApprovalRequestSummaryResponse, ApprovalStepResponse, AuditEventResponse,
    AuditEventSearchResponse, AuditExportGrantCreateRequest, AuditExportGrantResponse,
    ComplianceCaseCreateRequest, ComplianceCaseListResponse, ComplianceCaseResponse, ErrorResponse,
    HealthResponse, HumanDraftListItemResponse, HumanDraftListResponse, HumanDraftRestoreRequest,
    HumanDraftRestoreResponse, HumanMailboxDirectoryItemResponse, HumanMailboxDirectoryResponse,
    HumanSessionResponse, InboxAddressResponse, InboxAttachmentResponse,
    InboxMessageSummaryResponse, InboxPageResponse, InboxSearchRequest, LegalHoldApplyRequest,
    LegalHoldListResponse, LegalHoldReleaseRequest, LegalHoldResponse,
    MailDeliveryRecipientResponse, MailDeliveryResponse, MailDomainActionResponse,
    MailDomainChallengeRotationRequest, MailDomainChallengeRotationResponse,
    MailDomainCreateRequest, MailDomainCreateResponse, MailDomainDnsVerificationRequest,
    MailDomainDnsVerificationResponse, MailDomainListResponse, MailDomainResponse,
    MailDomainRouteResponse, MailDomainRouteUpdateRequest, MailDomainSuspendRequest,
    MailEngineMailboxBindingResponse, MailEngineMailboxBindingUpdateRequest,
    MailEventIngressResponse, MailSubmissionCreateRequest, MailSubmissionResponse,
    MailboxCreateRequest, MailboxCreateResponse, MailboxDelegationCreateRequest,
    MailboxDelegationResponse, MailboxDelegationRevokedResponse, MailboxResponse,
    MessageDetailResponse, MessageStateResponse, MessageStateUpdateRequest,
    OidcOwnerBindingResponse, OrganizationBootstrapResponse, OrganizationResponse,
    OwnAgentMailHourlyUsageResponse, OwnAgentMailPolicyLimitsResponse, OwnAgentMailPolicyResponse,
    ReadinessResponse, RetentionPolicyActivationRequest, RetentionPolicyCreateRequest,
    RetentionPolicyResponse, RetentionPolicyVersionCreateRequest, RetentionPolicyVersionResponse,
    RoleAssignmentGrantRequest, RoleAssignmentListResponse, RoleAssignmentResponse,
    RoleAssignmentRevocationRequest, RoleListResponse, RoleResponse, ScimTokenIssuedResponse,
    ScimTokenRevokedResponse, SystemInfoResponse, TextDraftResponse, TextDraftSaveRequest,
    WebhookDeadLetterListResponse, WebhookDeadLetterReplayRequest, WebhookDeadLetterReplayResponse,
    WebhookDeadLetterResponse, WebhookSubscriptionCreateRequest, WebhookSubscriptionListResponse,
    WebhookSubscriptionResponse, WebhookSubscriptionUpdateRequest,
};
use copilotmail_dns::{DnsError, DnsVerifier, DnsVerifierConfig, HickoryDnsLookup};
use copilotmail_domain::{
    AgentEnvironment, AgentRiskLevel, ApprovalAction, ApprovalDecision, ApprovalRequestId,
    ApprovalStatus, AuditArchiveId, AuditCheckpointId, AuditExportGrant, AuditExportId,
    AuditOutcome, AuthorizationContext, ComplianceCase, ComplianceCaseId, ComplianceCaseStatus,
    CredentialId, DelegationId, DraftId, LeaseId, LegalHold, LegalHoldId, LegalHoldStatus,
    MailDataPlaneId, MailDomainId, MailDomainName, MailDomainRouteMode, MailDomainStatus,
    MailEngineProjectionStatus, MailSubmissionId, MailboxCapability, MailboxId, MailboxKind,
    OrganizationId, Permission, PrincipalId, PrincipalKind, PrincipalStatus, RetentionPolicy,
    RetentionPolicyId, RetentionPolicyVersion, RetentionPolicyVersionId, RetentionResourceKind,
    RoleAssignmentId, RoleId, ScimTokenId, SubscriptionId, WebhookSubscriptionId,
};
use copilotmail_mail_engine::{
    MailEngineWebhookError, StalwartJmapClient, StalwartJmapConfig, StalwartWebhookParser,
};
use copilotmail_object_store::{
    ArchivedObjectEvidence, S3WormArchive, S3WormArchiveConfig, WormObjectSource, WormRetentionMode,
};
use copilotmail_oidc::{
    OidcAuthenticator, OidcAuthorizationResponse, OidcAuthorizationTransaction, OidcConfig,
    OidcFlowError, OidcPublicClientConfig, OidcRelyingParty,
};
use copilotmail_platform::{
    AgentOAuthSettings, AuditArchiveRetentionMode, AuditArchiveSettings, AuditMaintenanceSettings,
    ControlPlaneSettings, HumanSessionSettings, MailEngineSettings, MailEventIngressSettings,
};
use copilotmail_postgres::{PgControlPlaneStore, WebhookDeliveryOutcome};
use copilotmail_webhook::{
    SignedWebhookClient, WebhookDeliveryConfig, WebhookError, WebhookEvent, WebhookSigningKeySet,
};
use futures_util::stream::{self, Stream};
use hmac::{Hmac, Mac};
use opentelemetry::propagation::Extractor;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::convert::Infallible;
use std::fs::File;
use std::future::Future;
use std::io::{BufRead, BufReader};
use std::path::{Path as FsPath, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tls::{NativeTlsListener, TlsConnectionInfo, certificate_fingerprint_from_pem};
use tokio::io::AsyncWriteExt;
use tokio::time::{Instant, MissedTickBehavior};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;

const SERVICE: &str = "copilotmail-control";
const API_VERSION: &str = "v2";
const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");
const HUMAN_LOGIN_CLEANUP_LIMIT: u32 = 1_000;
const WORKER_ERROR_BACKOFF_INITIAL: Duration = Duration::from_secs(1);
const WORKER_ERROR_BACKOFF_MAX: Duration = Duration::from_secs(30);

fn worker_error_backoff(consecutive_failures: u32) -> Duration {
    if consecutive_failures == 0 {
        return Duration::ZERO;
    }
    let shift = consecutive_failures.saturating_sub(1).min(5);
    WORKER_ERROR_BACKOFF_INITIAL
        .saturating_mul(1_u32 << shift)
        .min(WORKER_ERROR_BACKOFF_MAX)
}

#[derive(Clone)]
struct AppState {
    store: PgControlPlaneStore,
    authenticator: Arc<dyn AccessTokenAuthenticator>,
    authentication_ready: bool,
    human_auth_required: bool,
    human_session: Option<HumanSessionRuntime>,
    human_session_required: bool,
    agent_token_service: Option<Arc<AgentTokenService<PgControlPlaneStore>>>,
    agent_authentication_ready: bool,
    agent_auth_required: bool,
    agent_event_stream_tenant_limit: u32,
    agent_event_stream_principal_limit: u32,
    agent_ingress_tenant_requests_per_minute: u32,
    agent_ingress_principal_requests_per_minute: u32,
    agent_event_stream_metrics: AgentEventStreamMetrics,
    mail_data_plane_id: Option<MailDataPlaneId>,
    mail_engine: Option<Arc<dyn MailEngine>>,
    mail_engine_required: bool,
    mail_content: Option<Arc<dyn MailContentReader>>,
    mail_content_required: bool,
    mail_drafts: Option<Arc<dyn MailDraftStore>>,
    mail_draft_content: Option<Arc<dyn MailDraftContentReader>>,
    mail_drafts_required: bool,
    mail_event_ingress: Option<MailEventIngressSettings>,
    mail_event_ingress_required: bool,
    audit_archive: Option<AuditArchiveSettings>,
}

#[derive(Clone)]
struct HumanBrowserClientRuntime {
    client_key: HumanSessionClient,
    public_origin: String,
    relying_party: Arc<OidcRelyingParty<PgControlPlaneStore>>,
}

#[derive(Clone)]
struct HumanSessionRuntime {
    clients: Arc<BTreeMap<String, HumanBrowserClientRuntime>>,
    protector: PkceProtector,
    maximum_lifetime: std::time::Duration,
    login_requests_per_minute: u32,
    secure_cookie: bool,
}

impl HumanSessionRuntime {
    fn client(&self, key: &str) -> Option<&HumanBrowserClientRuntime> {
        self.clients.get(key)
    }

    const fn login_cookie_name(&self, client: HumanSessionClient) -> &'static str {
        human_login_cookie_name(client, self.secure_cookie)
    }

    const fn session_cookie_name(&self, client: HumanSessionClient) -> &'static str {
        human_session_cookie_name(client, self.secure_cookie)
    }
}

const fn human_login_cookie_name(client: HumanSessionClient, secure_cookie: bool) -> &'static str {
    match (client, secure_cookie) {
        (HumanSessionClient::Admin, true) => "__Host-copilotmail-admin-login",
        (HumanSessionClient::Mail, true) => "__Host-copilotmail-mail-login",
        (HumanSessionClient::Admin, false) => "copilotmail-admin-login",
        (HumanSessionClient::Mail, false) => "copilotmail-mail-login",
    }
}

const fn human_session_cookie_name(
    client: HumanSessionClient,
    secure_cookie: bool,
) -> &'static str {
    match (client, secure_cookie) {
        (HumanSessionClient::Admin, true) => "__Host-copilotmail-admin-session",
        (HumanSessionClient::Mail, true) => "__Host-copilotmail-mail-session",
        (HumanSessionClient::Admin, false) => "copilotmail-admin-session",
        (HumanSessionClient::Mail, false) => "copilotmail-mail-session",
    }
}

const fn other_human_session_client(client: HumanSessionClient) -> HumanSessionClient {
    match client {
        HumanSessionClient::Admin => HumanSessionClient::Mail,
        HumanSessionClient::Mail => HumanSessionClient::Admin,
    }
}

#[derive(Clone)]
struct PkceProtector {
    key: Arc<LessSafeKey>,
}

impl PkceProtector {
    fn new(key_bytes: &[u8]) -> Result<Self> {
        anyhow::ensure!(key_bytes.len() == 32, "Human session key must be 32 bytes");
        let key = UnboundKey::new(&AES_256_GCM, key_bytes)
            .map_err(|_| anyhow::anyhow!("Human session key is invalid"))?;
        Ok(Self {
            key: Arc::new(LessSafeKey::new(key)),
        })
    }

    fn protect(&self, verifier: &str, state_sha256: &[u8; 32]) -> Result<Vec<u8>> {
        anyhow::ensure!(
            (43..=128).contains(&verifier.len()) && verifier.is_ascii(),
            "PKCE verifier is invalid"
        );
        let nonce_bytes = rand::random::<[u8; 12]>();
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut ciphertext = verifier.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::from(state_sha256), &mut ciphertext)
            .map_err(|_| anyhow::anyhow!("protect PKCE verifier"))?;
        let mut protected = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
        protected.extend_from_slice(&nonce_bytes);
        protected.extend_from_slice(&ciphertext);
        Ok(protected)
    }

    fn unprotect(&self, protected: &[u8], state_sha256: &[u8; 32]) -> Result<String> {
        anyhow::ensure!(
            (12 + 43 + 16..=12 + 128 + 16).contains(&protected.len()),
            "protected PKCE verifier is invalid"
        );
        let (nonce_bytes, ciphertext) = protected.split_at(12);
        let nonce_bytes: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("protected PKCE nonce is invalid"))?;
        let mut plaintext = ciphertext.to_vec();
        let plaintext = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(state_sha256),
                &mut plaintext,
            )
            .map_err(|_| anyhow::anyhow!("protected PKCE verifier authentication failed"))?;
        let verifier = std::str::from_utf8(plaintext)
            .map_err(|_| anyhow::anyhow!("PKCE verifier is not UTF-8"))?;
        anyhow::ensure!(
            (43..=128).contains(&verifier.len()) && verifier.is_ascii(),
            "PKCE verifier is invalid"
        );
        Ok(verifier.to_owned())
    }
}

struct UnconfiguredAuthenticator;
struct RejectingAuthenticator;

struct CompositeAuthenticator {
    agent: Arc<dyn AccessTokenAuthenticator>,
    human: Arc<dyn AccessTokenAuthenticator>,
}

#[derive(Clone, Default)]
struct AgentEventStreamMetrics {
    active: Arc<AtomicU64>,
    opened_total: Arc<AtomicU64>,
    tenant_rejected_total: Arc<AtomicU64>,
    principal_rejected_total: Arc<AtomicU64>,
    stream_errors_total: Arc<AtomicU64>,
    cleanup_failures_total: Arc<AtomicU64>,
    ingress_allowed_total: Arc<AtomicU64>,
    ingress_tenant_rejected_total: Arc<AtomicU64>,
    ingress_principal_rejected_total: Arc<AtomicU64>,
    ingress_failures_total: Arc<AtomicU64>,
}

impl AgentEventStreamMetrics {
    fn opened(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
        self.opened_total.fetch_add(1, Ordering::Relaxed);
    }

    fn closed(&self) {
        let _ = self
            .active
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_sub(1))
            });
    }

    fn render(&self) -> String {
        format!(
            concat!(
                "# HELP copilotmail_agent_event_stream_active Active SSE connections in this process.\n",
                "# TYPE copilotmail_agent_event_stream_active gauge\n",
                "copilotmail_agent_event_stream_active {}\n",
                "# HELP copilotmail_agent_event_stream_opened_total SSE connections opened by this process.\n",
                "# TYPE copilotmail_agent_event_stream_opened_total counter\n",
                "copilotmail_agent_event_stream_opened_total {}\n",
                "# HELP copilotmail_agent_event_stream_rejected_total Cluster quota rejections observed by this process.\n",
                "# TYPE copilotmail_agent_event_stream_rejected_total counter\n",
                "copilotmail_agent_event_stream_rejected_total{{scope=\"tenant\"}} {}\n",
                "copilotmail_agent_event_stream_rejected_total{{scope=\"principal\"}} {}\n",
                "# HELP copilotmail_agent_event_stream_errors_total Streams terminated after an internal polling or serialization error.\n",
                "# TYPE copilotmail_agent_event_stream_errors_total counter\n",
                "copilotmail_agent_event_stream_errors_total {}\n",
                "# HELP copilotmail_agent_event_stream_lease_cleanup_failures_total Lease cleanup failures recovered by expiry.\n",
                "# TYPE copilotmail_agent_event_stream_lease_cleanup_failures_total counter\n",
                "copilotmail_agent_event_stream_lease_cleanup_failures_total {}\n",
                "# HELP copilotmail_agent_ingress_requests_total Protected Agent API admissions observed by this process.\n",
                "# TYPE copilotmail_agent_ingress_requests_total counter\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"allowed\"}} {}\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"tenant_limited\"}} {}\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"principal_limited\"}} {}\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"failure\"}} {}\n"
            ),
            self.active.load(Ordering::Relaxed),
            self.opened_total.load(Ordering::Relaxed),
            self.tenant_rejected_total.load(Ordering::Relaxed),
            self.principal_rejected_total.load(Ordering::Relaxed),
            self.stream_errors_total.load(Ordering::Relaxed),
            self.cleanup_failures_total.load(Ordering::Relaxed),
            self.ingress_allowed_total.load(Ordering::Relaxed),
            self.ingress_tenant_rejected_total.load(Ordering::Relaxed),
            self.ingress_principal_rejected_total
                .load(Ordering::Relaxed),
            self.ingress_failures_total.load(Ordering::Relaxed),
        )
    }
}

fn agent_event_stream_metrics_router(metrics: AgentEventStreamMetrics) -> Router {
    Router::new()
        .route(
            "/metrics",
            get(
                |State(metrics): State<AgentEventStreamMetrics>| async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                        metrics.render(),
                    )
                },
            ),
        )
        .with_state(metrics)
}

#[async_trait]
impl AccessTokenAuthenticator for UnconfiguredAuthenticator {
    async fn authenticate(
        &self,
        _: &str,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        Err(AuthenticationError::Unavailable)
    }
}

#[async_trait]
impl AccessTokenAuthenticator for RejectingAuthenticator {
    async fn authenticate(
        &self,
        _: &str,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        Err(AuthenticationError::InvalidCredential)
    }
}

#[async_trait]
impl AccessTokenAuthenticator for CompositeAuthenticator {
    async fn authenticate(
        &self,
        token: &str,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        match self.agent.authenticate(token).await {
            Ok(identity) => Ok(identity),
            Err(AuthenticationError::Unavailable) => Err(AuthenticationError::Unavailable),
            Err(AuthenticationError::InvalidCredential) => self.human.authenticate(token).await,
        }
    }

    async fn authenticate_with_context(
        &self,
        token: &str,
        context: &AuthenticationContext,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        match self.agent.authenticate_with_context(token, context).await {
            Ok(identity) => Ok(identity),
            Err(AuthenticationError::Unavailable) => Err(AuthenticationError::Unavailable),
            Err(AuthenticationError::InvalidCredential) => {
                self.human.authenticate_with_context(token, context).await
            }
        }
    }
}

/// Connects the V2 control-plane store using validated process settings.
pub async fn connect_store(settings: &ControlPlaneSettings) -> Result<PgControlPlaneStore> {
    PgControlPlaneStore::connect(settings.database_url(), settings.database_max_connections)
        .await
        .context("connect to CopilotMail PostgreSQL control plane")
}

/// Runs versioned PostgreSQL migrations and explicitly binds the configured
/// data-plane ID to its canonical origin. Serving processes never perform this
/// first-write operation, so a configuration typo cannot claim an identity.
pub async fn run_migrations(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
) -> Result<()> {
    store
        .migrate()
        .await
        .context("apply CopilotMail control-plane migrations")?;
    if let Some((data_plane_id, origin)) = configured_mail_data_plane(settings)? {
        anyhow::ensure!(
            store
                .bind_mail_data_plane_origin(
                    data_plane_id,
                    MAIL_DATA_PLANE_ADAPTER_KIND,
                    origin,
                    Utc::now(),
                )
                .await
                .context("bind configured mail data-plane canonical origin")?,
            "configured mail data-plane ID is not registered for this adapter and canonical origin"
        );
    }
    Ok(())
}

/// Verifies database connectivity and migration readiness.
pub async fn run_check(settings: &ControlPlaneSettings, store: &PgControlPlaneStore) -> Result<()> {
    store
        .ping()
        .await
        .context("control-plane PostgreSQL ping failed")?;
    anyhow::ensure!(
        store.schema_ready().await?,
        "control-plane schema is not migrated"
    );
    ensure_mail_data_plane_registration(
        store,
        settings.mail_data_plane_id.map(MailDataPlaneId::from_uuid),
        settings.mail_data_plane_origin.as_deref(),
    )
    .await?;
    tracing::info!("control-plane dependencies are ready");
    Ok(())
}

/// Performs an authenticated, protocol-level JMAP data-plane probe.
pub async fn run_check_mail_engine(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
) -> Result<()> {
    run_check(settings, store).await?;
    let engine_settings = settings
        .mail_engine
        .as_ref()
        .context("mail engine is not configured")?;
    let engine = build_mail_engine(engine_settings, configured_mail_data_plane_id(settings)?)?;
    let readiness = engine
        .probe()
        .await
        .context("mail engine JMAP probe failed")?;
    tracing::info!(
        username = %readiness.username,
        api_origin = %readiness.api_url.origin().ascii_serialization(),
        session_state = %readiness.session_state,
        mail_capability_advertised = readiness.mail_capability_advertised,
        "mail data plane is ready"
    );
    Ok(())
}

/// Resolves and evaluates one domain without persisting or activating it.
pub async fn run_check_domain_dns(
    domain: String,
    ownership_token_file: String,
    dkim_selector: String,
) -> Result<()> {
    let domain = domain
        .parse::<copilotmail_domain::MailDomainName>()
        .context("validate managed mail domain")?;
    let token = read_bounded_secret_file(FsPath::new(&ownership_token_file), 1024).await?;
    let token = std::str::from_utf8(&token)
        .context("ownership token file is not UTF-8")?
        .trim();
    let lookup = HickoryDnsLookup::from_system().context("initialize system DNS resolver")?;
    let config = DnsVerifierConfig::new(dkim_selector, std::time::Duration::from_secs(10))
        .context("validate DNS verifier configuration")?;
    let report = DnsVerifier::new(lookup, config)
        .verify(&domain, token, Utc::now())
        .await
        .context("verify managed mail domain DNS")?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn configured_mail_data_plane_id(settings: &ControlPlaneSettings) -> Result<MailDataPlaneId> {
    settings
        .mail_data_plane_id
        .map(MailDataPlaneId::from_uuid)
        .context("mail data-plane ID is not configured")
}

fn configured_mail_data_plane(
    settings: &ControlPlaneSettings,
) -> Result<Option<(MailDataPlaneId, &str)>> {
    match (
        settings.mail_data_plane_id,
        settings.mail_data_plane_origin.as_deref(),
    ) {
        (None, None) => Ok(None),
        (Some(data_plane_id), Some(origin)) => {
            Ok(Some((MailDataPlaneId::from_uuid(data_plane_id), origin)))
        }
        _ => anyhow::bail!("mail data-plane ID and canonical origin must be configured together"),
    }
}

const MAIL_DATA_PLANE_ADAPTER_KIND: &str = "stalwart_jmap";
const LEGACY_MAIL_DATA_PLANE_SOURCE: &str = "stalwart";
const LEGACY_MAIL_DATA_PLANE_ID: u128 = 0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2101;

fn is_legacy_mail_data_plane(data_plane_id: MailDataPlaneId) -> bool {
    data_plane_id.as_uuid().as_u128() == LEGACY_MAIL_DATA_PLANE_ID
}

fn mail_event_fingerprint_scheme(
    data_plane_id: MailDataPlaneId,
) -> Result<MailEventFingerprintScheme, MailEventIngestionError> {
    if is_legacy_mail_data_plane(data_plane_id) {
        MailEventFingerprintScheme::try_legacy_source(LEGACY_MAIL_DATA_PLANE_SOURCE)
    } else {
        Ok(MailEventFingerprintScheme::data_plane_v2())
    }
}

fn mail_data_plane_fingerprint_scheme(
    data_plane_id: MailDataPlaneId,
) -> MailDataPlaneFingerprintScheme {
    if is_legacy_mail_data_plane(data_plane_id) {
        MailDataPlaneFingerprintScheme::LegacyV1
    } else {
        MailDataPlaneFingerprintScheme::DataPlaneV2
    }
}

async fn ensure_mail_data_plane_registration(
    store: &PgControlPlaneStore,
    data_plane_id: Option<MailDataPlaneId>,
    origin: Option<&str>,
) -> Result<()> {
    let (data_plane_id, origin) = match (data_plane_id, origin) {
        (None, None) => return Ok(()),
        (Some(data_plane_id), Some(origin)) => (data_plane_id, origin),
        _ => anyhow::bail!("mail data-plane ID and canonical origin must be configured together"),
    };
    anyhow::ensure!(
        store
            .mail_data_plane_registered(data_plane_id, MAIL_DATA_PLANE_ADAPTER_KIND, origin)
            .await
            .context("verify configured mail data-plane registration")?,
        "configured mail data-plane ID is not registered for the Stalwart JMAP adapter and canonical origin"
    );
    Ok(())
}

fn build_mail_engine(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailEngine>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

fn build_mail_content_reader(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailContentReader>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

fn build_mail_draft_store(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailDraftStore>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

fn build_mail_draft_content_reader(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailDraftContentReader>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

fn build_mail_submission_adapter(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailSubmissionAdapter>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

fn build_mail_domain_projector(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailDomainProjector>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

fn build_stalwart_client(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<StalwartJmapClient> {
    let config = StalwartJmapConfig::new(
        &settings.base_url,
        settings.api_token_file.clone(),
        settings.timeout,
        settings.allow_insecure_http,
    )
    .context("validate mail engine configuration")?;
    StalwartJmapClient::new(data_plane_id, config).context("initialize mail engine adapter")
}

/// Independently verifies a tenant-scoped audit chain and prints machine-readable evidence.
pub async fn run_verify_audit_chain(
    store: &PgControlPlaneStore,
    organization_id: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let report = store
        .verify_audit_chain(organization_id)
        .await
        .context("verify tenant audit hash chain")?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_create_audit_checkpoint(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    checkpoint_id: Option<String>,
    key_id: String,
    private_key_file: String,
    public_key_file: String,
    archive_object_key: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let actor_principal_id = actor_principal_id
        .parse::<PrincipalId>()
        .context("actor Principal ID is not a UUID")?;
    let checkpoint_id = checkpoint_id
        .map(|value| {
            value
                .parse::<AuditCheckpointId>()
                .context("checkpoint ID is not a UUID")
        })
        .transpose()?
        .unwrap_or_default();
    let private_key = tokio::fs::read(&private_key_file)
        .await
        .with_context(|| format!("read audit private key file {private_key_file}"))?;
    let public_key = tokio::fs::read(&public_key_file)
        .await
        .with_context(|| format!("read audit public key file {public_key_file}"))?;
    let signer = RsaAuditCheckpointSigner::new(key_id, &private_key, &public_key)
        .context("validate audit checkpoint signing key pair")?;
    let result = CreateAuditCheckpointService::new(store.clone(), signer)
        .execute(CreateAuditCheckpointCommand {
            checkpoint_id,
            organization_id,
            actor_principal_id,
            archive_object_key,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("create signed audit checkpoint")?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

pub async fn run_export_audit_checkpoint(
    store: &PgControlPlaneStore,
    organization_id: String,
    checkpoint_id: String,
    output_directory: String,
) -> Result<()> {
    run_export_audit_checkpoint_traced(
        store,
        organization_id,
        checkpoint_id,
        output_directory,
        None,
    )
    .await
}

async fn run_export_audit_checkpoint_traced(
    store: &PgControlPlaneStore,
    organization_id: String,
    checkpoint_id: String,
    output_directory: String,
    trace_id: Option<&str>,
) -> Result<()> {
    const PAGE_SIZE: i64 = 1_000;
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let checkpoint_id = checkpoint_id
        .parse::<AuditCheckpointId>()
        .context("checkpoint ID is not a UUID")?;
    let generated_trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
    let trace_id = trace_id.unwrap_or(&generated_trace_id);
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        "audit checkpoint export started"
    );
    let stored = store
        .get_signed_audit_checkpoint(organization_id, checkpoint_id)
        .await
        .context("load signed audit checkpoint")?
        .context("signed audit checkpoint does not exist in this organization")?;
    anyhow::ensure!(
        stored.checkpoint.payload.organization_id == organization_id
            && stored.checkpoint.payload.checkpoint_id == checkpoint_id,
        "stored checkpoint identity does not match request"
    );
    let chain_report = store
        .verify_audit_chain(organization_id)
        .await
        .context("reverify tenant audit chain before export")?;
    anyhow::ensure!(
        chain_report.event_count >= stored.checkpoint.payload.event_count,
        "current audit chain is shorter than signed checkpoint"
    );

    let output_directory = PathBuf::from(output_directory);
    anyhow::ensure!(
        !output_directory.as_os_str().is_empty(),
        "output directory must not be empty"
    );
    tokio::fs::create_dir(&output_directory)
        .await
        .with_context(|| {
            format!(
                "create new export directory {} (it must not already exist)",
                output_directory.display()
            )
        })?;
    let events_temporary = output_directory.join(".events.jsonl.tmp");
    let events_final = output_directory.join(copilotmail_audit::EVENTS_FILE_NAME);
    let manifest_temporary = output_directory.join(".manifest.json.tmp");
    let manifest_final = output_directory.join("manifest.json");
    let mut events_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&events_temporary)
        .await
        .context("create temporary audit event export")?;
    let mut digest = Sha256::new();
    let mut after_sequence = 0_i64;
    let mut exported_count = 0_u64;

    loop {
        let page = store
            .load_audit_export_page(
                organization_id,
                after_sequence,
                stored.checkpoint.payload.chain_sequence_end,
                PAGE_SIZE,
            )
            .await
            .context("load audit export page")?;
        if page.is_empty() {
            break;
        }
        for record in &page {
            let mut line = serde_json::to_vec(record)?;
            line.push(b'\n');
            digest.update(&line);
            events_file.write_all(&line).await?;
            after_sequence = record.chain_sequence;
            exported_count = exported_count
                .checked_add(1)
                .context("audit export event count overflow")?;
        }
        if page.len() < usize::try_from(PAGE_SIZE).expect("positive page size") {
            break;
        }
    }
    events_file.sync_all().await?;
    drop(events_file);
    anyhow::ensure!(
        exported_count == stored.checkpoint.payload.event_count
            && after_sequence == stored.checkpoint.payload.chain_sequence_end,
        "exported audit rows do not match signed checkpoint"
    );
    let events_sha256 = encode_bytes_hex(&digest.finalize());
    let manifest = AuditExportManifest::new(stored.checkpoint, events_sha256, Utc::now())?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let mut manifest_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_temporary)
        .await
        .context("create temporary audit manifest")?;
    manifest_file.write_all(&manifest_bytes).await?;
    manifest_file.write_all(b"\n").await?;
    manifest_file.sync_all().await?;
    drop(manifest_file);
    tokio::fs::rename(&events_temporary, &events_final)
        .await
        .context("publish audit events file")?;
    tokio::fs::rename(&manifest_temporary, &manifest_final)
        .await
        .context("publish audit manifest")?;

    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "organization_id": organization_id,
            "checkpoint_id": checkpoint_id,
            "event_count": exported_count,
            "manifest_file": manifest_final,
            "events_file": events_final,
            "archive_object_key": stored.archive_object_key,
        }))?
    );
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        event_count = exported_count,
        "audit checkpoint export completed"
    );
    Ok(())
}

pub async fn run_verify_audit_export(
    export_directory: String,
    public_key_file: String,
) -> Result<()> {
    let (_, report) = verify_audit_export_files(
        &PathBuf::from(export_directory),
        &PathBuf::from(public_key_file),
    )?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn verify_audit_export_files(
    export_directory: &FsPath,
    public_key_file: &FsPath,
) -> Result<(AuditExportManifest, AuditExportVerificationReport)> {
    const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
    const MAX_EVENT_LINE_BYTES: usize = 256 * 1024;
    let manifest_path = export_directory.join("manifest.json");
    let manifest_metadata = std::fs::metadata(&manifest_path)
        .with_context(|| format!("inspect {}", manifest_path.display()))?;
    anyhow::ensure!(
        manifest_metadata.is_file() && manifest_metadata.len() <= MAX_MANIFEST_BYTES,
        "audit manifest is not a bounded regular file"
    );
    let manifest: AuditExportManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .context("parse audit export manifest")?;
    let public_key = std::fs::read(public_key_file).with_context(|| {
        format!(
            "read trusted audit public key {}",
            public_key_file.display()
        )
    })?;
    let mut verifier =
        AuditExportVerifier::new(&manifest, &public_key).context("verify checkpoint signature")?;
    let events_path = export_directory.join(&manifest.events_file);
    let mut reader = BufReader::new(
        File::open(&events_path).with_context(|| format!("open {}", events_path.display()))?,
    );
    let mut digest = Sha256::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        anyhow::ensure!(
            line.len() <= MAX_EVENT_LINE_BYTES,
            "audit export record exceeds size limit"
        );
        anyhow::ensure!(
            line.last() == Some(&b'\n'),
            "audit event export must end every record with LF"
        );
        digest.update(&line);
        line.pop();
        anyhow::ensure!(
            !line.is_empty(),
            "audit event export contains an empty line"
        );
        let record: AuditExportRecord =
            serde_json::from_slice(&line).context("parse audit export record")?;
        verifier
            .push(&record)
            .context("verify audit export chain")?;
    }
    let events_sha256 = encode_bytes_hex(&digest.finalize());
    let report = verifier
        .finish(&manifest, &events_sha256)
        .context("verify audit export against signed checkpoint")?;
    Ok((manifest, report))
}

#[allow(clippy::too_many_arguments)]
pub async fn run_archive_audit_export(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    checkpoint_id: String,
    archive_id: Option<String>,
    export_directory: String,
    public_key_file: String,
) -> Result<()> {
    run_archive_audit_export_traced(
        settings,
        store,
        organization_id,
        actor_principal_id,
        checkpoint_id,
        archive_id,
        export_directory,
        public_key_file,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_archive_audit_export_traced(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    checkpoint_id: String,
    archive_id: Option<String>,
    export_directory: String,
    public_key_file: String,
    trace_id: Option<&str>,
) -> Result<()> {
    let archive_settings = settings
        .audit_archive
        .as_ref()
        .context("immutable audit archive is not configured")?;
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let actor_principal_id = actor_principal_id
        .parse::<PrincipalId>()
        .context("actor Principal ID is not a UUID")?;
    let checkpoint_id = checkpoint_id
        .parse::<AuditCheckpointId>()
        .context("checkpoint ID is not a UUID")?;
    let archive_id = archive_id
        .map(|value| {
            value
                .parse::<AuditArchiveId>()
                .context("archive ID is not a UUID")
        })
        .transpose()?
        .unwrap_or_default();
    let generated_trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
    let trace_id = trace_id.unwrap_or(&generated_trace_id);
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        %archive_id,
        "immutable audit archive started"
    );
    let export_directory = PathBuf::from(export_directory);
    let (manifest, report) =
        verify_audit_export_files(&export_directory, &PathBuf::from(public_key_file))
            .context("offline verification failed; nothing was uploaded")?;
    anyhow::ensure!(
        report.organization_id == organization_id && report.checkpoint_id == checkpoint_id,
        "verified export identity does not match archive request"
    );
    let stored = store
        .get_signed_audit_checkpoint(organization_id, checkpoint_id)
        .await
        .context("load signed audit checkpoint")?
        .context("signed audit checkpoint does not exist in this organization")?;
    anyhow::ensure!(
        manifest.checkpoint == stored.checkpoint,
        "portable export checkpoint differs from the database checkpoint"
    );

    let events_key = stored.archive_object_key;
    let manifest_key = format!("{events_key}.manifest.json");
    anyhow::ensure!(
        manifest_key.len() <= 1_024,
        "checkpoint archive object key leaves no room for manifest suffix"
    );
    let events_path = export_directory.join(&manifest.events_file);
    let manifest_path = export_directory.join("manifest.json");
    let manifest_bytes = tokio::fs::read(&manifest_path)
        .await
        .context("read verified audit manifest")?;
    let manifest_sha256 = encode_bytes_hex(&Sha256::digest(&manifest_bytes));
    let requested_retain_until = manifest
        .exported_at
        .checked_add_signed(chrono::Duration::days(i64::from(
            archive_settings.retention_days,
        )))
        .context("audit archive retention timestamp overflow")?;
    let upload_started_at = Utc::now();
    anyhow::ensure!(
        requested_retain_until > upload_started_at,
        "configured retention period has already elapsed for this export; create a fresh export"
    );
    let retention_mode = match archive_settings.retention_mode {
        AuditArchiveRetentionMode::Governance => WormRetentionMode::Governance,
        AuditArchiveRetentionMode::Compliance => WormRetentionMode::Compliance,
    };
    let archive = S3WormArchive::connect(S3WormArchiveConfig {
        endpoint_url: archive_settings.endpoint_url.clone(),
        region: archive_settings.region.clone(),
        bucket: archive_settings.bucket.clone(),
        force_path_style: archive_settings.force_path_style,
        allow_insecure_http: archive_settings.allow_insecure_http,
    })
    .await
    .context("initialize immutable audit archive")?;
    let metadata = audit_archive_object_metadata(
        organization_id,
        checkpoint_id,
        archive_id,
        manifest.format_version,
        trace_id,
    );
    let events = archive
        .put_immutable(
            &WormObjectSource {
                key: events_key,
                content_type: "application/x-ndjson".into(),
                path: events_path,
                sha256_hex: manifest.events_sha256.clone(),
                metadata: metadata.clone(),
            },
            retention_mode,
            requested_retain_until,
            upload_started_at,
        )
        .await
        .context("upload and verify immutable audit event object")?;
    let manifest_object = archive
        .put_immutable(
            &WormObjectSource {
                key: manifest_key,
                content_type: "application/json".into(),
                path: manifest_path,
                sha256_hex: manifest_sha256,
                metadata,
            },
            retention_mode,
            requested_retain_until,
            upload_started_at,
        )
        .await
        .context("upload and verify immutable audit manifest object")?;
    let verified_at = Utc::now();
    let retain_until = events.retain_until.min(manifest_object.retain_until);
    let record = AuditArchiveRecord {
        archive_id,
        organization_id,
        checkpoint_id,
        provider: "s3".into(),
        bucket: archive_settings.bucket.clone(),
        events: archived_object_record(events),
        manifest: archived_object_record(manifest_object),
        retention_mode: retention_mode.as_str().into(),
        retain_until,
        verified_at,
        created_by: actor_principal_id,
        created_at: Utc::now(),
    };
    let result = RecordAuditArchiveService::new(store.clone())
        .execute(RecordAuditArchiveCommand {
            archive: record,
            actor_principal_id,
            trace_id: trace_id.to_owned(),
        })
        .await
        .context("record verified immutable archive evidence")?;
    println!("{}", serde_json::to_string(&result)?);
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        %archive_id,
        "immutable audit archive completed"
    );
    Ok(())
}

fn archived_object_record(evidence: ArchivedObjectEvidence) -> ArchivedObjectRecord {
    ArchivedObjectRecord {
        key: evidence.key,
        version_id: evidence.version_id,
        sha256_hex: evidence.sha256_hex,
        size_bytes: evidence.size_bytes,
    }
}

pub async fn run_create_compliance_case(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    external_reference: String,
    title: String,
    purpose: String,
) -> Result<()> {
    let (organization_id, actor_principal_id, context, now) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let case = ComplianceGovernanceService::new(store.clone())
        .create_case(CreateComplianceCaseCommand {
            context,
            external_reference,
            title,
            purpose,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("create compliance case")?;
    anyhow::ensure!(
        case.organization_id() == organization_id && case.created_by() == actor_principal_id,
        "created compliance case identity does not match the request"
    );
    println!("{}", serde_json::to_string(&case)?);
    Ok(())
}

pub async fn run_close_compliance_case(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    case_id: String,
) -> Result<()> {
    let (_, _, context, now) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let case_id = case_id
        .parse::<ComplianceCaseId>()
        .context("compliance case ID is not a UUID")?;
    let case = ComplianceGovernanceService::new(store.clone())
        .close_case(CloseComplianceCaseCommand {
            context,
            case_id,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("close compliance case")?;
    println!("{}", serde_json::to_string(&case)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_apply_audit_legal_hold(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    case_id: String,
    archive_id: String,
    approval_request_id: String,
    reason: String,
) -> Result<()> {
    let (organization_id, requested_by, context, _) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let case_id = case_id
        .parse::<ComplianceCaseId>()
        .context("compliance case ID is not a UUID")?;
    let archive_id = archive_id
        .parse::<AuditArchiveId>()
        .context("audit archive ID is not a UUID")?;
    let approval_request_id = approval_request_id
        .parse::<ApprovalRequestId>()
        .context("approval request ID is not a UUID")?;
    let approval = legal_hold_approval(
        store,
        organization_id,
        approval_request_id,
        requested_by,
        "audit.archive",
        archive_id.to_string(),
    )
    .await?;
    if let Some(existing) = store
        .get_legal_hold_by_apply_approval(organization_id, approval_request_id)
        .await
        .context("read existing Legal Hold replay state")?
    {
        anyhow::ensure!(
            existing.case_id() == case_id
                && existing.archive_id() == archive_id
                && existing.reason() == reason
                && existing.status() == LegalHoldStatus::Active,
            "approval request was already consumed by a different or released Legal Hold"
        );
        println!("{}", serde_json::to_string(&existing)?);
        return Ok(());
    }
    let lock = store
        .try_acquire_audit_legal_hold_lock(organization_id, archive_id)
        .await
        .context("acquire archive Legal Hold operation lock")?
        .context("another Legal Hold operation is active for this archive")?;
    let result = async {
        if let Some(existing) = store
            .get_legal_hold_by_apply_approval(organization_id, approval_request_id)
            .await
            .context("recheck Legal Hold replay state")?
        {
            anyhow::ensure!(
                existing.case_id() == case_id
                    && existing.archive_id() == archive_id
                    && existing.reason() == reason
                    && existing.status() == LegalHoldStatus::Active,
                "approval request was already consumed by a different or released Legal Hold"
            );
            return Ok(existing);
        }
        let archive_record = store
            .get_audit_archive(organization_id, archive_id)
            .await
            .context("load immutable audit archive")?
            .context("audit archive does not exist in this organization")?;
        let archive = connect_legal_hold_archive(settings, &archive_record).await?;
        let events = archive
            .set_legal_hold(
                &archive_record.events.key,
                &archive_record.events.version_id,
                true,
            )
            .await
            .context("apply Legal Hold to audit events object version")?;
        let manifest = archive
            .set_legal_hold(
                &archive_record.manifest.key,
                &archive_record.manifest.version_id,
                true,
            )
            .await
            .context("apply Legal Hold to audit manifest object version")?;
        let now = Utc::now();
        ComplianceGovernanceService::new(store.clone())
            .apply_hold(ApplyLegalHoldCommand {
                context,
                case_id,
                archive_id,
                reason,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: true,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .context("record applied Legal Hold")
    }
    .await;
    lock.release()
        .await
        .context("release archive Legal Hold operation lock")?;
    let hold = result?;
    println!("{}", serde_json::to_string(&hold)?);
    Ok(())
}

pub async fn run_release_audit_legal_hold(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    hold_id: String,
    approval_request_id: String,
) -> Result<()> {
    let (organization_id, requested_by, context, _) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let hold_id = hold_id
        .parse::<LegalHoldId>()
        .context("Legal Hold ID is not a UUID")?;
    let approval_request_id = approval_request_id
        .parse::<ApprovalRequestId>()
        .context("approval request ID is not a UUID")?;
    let approval = legal_hold_approval(
        store,
        organization_id,
        approval_request_id,
        requested_by,
        "compliance.legal_hold",
        hold_id.to_string(),
    )
    .await?;
    let current = store
        .get_legal_hold(organization_id, hold_id)
        .await
        .context("load Legal Hold")?
        .context("Legal Hold does not exist in this organization")?;
    if current.status() == LegalHoldStatus::Released {
        anyhow::ensure!(
            current.release_approval_request_id() == Some(approval_request_id),
            "Legal Hold was released using a different approval request"
        );
        println!("{}", serde_json::to_string(&current)?);
        return Ok(());
    }
    let archive_id = current.archive_id();
    let lock = store
        .try_acquire_audit_legal_hold_lock(organization_id, archive_id)
        .await
        .context("acquire archive Legal Hold operation lock")?
        .context("another Legal Hold operation is active for this archive")?;
    let result = async {
        let current = store
            .get_legal_hold(organization_id, hold_id)
            .await
            .context("recheck Legal Hold state")?
            .context("Legal Hold no longer exists")?;
        if current.status() == LegalHoldStatus::Released {
            anyhow::ensure!(
                current.release_approval_request_id() == Some(approval_request_id),
                "Legal Hold was released using a different approval request"
            );
            return Ok(current);
        }
        let archive_record = store
            .get_audit_archive(organization_id, archive_id)
            .await
            .context("load immutable audit archive")?
            .context("audit archive does not exist in this organization")?;
        let archive = connect_legal_hold_archive(settings, &archive_record).await?;
        // The database remains Active until both exact object versions read
        // back Off. A partial remote release is therefore recoverable by retry.
        let manifest = archive
            .set_legal_hold(
                &archive_record.manifest.key,
                &archive_record.manifest.version_id,
                false,
            )
            .await
            .context("release Legal Hold from audit manifest object version")?;
        let events = archive
            .set_legal_hold(
                &archive_record.events.key,
                &archive_record.events.version_id,
                false,
            )
            .await
            .context("release Legal Hold from audit events object version")?;
        let now = Utc::now();
        ComplianceGovernanceService::new(store.clone())
            .release_hold(ReleaseLegalHoldCommand {
                context,
                hold_id,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: false,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .context("record released Legal Hold")
    }
    .await;
    lock.release()
        .await
        .context("release archive Legal Hold operation lock")?;
    let hold = result?;
    println!("{}", serde_json::to_string(&hold)?);
    Ok(())
}

async fn resolve_compliance_actor(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
) -> Result<(
    OrganizationId,
    PrincipalId,
    AuthorizationContext,
    DateTime<Utc>,
)> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let actor_principal_id = actor_principal_id
        .parse::<PrincipalId>()
        .context("actor Principal ID is not a UUID")?;
    let now = Utc::now();
    let context = store
        .resolve_authorization(actor_principal_id, now)
        .await
        .context("resolve live actor authorization")?
        .context("actor Principal is inactive or has no current role assignment")?;
    anyhow::ensure!(
        context.organization_id == organization_id
            && context.permits(organization_id, Permission::LegalHoldManage),
        "actor is not authorized to manage Legal Holds in this organization"
    );
    anyhow::ensure!(
        store
            .active_human_compliance_actor_exists(organization_id, actor_principal_id)
            .await
            .context("verify Active Human compliance actor")?,
        "Legal Hold operations require an Active Human actor"
    );
    Ok((organization_id, actor_principal_id, context, now))
}

async fn legal_hold_approval(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    request_id: ApprovalRequestId,
    requested_by: PrincipalId,
    resource_kind: &str,
    resource_id: String,
) -> Result<ApprovalProof> {
    let proof = ApprovalProof {
        organization_id,
        request_id,
        requested_by,
        action: ApprovalAction::LegalHold,
        resource_kind: resource_kind.into(),
        resource_id,
        now: Utc::now(),
    };
    anyhow::ensure!(
        store
            .approval_permits(&proof)
            .await
            .context("verify exact Legal Hold approval")?,
        "Legal Hold approval is missing, incomplete, expired, or targets another resource"
    );
    Ok(proof)
}

async fn connect_legal_hold_archive(
    settings: &ControlPlaneSettings,
    record: &AuditArchiveRecord,
) -> Result<S3WormArchive> {
    let archive_settings = settings
        .audit_archive
        .as_ref()
        .context("immutable audit archive is not configured")?;
    connect_legal_hold_archive_settings(archive_settings, record).await
}

async fn connect_legal_hold_archive_settings(
    archive_settings: &AuditArchiveSettings,
    record: &AuditArchiveRecord,
) -> Result<S3WormArchive> {
    anyhow::ensure!(
        record.provider == "s3" && record.bucket == archive_settings.bucket,
        "persisted audit archive does not match the configured immutable bucket"
    );
    S3WormArchive::connect(S3WormArchiveConfig {
        endpoint_url: archive_settings.endpoint_url.clone(),
        region: archive_settings.region.clone(),
        bucket: archive_settings.bucket.clone(),
        force_path_style: archive_settings.force_path_style,
        allow_insecure_http: archive_settings.allow_insecure_http,
    })
    .await
    .context("initialize immutable audit archive")
}

fn audit_archive_object_metadata(
    organization_id: OrganizationId,
    checkpoint_id: AuditCheckpointId,
    archive_id: AuditArchiveId,
    format_version: u16,
    trace_id: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("organization-id".into(), organization_id.to_string()),
        ("checkpoint-id".into(), checkpoint_id.to_string()),
        ("archive-id".into(), archive_id.to_string()),
        ("format-version".into(), format_version.to_string()),
        ("trace-id".into(), trace_id.to_owned()),
    ])
}

#[derive(Debug, serde::Serialize)]
struct AuditMaintenanceOutcome {
    trace_id: String,
    organization_id: OrganizationId,
    checkpoint_id: AuditCheckpointId,
    archive_id: AuditArchiveId,
    chain_sequence_end: i64,
    resumed_checkpoint: bool,
}

#[derive(Clone)]
struct AuditMaintenanceMetrics {
    organization_id: OrganizationId,
    failure_threshold: u32,
    max_pending_age_seconds: u64,
    max_cycle_seconds: u64,
    state: Arc<Mutex<AuditMaintenanceMetricState>>,
}

#[derive(Debug)]
struct AuditMaintenanceMetricState {
    started_at: i64,
    cycle_started_at: i64,
    last_completed_at: i64,
    last_success_at: i64,
    cycles_total: u64,
    successes_total: u64,
    failures_total: u64,
    lock_conflicts_total: u64,
    consecutive_failures: u32,
    cycle_in_progress: bool,
    pending_checkpoint_age_seconds: u64,
}

impl AuditMaintenanceMetrics {
    fn new(organization_id: OrganizationId, settings: &AuditMaintenanceSettings) -> Self {
        Self::with_limits(
            organization_id,
            settings.failure_threshold,
            settings.max_pending_age.as_secs(),
            settings.max_cycle_duration.as_secs(),
        )
    }

    fn with_limits(
        organization_id: OrganizationId,
        failure_threshold: u32,
        max_pending_age_seconds: u64,
        max_cycle_seconds: u64,
    ) -> Self {
        Self {
            organization_id,
            failure_threshold,
            max_pending_age_seconds,
            max_cycle_seconds,
            state: Arc::new(Mutex::new(AuditMaintenanceMetricState {
                started_at: Utc::now().timestamp(),
                cycle_started_at: 0,
                last_completed_at: 0,
                last_success_at: 0,
                cycles_total: 0,
                successes_total: 0,
                failures_total: 0,
                lock_conflicts_total: 0,
                consecutive_failures: 0,
                cycle_in_progress: false,
                pending_checkpoint_age_seconds: 0,
            })),
        }
    }

    fn cycle_started(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.cycles_total = state.cycles_total.saturating_add(1);
        state.cycle_started_at = Utc::now().timestamp();
        state.cycle_in_progress = true;
    }

    fn pending_checkpoint(&self, checkpoint: Option<&copilotmail_postgres::StoredAuditCheckpoint>) {
        let age = checkpoint.map_or(0, |stored| {
            Utc::now()
                .signed_duration_since(stored.checkpoint.payload.created_at)
                .num_seconds()
                .max(0) as u64
        });
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending_checkpoint_age_seconds = age;
    }

    fn succeeded(&self) {
        let now = Utc::now().timestamp();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.successes_total = state.successes_total.saturating_add(1);
        state.consecutive_failures = 0;
        state.last_success_at = now;
        state.last_completed_at = now;
        state.cycle_in_progress = false;
        state.pending_checkpoint_age_seconds = 0;
    }

    fn failed(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.failures_total = state.failures_total.saturating_add(1);
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.last_completed_at = Utc::now().timestamp();
        state.cycle_in_progress = false;
    }

    fn lock_conflict(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.lock_conflicts_total = state.lock_conflicts_total.saturating_add(1);
        state.last_completed_at = Utc::now().timestamp();
        state.cycle_in_progress = false;
    }

    fn render(&self) -> String {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let org = self.organization_id;
        format!(
            concat!(
                "# HELP copilotmail_audit_maintenance_cycles_total Maintenance cycles started.\n",
                "# TYPE copilotmail_audit_maintenance_cycles_total counter\n",
                "copilotmail_audit_maintenance_cycles_total{{organization_id=\"{org}\"}} {cycles}\n",
                "# HELP copilotmail_audit_maintenance_successes_total Successfully archived cycles.\n",
                "# TYPE copilotmail_audit_maintenance_successes_total counter\n",
                "copilotmail_audit_maintenance_successes_total{{organization_id=\"{org}\"}} {successes}\n",
                "# HELP copilotmail_audit_maintenance_failures_total Failed maintenance cycles.\n",
                "# TYPE copilotmail_audit_maintenance_failures_total counter\n",
                "copilotmail_audit_maintenance_failures_total{{organization_id=\"{org}\"}} {failures}\n",
                "# HELP copilotmail_audit_maintenance_lock_conflicts_total Cycles skipped due to another active instance.\n",
                "# TYPE copilotmail_audit_maintenance_lock_conflicts_total counter\n",
                "copilotmail_audit_maintenance_lock_conflicts_total{{organization_id=\"{org}\"}} {conflicts}\n",
                "# TYPE copilotmail_audit_maintenance_consecutive_failures gauge\n",
                "copilotmail_audit_maintenance_consecutive_failures{{organization_id=\"{org}\"}} {consecutive}\n",
                "# TYPE copilotmail_audit_maintenance_cycle_in_progress gauge\n",
                "copilotmail_audit_maintenance_cycle_in_progress{{organization_id=\"{org}\"}} {in_progress}\n",
                "# TYPE copilotmail_audit_maintenance_started_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_started_timestamp_seconds{{organization_id=\"{org}\"}} {started}\n",
                "# TYPE copilotmail_audit_maintenance_cycle_started_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_cycle_started_timestamp_seconds{{organization_id=\"{org}\"}} {cycle_started}\n",
                "# TYPE copilotmail_audit_maintenance_last_completed_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_last_completed_timestamp_seconds{{organization_id=\"{org}\"}} {completed}\n",
                "# TYPE copilotmail_audit_maintenance_last_success_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_last_success_timestamp_seconds{{organization_id=\"{org}\"}} {success}\n",
                "# TYPE copilotmail_audit_maintenance_pending_checkpoint_age_seconds gauge\n",
                "copilotmail_audit_maintenance_pending_checkpoint_age_seconds{{organization_id=\"{org}\"}} {pending_age}\n"
            ),
            org = org,
            cycles = state.cycles_total,
            successes = state.successes_total,
            failures = state.failures_total,
            conflicts = state.lock_conflicts_total,
            consecutive = state.consecutive_failures,
            in_progress = u8::from(state.cycle_in_progress),
            started = state.started_at,
            cycle_started = state.cycle_started_at,
            completed = state.last_completed_at,
            success = state.last_success_at,
            pending_age = state.pending_checkpoint_age_seconds,
        )
    }

    fn readiness(&self) -> Result<(), &'static str> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let now = Utc::now().timestamp();
        if state.consecutive_failures >= self.failure_threshold {
            return Err("failure_threshold_exceeded");
        }
        if state.pending_checkpoint_age_seconds > self.max_pending_age_seconds {
            return Err("pending_checkpoint_overdue");
        }
        if state.cycle_in_progress
            && now.saturating_sub(state.cycle_started_at) as u64 > self.max_cycle_seconds
        {
            return Err("cycle_duration_exceeded");
        }
        Ok(())
    }
}

fn audit_maintenance_observability_router(metrics: AuditMaintenanceMetrics) -> Router {
    Router::new()
        .route(
            "/metrics",
            get(
                |State(metrics): State<AuditMaintenanceMetrics>| async move {
                    (
                        [(
                            header::CONTENT_TYPE,
                            "text/plain; version=0.0.4; charset=utf-8",
                        )],
                        metrics.render(),
                    )
                },
            ),
        )
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"status": "ok"})) }),
        )
        .route(
            "/readyz",
            get(
                |State(metrics): State<AuditMaintenanceMetrics>| async move {
                    match metrics.readiness() {
                        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))),
                        Err(reason) => (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(serde_json::json!({
                                "status": "not_ready",
                                "reason": reason
                            })),
                        ),
                    }
                },
            ),
        )
        .with_state(metrics)
}

/// Runs exactly one recoverable checkpoint/export/WORM maintenance cycle.
pub async fn run_audit_maintenance_once(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
) -> Result<()> {
    let maintenance = settings
        .audit_maintenance
        .as_ref()
        .context("audit maintenance is not configured")?;
    let (organization_id, actor_principal_id, signer) =
        load_audit_maintenance_identity(maintenance).await?;
    ensure_audit_maintenance_root(&maintenance.work_directory).await?;
    let trace_id = Uuid::now_v7().to_string();
    let lock = store
        .try_acquire_audit_maintenance_lock(organization_id)
        .await
        .context("acquire tenant audit maintenance lock")?
        .context("another audit maintenance instance is active for this tenant")?;
    let outcome = audit_maintenance_cycle(
        settings,
        maintenance,
        store,
        organization_id,
        actor_principal_id,
        &signer,
        &trace_id,
    )
    .await?;
    lock.release()
        .await
        .context("release tenant audit maintenance lock")?;
    println!("{}", serde_json::to_string(&outcome)?);
    Ok(())
}

/// Runs a dedicated single-tenant audit maintenance loop.
///
/// This process is intentionally separate from Outbox delivery: large audit
/// exports and S3 retries must never hold or starve application-event leases.
pub async fn run_audit_maintenance_worker(
    settings: &ControlPlaneSettings,
    store: PgControlPlaneStore,
) -> Result<()> {
    let maintenance = settings
        .audit_maintenance
        .as_ref()
        .context("audit maintenance is not configured")?;
    let (organization_id, actor_principal_id, signer) =
        load_audit_maintenance_identity(maintenance).await?;
    ensure_audit_maintenance_root(&maintenance.work_directory).await?;
    let metrics = AuditMaintenanceMetrics::new(organization_id, maintenance);
    let metrics_router = audit_maintenance_observability_router(metrics.clone());
    let metrics_listener = tokio::net::TcpListener::bind(maintenance.metrics_addr)
        .await
        .with_context(|| {
            format!(
                "bind audit maintenance metrics listener {}",
                maintenance.metrics_addr
            )
        })?;
    let (metrics_shutdown_tx, metrics_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut metrics_server = tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_router)
            .with_graceful_shutdown(async move {
                let _ = metrics_shutdown_rx.await;
            })
            .await
    });
    let mut metrics_shutdown_tx = Some(metrics_shutdown_tx);
    let mut interval = tokio::time::interval(maintenance.interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut consecutive_failures = 0_u32;
    tracing::info!(
        %organization_id,
        %actor_principal_id,
        key_id = signer.key_id(),
        interval_seconds = maintenance.interval.as_secs(),
        metrics_addr = %maintenance.metrics_addr,
        "tenant audit maintenance worker started"
    );
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            result = &mut metrics_server => {
                return match result {
                    Ok(Ok(())) => anyhow::bail!("audit maintenance metrics server stopped unexpectedly"),
                    Ok(Err(error)) => Err(error).context("audit maintenance metrics server failed"),
                    Err(error) => Err(error).context("audit maintenance metrics task failed"),
                };
            }
            _ = &mut shutdown => {
                tracing::info!(%organization_id, "tenant audit maintenance worker stopping");
                if let Some(sender) = metrics_shutdown_tx.take() {
                    let _ = sender.send(());
                }
                metrics_server
                    .await
                    .context("join audit maintenance metrics server")?
                    .context("stop audit maintenance metrics server")?;
                return Ok(());
            }
            _ = interval.tick() => {
                let cycle_id = Uuid::now_v7().to_string();
                metrics.cycle_started();
                match store
                    .oldest_unarchived_audit_checkpoint(organization_id)
                    .await
                {
                    Ok(pending) => metrics.pending_checkpoint(pending.as_ref()),
                    Err(error) => {
                        metrics.failed();
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        tracing::error!(
                            %organization_id,
                            cycle_id,
                            consecutive_failures,
                            error = %error,
                            "tenant audit maintenance pending-checkpoint observation failed"
                        );
                        continue;
                    }
                }
                let lock = match store
                    .try_acquire_audit_maintenance_lock(organization_id)
                    .await
                {
                    Ok(Some(lock)) => lock,
                    Ok(None) => {
                        metrics.lock_conflict();
                        tracing::warn!(
                            %organization_id,
                            cycle_id,
                            "tenant audit maintenance cycle skipped because another instance is active"
                        );
                        continue;
                    }
                    Err(error) => {
                        metrics.failed();
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        tracing::error!(
                            %organization_id,
                            cycle_id,
                            consecutive_failures,
                            error = %error,
                            "tenant audit maintenance lock acquisition failed"
                        );
                        continue;
                    }
                };
                match audit_maintenance_cycle(
                    settings,
                    maintenance,
                    &store,
                    organization_id,
                    actor_principal_id,
                    &signer,
                    &cycle_id,
                )
                .await
                {
                    Ok(outcome) => {
                        match lock.release().await {
                            Ok(()) => {
                                metrics.succeeded();
                                consecutive_failures = 0;
                                tracing::info!(
                                    %organization_id,
                                    cycle_id,
                                    trace_id = outcome.trace_id,
                                    checkpoint_id = %outcome.checkpoint_id,
                                    archive_id = %outcome.archive_id,
                                    chain_sequence_end = outcome.chain_sequence_end,
                                    resumed_checkpoint = outcome.resumed_checkpoint,
                                    "tenant audit maintenance cycle completed"
                                );
                            }
                            Err(error) => {
                                metrics.failed();
                                consecutive_failures = consecutive_failures.saturating_add(1);
                                tracing::error!(
                                    %organization_id,
                                    cycle_id,
                                    consecutive_failures,
                                    error = %error,
                                    "tenant audit maintenance cycle completed but lock release failed"
                                );
                            }
                        }
                    }
                    Err(error) => {
                        drop(lock);
                        metrics.failed();
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        tracing::error!(
                            %organization_id,
                            cycle_id,
                            consecutive_failures,
                            error = %error,
                            "tenant audit maintenance cycle failed; exact pending checkpoint will be retried"
                        );
                    }
                }
            }
        }
    }
}

async fn load_audit_maintenance_identity(
    maintenance: &AuditMaintenanceSettings,
) -> Result<(OrganizationId, PrincipalId, RsaAuditCheckpointSigner)> {
    let organization_id = maintenance
        .organization_id
        .parse::<OrganizationId>()
        .context("audit maintenance organization ID is not a UUID")?;
    let actor_principal_id = maintenance
        .actor_principal_id
        .parse::<PrincipalId>()
        .context("audit maintenance actor Principal ID is not a UUID")?;
    let private_key = tokio::fs::read(&maintenance.private_key_file)
        .await
        .with_context(|| {
            format!(
                "read audit maintenance private key {}",
                maintenance.private_key_file.display()
            )
        })?;
    let public_key = tokio::fs::read(&maintenance.public_key_file)
        .await
        .with_context(|| {
            format!(
                "read audit maintenance public key {}",
                maintenance.public_key_file.display()
            )
        })?;
    let signer =
        RsaAuditCheckpointSigner::new(maintenance.key_id.clone(), &private_key, &public_key)
            .context("validate audit maintenance signing key pair")?;
    Ok((organization_id, actor_principal_id, signer))
}

#[tracing::instrument(
    name = "audit.maintenance.cycle",
    skip_all,
    fields(
        organization.id = %organization_id,
        actor.id = %actor_principal_id,
        cycle.id = cycle_id,
        copilotmail.trace_id = tracing::field::Empty,
        audit.checkpoint_id = tracing::field::Empty,
    )
)]
async fn audit_maintenance_cycle(
    settings: &ControlPlaneSettings,
    maintenance: &AuditMaintenanceSettings,
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    signer: &RsaAuditCheckpointSigner,
    cycle_id: &str,
) -> Result<AuditMaintenanceOutcome> {
    let authorization = store
        .resolve_authorization(actor_principal_id, Utc::now())
        .await
        .context("resolve audit maintenance actor")?
        .context("audit maintenance actor is inactive or missing")?;
    anyhow::ensure!(
        authorization.organization_id == organization_id
            && authorization.principal_id == actor_principal_id
            && authorization.permits(organization_id, Permission::AuditExport),
        "audit maintenance actor is not authorized for this tenant"
    );

    let pending = store
        .oldest_unarchived_audit_checkpoint(organization_id)
        .await
        .context("load oldest unarchived audit checkpoint")?;
    let resumed_checkpoint = pending.is_some();
    let stored = if let Some(stored) = pending {
        anyhow::ensure!(
            stored.checkpoint.payload.key_id == signer.key_id()
                && stored.checkpoint.payload.public_key_sha256 == signer.public_key_sha256(),
            "oldest unarchived checkpoint requires another trusted signing key"
        );
        stored
    } else {
        let candidate = store
            .audit_checkpoint_candidate(organization_id)
            .await
            .context("build verified audit checkpoint candidate")?
            .context("tenant audit chain is empty")?;
        let checkpoint_uuid = stable_maintenance_uuid(
            "checkpoint",
            &[
                organization_id.to_string().as_bytes(),
                candidate.chain_sequence_end.to_string().as_bytes(),
                signer.key_id().as_bytes(),
            ],
        );
        let checkpoint_id = AuditCheckpointId::from_uuid(checkpoint_uuid);
        let archive_object_key = format!(
            "{}/{}/{}-{}.events.jsonl",
            maintenance.object_prefix, organization_id, candidate.chain_sequence_end, checkpoint_id
        );
        anyhow::ensure!(
            archive_object_key.len() <= 1_024,
            "audit maintenance archive object key is too long"
        );
        CreateAuditCheckpointService::new(store.clone(), signer)
            .execute(CreateAuditCheckpointCommand {
                checkpoint_id,
                organization_id,
                actor_principal_id,
                archive_object_key,
                trace_id: stable_audit_artifact_trace_id(organization_id, checkpoint_id),
                now: Utc::now(),
            })
            .await
            .context("create deterministic audit checkpoint")?;
        store
            .get_signed_audit_checkpoint(organization_id, checkpoint_id)
            .await
            .context("reload deterministic audit checkpoint")?
            .context("new deterministic audit checkpoint is unavailable")?
    };
    let checkpoint_id = stored.checkpoint.payload.checkpoint_id;
    let trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
    tracing::Span::current().record("copilotmail.trace_id", &trace_id);
    tracing::Span::current().record(
        "audit.checkpoint_id",
        tracing::field::display(checkpoint_id),
    );
    tracing::info!(
        cycle_id,
        trace_id,
        %organization_id,
        %checkpoint_id,
        resumed_checkpoint,
        "tenant audit maintenance artifact selected"
    );
    let package_directory = maintenance
        .work_directory
        .join(organization_id.to_string())
        .join(checkpoint_id.to_string());
    prepare_audit_export_package(
        store,
        organization_id,
        checkpoint_id,
        &package_directory,
        &maintenance.public_key_file,
        &trace_id,
    )
    .await?;
    let archive_id = AuditArchiveId::from_uuid(stable_maintenance_uuid(
        "archive",
        &[
            organization_id.to_string().as_bytes(),
            checkpoint_id.to_string().as_bytes(),
        ],
    ));
    run_archive_audit_export_traced(
        settings,
        store,
        organization_id.to_string(),
        actor_principal_id.to_string(),
        checkpoint_id.to_string(),
        Some(archive_id.to_string()),
        package_directory.to_string_lossy().into_owned(),
        maintenance.public_key_file.to_string_lossy().into_owned(),
        Some(&trace_id),
    )
    .await
    .context("archive verified audit maintenance package")?;
    remove_completed_audit_package(&maintenance.work_directory, &package_directory, &trace_id)
        .await;
    Ok(AuditMaintenanceOutcome {
        trace_id,
        organization_id,
        checkpoint_id,
        archive_id,
        chain_sequence_end: stored.checkpoint.payload.chain_sequence_end,
        resumed_checkpoint,
    })
}

async fn prepare_audit_export_package(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    checkpoint_id: AuditCheckpointId,
    package_directory: &FsPath,
    public_key_file: &FsPath,
    trace_id: &str,
) -> Result<()> {
    if tokio::fs::try_exists(package_directory).await? {
        match verify_audit_export_files(package_directory, public_key_file) {
            Ok((manifest, report))
                if report.organization_id == organization_id
                    && report.checkpoint_id == checkpoint_id
                    && manifest.checkpoint.payload.checkpoint_id == checkpoint_id =>
            {
                tracing::info!(
                    trace_id,
                    %organization_id,
                    %checkpoint_id,
                    "reusing verified audit export package"
                );
                return Ok(());
            }
            _ => {
                let quarantine =
                    package_directory.with_extension(format!("incomplete-{}", Uuid::now_v7()));
                tokio::fs::rename(package_directory, &quarantine)
                    .await
                    .with_context(|| {
                        format!(
                            "quarantine incomplete audit package as {}",
                            quarantine.display()
                        )
                    })?;
                tracing::warn!(
                    trace_id,
                    %organization_id,
                    %checkpoint_id,
                    quarantine = %quarantine.display(),
                    "incomplete audit export package quarantined"
                );
            }
        }
    }
    if let Some(parent) = package_directory.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("create tenant audit maintenance work directory")?;
    }
    run_export_audit_checkpoint_traced(
        store,
        organization_id.to_string(),
        checkpoint_id.to_string(),
        package_directory.to_string_lossy().into_owned(),
        Some(trace_id),
    )
    .await
    .context("export audit maintenance checkpoint")
}

async fn ensure_audit_maintenance_root(path: &FsPath) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "audit maintenance work directory is empty"
    );
    tokio::fs::create_dir_all(path)
        .await
        .with_context(|| format!("create audit maintenance root {}", path.display()))?;
    let metadata = tokio::fs::symlink_metadata(path).await?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "audit maintenance root must be a real directory"
    );
    Ok(())
}

async fn remove_completed_audit_package(root: &FsPath, package: &FsPath, trace_id: &str) {
    let result = async {
        let root = tokio::fs::canonicalize(root).await?;
        let package = tokio::fs::canonicalize(package).await?;
        let metadata = tokio::fs::symlink_metadata(&package).await?;
        anyhow::ensure!(
            package.starts_with(&root)
                && package != root
                && metadata.is_dir()
                && !metadata.file_type().is_symlink(),
            "refusing unsafe audit package cleanup"
        );
        tokio::fs::remove_dir_all(&package).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(
            trace_id,
            package = %package.display(),
            error = %error,
            "completed audit package could not be cleaned; archive evidence is already durable"
        );
    }
}

fn stable_maintenance_uuid(label: &str, parts: &[&[u8]]) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.audit-maintenance.v1");
    digest.update((label.len() as u64).to_be_bytes());
    digest.update(label.as_bytes());
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    let digest = digest.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn stable_audit_artifact_trace_id(
    organization_id: OrganizationId,
    checkpoint_id: AuditCheckpointId,
) -> String {
    stable_maintenance_uuid(
        "trace",
        &[
            organization_id.to_string().as_bytes(),
            checkpoint_id.to_string().as_bytes(),
        ],
    )
    .to_string()
}

/// Idempotently creates the initial organization and root service identity.
pub async fn run_bootstrap_organization(
    store: &PgControlPlaneStore,
    slug: String,
    display_name: String,
    root_principal_name: String,
) -> Result<()> {
    let result = BootstrapOrganizationService::new(store.clone())
        .execute(BootstrapOrganizationCommand {
            slug,
            display_name,
            root_principal_name,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("bootstrap organization")?;
    let response = OrganizationBootstrapResponse {
        organization_id: result.organization_id.to_string(),
        root_principal_id: result.root_principal_id.to_string(),
        slug: result.slug,
        display_name: result.display_name,
        created: result.created,
    };
    println!("{}", serde_json::to_string(&response)?);
    Ok(())
}

/// Idempotently hands bootstrap ownership to the configured OIDC human.
pub async fn run_bind_oidc_owner(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    subject: String,
    display_name: String,
) -> Result<()> {
    let oidc = settings
        .oidc
        .as_ref()
        .context("COPILOTMAIL_OIDC_ISSUER and COPILOTMAIL_OIDC_AUDIENCE are required")?;
    let oidc_config = OidcConfig::new(
        oidc.issuer.clone(),
        oidc.audience.clone(),
        oidc.allow_insecure_http,
    )
    .context("validate enterprise OIDC configuration")?;
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let result = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id,
            issuer: oidc_config.issuer().into(),
            subject,
            display_name,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("bind initial OIDC organization owner")?;
    println!(
        "{}",
        serde_json::to_string(&OidcOwnerBindingResponse {
            organization_id: result.organization_id.to_string(),
            principal_id: result.principal_id.to_string(),
            retired_bootstrap_principal_id: result.retired_bootstrap_principal_id.to_string(),
            issuer: result.issuer,
            subject: result.subject,
            created: result.created,
        })?
    );
    Ok(())
}

/// Creates a high-entropy SCIM bearer credential and persists only its digest.
pub async fn run_issue_scim_token(
    store: &PgControlPlaneStore,
    organization_id: String,
    label: String,
    expires_in_days: u32,
) -> Result<()> {
    anyhow::ensure!(
        (1..=365).contains(&expires_in_days),
        "token lifetime must be 1..=365 days"
    );
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let token_id = ScimTokenId::new();
    let random_a = Uuid::new_v4().simple().to_string();
    let random_b = Uuid::new_v4().simple().to_string();
    let plaintext = format!("cpm_scim_{random_a}{random_b}");
    let token_hash = Sha256::digest(plaintext.as_bytes()).to_vec();
    let now = Utc::now();
    let expires_at = now
        .checked_add_signed(chrono::Duration::days(i64::from(expires_in_days)))
        .context("SCIM token expiration overflow")?;
    let credential = IssueScimTokenService::new(store.clone())
        .execute(IssueScimTokenCommand {
            organization_id,
            token_id,
            token_hash,
            label,
            expires_at: Some(expires_at),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("issue SCIM provisioning token")?;
    println!(
        "{}",
        serde_json::to_string(&ScimTokenIssuedResponse {
            organization_id: credential.organization_id.to_string(),
            token_id: credential.token_id.to_string(),
            label: credential.label,
            token: plaintext,
            expires_at: credential.expires_at.map(|value| value.to_rfc3339()),
        })?
    );
    Ok(())
}

/// Revokes a SCIM credential by non-secret stable ID.
pub async fn run_revoke_scim_token(
    store: &PgControlPlaneStore,
    organization_id: String,
    token_id: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let token_id = token_id
        .parse::<ScimTokenId>()
        .context("SCIM token ID is not a UUID")?;
    RevokeScimTokenService::new(store.clone())
        .execute(RevokeScimTokenCommand {
            organization_id,
            token_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("revoke SCIM provisioning token")?;
    println!(
        "{}",
        serde_json::to_string(&ScimTokenRevokedResponse {
            organization_id: organization_id.to_string(),
            token_id: token_id.to_string(),
            revoked: true,
        })?
    );
    Ok(())
}

/// Creates a tenant mailbox and its Owner membership atomically.
pub async fn run_create_mailbox(
    store: &PgControlPlaneStore,
    organization_id: String,
    kind: String,
    primary_address: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let kind = parse_mailbox_kind(&kind)?;
    let result = MailboxManagementService::new(store.clone())
        .create(CreateMailboxCommand {
            organization_id,
            kind,
            primary_address,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("create mailbox")?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "organization_id": result.organization_id,
            "mailbox_id": result.mailbox_id,
            "owner_principal_id": result.owner_principal_id,
            "primary_address": result.primary_address,
        }))?
    );
    Ok(())
}

/// Grants explicit, expiring mailbox scopes to one active Principal.
#[allow(clippy::too_many_arguments)]
pub async fn run_grant_mailbox_delegation(
    store: &PgControlPlaneStore,
    organization_id: String,
    mailbox_id: String,
    grantee_principal_id: String,
    capabilities: Vec<String>,
    reason: String,
    valid_in_hours: u32,
) -> Result<()> {
    anyhow::ensure!(
        (1..=8_760).contains(&valid_in_hours),
        "delegation lifetime must be 1..=8760 hours"
    );
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let mailbox_id = mailbox_id
        .parse::<MailboxId>()
        .context("mailbox ID is not a UUID")?;
    let grantee_principal_id = grantee_principal_id
        .parse::<PrincipalId>()
        .context("grantee Principal ID is not a UUID")?;
    let capabilities = parse_mailbox_capabilities(capabilities)?;
    let now = Utc::now();
    let result = MailboxDelegationService::new(store.clone())
        .grant(GrantMailboxDelegationCommand {
            organization_id,
            mailbox_id,
            grantee_principal_id,
            capabilities,
            reason,
            valid_from: now,
            valid_until: now + chrono::Duration::hours(i64::from(valid_in_hours)),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("grant mailbox delegation")?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "organization_id": organization_id,
            "delegation_id": result.delegation_id,
            "mailbox_id": result.mailbox_id,
            "grantor_principal_id": result.grantor_principal_id,
            "grantee_principal_id": result.grantee_principal_id,
        }))?
    );
    Ok(())
}

/// Immediately revokes one tenant mailbox delegation.
pub async fn run_revoke_mailbox_delegation(
    store: &PgControlPlaneStore,
    organization_id: String,
    delegation_id: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let delegation_id = delegation_id
        .parse::<DelegationId>()
        .context("delegation ID is not a UUID")?;
    MailboxDelegationService::new(store.clone())
        .revoke(RevokeMailboxDelegationCommand {
            organization_id,
            delegation_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("revoke mailbox delegation")?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "organization_id": organization_id,
            "delegation_id": delegation_id,
            "revoked": true,
        }))?
    );
    Ok(())
}

/// Creates and activates version 1 of a high-risk approval policy.
#[allow(clippy::too_many_arguments)]
pub async fn run_create_approval_policy(
    store: &PgControlPlaneStore,
    organization_id: String,
    policy_key: String,
    display_name: String,
    action: String,
    required_approvals: u8,
    rule_json: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let action = parse_approval_action(&action)?;
    let rule = serde_json::from_str(&rule_json).context("rule-json is not valid JSON")?;
    let result = ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id,
            policy_key,
            display_name,
            action,
            required_approvals,
            rule,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("create approval policy")?;
    print_policy_result(organization_id, result)?;
    Ok(())
}

/// Appends and atomically activates a new immutable approval policy version.
pub async fn run_append_approval_policy_version(
    store: &PgControlPlaneStore,
    organization_id: String,
    policy_key: String,
    action: String,
    required_approvals: u8,
    rule_json: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let action = parse_approval_action(&action)?;
    let rule = serde_json::from_str(&rule_json).context("rule-json is not valid JSON")?;
    let result = ApprovalPolicyService::new(store.clone())
        .append_version(AppendApprovalPolicyVersionCommand {
            organization_id,
            policy_key,
            action,
            required_approvals,
            rule,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("append approval policy version")?;
    print_policy_result(organization_id, result)?;
    Ok(())
}

/// Registers an Agent whose private key never enters CopilotMail.
#[allow(clippy::too_many_arguments)]
pub async fn run_register_agent(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    display_name: String,
    purpose: String,
    environment: String,
    risk_level: String,
    public_key_file: String,
    scopes: Vec<String>,
    review_in_days: u32,
    credential_expires_in_days: u32,
) -> Result<()> {
    anyhow::ensure!(
        (1..=365).contains(&review_in_days),
        "review lifetime must be 1..=365 days"
    );
    anyhow::ensure!(
        (1..=365).contains(&credential_expires_in_days),
        "credential lifetime must be 1..=365 days"
    );
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let environment = match environment.as_str() {
        "development" => AgentEnvironment::Development,
        "testing" => AgentEnvironment::Testing,
        "production" => AgentEnvironment::Production,
        _ => anyhow::bail!("environment must be development, testing, or production"),
    };
    let risk_level = match risk_level.as_str() {
        "low" => AgentRiskLevel::Low,
        "medium" => AgentRiskLevel::Medium,
        "high" => AgentRiskLevel::High,
        _ => anyhow::bail!("risk-level must be low, medium, or high"),
    };
    let scopes = parse_agent_scopes(scopes)?;
    let (public_key_pem, fingerprint) = load_agent_public_key(&public_key_file).await?;
    let now = Utc::now();
    let result = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id,
            display_name,
            environment,
            risk_level,
            purpose,
            review_due_at: now + chrono::Duration::days(i64::from(review_in_days)),
            public_key_pem,
            public_key_fingerprint: fingerprint,
            allowed_scopes: scopes,
            credential_expires_at: now
                + chrono::Duration::days(i64::from(credential_expires_in_days)),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("register Agent identity")?;
    println!(
        "{}",
        serde_json::to_string(&AgentRegisteredResponse {
            organization_id: result.organization_id.to_string(),
            client_id: result.principal_id.to_string(),
            credential_id: result.credential_id.to_string(),
            owner_principal_id: result.owner_principal_id.to_string(),
            allowed_scopes: result
                .allowed_scopes
                .iter()
                .map(|scope| scope.as_str().to_owned())
                .collect(),
            credential_expires_at: result.credential_expires_at.to_rfc3339(),
            token_endpoint: settings
                .agent_oauth
                .as_ref()
                .map(|agent| agent.token_endpoint.clone()),
        })?
    );
    Ok(())
}

/// Immediately revokes an Agent credential and every token bound to it.
pub async fn run_revoke_agent_credential(
    store: &PgControlPlaneStore,
    organization_id: String,
    credential_id: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let credential_id = credential_id
        .parse::<CredentialId>()
        .context("Agent credential ID is not a UUID")?;
    RevokeAgentCredentialService::new(store.clone())
        .execute(RevokeAgentCredentialCommand {
            organization_id,
            credential_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("revoke Agent credential")?;
    println!(
        "{}",
        serde_json::to_string(&AgentCredentialRevokedResponse {
            organization_id: organization_id.to_string(),
            credential_id: credential_id.to_string(),
            revoked: true,
        })?
    );
    Ok(())
}

/// Adds a second live key before the old credential is revoked.
pub async fn run_add_agent_credential(
    store: &PgControlPlaneStore,
    organization_id: String,
    client_id: String,
    public_key_file: String,
    scopes: Vec<String>,
    credential_expires_in_days: u32,
) -> Result<()> {
    anyhow::ensure!(
        (1..=365).contains(&credential_expires_in_days),
        "credential lifetime must be 1..=365 days"
    );
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let principal_id = client_id
        .parse::<copilotmail_domain::PrincipalId>()
        .context("Agent client ID is not a UUID")?;
    let scopes = parse_agent_scopes(scopes)?;
    let (public_key_pem, fingerprint) = load_agent_public_key(&public_key_file).await?;
    let now = Utc::now();
    let result = AddAgentCredentialService::new(store.clone())
        .execute(AddAgentCredentialCommand {
            organization_id,
            principal_id,
            public_key_pem,
            public_key_fingerprint: fingerprint,
            allowed_scopes: scopes,
            credential_expires_at: now
                + chrono::Duration::days(i64::from(credential_expires_in_days)),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("add Agent credential")?;
    println!(
        "{}",
        serde_json::to_string(&AgentCredentialAddedResponse {
            organization_id: result.organization_id.to_string(),
            client_id: result.principal_id.to_string(),
            credential_id: result.credential_id.to_string(),
            allowed_scopes: result
                .allowed_scopes
                .iter()
                .map(|scope| scope.as_str().to_owned())
                .collect(),
            credential_expires_at: result.credential_expires_at.to_rfc3339(),
        })?
    );
    Ok(())
}

/// Adds a TLS-verified certificate credential for RFC 8705-bound Agent tokens.
pub async fn run_add_agent_mtls_credential(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    client_id: String,
    certificate_file: String,
    scopes: Vec<String>,
    credential_expires_in_days: u32,
) -> Result<()> {
    anyhow::ensure!(
        settings.native_tls.is_some(),
        "native TLS must be configured before adding an Agent mTLS credential"
    );
    anyhow::ensure!(
        (1..=365).contains(&credential_expires_in_days),
        "credential lifetime must be 1..=365 days"
    );
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let principal_id = client_id
        .parse::<copilotmail_domain::PrincipalId>()
        .context("Agent client ID is not a UUID")?;
    let scopes = parse_agent_scopes(scopes)?;
    let certificate = tokio::fs::read(&certificate_file)
        .await
        .with_context(|| format!("read Agent mTLS certificate file {certificate_file}"))?;
    anyhow::ensure!(
        certificate.len() <= 64 * 1024,
        "Agent mTLS certificate file is too large"
    );
    let certificate_fingerprint = certificate_fingerprint_from_pem(&certificate)?;
    let now = Utc::now();
    let result = AddAgentMtlsCredentialService::new(store.clone())
        .execute(AddAgentMtlsCredentialCommand {
            organization_id,
            principal_id,
            certificate_fingerprint: certificate_fingerprint.clone(),
            allowed_scopes: scopes,
            credential_expires_at: now
                + chrono::Duration::days(i64::from(credential_expires_in_days)),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("add Agent mTLS credential")?;
    println!(
        "{}",
        serde_json::to_string(&AgentMtlsCredentialAddedResponse {
            organization_id: result.organization_id.to_string(),
            client_id: result.principal_id.to_string(),
            credential_id: result.credential_id.to_string(),
            credential_kind: "mtls".into(),
            certificate_fingerprint,
            allowed_scopes: result
                .allowed_scopes
                .iter()
                .map(|scope| scope.as_str().to_owned())
                .collect(),
            credential_expires_at: result.credential_expires_at.to_rfc3339(),
        })?
    );
    Ok(())
}

fn parse_mailbox_kind(value: &str) -> Result<MailboxKind> {
    mailbox_kind_from_wire(value).context("mailbox kind must be human, agent, shared, or group")
}

fn mailbox_kind_from_wire(value: &str) -> Option<MailboxKind> {
    match value {
        "human" => Some(MailboxKind::Human),
        "agent" => Some(MailboxKind::Agent),
        "shared" => Some(MailboxKind::Shared),
        "group" => Some(MailboxKind::Group),
        _ => None,
    }
}

fn parse_mailbox_capabilities(capabilities: Vec<String>) -> Result<BTreeSet<MailboxCapability>> {
    capabilities
        .into_iter()
        .map(|capability| {
            mailbox_capability_from_wire(&capability)
                .with_context(|| format!("unsupported mailbox capability: {capability}"))
        })
        .collect()
}

fn mailbox_capability_from_wire(value: &str) -> Option<MailboxCapability> {
    MailboxCapability::DELEGABLE
        .into_iter()
        .find(|candidate| candidate.as_str() == value)
}

fn parse_approval_action(value: &str) -> Result<ApprovalAction> {
    approval_action_from_wire(value)
        .with_context(|| format!("unsupported approval action: {value}"))
}

fn approval_action_from_wire(value: &str) -> Option<ApprovalAction> {
    [
        ApprovalAction::SendAs,
        ApprovalAction::SendOnBehalfOf,
        ApprovalAction::ContentReview,
        ApprovalAction::AuditExport,
        ApprovalAction::RetentionChange,
        ApprovalAction::PrivilegeElevation,
        ApprovalAction::LegalHold,
        ApprovalAction::BreakGlass,
        ApprovalAction::BulkDelete,
        ApprovalAction::ProductionAgentMail,
        ApprovalAction::AgentFirstContact,
    ]
    .into_iter()
    .find(|action| action.as_str() == value)
}

fn print_policy_result(
    organization_id: OrganizationId,
    result: copilotmail_application::CreateApprovalPolicyResult,
) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "organization_id": organization_id,
            "policy_id": result.policy_id,
            "policy_version_id": result.policy_version_id,
        }))?
    );
    Ok(())
}

fn parse_agent_scopes(scopes: Vec<String>) -> Result<BTreeSet<Permission>> {
    scopes
        .into_iter()
        .map(|scope| {
            Permission::AGENT_TOKEN
                .into_iter()
                .find(|permission| permission.as_str() == scope)
                .with_context(|| format!("unsupported Agent scope: {scope}"))
        })
        .collect()
}

async fn load_agent_public_key(public_key_file: &str) -> Result<(String, String)> {
    let public_key = tokio::fs::read(public_key_file)
        .await
        .with_context(|| format!("read Agent public key file {public_key_file}"))?;
    anyhow::ensure!(
        public_key.len() <= 16 * 1024,
        "Agent public key file is too large"
    );
    let fingerprint =
        rsa_public_key_fingerprint(&public_key).context("validate Agent RSA public key")?;
    let public_key_pem =
        String::from_utf8(public_key).context("Agent public key must be UTF-8 PEM")?;
    Ok((public_key_pem, fingerprint))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentSigningKeySetManifest {
    active_key_id: String,
    keys: Vec<AgentSigningKeyFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentSigningKeyFile {
    key_id: String,
    private_key_file: Option<PathBuf>,
    public_key_file: PathBuf,
}

struct LoadedAgentSigningKeySet {
    active_key_id: String,
    keys: Vec<AgentSigningKeyInput>,
    digest: Vec<u8>,
}

async fn load_agent_signing_key_set(
    settings: &AgentOAuthSettings,
) -> Result<LoadedAgentSigningKeySet> {
    if let Some(manifest_path) = &settings.signing_key_set_file {
        let manifest_bytes = read_bounded_secret_file(manifest_path, 64 * 1024).await?;
        let manifest = serde_json::from_slice::<AgentSigningKeySetManifest>(&manifest_bytes)
            .context("parse Agent OAuth signing key-set manifest")?;
        anyhow::ensure!(
            (1..=10).contains(&manifest.keys.len()),
            "Agent OAuth signing key-set must contain 1..=10 keys"
        );
        let base = manifest_path.parent().unwrap_or_else(|| FsPath::new("."));
        let mut digest = Sha256::new();
        digest.update(&manifest_bytes);
        let mut keys = Vec::with_capacity(manifest.keys.len());
        for key in manifest.keys {
            let public_path = resolve_key_path(base, &key.public_key_file);
            let public_key_pem = read_bounded_secret_file(&public_path, 64 * 1024).await?;
            digest.update(key.key_id.as_bytes());
            digest.update(&public_key_pem);
            let private_key_pem = if let Some(private_path) = key.private_key_file {
                let private_path = resolve_key_path(base, &private_path);
                let value = read_bounded_secret_file(&private_path, 64 * 1024).await?;
                digest.update(&value);
                Some(value)
            } else {
                None
            };
            keys.push(AgentSigningKeyInput {
                key_id: key.key_id,
                private_key_pem,
                public_key_pem,
            });
        }
        return Ok(LoadedAgentSigningKeySet {
            active_key_id: manifest.active_key_id,
            keys,
            digest: digest.finalize().to_vec(),
        });
    }

    let key_id = settings
        .signing_key_id
        .clone()
        .context("Agent OAuth single signing key ID is missing")?;
    let private_path = settings
        .private_key_file
        .as_ref()
        .context("Agent OAuth single signing private key file is missing")?;
    let public_path = settings
        .public_key_file
        .as_ref()
        .context("Agent OAuth single signing public key file is missing")?;
    let private_key_pem = read_bounded_secret_file(private_path, 64 * 1024).await?;
    let public_key_pem = read_bounded_secret_file(public_path, 64 * 1024).await?;
    let mut digest = Sha256::new();
    digest.update(key_id.as_bytes());
    digest.update(&private_key_pem);
    digest.update(&public_key_pem);
    Ok(LoadedAgentSigningKeySet {
        active_key_id: key_id.clone(),
        keys: vec![AgentSigningKeyInput {
            key_id,
            private_key_pem: Some(private_key_pem),
            public_key_pem,
        }],
        digest: digest.finalize().to_vec(),
    })
}

fn spawn_signing_key_reload(
    settings: AgentOAuthSettings,
    manager: AgentSigningKeyManager,
    mut current_digest: Vec<u8>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            settings.signing_key_reload_seconds,
        ));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            match load_agent_signing_key_set(&settings).await {
                Ok(key_set) if key_set.digest == current_digest => {}
                Ok(key_set) => {
                    match manager.replace(key_set.active_key_id, key_set.keys, Utc::now()) {
                        Ok(()) => {
                            current_digest = key_set.digest;
                            tracing::info!("Agent OAuth signing key set reloaded atomically");
                        }
                        Err(error) => {
                            tracing::error!(%error, "Agent OAuth signing key-set reload rejected");
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(%error, "Agent OAuth signing key-set reload failed");
                }
            }
        }
    });
}

fn resolve_key_path(base: &FsPath, path: &FsPath) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

async fn build_human_session_runtime(
    oidc: &copilotmail_platform::OidcSettings,
    settings: &HumanSessionSettings,
    store: PgControlPlaneStore,
) -> Result<HumanSessionRuntime> {
    let key_bytes = read_bounded_secret_file(&settings.encryption_key_file, 32).await?;
    anyhow::ensure!(
        key_bytes.len() == 32,
        "COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE must contain exactly 32 bytes"
    );
    let protector = PkceProtector::new(&key_bytes)?;
    let mut clients = BTreeMap::new();
    for client in &settings.clients {
        let client_key = match client.key.as_str() {
            "admin" => HumanSessionClient::Admin,
            "mail" => HumanSessionClient::Mail,
            _ => anyhow::bail!("unsupported Human browser client key"),
        };
        let redirect_uri = format!(
            "{}/api/v2/auth/{}/callback",
            client.public_origin, client.key
        );
        let public_client = OidcPublicClientConfig::new(
            client.client_id.clone(),
            &redirect_uri,
            client.scope.split_ascii_whitespace(),
            settings.allow_insecure_cookie,
        )
        .context("validate Human browser OIDC client")?;
        let relying_party = OidcRelyingParty::discover(
            OidcConfig::new(
                oidc.issuer.clone(),
                oidc.audience.clone(),
                oidc.allow_insecure_http,
            )
            .context("validate Human browser OIDC provider")?,
            public_client,
            store.clone(),
        )
        .await
        .context("initialize Human browser OIDC relying party")?;
        anyhow::ensure!(
            clients
                .insert(
                    client.key.clone(),
                    HumanBrowserClientRuntime {
                        client_key,
                        public_origin: client.public_origin.clone(),
                        relying_party: Arc::new(relying_party),
                    },
                )
                .is_none(),
            "duplicate Human browser client key"
        );
    }
    anyhow::ensure!(!clients.is_empty(), "Human browser clients are empty");
    Ok(HumanSessionRuntime {
        clients: Arc::new(clients),
        protector,
        maximum_lifetime: settings.maximum_lifetime,
        login_requests_per_minute: settings.login_requests_per_minute,
        secure_cookie: !settings.allow_insecure_cookie,
    })
}

async fn read_bounded_secret_file(path: &FsPath, maximum_bytes: usize) -> Result<Vec<u8>> {
    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("inspect secret file {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file() && metadata.len() <= maximum_bytes as u64,
        "secret file {} is invalid or too large",
        path.display()
    );
    tokio::fs::read(path)
        .await
        .with_context(|| format!("read secret file {}", path.display()))
}

/// Serves the V2 health, readiness, and system information endpoints.
pub async fn run_http(settings: &ControlPlaneSettings, store: PgControlPlaneStore) -> Result<()> {
    let agent_event_stream_metrics = AgentEventStreamMetrics::default();
    let scim_identity_provider = settings.oidc.as_ref().map(|oidc| oidc.issuer.clone());
    let mail_data_plane_id = settings.mail_data_plane_id.map(MailDataPlaneId::from_uuid);
    ensure_mail_data_plane_registration(
        &store,
        mail_data_plane_id,
        settings.mail_data_plane_origin.as_deref(),
    )
    .await?;
    let mail_engine = settings
        .mail_engine
        .as_ref()
        .map(|adapter| {
            build_mail_engine(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_content = settings
        .mail_content
        .as_ref()
        .map(|adapter| {
            build_mail_content_reader(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_drafts = settings
        .mail_compose
        .as_ref()
        .map(|adapter| {
            build_mail_draft_store(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_draft_content = settings
        .mail_compose
        .as_ref()
        .map(|adapter| {
            build_mail_draft_content_reader(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let human_session = match (&settings.oidc, &settings.human_session) {
        (Some(oidc), Some(session)) => {
            Some(build_human_session_runtime(oidc, session, store.clone()).await?)
        }
        (None, Some(_)) => {
            anyhow::bail!("Human browser sessions require enterprise OIDC configuration")
        }
        (_, None) => None,
    };
    let (human_authenticator, authentication_ready) = if let Some(oidc) = &settings.oidc {
        let config = OidcConfig::new(
            oidc.issuer.clone(),
            oidc.audience.clone(),
            oidc.allow_insecure_http,
        )
        .context("validate enterprise OIDC configuration")?;
        let authenticator = OidcAuthenticator::discover(config, store.clone())
            .await
            .context("initialize enterprise OIDC provider")?;
        (
            Arc::new(authenticator) as Arc<dyn AccessTokenAuthenticator>,
            true,
        )
    } else {
        (
            Arc::new(UnconfiguredAuthenticator) as Arc<dyn AccessTokenAuthenticator>,
            false,
        )
    };
    let (agent_authenticator, agent_token_service, agent_authentication_ready) =
        if let Some(agent) = &settings.agent_oauth {
            let key_set = load_agent_signing_key_set(agent).await?;
            let config = AgentOAuthConfig::new_with_signing_keys(
                agent.issuer.clone(),
                agent.audience.clone(),
                agent.token_endpoint.clone(),
                key_set.active_key_id.clone(),
                key_set.keys,
                agent.access_token_ttl_seconds,
                agent.allow_insecure_http,
            )
            .context("validate Agent OAuth configuration")?;
            let service = Arc::new(AgentTokenService::new(store.clone(), config));
            if agent.signing_key_set_file.is_some() {
                spawn_signing_key_reload(
                    agent.clone(),
                    service.signing_key_manager(),
                    key_set.digest,
                );
            }
            let authenticator =
                Arc::new(service.authenticator()) as Arc<dyn AccessTokenAuthenticator>;
            (Some(authenticator), Some(service), true)
        } else {
            (None, None, false)
        };
    let authenticator: Arc<dyn AccessTokenAuthenticator> = match agent_authenticator {
        Some(agent) => Arc::new(CompositeAuthenticator {
            agent,
            human: if authentication_ready {
                human_authenticator
            } else {
                Arc::new(RejectingAuthenticator)
            },
        }),
        None => human_authenticator,
    };
    let app = router_with_state(
        AppState {
            store,
            authenticator,
            authentication_ready,
            human_auth_required: settings.require_human_auth,
            human_session,
            human_session_required: settings.require_human_session,
            agent_token_service,
            agent_authentication_ready,
            agent_auth_required: settings.require_agent_auth,
            agent_event_stream_tenant_limit: settings.agent_event_stream_tenant_limit,
            agent_event_stream_principal_limit: settings.agent_event_stream_principal_limit,
            agent_ingress_tenant_requests_per_minute: settings
                .agent_ingress_tenant_requests_per_minute,
            agent_ingress_principal_requests_per_minute: settings
                .agent_ingress_principal_requests_per_minute,
            agent_event_stream_metrics: agent_event_stream_metrics.clone(),
            mail_data_plane_id,
            mail_engine,
            mail_engine_required: settings.require_mail_engine,
            mail_content,
            mail_content_required: settings.require_mail_content,
            mail_drafts,
            mail_draft_content,
            mail_drafts_required: settings.require_mail_compose,
            mail_event_ingress: settings.mail_event_ingress.clone(),
            mail_event_ingress_required: settings.require_mail_event_ingress,
            audit_archive: settings.audit_archive.clone(),
        },
        scim_identity_provider,
    );
    tracing::info!(
        address = %settings.http_addr,
        environment = %settings.environment,
        native_tls = settings.native_tls.is_some(),
        "V2 control plane listening"
    );
    let metrics_listener = tokio::net::TcpListener::bind(settings.agent_event_stream_metrics_addr)
        .await
        .with_context(|| {
            format!(
                "bind Agent event stream metrics listener {}",
                settings.agent_event_stream_metrics_addr
            )
        })?;
    let metrics_router = agent_event_stream_metrics_router(agent_event_stream_metrics);
    let metrics_task =
        tokio::spawn(async move { axum::serve(metrics_listener, metrics_router).await });
    let result = if let Some(native_tls) = &settings.native_tls {
        let listener = NativeTlsListener::bind(settings.http_addr, native_tls).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<TlsConnectionInfo>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve V2 native TLS control plane")
    } else {
        let listener = tokio::net::TcpListener::bind(settings.http_addr)
            .await
            .context("bind V2 HTTP listener")?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<TlsConnectionInfo>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve V2 HTTP control plane")
    };
    metrics_task.abort();
    let _ = metrics_task.await;
    result
}

/// Runs the initial durable outbox worker using a structured-log event sink.
///
/// The log sink is deliberately an adapter: later phases replace it with
/// signed webhooks and other configured event publishers without changing
/// the outbox leasing semantics.
pub async fn run_worker(settings: &ControlPlaneSettings, store: PgControlPlaneStore) -> Result<()> {
    let worker_id = Uuid::now_v7();
    let mail_data_plane_id = settings.mail_data_plane_id.map(MailDataPlaneId::from_uuid);
    ensure_mail_data_plane_registration(
        &store,
        mail_data_plane_id,
        settings.mail_data_plane_origin.as_deref(),
    )
    .await?;
    let mail_domain_projector = settings
        .mail_engine
        .as_ref()
        .map(|adapter| {
            build_mail_domain_projector(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_submission_adapter = settings
        .mail_submission
        .as_ref()
        .map(|adapter| {
            build_mail_submission_adapter(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    if let Some(adapter) = &mail_submission_adapter {
        adapter
            .probe_submissions()
            .await
            .context("probe dedicated mail-submission credential")?;
    } else if settings.require_mail_submission {
        anyhow::bail!("mail submission is required but not configured");
    }
    let webhook_client = settings
        .webhook_delivery
        .as_ref()
        .map(|webhook| {
            let dns = HickoryDnsLookup::from_system().context("initialize webhook DNS resolver")?;
            SignedWebhookClient::new(
                dns,
                WebhookDeliveryConfig {
                    timeout: webhook.timeout,
                    allow_insecure_http: webhook.allow_insecure_http,
                    allow_private_network: webhook.allow_private_network,
                },
            )
            .context("initialize signed webhook client")
        })
        .transpose()?;
    if let Some(webhook) = &settings.webhook_delivery {
        WebhookSigningKeySet::load(&webhook.signing_key_set_file)
            .context("load webhook signing key set")?;
    }
    let mut interval = tokio::time::interval(settings.worker_poll_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut consecutive_failures = 0_u32;
    tracing::info!(%worker_id, "durable outbox worker started");

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!(%worker_id, "durable outbox worker stopping");
                return Ok(());
            }
            _ = interval.tick() => {
                let tick_result: Result<()> = async {
                    let events = store
                        .claim_outbox_batch(
                            worker_id,
                            mail_data_plane_id,
                            settings.worker_batch_size,
                        )
                        .await
                        .context("claim outbox batch")?;
                    for event in events {
                    if mail_event_requires_data_plane_route(&event.event_type)
                        && (event.data_plane_id.is_none()
                            || event.data_plane_id != mail_data_plane_id)
                    {
                        let released = store
                            .release_outbox_event(
                                event.id,
                                worker_id,
                                "mail_data_plane_route_invalid",
                            )
                            .await
                            .context("release incorrectly routed mail event")?;
                        tracing::error!(
                            event_id = %event.id,
                            event_type = %event.event_type,
                            released,
                            "mail event route is missing or does not match this worker"
                        );
                        continue;
                    }
                    if event.event_type == "mail.submission.requested.v1" {
                        let Some(adapter) = mail_submission_adapter.clone() else {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_submission_not_configured",
                                )
                                .await
                                .context("schedule mail submission after missing configuration")?;
                            tracing::warn!(
                                event_id = %event.id,
                                released,
                                "mail submission credential is not configured"
                            );
                            continue;
                        };
                        let submission_id = match Uuid::parse_str(&event.aggregate_id) {
                            Ok(id) => MailSubmissionId::from_uuid(id),
                            Err(_) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "mail_submission_event_invalid",
                                    )
                                    .await
                                    .context("schedule malformed mail submission event")?;
                                tracing::error!(
                                    event_id = %event.id,
                                    released,
                                    "mail submission event aggregate ID is invalid"
                                );
                                continue;
                            }
                        };
                        let execution = MailSubmissionExecutor::new(store.clone(), adapter)
                            .execute(ExecuteMailSubmissionCommand {
                                submission_id,
                                trace_id: event.id.to_string(),
                                now: Utc::now(),
                            })
                            .await;
                        let terminal = match execution {
                            Ok(
                                MailSubmissionExecutionResult::Accepted
                                | MailSubmissionExecutionResult::Rejected
                                | MailSubmissionExecutionResult::ManualReview,
                            ) => true,
                            Ok(MailSubmissionExecutionResult::NotAcquired) => store
                                .get_mail_submission_intent_for_execution(submission_id)
                                .await
                                .context("inspect unacquired mail submission intent")?
                                .is_some_and(|intent| {
                                    matches!(
                                        intent.status,
                                        MailSubmissionStatus::Accepted
                                            | MailSubmissionStatus::Rejected
                                            | MailSubmissionStatus::ManualReview
                                    )
                                }),
                            Ok(
                                MailSubmissionExecutionResult::Deferred
                                | MailSubmissionExecutionResult::LostLease,
                            ) => false,
                            Err(error) => {
                                tracing::warn!(
                                    event_id = %event.id,
                                    submission_id = %submission_id,
                                    error = %error,
                                    "mail submission execution failed safely"
                                );
                                false
                            }
                        };
                        if !terminal {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_submission_reconciling",
                                )
                                .await
                                .context("schedule mail submission reconciliation")?;
                            tracing::warn!(
                                event_id = %event.id,
                                submission_id = %submission_id,
                                released,
                                "mail submission remains non-terminal"
                            );
                            continue;
                        }
                    }
                    if event.event_type == "mail.delivery.reconciliation.requested.v1" {
                        let Some(adapter) = mail_submission_adapter.clone() else {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_delivery_not_configured",
                                )
                                .await
                                .context("schedule delivery reconciliation after missing configuration")?;
                            tracing::warn!(
                                event_id = %event.id,
                                released,
                                "mail delivery reconciliation credential is not configured"
                            );
                            continue;
                        };
                        let submission_id = match Uuid::parse_str(&event.aggregate_id) {
                            Ok(id) => MailSubmissionId::from_uuid(id),
                            Err(_) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "mail_delivery_event_invalid",
                                    )
                                    .await
                                    .context("schedule malformed delivery reconciliation event")?;
                                tracing::error!(
                                    event_id = %event.id,
                                    released,
                                    "mail delivery event aggregate ID is invalid"
                                );
                                continue;
                            }
                        };
                        let fingerprint_scheme =
                            mail_data_plane_fingerprint_scheme(adapter.data_plane_id());
                        let reconciliation = MailDeliveryReconciliationService::new(
                            store.clone(),
                            adapter,
                            fingerprint_scheme,
                        )
                                .reconcile(ReconcileMailDeliveryCommand {
                                    submission_id,
                                    now: Utc::now(),
                                })
                                .await;
                        if !matches!(
                            reconciliation,
                            Ok(MailDeliveryReconciliationResult::Terminal { .. })
                        ) {
                            let error_code = match reconciliation {
                                Ok(MailDeliveryReconciliationResult::Pending { .. }) => {
                                    "mail_delivery_pending"
                                }
                                Ok(MailDeliveryReconciliationResult::Terminal { .. }) => {
                                    unreachable!("terminal result handled above")
                                }
                                Err(_) => "mail_delivery_reconciliation_failed",
                            };
                            let released = store
                                .release_outbox_event(event.id, worker_id, error_code)
                                .await
                                .context("schedule mail delivery reconciliation")?;
                            tracing::warn!(
                                event_id = %event.id,
                                submission_id = %submission_id,
                                released,
                                error_code,
                                "mail delivery remains non-terminal"
                            );
                            continue;
                        }
                    }
                    if matches!(
                        event.event_type.as_str(),
                        "mail_domain.activated.v1" | "mail_domain.suspended.v1"
                    ) {
                        let projection = project_mail_domain_event(
                            &store,
                            mail_domain_projector.as_deref(),
                            &event,
                        )
                        .await;
                        if let Err(error) = projection {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_domain_projection_failed",
                                )
                                .await
                                .context("schedule mail domain projection retry")?;
                            tracing::warn!(
                                event_id = %event.id,
                                event_type = %event.event_type,
                                released,
                                error = %error,
                                "mail domain projection failed and was scheduled for retry"
                            );
                            continue;
                        }
                    }
                    if let (Some(webhook), Some(client)) =
                        (&settings.webhook_delivery, webhook_client.as_ref())
                    {
                        match publish_event_to_webhooks(
                            &store,
                            client,
                            &webhook.signing_key_set_file,
                            webhook.max_attempts,
                            &event,
                        )
                        .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "webhook_delivery_retrying",
                                    )
                                    .await
                                    .context("schedule webhook delivery retry")?;
                                tracing::warn!(
                                    event_id = %event.id,
                                    event_type = %event.event_type,
                                    released,
                                    "webhook delivery remains retryable"
                                );
                                continue;
                            }
                            Err(error) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "webhook_delivery_infrastructure_failed",
                                    )
                                    .await
                                    .context("schedule webhook infrastructure retry")?;
                                tracing::warn!(
                                    event_id = %event.id,
                                    event_type = %event.event_type,
                                    released,
                                    error = %error,
                                    "webhook delivery infrastructure failed"
                                );
                                continue;
                            }
                        }
                    }
                    tracing::info!(
                        event_id = %event.id,
                        event_type = %event.event_type,
                        aggregate_type = %event.aggregate_type,
                        aggregate_id = %event.aggregate_id,
                        schema_version = event.schema_version,
                        attempts = event.attempts,
                        occurred_at = %event.occurred_at,
                        webhook_delivery_configured = settings.webhook_delivery.is_some(),
                        "application event reached a terminal publication state"
                    );
                    let marked = store
                        .mark_outbox_published(event.id, worker_id)
                        .await
                        .context("mark outbox event published")?;
                        if !marked {
                            tracing::warn!(event_id = %event.id, "outbox claim was lost before acknowledgement");
                        }
                    }
                    Ok(())
                }
                .await;
                match tick_result {
                    Ok(()) => consecutive_failures = 0,
                    Err(error) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        let retry_after = worker_error_backoff(consecutive_failures);
                        tracing::error!(
                            %worker_id,
                            consecutive_failures,
                            retry_after_ms = retry_after.as_millis(),
                            error = %error,
                            "durable outbox worker tick failed; retrying"
                        );
                        tokio::select! {
                            _ = &mut shutdown => {
                                tracing::info!(%worker_id, "durable outbox worker stopping during retry backoff");
                                return Ok(());
                            }
                            _ = tokio::time::sleep(retry_after) => {}
                        }
                    }
                }
            }
        }
    }
}

fn mail_event_requires_data_plane_route(event_type: &str) -> bool {
    matches!(
        event_type,
        "mail_engine.mailbox_binding.updated.v1"
            | "mail_engine.mailbox_binding.updated.v2"
            | "message.received.v1"
            | "mail_engine.delivery_trace.observed.v1"
            | "mail.submission.requested.v1"
            | "agent.mail.admitted.v1"
            | "mail.submission.accepted.v1"
            | "mail.submission.rejected.v1"
            | "mail.submission.review_required.v1"
            | "mail.delivery.reconciliation.requested.v1"
            | "message.delivery_status_changed.v1"
            | "mail_domain.activated.v1"
            | "mail_domain.suspended.v1"
    )
}

async fn publish_event_to_webhooks(
    store: &PgControlPlaneStore,
    client: &SignedWebhookClient<HickoryDnsLookup>,
    signing_key_set_file: &FsPath,
    max_attempts: i32,
    event: &copilotmail_postgres::OutboxRecord,
) -> Result<bool> {
    let organization_id = event
        .organization_id()
        .context("Outbox event has no valid organization_id")?;
    let targets = store
        .pending_webhook_delivery_targets(
            organization_id,
            event.id,
            &event.event_type,
            event.occurred_at,
        )
        .await
        .context("load pending webhook delivery targets")?;
    if targets.is_empty() {
        return Ok(true);
    }
    let key_set =
        WebhookSigningKeySet::load(signing_key_set_file).context("reload webhook signing keys")?;
    let webhook_event = WebhookEvent {
        id: event.id,
        organization_id: organization_id.as_uuid(),
        event_type: event.event_type.clone(),
        aggregate_type: event.aggregate_type.clone(),
        aggregate_id: event.aggregate_id.clone(),
        schema_version: event.schema_version,
        payload: event.payload.clone(),
        occurred_at: event.occurred_at,
    };
    let mut all_terminal = true;
    for target in targets {
        let attempted_at = Utc::now();
        let next_attempt = target.previous_attempts.saturating_add(1);
        let delivery = match key_set.get(&target.signing_key_reference) {
            Some(secret) => {
                client
                    .deliver(
                        &target.endpoint_url,
                        &target.signing_key_reference,
                        secret,
                        &webhook_event,
                        attempted_at,
                    )
                    .await
            }
            None => Err(WebhookError::SigningKeyNotFound),
        };
        let outcome = match delivery {
            Ok(receipt) => WebhookDeliveryOutcome::Delivered {
                response_status: receipt.response_status,
            },
            Err(error) => {
                let response_status = match &error {
                    WebhookError::Rejected(status) => Some(*status),
                    _ => None,
                };
                if error.retryable() && next_attempt < max_attempts {
                    all_terminal = false;
                    WebhookDeliveryOutcome::Retrying {
                        response_status,
                        error_code: error.code().into(),
                        next_attempt_at: attempted_at + webhook_retry_delay(next_attempt),
                    }
                } else {
                    tracing::error!(
                        event_id = %event.id,
                        subscription_id = %target.subscription_id,
                        error_code = error.code(),
                        attempts = next_attempt,
                        "webhook delivery entered dead letter"
                    );
                    WebhookDeliveryOutcome::DeadLettered {
                        response_status,
                        error_code: error.code().into(),
                    }
                }
            }
        };
        let recorded = store
            .append_webhook_delivery(target.subscription_id, event.id, outcome, attempted_at)
            .await
            .context("append webhook delivery outcome")?;
        anyhow::ensure!(
            recorded.is_some(),
            "webhook subscription disappeared during delivery"
        );
    }
    Ok(all_terminal)
}

fn webhook_retry_delay(attempt: i32) -> chrono::Duration {
    let exponent = u32::try_from(attempt.clamp(0, 8)).unwrap_or(8);
    chrono::Duration::seconds(5_i64.saturating_mul(2_i64.saturating_pow(exponent)))
}

async fn project_mail_domain_event(
    store: &PgControlPlaneStore,
    projector: Option<&dyn MailDomainProjector>,
    event: &copilotmail_postgres::OutboxRecord,
) -> Result<()> {
    let mail_domain_id = event
        .aggregate_id
        .parse::<MailDomainId>()
        .context("mail domain Outbox aggregate ID is invalid")?;
    let target = store
        .load_mail_domain_projection_target(mail_domain_id)
        .await
        .context("load mail domain projection target")?
        .context("mail domain projection target no longer exists")?;
    let Some(projector) = projector else {
        anyhow::bail!("mail engine is not configured");
    };
    let data_plane_id = projector.data_plane_id();
    anyhow::ensure!(
        event.data_plane_id == Some(data_plane_id)
            && target.mail_data_plane_id == Some(data_plane_id),
        "mail domain event, durable assignment, and projector data plane do not match"
    );
    let attempted_at = Utc::now();
    anyhow::ensure!(
        store
            .mark_mail_domain_projection_attempt(
                mail_domain_id,
                target.status,
                data_plane_id,
                target.projection_last_attempt_at,
                attempted_at,
            )
            .await
            .context("record mail domain projection attempt")?,
        "mail domain changed before projection attempt"
    );
    let enabled = target.status == MailDomainStatus::Active;
    let projection = match projector
        .reconcile_domain(
            &target.name,
            target.mail_engine_domain_ref.as_deref(),
            enabled,
        )
        .await
    {
        Ok(projection) => projection,
        Err(error) => {
            store
                .fail_mail_domain_projection(
                    mail_domain_id,
                    target.status,
                    data_plane_id,
                    attempted_at,
                    mail_engine_error_code(error),
                    Utc::now(),
                )
                .await
                .context("record mail engine projection failure")?;
            return Err(error).context("reconcile mail engine domain");
        }
    };
    if projection.name != target.name.as_str() || projection.enabled != enabled {
        store
            .fail_mail_domain_projection(
                mail_domain_id,
                target.status,
                data_plane_id,
                attempted_at,
                "mail_engine_projection_mismatch",
                Utc::now(),
            )
            .await
            .context("record mismatched mail engine projection")?;
        anyhow::bail!("mail engine returned a mismatched domain projection");
    }
    anyhow::ensure!(
        store
            .complete_mail_domain_projection(
                mail_domain_id,
                target.status,
                data_plane_id,
                attempted_at,
                &projection.engine_ref,
                enabled,
                Utc::now(),
            )
            .await
            .context("complete mail engine domain projection")?,
        "mail domain changed or engine reference conflicted before projection completion"
    );
    Ok(())
}

const fn mail_engine_error_code(error: MailEngineError) -> &'static str {
    match error {
        MailEngineError::InvalidConfiguration => "mail_engine_invalid_configuration",
        MailEngineError::CredentialUnavailable => "mail_engine_credential_unavailable",
        MailEngineError::Unavailable => "mail_engine_unavailable",
        MailEngineError::InvalidSession => "mail_engine_invalid_session",
        MailEngineError::InvalidResponse => "mail_engine_invalid_response",
        MailEngineError::Rejected => "mail_engine_rejected",
        MailEngineError::ProjectionConflict => "mail_engine_projection_conflict",
    }
}

#[cfg(test)]
fn router(store: PgControlPlaneStore) -> Router {
    router_with_state(
        AppState {
            store,
            authenticator: Arc::new(UnconfiguredAuthenticator),
            authentication_ready: false,
            human_auth_required: false,
            human_session: None,
            human_session_required: false,
            agent_token_service: None,
            agent_authentication_ready: false,
            agent_auth_required: false,
            agent_event_stream_tenant_limit: 100,
            agent_event_stream_principal_limit: 5,
            agent_ingress_tenant_requests_per_minute: 6_000,
            agent_ingress_principal_requests_per_minute: 600,
            agent_event_stream_metrics: AgentEventStreamMetrics::default(),
            mail_data_plane_id: None,
            mail_engine: None,
            mail_engine_required: false,
            mail_content: None,
            mail_content_required: false,
            mail_drafts: None,
            mail_draft_content: None,
            mail_drafts_required: false,
            mail_event_ingress: None,
            mail_event_ingress_required: false,
            audit_archive: None,
        },
        None,
    )
}

fn router_with_state(state: AppState, scim_identity_provider: Option<String>) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    let mut router = Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(readiness))
        .route("/api/v2/system/info", get(system_info))
        .route(
            "/api/v2/auth/{client}/login",
            get(begin_human_login),
        )
        .route(
            "/api/v2/auth/{client}/callback",
            get(complete_human_login),
        )
        .route(
            "/api/v2/auth/{client}/session",
            get(get_human_session),
        )
        .route(
            "/api/v2/auth/{client}/logout",
            axum::routing::post(logout_human_session),
        )
        .route("/oauth2/token", axum::routing::post(issue_agent_token))
        .route("/oauth2/jwks.json", get(agent_jwks))
        .route(
            "/internal/v2/mail-engine/stalwart/events",
            axum::routing::post(ingest_stalwart_mail_events),
        )
        .route(
            "/api/v2/organizations/{organization_id}",
            get(get_organization),
        )
        .route(
            "/api/v2/organizations/{organization_id}/audit-events",
            get(search_audit_events),
        )
        .route(
            "/api/v2/organizations/{organization_id}/compliance-cases",
            get(list_compliance_cases).post(create_compliance_case),
        )
        .route(
            "/api/v2/organizations/{organization_id}/legal-holds",
            get(list_legal_holds),
        )
        .route(
            "/api/v2/organizations/{organization_id}/approval-requests",
            get(list_approval_requests),
        )
        .route(
            "/api/v2/organizations/{organization_id}/roles",
            get(list_roles),
        )
        .route(
            "/api/v2/organizations/{organization_id}/role-assignments",
            get(list_role_assignments).post(grant_role_assignment),
        )
        .route(
            "/api/v2/compliance-cases/{case_id}",
            get(get_compliance_case),
        )
        .route(
            "/api/v2/compliance-cases/{case_id}/closure",
            axum::routing::post(close_compliance_case),
        )
        .route(
            "/api/v2/audit-archives/{archive_id}/legal-holds",
            axum::routing::post(apply_audit_legal_hold),
        )
        .route(
            "/api/v2/audit-archives/{archive_id}/exports",
            axum::routing::post(authorize_audit_export),
        )
        .route(
            "/api/v2/audit-export-grants/{grant_id}",
            get(get_audit_export_grant),
        )
        .route(
            "/api/v2/audit-export-grants/{grant_id}/artifacts/{artifact}",
            axum::routing::post(download_audit_export_artifact),
        )
        .route(
            "/api/v2/organizations/{organization_id}/retention-policies",
            axum::routing::post(create_retention_policy),
        )
        .route(
            "/api/v2/retention-policies/{policy_id}",
            get(get_retention_policy),
        )
        .route(
            "/api/v2/retention-policies/{policy_id}/versions",
            axum::routing::post(append_retention_policy_version),
        )
        .route(
            "/api/v2/retention-policies/{policy_id}/versions/{version_id}/activation",
            axum::routing::post(activate_retention_policy_version),
        )
        .route(
            "/api/v2/agents/{agent_id}/mail-policy-versions",
            axum::routing::post(create_agent_mail_policy_version),
        )
        .route(
            "/api/v2/agents/{agent_id}/mail-policy-versions/{version_id}",
            get(get_agent_mail_policy_version),
        )
        .route(
            "/api/v2/agents/{agent_id}/mail-policy-versions/{version_id}/activation",
            axum::routing::post(activate_agent_mail_policy_version),
        )
        .route("/api/v2/legal-holds/{hold_id}", get(get_legal_hold))
        .route(
            "/api/v2/legal-holds/{hold_id}/release",
            axum::routing::post(release_audit_legal_hold),
        )
        .route(
            "/api/v2/organizations/{organization_id}/mailboxes",
            axum::routing::post(create_mailbox),
        )
        .route(
            "/api/v2/organizations/{organization_id}/mail-domains",
            get(list_mail_domains).post(create_mail_domain),
        )
        .route(
            "/api/v2/organizations/{organization_id}/webhook-subscriptions",
            get(list_webhook_subscriptions).post(create_webhook_subscription),
        )
        .route(
            "/api/v2/organizations/{organization_id}/webhook-dead-letters",
            get(list_webhook_dead_letters),
        )
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}",
            get(get_webhook_subscription).put(update_webhook_subscription),
        )
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}/events",
            get(pull_agent_events),
        )
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}/stream",
            get(stream_agent_events),
        )
        .route(
            "/api/v2/agent/mailboxes/{mailbox_id}/messages/{message_id}/content",
            get(read_agent_mail_content),
        )
        .route(
            "/api/v2/agent/mailboxes/{mailbox_id}/processing-subscription",
            axum::routing::put(ensure_agent_processing_subscription),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/claims",
            axum::routing::post(claim_agent_message),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}",
            axum::routing::patch(change_agent_processing_subscription),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/dead-letters",
            get(list_agent_processing_dead_letters),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/lease-extension",
            axum::routing::post(extend_agent_message_lease),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/acknowledgement",
            axum::routing::post(acknowledge_agent_message),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/nack",
            axum::routing::post(nack_agent_message),
        )
        .route("/api/v2/agent/mail-policy", get(get_own_agent_mail_policy))
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}/dead-letters/{event_id}/replay",
            axum::routing::post(replay_webhook_dead_letter),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}",
            get(get_mail_domain),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/dns-verifications",
            axum::routing::post(verify_mail_domain_dns),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/ownership-challenge",
            axum::routing::post(rotate_mail_domain_ownership_challenge),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/activation",
            axum::routing::post(activate_mail_domain),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/suspension",
            axum::routing::post(suspend_mail_domain),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/route",
            axum::routing::put(update_mail_domain_route),
        )
        .route("/api/v2/mailboxes", get(list_human_mailboxes))
        .route("/api/v2/mailboxes/{mailbox_id}", get(get_mailbox))
        .route(
            "/api/v2/mailboxes/{mailbox_id}/messages",
            get(list_inbox_messages),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/messages/search",
            axum::routing::post(search_inbox_messages),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/messages/{message_id}",
            get(get_inbox_message).patch(update_inbox_message_state),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/drafts",
            get(list_human_drafts).post(save_text_draft),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/drafts/{draft_id}/restore",
            axum::routing::post(restore_human_draft),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/submissions",
            axum::routing::post(request_mail_submission),
        )
        .route(
            "/api/v2/agent/mailboxes/{mailbox_id}/sandbox-submissions",
            axum::routing::post(capture_agent_mail_sandbox_submission),
        )
        .route(
            "/api/v2/mail-submissions/{submission_id}",
            get(get_mail_submission),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding",
            axum::routing::put(update_mail_engine_mailbox_binding),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/delegations",
            axum::routing::post(create_mailbox_delegation),
        )
        .route(
            "/api/v2/mailbox-delegations/{delegation_id}",
            axum::routing::delete(revoke_mailbox_delegation),
        )
        .route(
            "/api/v2/approval-requests",
            axum::routing::post(create_approval_request),
        )
        .route(
            "/api/v2/approval-requests/{approval_request_id}/decisions",
            axum::routing::post(decide_approval_request),
        )
        .route(
            "/api/v2/approval-requests/{approval_request_id}",
            get(get_approval_request),
        )
        .route(
            "/api/v2/approval-requests/{approval_request_id}/cancellation",
            axum::routing::post(cancel_approval_request),
        )
        .route(
            "/api/v2/role-assignments/{role_assignment_id}/revocation",
            axum::routing::post(revoke_role_assignment),
        )
        .fallback(not_found);
    if let Some(identity_provider) = scim_identity_provider {
        router = router.nest_service(
            "/scim/v2",
            copilotmail_scim::router(state.store.clone(), identity_provider),
        );
    }
    let state = Arc::new(state);
    router
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing");
                let span = tracing::info_span!(
                    "http.request",
                    http.request.method = %request.method(),
                    url.path = %request.uri().path(),
                    request.id = request_id,
                );
                let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
                    propagator.extract(&RequestHeaderExtractor(request.headers()))
                });
                // A malformed or unsupported remote context is deliberately
                // ignored; the request keeps its fresh local root span.
                let _ = span.set_parent(parent);
                span
            }),
        )
        .layer(middleware::from_fn(human_auth_response_headers))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            human_session_csrf_guard,
        ))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .with_state(state)
}

struct RequestHeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for RequestHeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(HeaderName::as_str).collect()
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        service: SERVICE.into(),
        version: BUILD_VERSION.into(),
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanLoginQuery {
    return_to: Option<String>,
}

async fn begin_human_login(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Query(query): Query<HumanLoginQuery>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state.human_session.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser sessions are not configured",
            true,
            &request_id,
        )
    })?;
    let client = runtime.client(&client_key).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "human_session_client_not_found",
            "the Human browser client is not configured",
            false,
            &request_id,
        )
    })?;
    let authorization = client
        .relying_party
        .begin_authorization()
        .map_err(|error| oidc_flow_api_error(error, &request_id))?;
    let (authorization_url, transaction) = authorization.into_parts();
    let (state_value, verifier, issued_at, expires_at) = transaction.into_parts();
    let state_sha256: [u8; 32] = Sha256::digest(state_value.as_bytes()).into();
    let encrypted_pkce_verifier = runtime
        .protector
        .protect(verifier.expose_secret(), &state_sha256)
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "the Human login transaction could not be protected",
                true,
                &request_id,
            )
        })?;
    let browser_nonce = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let now = Utc::now();
    let issued_at = datetime_from_unix_seconds(issued_at).ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login transaction has an invalid timestamp",
            true,
            &request_id,
        )
    })?;
    let expires_at = datetime_from_unix_seconds(expires_at).ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login transaction has an invalid expiry",
            true,
            &request_id,
        )
    })?;
    if issued_at > now + chrono::Duration::seconds(60) {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login transaction clock is invalid",
            true,
            &request_id,
        ));
    }
    HumanSessionService::new(state.store.clone())
        .begin_login(BeginHumanLoginCommand {
            client_key: client.client_key,
            return_to: query.return_to.unwrap_or_else(|| "/".into()),
            state: state_value,
            browser_nonce: browser_nonce.clone(),
            encrypted_pkce_verifier,
            now,
            expires_at,
            admission_limit: runtime.login_requests_per_minute,
            cleanup_limit: HUMAN_LOGIN_CLEANUP_LIMIT,
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;

    let lifetime = (expires_at - now).num_seconds().clamp(1, 600);
    let cookie = browser_cookie_header(
        runtime.login_cookie_name(client.client_key),
        &browser_nonce,
        lifetime,
        runtime.secure_cookie,
    )
    .map_err(|_| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login cookie could not be created",
            true,
            &request_id,
        )
    })?;
    let mut response = Redirect::temporary(authorization_url.as_str()).into_response();
    response.headers_mut().append(header::SET_COOKIE, cookie);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

async fn complete_human_login(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Query(response): Query<OidcAuthorizationResponse>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state.human_session.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser sessions are not configured",
            true,
            &request_id,
        )
    })?;
    let client = runtime.client(&client_key).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "human_session_client_not_found",
            "the Human browser client is not configured",
            false,
            &request_id,
        )
    })?;
    let state_value = response.state().ok_or_else(|| {
        api_error(
            StatusCode::UNAUTHORIZED,
            "invalid_human_login_callback",
            "the Human login callback is invalid or expired",
            false,
            &request_id,
        )
    })?;
    let browser_nonce =
        browser_cookie_value(&headers, runtime.login_cookie_name(client.client_key))
            .map_err(|_| invalid_human_session_cookie(&request_id))?
            .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let now = Utc::now();
    let consumed = HumanSessionService::new(state.store.clone())
        .consume_login(ConsumeHumanLoginCommand {
            state: state_value.into(),
            browser_nonce,
            now,
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if consumed.client_key != client.client_key {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "invalid_human_login_callback",
            "the Human login callback is invalid or expired",
            false,
            &request_id,
        ));
    }
    let state_sha256: [u8; 32] = Sha256::digest(state_value.as_bytes()).into();
    let verifier = runtime
        .protector
        .unprotect(&consumed.encrypted_pkce_verifier, &state_sha256)
        .map_err(|_| invalid_human_session_cookie(&request_id))?;
    let issued_at = unix_seconds_from_datetime(consumed.created_at)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let expires_at = unix_seconds_from_datetime(consumed.expires_at)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let transaction =
        OidcAuthorizationTransaction::from_persisted(state_value, verifier, issued_at, expires_at)
            .map_err(|error| oidc_flow_api_error(error, &request_id))?;
    let token = client
        .relying_party
        .complete_authorization(transaction, response)
        .await
        .map_err(|error| oidc_flow_api_error(error, &request_id))?;
    let upstream_expires_at =
        datetime_from_unix_seconds(token.credential_expires_at_unix_seconds())
            .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let identity = token.identity().clone();
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(invalid_human_session_cookie(&request_id));
    }
    let authorization = state
        .store
        .resolve_authorization(identity.principal_id, now)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "authorization could not be evaluated",
                true,
                &request_id,
            )
        })?
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let configured_expiry = now
        + chrono::Duration::from_std(runtime.maximum_lifetime).map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "the Human session lifetime is invalid",
                true,
                &request_id,
            )
        })?;
    let expires_at = upstream_expires_at.min(configured_expiry);
    let session_token = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let record = HumanSessionService::new(state.store.clone())
        .create_session(CreateHumanSessionCommand {
            organization_id: authorization.organization_id,
            principal_id: identity.principal_id,
            login_transaction_id: consumed.login_transaction_id,
            client_key: consumed.client_key,
            session_token: session_token.clone(),
            oidc_access_token: token.access_token().expose_secret().to_owned(),
            oidc_jti: identity.credential_id,
            upstream_expires_at,
            expires_at,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    let lifetime = (record.expires_at - now).num_seconds().clamp(1, 28_800);
    let [session_cookie, clear_other_session] = browser_session_cookie_headers(
        client.client_key,
        &session_token,
        lifetime,
        runtime.secure_cookie,
    )
    .map_err(|_| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human session cookie could not be created",
            true,
            &request_id,
        )
    })?;
    let clear_login = clear_browser_cookie_header(
        runtime.login_cookie_name(client.client_key),
        runtime.secure_cookie,
    )
    .map_err(|_| invalid_human_session_cookie(&request_id))?;
    let target = format!("{}{}", client.public_origin, consumed.return_to);
    let mut response = Redirect::to(&target).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, session_cookie);
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_login);
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_other_session);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

async fn get_human_session(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Json<HumanSessionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state
        .human_session
        .as_ref()
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let expected_client = runtime
        .client(&client_key)
        .map(|client| client.client_key)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let (cookie_client, token) = select_human_browser_session(&headers, runtime.secure_cookie)
        .map_err(|error| credential_selection_api_error(error, &request_id))?;
    if cookie_client != expected_client {
        return Err(human_client_route_denied(&request_id));
    }
    let record = HumanSessionService::new(state.store.clone())
        .resolve_session(ResolveHumanSessionCommand {
            session_token: token,
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if record.client_key != cookie_client {
        return Err(invalid_human_session_cookie(&request_id));
    }
    state
        .store
        .resolve_authorization(record.principal_id, Utc::now())
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "authorization could not be evaluated",
                true,
                &request_id,
            )
        })?
        .filter(|context| context.organization_id == record.organization_id)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    Ok(Json(HumanSessionResponse {
        principal_id: record.principal_id.to_string(),
        organization_id: record.organization_id.to_string(),
        client: record.client_key.as_str().into(),
        expires_at: record.expires_at.to_rfc3339(),
    }))
}

async fn logout_human_session(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state
        .human_session
        .as_ref()
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let expected_client = runtime
        .client(&client_key)
        .map(|client| client.client_key)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let (cookie_client, token) = select_human_browser_session(&headers, runtime.secure_cookie)
        .map_err(|error| credential_selection_api_error(error, &request_id))?;
    if cookie_client != expected_client {
        return Err(human_client_route_denied(&request_id));
    }
    let existing = HumanSessionService::new(state.store.clone())
        .resolve_session(ResolveHumanSessionCommand {
            session_token: token.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if existing.client_key != cookie_client {
        return Err(invalid_human_session_cookie(&request_id));
    }
    let revoked = HumanSessionService::new(state.store.clone())
        .revoke_session(RevokeHumanSessionCommand {
            session_token: token,
            cause: HumanSessionRevocationCause::Logout,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if revoked.session.client_key != cookie_client {
        return Err(invalid_human_session_cookie(&request_id));
    }
    let clear_cookie = clear_browser_cookie_header(
        runtime.session_cookie_name(cookie_client),
        runtime.secure_cookie,
    )
    .map_err(|_| invalid_human_session_cookie(&request_id))?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_cookie);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

async fn readiness(State(state): State<Arc<AppState>>) -> (StatusCode, Json<ReadinessResponse>) {
    let mut dependencies = BTreeMap::new();
    let database = match state.store.schema_ready().await {
        Ok(true) => "ready",
        Ok(false) => "migration_required",
        Err(error) => {
            tracing::warn!(%error, "PostgreSQL readiness check failed");
            "unavailable"
        }
    };
    dependencies.insert("postgresql".into(), database.into());
    dependencies.insert(
        "human_authentication".into(),
        if state.authentication_ready {
            "ready"
        } else if state.human_auth_required {
            "required_but_unavailable"
        } else {
            "not_configured"
        }
        .into(),
    );
    dependencies.insert(
        "human_browser_session".into(),
        if state.human_session.is_some() {
            "ready"
        } else if state.human_session_required {
            "required_but_unavailable"
        } else {
            "not_configured"
        }
        .into(),
    );
    dependencies.insert(
        "agent_authentication".into(),
        if state.agent_authentication_ready {
            "ready"
        } else if state.agent_auth_required {
            "required_but_unavailable"
        } else {
            "not_configured"
        }
        .into(),
    );
    let mail_engine_ready = if let Some(engine) = &state.mail_engine {
        match engine.probe().await {
            Ok(_) => {
                dependencies.insert("mail_data_plane".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail data-plane readiness check failed");
                dependencies.insert("mail_data_plane".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_data_plane".into(),
            if state.mail_engine_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let mail_event_ingress_ready = if let Some(settings) = &state.mail_event_ingress {
        match load_mail_event_ingress_secrets(settings).await {
            Ok(_) => {
                dependencies.insert("mail_event_ingress".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail event ingress secret readiness check failed");
                dependencies.insert("mail_event_ingress".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_event_ingress".into(),
            if state.mail_event_ingress_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let mail_content_ready = if let Some(reader) = &state.mail_content {
        match reader.probe_content().await {
            Ok(()) => {
                dependencies.insert("mail_content".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail-content readiness check failed");
                dependencies.insert("mail_content".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_content".into(),
            if state.mail_content_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let mail_drafts_ready = if let Some(drafts) = &state.mail_drafts {
        match drafts.probe_drafts().await {
            Ok(()) => {
                dependencies.insert("mail_compose".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail-compose readiness check failed");
                dependencies.insert("mail_compose".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_compose".into(),
            if state.mail_drafts_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let ready = database == "ready"
        && (!state.human_auth_required || state.authentication_ready)
        && (!state.human_session_required || state.human_session.is_some())
        && (!state.agent_auth_required || state.agent_authentication_ready)
        && (!state.mail_engine_required || mail_engine_ready)
        && (!state.mail_content_required || mail_content_ready)
        && (!state.mail_drafts_required || mail_drafts_ready)
        && (!state.mail_event_ingress_required || mail_event_ingress_ready);
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(ReadinessResponse {
            status: if ready { "ready" } else { "not_ready" }.into(),
            dependencies,
        }),
    )
}

async fn system_info() -> Json<SystemInfoResponse> {
    Json(SystemInfoResponse {
        api_version: API_VERSION.into(),
        build_version: BUILD_VERSION.into(),
        principal_model: "human_agent_service".into(),
    })
}

#[derive(Debug, Deserialize)]
struct AgentTokenForm {
    grant_type: String,
    client_id: String,
    client_assertion_type: Option<String>,
    client_assertion: Option<String>,
    scope: Option<String>,
}

struct ConnectionProof(Option<TlsConnectionInfo>);

impl<S> FromRequestParts<S> for ConnectionProof
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<TlsConnectionInfo>>()
                .map(|axum::extract::ConnectInfo(info)| info.clone()),
        ))
    }
}

async fn agent_jwks(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(service) = &state.agent_token_service else {
        return oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "Agent OAuth is not configured",
        );
    };
    let jwks = service.jwks();
    let bytes = serde_json::to_vec(&jwks).expect("JWKS serializes");
    let etag = format!("\"{}\"", hex_digest(&bytes));
    let mut response = if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        == Some(etag.as_str())
    {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Json(jwks).into_response()
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "public, max-age=60, must-revalidate".parse().unwrap(),
    );
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().unwrap());
    response
}

async fn issue_agent_token(
    State(state): State<Arc<AppState>>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    form: Result<Form<AgentTokenForm>, FormRejection>,
) -> Response {
    let Ok(Form(form)) = form else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the token request form is invalid",
        );
    };
    let Some(service) = &state.agent_token_service else {
        return oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "Agent OAuth is not configured",
        );
    };
    let certificate_fingerprint = connection
        .as_ref()
        .and_then(|info| info.client_certificate_fingerprint.as_deref());
    let source = connection
        .as_ref()
        .map(|info| info.remote_addr.ip().to_string())
        .or_else(|| forwarded_source(&headers));
    match service
        .exchange(
            TokenRequest {
                grant_type: form.grant_type,
                client_id: form.client_id,
                client_assertion_type: form.client_assertion_type,
                client_assertion: form.client_assertion,
                scope: form.scope,
            },
            certificate_fingerprint,
            source.as_deref(),
            Utc::now(),
        )
        .await
    {
        Ok(token) => oauth_json(
            StatusCode::OK,
            serde_json::to_value(token).expect("token response serializes"),
        ),
        Err(TokenError::InvalidRequest) => oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the token request is invalid",
        ),
        Err(TokenError::InvalidClient) => oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        ),
        Err(TokenError::InvalidScope) => oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "the requested scope is not permitted",
        ),
        Err(TokenError::TemporarilyUnavailable) => oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "the authorization server is temporarily unavailable",
        ),
    }
}

fn hex_digest(value: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn encode_bytes_hex(value: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    let mut response = oauth_json(
        status,
        serde_json::json!({"error": error, "error_description": description}),
    );
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            "Bearer error=\"invalid_client\"".parse().unwrap(),
        );
    }
    response
}

fn oauth_json(status: StatusCode, value: serde_json::Value) -> Response {
    let mut response = (status, Json(value)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::PRAGMA, "no-cache".parse().unwrap());
    response
}

fn forwarded_source(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebhookSubscriptionListQuery {
    principal_id: Option<String>,
    after: Option<String>,
    limit: Option<u16>,
}

async fn create_webhook_subscription(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<WebhookSubscriptionCreateRequest>,
) -> Result<(StatusCode, Json<WebhookSubscriptionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to manage webhook subscriptions",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let principal_id = body.principal_id.parse::<PrincipalId>().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_principal_id",
            "principal_id must be a valid UUID",
            false,
            &request_id,
        )
    })?;
    if principal_id != authorization.principal_id {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::IdentityManage,
            "identity.manage permission is required to manage another Agent's subscription",
            &request_id,
        )?;
    }
    let result = WebhookSubscriptionService::new(state.store.clone())
        .create(
            &authorization,
            CreateWebhookSubscriptionCommand {
                organization_id,
                principal_id,
                endpoint_url: body.endpoint_url,
                event_types: body.event_types,
                signing_key_reference: body.signing_key_reference,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| webhook_subscription_api_error(error, &request_id))?;
    let subscription = state
        .store
        .get_webhook_subscription(organization_id, result.subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| webhook_subscription_query_unavailable(&request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(webhook_subscription_response(subscription)),
    ))
}

async fn list_webhook_subscriptions(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<WebhookSubscriptionListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<WebhookSubscriptionListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to list webhook subscriptions",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let requested_principal = query
        .principal_id
        .map(|value| value.parse::<PrincipalId>())
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_principal_id",
                "principal_id must be a valid UUID",
                false,
                &request_id,
            )
        })?;
    let can_manage_all = authorization.permits(organization_id, Permission::IdentityManage)
        && identity
            .scope_restriction
            .as_ref()
            .is_none_or(|scopes| scopes.contains(&Permission::IdentityManage));
    let principal_id = if can_manage_all {
        requested_principal
    } else {
        if requested_principal.is_some_and(|value| value != authorization.principal_id) {
            return Err(api_error(
                StatusCode::FORBIDDEN,
                "permission_denied",
                "identity.manage permission is required to list another Agent's subscriptions",
                false,
                &request_id,
            ));
        }
        Some(authorization.principal_id)
    };
    let after = query
        .after
        .map(|value| value.parse::<WebhookSubscriptionId>())
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_page_cursor",
                "after must be a valid webhook subscription ID",
                false,
                &request_id,
            )
        })?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_page_size",
            "limit must be between 1 and 100",
            false,
            &request_id,
        ));
    }
    let mut subscriptions = state
        .store
        .list_webhook_subscriptions(organization_id, principal_id, after, i64::from(limit) + 1)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?;
    let has_more = subscriptions.len() > usize::from(limit);
    if has_more {
        subscriptions.pop();
    }
    let next_after = has_more.then(|| {
        subscriptions
            .last()
            .expect("a page with more results is non-empty")
            .subscription_id
            .to_string()
    });
    Ok(Json(WebhookSubscriptionListResponse {
        items: subscriptions
            .into_iter()
            .map(webhook_subscription_response)
            .collect(),
        next_after,
    }))
}

async fn get_webhook_subscription(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<WebhookSubscriptionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to read webhook subscriptions",
        &request_id,
    )?;
    let subscription = state
        .store
        .get_webhook_subscription(authorization.organization_id, subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    if subscription.principal_id != authorization.principal_id {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::IdentityManage,
            "identity.manage permission is required to read another Agent's subscription",
            &request_id,
        )?;
    }
    Ok(Json(webhook_subscription_response(subscription)))
}

async fn update_webhook_subscription(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<WebhookSubscriptionUpdateRequest>,
) -> Result<Json<WebhookSubscriptionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to update webhook subscriptions",
        &request_id,
    )?;
    let existing = state
        .store
        .get_webhook_subscription(authorization.organization_id, subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    if existing.principal_id != authorization.principal_id {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::IdentityManage,
            "identity.manage permission is required to update another Agent's subscription",
            &request_id,
        )?;
    }
    let result = WebhookSubscriptionService::new(state.store.clone())
        .update(
            &authorization,
            UpdateWebhookSubscriptionCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                expected_version: body.expected_version,
                endpoint_url: body.endpoint_url,
                event_types: body.event_types,
                signing_key_reference: body.signing_key_reference,
                enabled: body.enabled,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| webhook_subscription_api_error(error, &request_id))?;
    let subscription = state
        .store
        .get_webhook_subscription(authorization.organization_id, result.subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| webhook_subscription_query_unavailable(&request_id))?;
    Ok(Json(webhook_subscription_response(subscription)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentEventCursorQuery {
    cursor: Option<String>,
    limit: Option<u16>,
}

async fn enforce_agent_ingress_limit(
    state: &AppState,
    identity: &AuthenticatedIdentity,
    authorization: &AuthorizationContext,
    request_id: &str,
) -> Result<(), Response> {
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Ok(());
    }
    let decision = AgentIngressRateLimiter::new(state.store.clone())
        .admit(
            authorization.organization_id,
            authorization.principal_id,
            state.agent_ingress_tenant_requests_per_minute,
            state.agent_ingress_principal_requests_per_minute,
        )
        .await;
    match decision {
        Ok(AgentIngressRateLimitDecision::Allowed) => {
            state
                .agent_event_stream_metrics
                .ingress_allowed_total
                .fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        Ok(AgentIngressRateLimitDecision::TenantLimited {
            retry_after_seconds,
        }) => {
            state
                .agent_event_stream_metrics
                .ingress_tenant_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            Err(agent_ingress_limited_response(
                "agent_ingress_tenant_rate_limited",
                "tenant Agent API ingress rate is exhausted",
                retry_after_seconds,
                request_id,
            ))
        }
        Ok(AgentIngressRateLimitDecision::PrincipalLimited {
            retry_after_seconds,
        }) => {
            state
                .agent_event_stream_metrics
                .ingress_principal_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            Err(agent_ingress_limited_response(
                "agent_ingress_principal_rate_limited",
                "Agent API ingress rate is exhausted",
                retry_after_seconds,
                request_id,
            ))
        }
        Err(AgentIngressRateLimitError::InvalidLimits | AgentIngressRateLimitError::Store(_)) => {
            state
                .agent_event_stream_metrics
                .ingress_failures_total
                .fetch_add(1, Ordering::Relaxed);
            Err(api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "agent_ingress_rate_limit_unavailable",
                "Agent API ingress admission is unavailable",
                true,
                request_id,
            ))
        }
    }
}

fn agent_ingress_limited_response(
    code: &'static str,
    message: &'static str,
    retry_after_seconds: u32,
    request_id: &str,
) -> Response {
    let mut response = api_error(
        StatusCode::TOO_MANY_REQUESTS,
        code,
        message,
        true,
        request_id,
    );
    response.headers_mut().insert(
        header::RETRY_AFTER,
        header::HeaderValue::from_str(&retry_after_seconds.clamp(1, 60).to_string())
            .expect("bounded Retry-After is a valid header value"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
    response
}

async fn pull_agent_events(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Query(query): Query<AgentEventCursorQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentEventCursorPageResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to pull subscription events",
        &request_id,
    )?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let after_event_id = query
        .cursor
        .as_deref()
        .map(|cursor| parse_agent_event_cursor(cursor, subscription_id))
        .transpose()
        .map_err(|error| agent_event_cursor_parse_error(error, &request_id))?;
    let page = AgentEventCursorService::new(state.store.clone())
        .pull(
            &authorization,
            PullAgentEventsCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                after_event_id,
                limit: i64::from(query.limit.unwrap_or(50)),
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_event_cursor_api_error(error, &request_id))?;
    Ok(Json(AgentEventCursorPageResponse {
        items: page
            .items
            .into_iter()
            .map(agent_event_envelope_response)
            .collect(),
        next_cursor: page
            .next_event_id
            .map(|event_id| format_agent_event_cursor(subscription_id, event_id)),
        has_more: page.has_more,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentEventCursorParseError {
    Invalid,
    ScopeMismatch,
    ConflictingResumeSources,
}

fn parse_agent_event_cursor(
    value: &str,
    expected_subscription_id: WebhookSubscriptionId,
) -> Result<Uuid, AgentEventCursorParseError> {
    if value.len() > 80 {
        return Err(AgentEventCursorParseError::Invalid);
    }
    let mut parts = value.split('.');
    let version = parts.next();
    let subscription = parts
        .next()
        .and_then(|value| value.parse::<WebhookSubscriptionId>().ok());
    let event = parts.next().and_then(|value| Uuid::parse_str(value).ok());
    if version != Some("v1") || parts.next().is_some() || subscription.is_none() || event.is_none()
    {
        return Err(AgentEventCursorParseError::Invalid);
    }
    if subscription != Some(expected_subscription_id) {
        return Err(AgentEventCursorParseError::ScopeMismatch);
    }
    Ok(event.expect("validated event cursor UUID"))
}

fn agent_event_cursor_parse_error(error: AgentEventCursorParseError, request_id: &str) -> Response {
    match error {
        AgentEventCursorParseError::Invalid => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_event_cursor",
            "cursor must be a valid versioned Agent event cursor",
            false,
            request_id,
        ),
        AgentEventCursorParseError::ScopeMismatch => api_error(
            StatusCode::BAD_REQUEST,
            "event_cursor_scope_mismatch",
            "cursor belongs to another webhook subscription",
            false,
            request_id,
        ),
        AgentEventCursorParseError::ConflictingResumeSources => api_error(
            StatusCode::BAD_REQUEST,
            "conflicting_event_cursor",
            "cursor and Last-Event-ID must match when both are provided",
            false,
            request_id,
        ),
    }
}

fn format_agent_event_cursor(subscription_id: WebhookSubscriptionId, event_id: Uuid) -> String {
    format!("v1.{subscription_id}.{event_id}")
}

const AGENT_EVENT_STREAM_BATCH_SIZE: i64 = 50;
const AGENT_EVENT_STREAM_POLL_INTERVAL: Duration = Duration::from_secs(2);
const AGENT_EVENT_STREAM_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const AGENT_EVENT_STREAM_MAX_LIFETIME: Duration = Duration::from_secs(300);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentEventStreamQuery {
    cursor: Option<String>,
}

struct AgentEventStreamState {
    app: Arc<AppState>,
    authorization: AuthorizationContext,
    subscription_id: WebhookSubscriptionId,
    cursor: Option<Uuid>,
    pending: VecDeque<AgentCursorEvent>,
    has_more: bool,
    request_id: String,
    expires_at: Instant,
    terminal: bool,
    _lease: AgentEventStreamLeaseGuard,
}

struct AgentEventStreamLeaseGuard {
    store: PgControlPlaneStore,
    lease: Option<AgentEventStreamLease>,
    metrics: AgentEventStreamMetrics,
}

impl Drop for AgentEventStreamLeaseGuard {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        self.metrics.closed();
        let store = self.store.clone();
        let metrics = self.metrics.clone();
        let lease_id = lease.lease_id;
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(async move {
            if let Err(error) = store.release_agent_event_stream_lease(&lease).await {
                metrics
                    .cleanup_failures_total
                    .fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    lease_id = %lease_id,
                    error = %error,
                    "failed to release Agent event stream lease; expiry will recover capacity"
                );
            }
        });
    }
}

async fn stream_agent_events(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Query(query): Query<AgentEventStreamQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to stream subscription events",
        &request_id,
    )?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let mut last_event_ids = headers.get_all("last-event-id").iter();
    let last_event_id = last_event_ids.next();
    if last_event_ids.next().is_some() {
        return Err(agent_event_cursor_parse_error(
            AgentEventCursorParseError::ConflictingResumeSources,
            &request_id,
        ));
    }
    let after_event_id =
        resolve_agent_stream_cursor(query.cursor.as_deref(), last_event_id, subscription_id)
            .map_err(|error| agent_event_cursor_parse_error(error, &request_id))?;

    // Validate the resume anchor and obtain the first bounded page before
    // sending HTTP 200. Authentication, ownership, revocation and cursor
    // failures therefore remain ordinary machine-readable API errors.
    let service = AgentEventCursorService::new(state.store.clone());
    let first_page = service
        .pull(
            &authorization,
            PullAgentEventsCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                after_event_id,
                limit: AGENT_EVENT_STREAM_BATCH_SIZE,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_event_cursor_api_error(error, &request_id))?;
    let lease_result = service
        .open_stream(
            &authorization,
            OpenAgentEventStreamCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
                lease_id: Uuid::now_v7(),
                tenant_connection_limit: state.agent_event_stream_tenant_limit,
                principal_connection_limit: state.agent_event_stream_principal_limit,
            },
        )
        .await;
    let lease = match lease_result {
        Ok(lease) => lease,
        Err(AgentEventCursorError::TenantCapacityReached) => {
            state
                .agent_event_stream_metrics
                .tenant_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            return Err(agent_event_cursor_api_error(
                AgentEventCursorError::TenantCapacityReached,
                &request_id,
            ));
        }
        Err(AgentEventCursorError::PrincipalCapacityReached) => {
            state
                .agent_event_stream_metrics
                .principal_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            return Err(agent_event_cursor_api_error(
                AgentEventCursorError::PrincipalCapacityReached,
                &request_id,
            ));
        }
        Err(error) => return Err(agent_event_cursor_api_error(error, &request_id)),
    };
    state.agent_event_stream_metrics.opened();

    let stream = agent_event_stream(AgentEventStreamState {
        app: state.clone(),
        authorization,
        subscription_id,
        cursor: first_page.next_event_id,
        pending: first_page.items.into(),
        has_more: first_page.has_more,
        request_id,
        expires_at: Instant::now() + AGENT_EVENT_STREAM_MAX_LIFETIME,
        terminal: false,
        _lease: AgentEventStreamLeaseGuard {
            store: state.store.clone(),
            lease: Some(lease),
            metrics: state.agent_event_stream_metrics.clone(),
        },
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(AGENT_EVENT_STREAM_HEARTBEAT_INTERVAL)
                .text("keepalive"),
        )
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        header::HeaderValue::from_static("no"),
    );
    Ok(response)
}

fn resolve_agent_stream_cursor(
    query_cursor: Option<&str>,
    last_event_id: Option<&header::HeaderValue>,
    subscription_id: WebhookSubscriptionId,
) -> Result<Option<Uuid>, AgentEventCursorParseError> {
    let last_event_id = last_event_id
        .map(|value| {
            value
                .to_str()
                .map_err(|_| AgentEventCursorParseError::Invalid)
        })
        .transpose()?;
    if let (Some(query), Some(last)) = (query_cursor, last_event_id)
        && query != last
    {
        return Err(AgentEventCursorParseError::ConflictingResumeSources);
    }
    query_cursor
        .or(last_event_id)
        .map(|cursor| parse_agent_event_cursor(cursor, subscription_id))
        .transpose()
}

fn agent_event_stream(
    state: AgentEventStreamState,
) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    stream::unfold(state, |mut state| async move {
        if state.terminal {
            return None;
        }
        loop {
            if Instant::now() >= state.expires_at {
                return None;
            }
            if let Some(event) = state.pending.pop_front() {
                state.cursor = Some(event.event_id);
                let event_type = event.event_type.as_str();
                let event_id = format_agent_event_cursor(state.subscription_id, event.event_id);
                let envelope = agent_event_envelope_response(event);
                match serde_json::to_string(&envelope) {
                    Ok(data) => {
                        let item = SseEvent::default()
                            .id(event_id)
                            .event(event_type)
                            .data(data);
                        return Some((Ok(item), state));
                    }
                    Err(_) => {
                        state
                            .app
                            .agent_event_stream_metrics
                            .stream_errors_total
                            .fetch_add(1, Ordering::Relaxed);
                        state.terminal = true;
                        return Some((
                            Ok(agent_event_stream_error("event_serialization_failed")),
                            state,
                        ));
                    }
                }
            }
            if !state.has_more {
                tokio::time::sleep(AGENT_EVENT_STREAM_POLL_INTERVAL).await;
                if Instant::now() >= state.expires_at {
                    return None;
                }
            }
            let result = AgentEventCursorService::new(state.app.store.clone())
                .pull(
                    &state.authorization,
                    PullAgentEventsCommand {
                        organization_id: state.authorization.organization_id,
                        subscription_id: state.subscription_id,
                        after_event_id: state.cursor,
                        limit: AGENT_EVENT_STREAM_BATCH_SIZE,
                        trace_id: state.request_id.clone(),
                        now: Utc::now(),
                    },
                )
                .await;
            match result {
                Ok(page) => {
                    state.cursor = page.next_event_id;
                    state.pending = page.items.into();
                    state.has_more = page.has_more;
                }
                Err(error) => {
                    state
                        .app
                        .agent_event_stream_metrics
                        .stream_errors_total
                        .fetch_add(1, Ordering::Relaxed);
                    state.terminal = true;
                    return Some((
                        Ok(agent_event_stream_error(agent_event_stream_error_code(
                            &error,
                        ))),
                        state,
                    ));
                }
            }
        }
    })
}

fn agent_event_stream_error(code: &str) -> SseEvent {
    SseEvent::default()
        .event("stream.error")
        .data(format!(r#"{{"code":"{code}","retryable":true}}"#))
}

const fn agent_event_stream_error_code(error: &AgentEventCursorError) -> &'static str {
    match error {
        AgentEventCursorError::Forbidden | AgentEventCursorError::NotFound => {
            "stream_authorization_revoked"
        }
        AgentEventCursorError::InvalidCursor => "stream_cursor_unavailable",
        AgentEventCursorError::InvalidPageSize
        | AgentEventCursorError::InvalidQuota
        | AgentEventCursorError::Domain(_) => "stream_protocol_error",
        AgentEventCursorError::TenantCapacityReached
        | AgentEventCursorError::PrincipalCapacityReached => "stream_capacity_reached",
        AgentEventCursorError::Store(_) => "stream_temporarily_unavailable",
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditEventSearchQuery {
    occurred_from: Option<String>,
    occurred_to: Option<String>,
    action: Option<String>,
    actor_principal_id: Option<String>,
    resource_type: Option<String>,
    resource_id: Option<String>,
    outcome: Option<String>,
    request_id: Option<String>,
    cursor: Option<String>,
    limit: Option<u16>,
}

async fn search_audit_events(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<AuditEventSearchQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AuditEventSearchResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuditRead,
        "audit.read permission is required to search audit events",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }

    let now = Utc::now();
    let occurred_from = query
        .occurred_from
        .as_deref()
        .map(parse_audit_query_time)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?
        .unwrap_or(now - chrono::Duration::hours(24));
    let occurred_to = query
        .occurred_to
        .as_deref()
        .map(parse_audit_query_time)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?
        .unwrap_or(now);
    let actor_principal_id = query
        .actor_principal_id
        .as_deref()
        .map(str::parse::<PrincipalId>)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?;
    let outcome = query
        .outcome
        .as_deref()
        .map(parse_audit_query_outcome)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?;
    let before_chain_sequence = query
        .cursor
        .as_deref()
        .map(|cursor| parse_audit_event_cursor(cursor, organization_id))
        .transpose()
        .map_err(|error| audit_event_cursor_parse_error(error, &request_id))?;

    let page = AuditEventSearchService::new(state.store.clone())
        .search(
            &authorization,
            SearchAuditEventsCommand {
                organization_id,
                occurred_from,
                occurred_to,
                action: query.action,
                actor_principal_id,
                resource_type: query.resource_type,
                resource_id: query.resource_id,
                outcome,
                request_id: query.request_id,
                before_chain_sequence,
                limit: i64::from(query.limit.unwrap_or(50)),
                trace_id: request_id.clone(),
                now,
            },
        )
        .await
        .map_err(|error| audit_event_search_api_error(error, &request_id))?;
    Ok(Json(AuditEventSearchResponse {
        query_id: page.query_id.to_string(),
        occurred_from: occurred_from.to_rfc3339(),
        occurred_to: occurred_to.to_rfc3339(),
        items: page.items.into_iter().map(audit_event_response).collect(),
        next_cursor: page
            .next_chain_sequence
            .map(|sequence| format_audit_event_cursor(organization_id, sequence)),
        has_more: page.has_more,
    }))
}

fn parse_audit_query_time(value: &str) -> Result<DateTime<Utc>, ()> {
    if value.len() > 40 {
        return Err(());
    }
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ())
}

fn parse_audit_query_outcome(value: &str) -> Result<AuditOutcome, ()> {
    match value {
        "success" => Ok(AuditOutcome::Success),
        "failure" => Ok(AuditOutcome::Failure),
        "denied" => Ok(AuditOutcome::Denied),
        _ => Err(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuditEventCursorParseError {
    Invalid,
    ScopeMismatch,
}

fn parse_audit_event_cursor(
    value: &str,
    expected_organization_id: OrganizationId,
) -> Result<i64, AuditEventCursorParseError> {
    if value.len() > 64 {
        return Err(AuditEventCursorParseError::Invalid);
    }
    let mut parts = value.split('.');
    let version = parts.next();
    let organization_id = parts
        .next()
        .and_then(|value| value.parse::<OrganizationId>().ok());
    let chain_sequence = parts.next().and_then(|value| value.parse::<i64>().ok());
    if version != Some("v1")
        || parts.next().is_some()
        || organization_id.is_none()
        || chain_sequence.is_none_or(|sequence| sequence <= 0)
    {
        return Err(AuditEventCursorParseError::Invalid);
    }
    if organization_id != Some(expected_organization_id) {
        return Err(AuditEventCursorParseError::ScopeMismatch);
    }
    Ok(chain_sequence.expect("validated positive audit chain sequence"))
}

fn format_audit_event_cursor(organization_id: OrganizationId, chain_sequence: i64) -> String {
    format!("v1.{organization_id}.{chain_sequence}")
}

fn audit_event_cursor_parse_error(error: AuditEventCursorParseError, request_id: &str) -> Response {
    match error {
        AuditEventCursorParseError::Invalid => invalid_audit_event_query(request_id),
        AuditEventCursorParseError::ScopeMismatch => api_error(
            StatusCode::BAD_REQUEST,
            "audit_cursor_scope_mismatch",
            "the audit cursor belongs to another organization",
            false,
            request_id,
        ),
    }
}

fn invalid_audit_event_query(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_audit_event_query",
        "the audit event filters, time window, cursor, or limit are invalid",
        false,
        request_id,
    )
}

fn audit_event_response(event: AuditEventSummary) -> AuditEventResponse {
    AuditEventResponse {
        event_id: event.event_id.to_string(),
        organization_id: event.organization_id.to_string(),
        actor_principal_id: event.actor_principal_id.to_string(),
        actor_kind: event.actor_kind,
        on_behalf_of_principal_id: event.on_behalf_of_principal_id.map(|id| id.to_string()),
        credential_id: event.credential_id,
        subject_principal_id: event.subject_principal_id.map(|id| id.to_string()),
        action: event.action,
        resource_type: event.resource_type,
        resource_id: event.resource_id,
        outcome: match event.outcome {
            AuditOutcome::Success => "success",
            AuditOutcome::Failure => "failure",
            AuditOutcome::Denied => "denied",
        }
        .into(),
        trace_id: event.trace_id,
        request_id: event.request_id,
        source_ip: event.source_ip,
        client_id: event.client_id,
        session_id: event.session_id,
        token_jti: event.token_jti,
        policy_id: event.policy_id.map(|id| id.to_string()),
        policy_version_id: event.policy_version_id.map(|id| id.to_string()),
        approval_request_id: event.approval_request_id.map(|id| id.to_string()),
        ticket_reference: event.ticket_reference,
        environment: event.environment,
        region: event.region,
        details: event.details,
        occurred_at: event.occurred_at.to_rfc3339(),
        received_at: event.received_at.to_rfc3339(),
        chain_sequence: event.chain_sequence.to_string(),
        hash_version: event.hash_version,
        previous_hash: event.previous_hash_hex,
        event_hash: event.event_hash_hex,
        signature_batch_id: event.signature_batch_id.map(|id| id.to_string()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebhookDeadLetterListQuery {
    after: Option<String>,
    limit: Option<u16>,
}

async fn list_webhook_dead_letters(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<WebhookDeadLetterListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<WebhookDeadLetterListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::SecurityAdmin,
        "security.admin permission is required to investigate webhook dead letters",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let after = query
        .after
        .map(|value| Uuid::parse_str(&value))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_page_cursor",
                "after must be a valid delivery ID",
                false,
                &request_id,
            )
        })?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_page_size",
            "limit must be between 1 and 100",
            false,
            &request_id,
        ));
    }
    let mut dead_letters = state
        .store
        .list_webhook_dead_letters(organization_id, after, i64::from(limit) + 1)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?;
    let has_more = dead_letters.len() > usize::from(limit);
    if has_more {
        dead_letters.pop();
    }
    let next_after = has_more.then(|| {
        dead_letters
            .last()
            .expect("a page with more results is non-empty")
            .delivery_id
            .to_string()
    });
    Ok(Json(WebhookDeadLetterListResponse {
        items: dead_letters
            .into_iter()
            .map(webhook_dead_letter_response)
            .collect(),
        next_after,
    }))
}

async fn replay_webhook_dead_letter(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, event_id)): Path<(WebhookSubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(_body): Json<WebhookDeadLetterReplayRequest>,
) -> Result<Json<WebhookDeadLetterReplayResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::SecurityAdmin,
        "security.admin permission is required to replay webhook dead letters",
        &request_id,
    )?;
    let result = WebhookDeadLetterService::new(state.store.clone())
        .replay(
            &authorization,
            ReplayWebhookDeadLetterCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                event_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| webhook_dead_letter_api_error(error, &request_id))?;
    Ok(Json(WebhookDeadLetterReplayResponse {
        subscription_id: result.subscription_id.to_string(),
        event_id: result.event_id.to_string(),
        attempt: result.attempt,
        status: result.status.into(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MailDomainListQuery {
    after: Option<String>,
    limit: Option<u16>,
}

async fn list_mail_domains(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<MailDomainListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailDomainListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to list mail domains",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_page_size",
            "limit must be between 1 and 100",
            false,
            &request_id,
        ));
    }
    let after = query
        .after
        .map(MailDomainName::parse)
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_page_cursor",
                "after must be a canonicalizable mail domain name",
                false,
                &request_id,
            )
        })?;
    let mut domains = state
        .store
        .list_mail_domains(
            organization_id,
            after.as_ref(),
            i64::from(limit) + 1,
            Utc::now(),
        )
        .await
        .map_err(|_| mail_domain_query_unavailable(&request_id))?;
    let has_more = domains.len() > usize::from(limit);
    if has_more {
        domains.pop();
    }
    let next_after = has_more
        .then(|| domains.last().map(|domain| domain.name.to_string()))
        .flatten();
    Ok(Json(MailDomainListResponse {
        items: domains.into_iter().map(mail_domain_response).collect(),
        next_after,
    }))
}

async fn get_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailDomainResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to read mail domains",
        &request_id,
    )?;
    let domain = state
        .store
        .get_mail_domain(authorization.organization_id, mail_domain_id, Utc::now())
        .await
        .map_err(|_| mail_domain_query_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    Ok(Json(mail_domain_response(domain)))
}

async fn create_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainCreateRequest>,
) -> Result<(StatusCode, Json<MailDomainCreateResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to register mail domains",
        &request_id,
    )?;
    let route_mode = match body.route_mode.as_str() {
        "direct_mx" => MailDomainRouteMode::DirectMx,
        "smart_host" => MailDomainRouteMode::SmartHost,
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "invalid_route_mode",
                "route_mode must be direct_mx or smart_host",
                false,
                &request_id,
            ));
        }
    };
    let ownership_token = format!(
        "cmv1_{}",
        URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
    );
    let ownership_challenge_sha256: [u8; 32] = Sha256::digest(ownership_token.as_bytes()).into();
    let result = MailDomainGovernanceService::new(state.store.clone())
        .register(
            &authorization,
            RegisterMailDomainCommand {
                organization_id,
                name: body.name,
                route_mode,
                ownership_challenge_sha256,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(MailDomainCreateResponse {
            mail_domain_id: result.mail_domain_id.to_string(),
            organization_id: organization_id.to_string(),
            ownership_record_name: format!("_copilotmail-verification.{}", result.name),
            ownership_token,
            name: result.name,
            status: mail_domain_status_wire(result.status).into(),
            ownership_challenge_version: result.ownership_challenge_version,
            route_version: result.route_version,
        }),
    ))
}

async fn rotate_mail_domain_ownership_challenge(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainChallengeRotationRequest>,
) -> Result<Json<MailDomainChallengeRotationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to rotate domain ownership challenges",
        &request_id,
    )?;
    let ownership_token = format!(
        "cmv1_{}",
        URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
    );
    let ownership_challenge_sha256: [u8; 32] = Sha256::digest(ownership_token.as_bytes()).into();
    let result = MailDomainGovernanceService::new(state.store.clone())
        .rotate_ownership_challenge(
            &authorization,
            RotateMailDomainChallengeCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                expected_version: body.expected_version,
                ownership_challenge_sha256,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainChallengeRotationResponse {
        mail_domain_id: result.mail_domain_id.to_string(),
        status: mail_domain_status_wire(result.status).into(),
        ownership_challenge_version: result.ownership_challenge_version,
        ownership_record_name: format!("_copilotmail-verification.{}", result.name),
        ownership_token,
    }))
}

async fn verify_mail_domain_dns(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainDnsVerificationRequest>,
) -> Result<Json<MailDomainDnsVerificationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to verify mail domains",
        &request_id,
    )?;
    let domain = MailDomainName::parse(body.domain).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_domain",
            "domain is not a valid canonicalizable DNS name",
            false,
            &request_id,
        )
    })?;
    let config = DnsVerifierConfig::new(body.dkim_selector, Duration::from_secs(10))
        .map_err(|error| dns_api_error(error, &request_id))?;
    let lookup =
        HickoryDnsLookup::from_system().map_err(|error| dns_api_error(error, &request_id))?;
    let now = Utc::now();
    let report = DnsVerifier::new(lookup, config)
        .verify(&domain, &body.ownership_token, now)
        .await
        .map_err(|error| dns_api_error(error, &request_id))?;
    let activation_ready = report.evidence.activation_ready(now);
    let valid_until = report.evidence.valid_until;
    let failure_codes = report.failure_codes.clone();
    let status = MailDomainGovernanceService::new(state.store.clone())
        .record_dns(
            &authorization,
            RecordMailDomainDnsCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                observed_domain: domain,
                evidence: report.evidence,
                raw_answers: report.raw_answers,
                failure_codes: report.failure_codes,
                trace_id: request_id.clone(),
                now,
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainDnsVerificationResponse {
        mail_domain_id: mail_domain_id.to_string(),
        status: mail_domain_status_wire(status).into(),
        activation_ready,
        valid_until: valid_until.to_rfc3339(),
        failure_codes,
    }))
}

async fn activate_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailDomainActionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to activate mail domains",
        &request_id,
    )?;
    let data_plane_id = state.mail_data_plane_id.ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_data_plane_unavailable",
            "mail data plane is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    MailDomainGovernanceService::new(state.store.clone())
        .activate(
            &authorization,
            ActivateMailDomainCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                data_plane_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainActionResponse {
        mail_domain_id: mail_domain_id.to_string(),
        status: "active".into(),
    }))
}

async fn suspend_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainSuspendRequest>,
) -> Result<Json<MailDomainActionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to suspend mail domains",
        &request_id,
    )?;
    MailDomainGovernanceService::new(state.store.clone())
        .suspend(
            &authorization,
            SuspendMailDomainCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                reason_code: body.reason_code,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainActionResponse {
        mail_domain_id: mail_domain_id.to_string(),
        status: "suspended".into(),
    }))
}

async fn update_mail_domain_route(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainRouteUpdateRequest>,
) -> Result<Json<MailDomainRouteResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to update mail routes",
        &request_id,
    )?;
    let result = MailDomainGovernanceService::new(state.store.clone())
        .update_route(
            &authorization,
            UpdateMailDomainRouteCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                expected_version: body.expected_version,
                inbound_enabled: body.inbound_enabled,
                outbound_enabled: body.outbound_enabled,
                smart_host_ref: body.smart_host_ref,
                dkim_key_ref: body.dkim_key_ref,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainRouteResponse {
        mail_domain_id: result.mail_domain_id.to_string(),
        version: result.version,
        inbound_enabled: result.inbound_enabled,
        outbound_enabled: result.outbound_enabled,
    }))
}

async fn create_mailbox(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailboxCreateRequest>,
) -> Result<(StatusCode, Json<MailboxCreateResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to create mailboxes",
        &request_id,
    )?;
    let Some(kind) = mailbox_kind_from_wire(&body.kind) else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_kind",
            "mailbox kind must be human, agent, shared, or group",
            false,
            &request_id,
        ));
    };
    let result = MailboxManagementService::new(state.store.clone())
        .create_authorized(
            &authorization,
            CreateMailboxCommand {
                organization_id,
                kind,
                primary_address: body.primary_address,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mailbox_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(MailboxCreateResponse {
            mailbox_id: result.mailbox_id.to_string(),
            organization_id: result.organization_id.to_string(),
            owner_principal_id: result.owner_principal_id.to_string(),
            primary_address: result.primary_address,
        }),
    ))
}

async fn update_mail_engine_mailbox_binding(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<MailEngineMailboxBindingResponse>, Response> {
    const MAX_BODY_BYTES: usize = 16 * 1024;
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to bind mail-engine mailboxes",
        &request_id,
    )?;
    let content_type_is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value
                    .strip_prefix("application/")
                    .is_some_and(|subtype| subtype.to_ascii_lowercase().ends_with("+json"))
        });
    if !content_type_is_json {
        return Err(api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
            "Content-Type must be application/json",
            false,
            &request_id,
        ));
    }
    let bytes = to_bytes(request.into_body(), MAX_BODY_BYTES)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_payload_too_large",
                "request payload exceeds the accepted limit",
                false,
                &request_id,
            )
        })?;
    let body: MailEngineMailboxBindingUpdateRequest =
        serde_json::from_slice(&bytes).map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "request payload is invalid",
                false,
                &request_id,
            )
        })?;
    let data_plane_id = state.mail_data_plane_id.ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_data_plane_unavailable",
            "mail data plane is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let result = MailEngineMailboxBindingService::try_new(
        state.store.clone(),
        data_plane_id,
        LEGACY_MAIL_DATA_PLANE_SOURCE,
    )
    .map_err(|error| mail_event_ingress_api_error(error, &request_id))?
    .bind_authorized(
        &authorization,
        BindMailEngineMailboxCommand {
            organization_id: authorization.organization_id,
            mailbox_id,
            account_ref: body.account_ref,
            expected_version: body.expected_version,
            trace_id: request_id.clone(),
            now: Utc::now(),
        },
    )
    .await
    .map_err(|error| mail_event_ingress_api_error(error, &request_id))?;
    Ok(Json(MailEngineMailboxBindingResponse {
        organization_id: result.organization_id.to_string(),
        mailbox_id: result.mailbox_id.to_string(),
        data_plane_id: result.data_plane_id.to_string(),
        account_ref: result.account_ref,
        version: result.version,
        updated_at: result.updated_at.to_rfc3339(),
    }))
}

async fn ingest_stalwart_mail_events(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    request: Request,
) -> Result<(StatusCode, Json<MailEventIngressResponse>), Response> {
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let request_id = request_id_value(&request_id);
    let Some(settings) = &state.mail_event_ingress else {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_event_ingress_unavailable",
            "mail event ingestion is not configured",
            true,
            &request_id,
        ));
    };
    let (expected_bearer, signature_key) = load_mail_event_ingress_secrets(settings)
        .await
        .map_err(|error| {
            tracing::error!(%error, "mail event ingress credentials could not be loaded");
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_event_ingress_unavailable",
                "mail event ingestion is temporarily unavailable",
                true,
                &request_id,
            )
        })?;
    let provided_bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if !provided_bearer
        .is_some_and(|provided| secure_token_matches(&expected_bearer, provided.as_bytes()))
    {
        return Err(mail_event_ingress_unauthorized(&request_id));
    }
    let signature = request
        .headers()
        .get("x-signature")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| mail_event_ingress_unauthorized(&request_id))?;
    let body = to_bytes(request.into_body(), MAX_BODY_BYTES)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "mail_event_payload_too_large",
                "mail event payload exceeds the accepted limit",
                false,
                &request_id,
            )
        })?;
    let events = StalwartWebhookParser::verify_and_parse_all(&body, &signature, &signature_key)
        .map_err(|error| mail_engine_webhook_api_error(error, &request_id))?;
    let data_plane_id = state.mail_data_plane_id.ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_data_plane_unavailable",
            "mail data plane is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let fingerprint_scheme = mail_event_fingerprint_scheme(data_plane_id)
        .map_err(|error| mail_event_ingress_api_error(error, &request_id))?;
    let result = MailEngineEventIngestionService::new(
        state.store.clone(),
        data_plane_id,
        fingerprint_scheme,
    )
    .ingest(IngestMailEngineEventsCommand {
        events,
        received_at: Utc::now(),
    })
    .await
    .map_err(|error| mail_event_ingress_api_error(error, &request_id))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(MailEventIngressResponse {
            accepted: result.accepted,
            inserted: result.inserted,
            duplicates: result.duplicates,
        }),
    ))
}

async fn load_mail_event_ingress_secrets(
    settings: &MailEventIngressSettings,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let bearer = read_bounded_secret_file(&settings.bearer_token_file, 4 * 1024).await?;
    let signature_key = read_bounded_secret_file(&settings.signature_key_file, 16 * 1024).await?;
    let bearer = normalize_ingress_secret(bearer, 32, 4 * 1024)
        .context("mail event ingress bearer token file is invalid")?;
    let signature_key = normalize_ingress_secret(signature_key, 32, 16 * 1024)
        .context("mail event ingress signature key file is invalid")?;
    Ok((bearer, signature_key))
}

fn normalize_ingress_secret(
    mut value: Vec<u8>,
    minimum_bytes: usize,
    maximum_bytes: usize,
) -> Option<Vec<u8>> {
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
    {
        value.pop();
    }
    (value.len() >= minimum_bytes
        && value.len() <= maximum_bytes
        && value.iter().all(|byte| (0x21..=0x7e).contains(byte)))
    .then_some(value)
}

fn secure_token_matches(expected: &[u8], provided: &[u8]) -> bool {
    const COMPARISON_KEY: &[u8] = b"CopilotMail/mail-event-ingress/bearer/v1";
    let mut expected_mac =
        Hmac::<Sha256>::new_from_slice(COMPARISON_KEY).expect("HMAC accepts fixed key");
    expected_mac.update(expected);
    let expected_tag = expected_mac.finalize().into_bytes();
    let mut provided_mac =
        Hmac::<Sha256>::new_from_slice(COMPARISON_KEY).expect("HMAC accepts fixed key");
    provided_mac.update(provided);
    provided_mac.verify_slice(&expected_tag).is_ok()
}

fn mail_event_ingress_unauthorized(request_id: &str) -> Response {
    let mut response = api_error(
        StatusCode::UNAUTHORIZED,
        "mail_event_authentication_failed",
        "mail event source authentication failed",
        false,
        request_id,
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        "Bearer realm=\"mail-event-ingress\"".parse().unwrap(),
    );
    response
}

fn mail_engine_webhook_api_error(error: MailEngineWebhookError, request_id: &str) -> Response {
    match error {
        MailEngineWebhookError::InvalidConfiguration => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_event_ingress_unavailable",
            "mail event ingestion is temporarily unavailable",
            true,
            request_id,
        ),
        MailEngineWebhookError::InvalidSignature => mail_event_ingress_unauthorized(request_id),
        MailEngineWebhookError::InvalidPayload | MailEngineWebhookError::UnsupportedEvent => {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_mail_event_payload",
                "mail event payload is not accepted",
                false,
                request_id,
            )
        }
    }
}

fn mail_event_ingress_api_error(error: MailEventIngestionError, request_id: &str) -> Response {
    match error {
        MailEventIngestionError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "a Human MailAdmin is required to manage this binding",
            false,
            request_id,
        ),
        MailEventIngestionError::InvalidBinding
        | MailEventIngestionError::InvalidTimestamp
        | MailEventIngestionError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_event_configuration",
            "mail event input or mailbox binding is invalid",
            false,
            request_id,
        ),
        MailEventIngestionError::UnmappedMailbox => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_engine_mailbox_unmapped",
            "mail engine account is not mapped to a mailbox",
            true,
            request_id,
        ),
        MailEventIngestionError::Store(copilotmail_application::StoreError::Conflict) => api_error(
            StatusCode::CONFLICT,
            "mail_event_replay_conflict",
            "mail event identity conflicts with existing immutable evidence",
            false,
            request_id,
        ),
        MailEventIngestionError::Store(copilotmail_application::StoreError::MissingReference) => {
            resource_not_found(request_id)
        }
        MailEventIngestionError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_event_store_unavailable",
            "mail event state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

async fn create_mailbox_delegation(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailboxDelegationCreateRequest>,
) -> Result<(StatusCode, Json<MailboxDelegationResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to grant mailbox delegations",
        &request_id,
    )?;
    let grantee_principal_id = body
        .grantee_principal_id
        .parse::<PrincipalId>()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_principal_id",
                "grantee_principal_id must be a UUID",
                false,
                &request_id,
            )
        })?;
    let capabilities = body
        .capabilities
        .into_iter()
        .map(|value| mailbox_capability_from_wire(&value))
        .collect::<Option<BTreeSet<_>>>();
    let Some(capabilities) = capabilities else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_capability",
            "delegation capabilities must be mailbox.read, mailbox.send, or mailbox.send_as",
            false,
            &request_id,
        ));
    };
    let valid_until = chrono::DateTime::parse_from_rfc3339(&body.valid_until)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_expiration",
                "valid_until must be an RFC 3339 instant",
                false,
                &request_id,
            )
        })?;
    let now = Utc::now();
    let result = MailboxDelegationService::new(state.store.clone())
        .grant_authorized(
            &authorization,
            GrantMailboxDelegationCommand {
                organization_id: authorization.organization_id,
                mailbox_id,
                grantee_principal_id,
                capabilities,
                reason: body.reason,
                valid_from: now,
                valid_until,
                trace_id: request_id.clone(),
                now,
            },
        )
        .await
        .map_err(|error| mailbox_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(MailboxDelegationResponse {
            delegation_id: result.delegation_id.to_string(),
            mailbox_id: result.mailbox_id.to_string(),
            grantor_principal_id: result.grantor_principal_id.to_string(),
            grantee_principal_id: result.grantee_principal_id.to_string(),
        }),
    ))
}

async fn revoke_mailbox_delegation(
    State(state): State<Arc<AppState>>,
    Path(delegation_id): Path<DelegationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailboxDelegationRevokedResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to revoke mailbox delegations",
        &request_id,
    )?;
    MailboxDelegationService::new(state.store.clone())
        .revoke_authorized(
            &authorization,
            RevokeMailboxDelegationCommand {
                organization_id: authorization.organization_id,
                delegation_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mailbox_api_error(error, &request_id))?;
    Ok(Json(MailboxDelegationRevokedResponse {
        delegation_id: delegation_id.to_string(),
        revoked: true,
    }))
}

fn mailbox_api_error(
    error: copilotmail_application::MailboxAuthorizationError,
    request_id: &str,
) -> Response {
    match error {
        copilotmail_application::MailboxAuthorizationError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_authorization",
            "the mailbox or delegation policy is invalid",
            false,
            request_id,
        ),
        copilotmail_application::MailboxAuthorizationError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authenticated Principal cannot manage this mailbox",
            false,
            request_id,
        ),
        copilotmail_application::MailboxAuthorizationError::Store(
            copilotmail_application::StoreError::Conflict,
        ) => api_error(
            StatusCode::CONFLICT,
            "mailbox_conflict",
            "the mailbox address or active delegation already exists",
            false,
            request_id,
        ),
        copilotmail_application::MailboxAuthorizationError::Store(
            copilotmail_application::StoreError::MissingReference,
        ) => resource_not_found(request_id),
        copilotmail_application::MailboxAuthorizationError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mailbox_store_unavailable",
            "mailbox state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

async fn get_mailbox(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailboxResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let now = Utc::now();
    let decision = AuthorizeMailboxActionService::new(state.store.clone())
        .authorize(
            &authorization,
            mailbox_id,
            MailboxCapability::Read,
            None,
            now,
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mailbox_authorization_unavailable",
                "mailbox authorization could not be evaluated",
                true,
                &request_id,
            )
        })?;
    if decision.is_none() {
        return Err(resource_not_found(&request_id));
    }
    let mailbox = state
        .store
        .get_mailbox(mailbox_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mailbox_store_unavailable",
                "mailbox data is temporarily unavailable",
                true,
                &request_id,
            )
        })?
        .filter(|mailbox| mailbox.organization_id == authorization.organization_id)
        .ok_or_else(|| resource_not_found(&request_id))?;
    Ok(Json(MailboxResponse {
        mailbox_id: mailbox.mailbox_id.to_string(),
        organization_id: mailbox.organization_id.to_string(),
        kind: mailbox_kind_wire(mailbox.kind).into(),
        primary_address: mailbox.primary_address.to_string(),
        created_at: mailbox.created_at.to_rfc3339(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanMailboxDirectoryQuery {
    limit: Option<u16>,
    cursor: Option<String>,
}

async fn list_human_mailboxes(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<HumanMailboxDirectoryResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let authorization =
        authenticate_human_mail_session_request(&state, &headers, connection, &request_id).await?;
    let Query(query) = Query::<HumanMailboxDirectoryQuery>::try_from_uri(&uri).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_directory_query",
            "the mailbox directory query is invalid",
            false,
            &request_id,
        )
    })?;
    let page = HumanMailboxDirectoryService::new(state.store.clone())
        .list(ListHumanMailboxDirectoryCommand {
            context: authorization,
            limit: query.limit.unwrap_or(25),
            cursor: query.cursor,
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_mailbox_directory_api_error(error, &request_id))?;
    let has_more = page.next_cursor.is_some();
    Ok(Json(HumanMailboxDirectoryResponse {
        items: page
            .items
            .into_iter()
            .map(|item| HumanMailboxDirectoryItemResponse {
                mailbox_id: item.mailbox_id.to_string(),
                kind: mailbox_kind_wire(item.kind).into(),
                primary_address: item.primary_address.to_string(),
                can_send: item.can_send,
            })
            .collect(),
        next_cursor: page.next_cursor,
        has_more,
    }))
}

fn human_mailbox_directory_api_error(
    error: HumanMailboxDirectoryError,
    request_id: &str,
) -> Response {
    match error {
        HumanMailboxDirectoryError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "mailbox_directory_forbidden",
            "the Human is not permitted to use mailboxes",
            false,
            request_id,
        ),
        HumanMailboxDirectoryError::InvalidPageSize => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_directory_limit",
            "limit must be between 1 and 50",
            false,
            request_id,
        ),
        HumanMailboxDirectoryError::InvalidCursor => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_directory_cursor",
            "the mailbox directory cursor is invalid",
            false,
            request_id,
        ),
        HumanMailboxDirectoryError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mailbox_directory_unavailable",
            "the mailbox directory is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HumanDraftListQuery {
    limit: Option<u16>,
    cursor: Option<String>,
}

async fn list_human_drafts(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    OriginalUri(uri): OriginalUri,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<HumanDraftListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let authorization =
        authenticate_human_mail_session_request(&state, &headers, connection, &request_id).await?;
    let Query(query) = Query::<HumanDraftListQuery>::try_from_uri(&uri).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_query",
            "the Human draft query is invalid",
            false,
            &request_id,
        )
    })?;
    let reader = state.mail_draft_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let page = HumanDraftService::new(state.store.clone(), reader)
        .list(ListHumanDraftsCommand {
            context: authorization,
            mailbox_id,
            limit: query.limit.unwrap_or(25),
            cursor: query.cursor,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_draft_api_error(error, &request_id))?;
    let has_more = page.next_cursor.is_some();
    Ok(Json(HumanDraftListResponse {
        items: page
            .items
            .into_iter()
            .map(|item| HumanDraftListItemResponse {
                draft_id: item.draft_id.to_string(),
                version: item.version,
                reply_to_message_id: item.reply_to_message_id.map(|value| value.to_string()),
                recipient_count: item.recipient_count,
                created_at: item.created_at.to_rfc3339(),
                updated_at: item.updated_at.to_rfc3339(),
            })
            .collect(),
        next_cursor: page.next_cursor,
        has_more,
    }))
}

async fn restore_human_draft(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, draft_id)): Path<(MailboxId, DraftId)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    request: Request,
) -> Result<Json<HumanDraftRestoreResponse>, Response> {
    const MAX_BODY_BYTES: usize = 4 * 1024;
    let request_id = request_id_value(&request_id);
    let authorization =
        authenticate_human_mail_session_request(&state, request.headers(), connection, &request_id)
            .await?;
    let content_type_is_json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("application/json"));
    if !content_type_is_json {
        return Err(api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
            "Content-Type must be application/json",
            false,
            &request_id,
        ));
    }
    let bytes = to_bytes(request.into_body(), MAX_BODY_BYTES)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_payload_too_large",
                "request payload exceeds the accepted limit",
                false,
                &request_id,
            )
        })?;
    let body: HumanDraftRestoreRequest = serde_json::from_slice(&bytes).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_request",
            "the Human draft request is invalid",
            false,
            &request_id,
        )
    })?;
    let reader = state.mail_draft_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let restored = HumanDraftService::new(state.store.clone(), reader)
        .restore(RestoreHumanDraftCommand {
            context: authorization,
            mailbox_id,
            draft_id,
            expected_version: body.expected_version,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_draft_api_error(error, &request_id))?;
    Ok(Json(HumanDraftRestoreResponse {
        draft_id: restored.draft_id.to_string(),
        version: restored.version,
        reply_to_message_id: restored.reply_to_message_id.map(|value| value.to_string()),
        to: restored.to,
        cc: restored.cc,
        bcc: restored.bcc,
        subject: restored.subject,
        text_body: restored.text_body,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InboxListQuery {
    cursor: Option<String>,
    page_size: Option<u32>,
}

async fn get_own_agent_mail_policy(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent mail policy self-service requires an authenticated Agent",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to inspect Agent mail policy",
        &request_id,
    )?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;

    let snapshot = AgentMailPolicyQueryService::new(state.store.clone())
        .get_own_policy(GetOwnAgentMailPolicyCommand {
            context: authorization.clone(),
        })
        .await
        .map_err(|error| agent_mail_policy_query_api_error(error, &request_id))?;
    let effective_permissions = Permission::AGENT_TOKEN
        .into_iter()
        .filter(|permission| {
            authorization.permits(authorization.organization_id, *permission)
                && identity
                    .scope_restriction
                    .as_ref()
                    .is_none_or(|scopes| scopes.contains(permission))
        })
        .map(|permission| permission.as_str().to_owned())
        .collect();
    let mut response = Json(own_agent_mail_policy_response(
        snapshot,
        effective_permissions,
    ))
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("static Cache-Control is valid"),
    );
    Ok(response)
}

fn own_agent_mail_policy_response(
    snapshot: AgentMailPolicyRuntimeSnapshot,
    effective_permissions: Vec<String>,
) -> OwnAgentMailPolicyResponse {
    let active_policy =
        snapshot
            .active_policy
            .as_ref()
            .map(|policy| OwnAgentMailPolicyLimitsResponse {
                version: policy.version(),
                allowed_recipient_domains: policy
                    .allowed_recipient_domains()
                    .iter()
                    .cloned()
                    .collect(),
                max_recipients_per_message: policy.max_recipients_per_message(),
                max_messages_per_hour: policy.max_messages_per_hour(),
                max_messages_per_recipient_per_day: policy.max_messages_per_recipient_per_day(),
                require_first_contact_approval: policy.require_first_contact_approval(),
                max_auto_replies_per_thread_per_day: policy.max_auto_replies_per_thread_per_day(),
            });
    let admission_state = match (snapshot.environment, active_policy.is_some()) {
        (AgentEnvironment::Production, true) => "active",
        (AgentEnvironment::Production, false) => "blocked_no_active_policy",
        (AgentEnvironment::Development | AgentEnvironment::Testing, _) => "not_required",
    };
    let remaining_messages = snapshot.active_policy.as_ref().map(|policy| {
        policy
            .max_messages_per_hour()
            .saturating_sub(snapshot.admitted_messages_in_hour)
    });
    OwnAgentMailPolicyResponse {
        agent_id: snapshot.agent_id.to_string(),
        environment: agent_environment_wire(snapshot.environment).into(),
        admission_state: admission_state.into(),
        effective_permissions,
        active_policy,
        hourly_usage: OwnAgentMailHourlyUsageResponse {
            window_started_at: snapshot.hourly_window_started_at.to_rfc3339(),
            admitted_messages: snapshot.admitted_messages_in_hour,
            remaining_messages,
        },
        observed_at: snapshot.observed_at.to_rfc3339(),
    }
}

async fn read_agent_mail_content(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, message_id)): Path<(MailboxId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentMailContentResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent mail content requires an authenticated Agent",
            false,
            &request_id,
        ));
    }
    for permission in [
        Permission::AgentOperate,
        Permission::AgentMailRead,
        Permission::MailboxUse,
    ] {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            permission,
            "Agent mail content permission is required",
            &request_id,
        )?;
    }
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let content = AgentMailContentService::new(state.store.clone(), reader)
        .read(
            &authorization,
            ReadAgentMailContentCommand {
                mailbox_id,
                message_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_mail_content_api_error(error, &request_id))?;
    Ok(Json(AgentMailContentResponse {
        message_id: content.message_id.to_string(),
        subject: content.subject,
        from: content
            .from
            .into_iter()
            .map(inbox_address_response)
            .collect(),
        received_at: content.received_at.to_rfc3339(),
        text_body: content.text_body,
        text_body_truncated: content.text_body_truncated,
        has_attachment: content.has_attachment,
        untrusted_external_content: true,
        html_omitted: true,
        attachments_omitted: true,
    }))
}

fn agent_mail_content_api_error(error: AgentMailContentError, request_id: &str) -> Response {
    match error {
        AgentMailContentError::NotFound => resource_not_found(request_id),
        AgentMailContentError::Unavailable | AgentMailContentError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_mail_content_unavailable",
            "Agent mail content is temporarily unavailable",
            true,
            request_id,
        ),
        AgentMailContentError::Domain(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "agent_mail_content_evidence_failed",
            "Agent mail content evidence could not be committed",
            true,
            request_id,
        ),
    }
}

async fn ensure_agent_processing_subscription(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentProcessingSubscriptionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let subscription = AgentProcessingService::new(state.store.clone())
        .ensure_subscription(EnsureAgentProcessingSubscriptionCommand {
            context: authorization,
            mailbox_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(AgentProcessingSubscriptionResponse {
        subscription_id: subscription.subscription_id.to_string(),
        organization_id: subscription.organization_id.to_string(),
        agent_id: subscription.principal_id.to_string(),
        mailbox_id: subscription.mailbox_id.to_string(),
        status: subscription.status.as_str().into(),
        created_at: subscription.created_at.to_rfc3339(),
        updated_at: subscription.updated_at.to_rfc3339(),
    }))
}

async fn change_agent_processing_subscription(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<SubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentProcessingSubscriptionChangeRequest>,
) -> Result<Json<AgentProcessingSubscriptionChangeResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let expected_status =
        parse_agent_processing_subscription_status(&body.expected_status, &request_id)?;
    let target_status = parse_agent_processing_subscription_status(&body.status, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .change_subscription(ChangeAgentProcessingSubscriptionCommand {
            context: authorization,
            subscription_id,
            expected_status,
            target_status,
            idempotency_key,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(AgentProcessingSubscriptionChangeResponse {
        operation_id: result.operation_id.to_string(),
        subscription_id: result.subscription.subscription_id.to_string(),
        organization_id: result.subscription.organization_id.to_string(),
        agent_id: result.subscription.principal_id.to_string(),
        mailbox_id: result.subscription.mailbox_id.to_string(),
        previous_status: result.previous_status.as_str().into(),
        status: result.subscription.status.as_str().into(),
        replayed: result.replayed,
        created_at: result.subscription.created_at.to_rfc3339(),
        updated_at: result.subscription.updated_at.to_rfc3339(),
    }))
}

async fn claim_agent_message(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<SubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageClaimRequest>,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .claim(ClaimAgentMessageCommand {
            context: authorization,
            subscription_id,
            idempotency_key,
            lease_seconds: body.lease_seconds,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    match result {
        Some(result) => Ok(Json(agent_message_processing_response(result)).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentProcessingDeadLetterQueryParams {
    cursor: Option<String>,
    limit: Option<u16>,
}

async fn list_agent_processing_dead_letters(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<SubscriptionId>,
    Query(query): Query<AgentProcessingDeadLetterQueryParams>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let before_message_id = query
        .cursor
        .as_deref()
        .map(|cursor| parse_agent_processing_dead_letter_cursor(cursor, subscription_id))
        .transpose()
        .map_err(|_| invalid_agent_processing_cursor(&request_id))?;
    let page = AgentProcessingService::new(state.store.clone())
        .list_dead_letters(ListAgentProcessingDeadLettersCommand {
            context: authorization,
            subscription_id,
            before_message_id,
            limit: query.limit.unwrap_or(50),
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    let mut response = Json(AgentProcessingDeadLetterListResponse {
        items: page
            .items
            .into_iter()
            .map(|item| AgentProcessingDeadLetterResponse {
                message_id: item.message_id.to_string(),
                attempt_count: item.attempt_count,
                reason_code: item.reason_code,
                failed_at: item.failed_at.to_rfc3339(),
            })
            .collect(),
        next_cursor: page.next_before_message_id.map(|message_id| {
            format_agent_processing_dead_letter_cursor(subscription_id, message_id)
        }),
        has_more: page.has_more,
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

async fn extend_agent_message_lease(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, message_id)): Path<(SubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageLeaseExtensionRequest>,
) -> Result<Json<AgentMessageProcessingResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let lease_id = parse_agent_processing_lease_id(&body.lease_id, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .extend_lease(ExtendAgentMessageLeaseCommand {
            context: authorization,
            subscription_id,
            message_id,
            lease_id,
            idempotency_key,
            lease_seconds: body.lease_seconds,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(agent_message_processing_response(result)))
}

async fn acknowledge_agent_message(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, message_id)): Path<(SubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageAcknowledgementRequest>,
) -> Result<Json<AgentMessageProcessingResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let lease_id = parse_agent_processing_lease_id(&body.lease_id, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .acknowledge(AcknowledgeAgentMessageCommand {
            context: authorization,
            subscription_id,
            message_id,
            lease_id,
            idempotency_key,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(agent_message_processing_response(result)))
}

async fn nack_agent_message(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, message_id)): Path<(SubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageNackRequest>,
) -> Result<Json<AgentMessageProcessingResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let lease_id = parse_agent_processing_lease_id(&body.lease_id, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .nack(NackAgentMessageCommand {
            context: authorization,
            subscription_id,
            message_id,
            lease_id,
            idempotency_key,
            reason_code: body.reason_code,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(agent_message_processing_response(result)))
}

async fn authenticate_agent_processing_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent message processing requires an authenticated Agent",
            false,
            request_id,
        ));
    }
    for permission in [Permission::AgentOperate, Permission::MailboxUse] {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            permission,
            "Agent message processing permission is required",
            request_id,
        )?;
    }
    enforce_agent_ingress_limit(state, &identity, &authorization, request_id).await?;
    Ok((identity, authorization))
}

#[allow(clippy::result_large_err)]
fn required_idempotency_key(headers: &HeaderMap, request_id: &str) -> Result<String, Response> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "idempotency_key_required",
                "a valid Idempotency-Key header is required",
                false,
                request_id,
            )
        })
}

#[allow(clippy::result_large_err)]
fn parse_agent_processing_lease_id(value: &str, request_id: &str) -> Result<LeaseId, Response> {
    value.parse().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_processing_request",
            "the Agent processing request is invalid",
            false,
            request_id,
        )
    })
}

#[allow(clippy::result_large_err)]
fn parse_agent_processing_subscription_status(
    value: &str,
    request_id: &str,
) -> Result<AgentProcessingSubscriptionStatus, Response> {
    match value {
        "active" => Ok(AgentProcessingSubscriptionStatus::Active),
        "paused" => Ok(AgentProcessingSubscriptionStatus::Paused),
        "retired" => Ok(AgentProcessingSubscriptionStatus::Retired),
        _ => Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_processing_request",
            "the Agent processing subscription status is invalid",
            false,
            request_id,
        )),
    }
}

fn format_agent_processing_dead_letter_cursor(
    subscription_id: SubscriptionId,
    message_id: Uuid,
) -> String {
    format!("v1.{subscription_id}.{message_id}")
}

fn parse_agent_processing_dead_letter_cursor(
    value: &str,
    expected_subscription_id: SubscriptionId,
) -> Result<Uuid, ()> {
    if value.len() > 80 {
        return Err(());
    }
    let mut segments = value.split('.');
    let version = segments.next().ok_or(())?;
    let subscription_id = segments
        .next()
        .ok_or(())?
        .parse::<SubscriptionId>()
        .map_err(|_| ())?;
    let message_id = segments.next().ok_or(())?.parse::<Uuid>().map_err(|_| ())?;
    if version != "v1" || subscription_id != expected_subscription_id || segments.next().is_some() {
        return Err(());
    }
    Ok(message_id)
}

fn invalid_agent_processing_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_agent_processing_cursor",
        "the Agent processing dead-letter cursor is invalid or belongs to another subscription",
        false,
        request_id,
    )
}

fn agent_message_processing_response(
    result: AgentMessageProcessingResult,
) -> AgentMessageProcessingResponse {
    AgentMessageProcessingResponse {
        operation_id: result.operation_id.to_string(),
        subscription_id: result.subscription_id.to_string(),
        message_id: result.message_id.to_string(),
        lease_id: result.lease_id.to_string(),
        operation: result.operation.as_str().into(),
        state: result.state.as_str().into(),
        attempt_count: result.attempt_count,
        lease_expires_at: result.lease_expires_at.map(|value| value.to_rfc3339()),
        reason_code: result.reason_code,
        replayed: result.replayed,
        occurred_at: result.occurred_at.to_rfc3339(),
    }
}

fn agent_processing_api_error(error: AgentProcessingError, request_id: &str) -> Response {
    match error {
        AgentProcessingError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_processing_request",
            "the Agent processing request is invalid",
            false,
            request_id,
        ),
        AgentProcessingError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_processing_forbidden",
            "Agent message processing is forbidden",
            false,
            request_id,
        ),
        AgentProcessingError::NotFound => resource_not_found(request_id),
        AgentProcessingError::Conflict => api_error(
            StatusCode::CONFLICT,
            "agent_processing_conflict",
            "the subscription, lease, processing state, or idempotency key conflicts",
            false,
            request_id,
        ),
        AgentProcessingError::Unavailable | AgentProcessingError::Domain(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_processing_unavailable",
            "Agent message processing is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

async fn list_inbox_messages(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Query(query): Query<InboxListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<InboxPageResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let position = query
        .cursor
        .as_deref()
        .map(|cursor| decode_inbox_cursor(cursor, mailbox_id))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "the inbox cursor is invalid",
                false,
                &request_id,
            )
        })?
        .unwrap_or(0);
    let page = InboxService::new(state.store.clone(), reader)
        .list(
            &authorization,
            ListInboxCommand {
                mailbox_id,
                position,
                page_size: query.page_size.unwrap_or(30),
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    let next_cursor = page
        .next_position
        .map(|next| encode_inbox_cursor(mailbox_id, next));
    let items = page.items.into_iter().map(inbox_summary_response).collect();
    Ok(Json(InboxPageResponse {
        items,
        has_more: next_cursor.is_some(),
        next_cursor,
    }))
}

async fn search_inbox_messages(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<InboxSearchRequest>,
) -> Result<Json<InboxPageResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let search_digest = inbox_search_digest(&body.text);
    let position = body
        .cursor
        .as_deref()
        .map(|cursor| decode_inbox_search_cursor(cursor, mailbox_id, &search_digest))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "the inbox search cursor is invalid",
                false,
                &request_id,
            )
        })?
        .unwrap_or(0);
    let page = InboxService::new(state.store.clone(), reader)
        .search(
            &authorization,
            SearchInboxCommand {
                mailbox_id,
                text: body.text,
                position,
                page_size: body.page_size.unwrap_or(30),
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    let next_cursor = page
        .next_position
        .map(|next| encode_inbox_search_cursor(mailbox_id, next, &search_digest));
    Ok(Json(InboxPageResponse {
        items: page.items.into_iter().map(inbox_summary_response).collect(),
        has_more: next_cursor.is_some(),
        next_cursor,
    }))
}

async fn get_inbox_message(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, message_id)): Path<(MailboxId, uuid::Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MessageDetailResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let detail = InboxService::new(state.store.clone(), reader)
        .get(
            &authorization,
            GetMessageCommand {
                mailbox_id,
                message_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    Ok(Json(MessageDetailResponse {
        message: inbox_summary_response(detail.summary),
        reply_to_message_id: Some(message_id.to_string()),
        to: detail.to.into_iter().map(inbox_address_response).collect(),
        cc: detail.cc.into_iter().map(inbox_address_response).collect(),
        text_body: detail.text_body,
        text_body_truncated: detail.text_body_truncated,
        attachments: detail
            .attachments
            .into_iter()
            .map(|attachment| InboxAttachmentResponse {
                name: attachment.name,
                media_type: attachment.media_type,
                size_bytes: attachment.size_bytes,
            })
            .collect(),
    }))
}

async fn update_inbox_message_state(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, message_id)): Path<(MailboxId, uuid::Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MessageStateUpdateRequest>,
) -> Result<Json<MessageStateResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let state = InboxService::new(state.store.clone(), reader)
        .update_state(
            &authorization,
            UpdateMessageStateCommand {
                mailbox_id,
                message_id,
                is_read: body.is_read,
                is_flagged: body.is_flagged,
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    Ok(Json(MessageStateResponse {
        message_id: message_id.to_string(),
        is_read: state.is_read,
        is_flagged: state.is_flagged,
    }))
}

#[allow(clippy::result_large_err)]
fn enforce_mail_submission_scopes(
    identity: &AuthenticatedIdentity,
    authorization: &AuthorizationContext,
    request_id: &str,
) -> Result<(), Response> {
    enforce_permission_and_scope(
        identity,
        authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        request_id,
    )?;
    if identity.principal_kind == AuthenticatedPrincipalKind::Agent {
        enforce_permission_and_scope(
            identity,
            authorization,
            Permission::AgentOperate,
            "agent.operate permission is required for Agent mail submission",
            request_id,
        )?;
    }
    Ok(())
}

async fn save_text_draft(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<TextDraftSaveRequest>,
) -> Result<(StatusCode, Json<TextDraftResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let drafts = state.mail_drafts.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let draft_id = body
        .draft_id
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_draft_request",
                "the draft request is invalid",
                false,
                &request_id,
            )
        })?;
    let reply_to_message_id = body
        .reply_to_message_id
        .map(|value| Uuid::parse_str(&value))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_draft_request",
                "the draft request is invalid",
                false,
                &request_id,
            )
        })?;
    let draft = DraftService::new(state.store.clone(), drafts)
        .save(
            &authorization,
            SaveTextDraftCommand {
                mailbox_id,
                draft_id,
                expected_version: body.expected_version,
                reply_to_message_id,
                to: body.to,
                cc: body.cc,
                bcc: body.bcc,
                subject: body.subject,
                text_body: body.text_body,
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| draft_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(TextDraftResponse {
            draft_id: draft.draft_id.to_string(),
            version: draft.version,
            reply_to_message_id: draft.reply_to_message_id.map(|value| value.to_string()),
            previous_draft_destroyed: draft.previous_draft_destroyed,
        }),
    ))
}

async fn request_mail_submission(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailSubmissionCreateRequest>,
) -> Result<(StatusCode, Json<MailSubmissionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "idempotency_key_required",
                "a valid Idempotency-Key header is required",
                false,
                &request_id,
            )
        })?;
    let draft_id = body.draft_id.parse().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_submission",
            "the mail submission request is invalid",
            false,
            &request_id,
        )
    })?;
    let first_contact_approval_request_id = body
        .first_contact_approval_request_id
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_mail_submission",
                "the mail submission request is invalid",
                false,
                &request_id,
            )
        })?;
    let intent = MailSubmissionService::new(state.store.clone())
        .request(
            &authorization,
            RequestMailSubmissionCommand {
                mailbox_id,
                draft_id,
                expected_draft_version: body.expected_draft_version,
                idempotency_key,
                first_contact_approval_request_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_submission_api_error(error, &request_id))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(mail_submission_response(intent, None)),
    ))
}

async fn capture_agent_mail_sandbox_submission(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMailSandboxCreateRequest>,
) -> Result<(StatusCode, Json<AgentMailSandboxResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent mail sandbox capture requires an authenticated Agent",
            false,
            &request_id,
        ));
    }
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let draft_id = body.draft_id.parse().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_sandbox_request",
            "the Agent mail sandbox request is invalid",
            false,
            &request_id,
        )
    })?;
    let capture = AgentMailSandboxService::new(state.store.clone())
        .capture(
            &authorization,
            CaptureAgentMailSandboxCommand {
                mailbox_id,
                draft_id,
                expected_draft_version: body.expected_draft_version,
                idempotency_key,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_mail_sandbox_api_error(error, &request_id))?;
    let status = if capture.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        Json(AgentMailSandboxResponse {
            sandbox_submission_id: capture.record.submission_id.to_string(),
            mailbox_id: capture.record.mailbox_id.to_string(),
            draft_id: capture.record.draft_id.to_string(),
            draft_version: capture.record.draft_version,
            recipient_count: capture.record.recipient_count,
            delivery_suppressed: true,
            captured_at: capture.record.captured_at.to_rfc3339(),
            replayed: capture.replayed,
        }),
    ))
}

async fn get_mail_submission(
    State(state): State<Arc<AppState>>,
    Path(submission_id): Path<MailSubmissionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailSubmissionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let intent = state
        .store
        .get_mail_submission_intent(
            authorization.organization_id,
            authorization.principal_id,
            submission_id,
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_submission_unavailable",
                "mail submission status is temporarily unavailable",
                true,
                &request_id,
            )
        })?
        .ok_or_else(|| resource_not_found(&request_id))?;
    let allowed = AuthorizeMailboxActionService::new(state.store.clone())
        .authorize(
            &authorization,
            intent.mailbox_id,
            MailboxCapability::Send,
            None,
            Utc::now(),
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_submission_unavailable",
                "mail submission status is temporarily unavailable",
                true,
                &request_id,
            )
        })?;
    if allowed.is_none() {
        return Err(resource_not_found(&request_id));
    }
    let delivery = state
        .store
        .get_latest_mail_delivery_snapshot(
            authorization.organization_id,
            authorization.principal_id,
            submission_id,
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_submission_unavailable",
                "mail submission delivery status is temporarily unavailable",
                true,
                &request_id,
            )
        })?;
    Ok(Json(mail_submission_response(intent, delivery)))
}

fn mail_submission_response(
    intent: copilotmail_application::MailSubmissionIntentRecord,
    delivery: Option<copilotmail_application::MailDeliverySnapshotRecord>,
) -> MailSubmissionResponse {
    MailSubmissionResponse {
        submission_id: intent.submission_id.to_string(),
        mailbox_id: intent.mailbox_id.to_string(),
        draft_id: intent.draft_id.to_string(),
        draft_version: intent.draft_version,
        status: match intent.status {
            MailSubmissionStatus::Pending => "pending",
            MailSubmissionStatus::Reconciling => "reconciling",
            MailSubmissionStatus::Accepted => "accepted",
            MailSubmissionStatus::Rejected => "rejected",
            MailSubmissionStatus::ManualReview => "manual_review",
        }
        .into(),
        state_version: intent.state_version,
        created_at: intent.created_at.to_rfc3339(),
        updated_at: intent.updated_at.to_rfc3339(),
        delivery: delivery.map(mail_delivery_response),
    }
}

fn mail_delivery_response(
    snapshot: copilotmail_application::MailDeliverySnapshotRecord,
) -> MailDeliveryResponse {
    let mut delivered_count = 0;
    let mut failed_count = 0;
    let mut queued_count = 0;
    let mut unknown_count = 0;
    let recipients = snapshot
        .recipients
        .into_iter()
        .map(|recipient| {
            let status = match recipient.state {
                MailDeliveryRecipientState::Delivered => {
                    delivered_count += 1;
                    "delivered"
                }
                MailDeliveryRecipientState::PermanentFailure => {
                    failed_count += 1;
                    "permanent_failure"
                }
                MailDeliveryRecipientState::Queued => {
                    queued_count += 1;
                    "queued"
                }
                MailDeliveryRecipientState::Unknown => {
                    unknown_count += 1;
                    "unknown"
                }
            };
            MailDeliveryRecipientResponse {
                recipient_ref: sha256_hex(&recipient.recipient_ref),
                status: status.into(),
                smtp_reply: recipient.smtp_reply,
            }
        })
        .collect::<Vec<_>>();
    MailDeliveryResponse {
        observed_at: snapshot.observed_at.to_rfc3339(),
        terminal: snapshot.terminal,
        recipient_count: recipients.len(),
        delivered_count,
        failed_count,
        queued_count,
        unknown_count,
        recipients,
    }
}

fn sha256_hex(value: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in value {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn mail_submission_api_error(error: MailSubmissionError, request_id: &str) -> Response {
    match error {
        MailSubmissionError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_submission",
            "the mail submission request is invalid",
            false,
            request_id,
        ),
        MailSubmissionError::NotFound => resource_not_found(request_id),
        MailSubmissionError::Conflict => api_error(
            StatusCode::CONFLICT,
            "mail_submission_conflict",
            "the draft changed or the idempotency key has another meaning",
            false,
            request_id,
        ),
        MailSubmissionError::AgentAdmission(reason) => {
            agent_mail_admission_api_error(reason, request_id)
        }
        MailSubmissionError::Domain(_) | MailSubmissionError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_submission_unavailable",
            "mail submission is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn agent_mail_sandbox_api_error(error: AgentMailSandboxError, request_id: &str) -> Response {
    match error {
        AgentMailSandboxError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_sandbox_request",
            "the Agent mail sandbox request is invalid",
            false,
            request_id,
        ),
        AgentMailSandboxError::NotFound => resource_not_found(request_id),
        AgentMailSandboxError::Conflict => api_error(
            StatusCode::CONFLICT,
            "agent_mail_sandbox_conflict",
            "the draft changed or the idempotency key has another meaning",
            false,
            request_id,
        ),
        AgentMailSandboxError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_mail_sandbox_unavailable",
            "the Agent mail sandbox is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn agent_mail_admission_api_error(reason: AgentMailAdmissionFailure, request_id: &str) -> Response {
    let (status, code, message, retryable, details, retry_after) = match reason {
        AgentMailAdmissionFailure::RecipientPolicy => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "agent_mail_recipient_policy_denied",
            "one or more recipients are outside the active Agent mail policy",
            false,
            json!({"scope": "recipient_policy"}),
            None,
        ),
        AgentMailAdmissionFailure::HourlyQuota => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_hourly_quota_exceeded",
            "the production Agent hourly mail quota is exhausted",
            true,
            json!({"scope": "agent_hour"}),
            Some(60),
        ),
        AgentMailAdmissionFailure::RecipientDailyQuota => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_recipient_daily_quota_exceeded",
            "a recipient daily mail quota is exhausted",
            true,
            json!({"scope": "recipient_day"}),
            Some(3_600),
        ),
        AgentMailAdmissionFailure::ThreadCircuitBreaker => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_thread_circuit_breaker_open",
            "the production Agent thread circuit breaker is open",
            true,
            json!({"scope": "thread_day"}),
            Some(3_600),
        ),
        AgentMailAdmissionFailure::AutomaticReplyLoop => (
            StatusCode::CONFLICT,
            "agent_mail_automatic_reply_blocked",
            "Agents cannot reply to an automatically submitted message",
            false,
            json!({"scope": "automatic_reply_loop"}),
            None,
        ),
        AgentMailAdmissionFailure::DuplicateContentCircuitBreaker => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_duplicate_content_circuit_breaker_open",
            "the Agent duplicate-content circuit breaker is open",
            true,
            json!({"scope": "identical_body_day"}),
            Some(3_600),
        ),
        AgentMailAdmissionFailure::SandboxRequired => (
            StatusCode::CONFLICT,
            "agent_mail_sandbox_required",
            "testing Agents must use the non-delivering mail sandbox",
            false,
            json!({"scope": "testing_environment"}),
            None,
        ),
        AgentMailAdmissionFailure::FirstContactApproval => (
            StatusCode::CONFLICT,
            "agent_first_contact_approval_required",
            "an exact live first-contact approval is required",
            false,
            json!({
                "approval_action": "agent.first_contact",
                "resource_kind": "agent.mail_draft",
                "resource_id_format": "{draft_id}:{expected_draft_version}",
            }),
            None,
        ),
    };
    let mut response =
        api_error_with_details(status, code, message, retryable, details, request_id);
    if let Some(retry_after) = retry_after {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            retry_after.to_string().parse().unwrap(),
        );
    }
    response
}

fn inbox_summary_response(item: HumanInboxMessageSummary) -> InboxMessageSummaryResponse {
    InboxMessageSummaryResponse {
        message_id: item.message_id.to_string(),
        subject: item.subject,
        from: item.from.into_iter().map(inbox_address_response).collect(),
        received_at: item.received_at.to_rfc3339(),
        preview: item.preview,
        is_read: item.is_read,
        is_flagged: item.is_flagged,
        has_attachment: item.has_attachment,
    }
}

fn inbox_address_response(address: MailAddress) -> InboxAddressResponse {
    InboxAddressResponse {
        name: address.name,
        email: address.email,
    }
}

fn encode_inbox_cursor(mailbox_id: MailboxId, position: u32) -> String {
    URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "v": 2,
            "mailbox_id": mailbox_id,
            "position": position,
        }))
        .expect("inbox cursor consists only of serializable primitives"),
    )
}

fn decode_inbox_cursor(cursor: &str, mailbox_id: MailboxId) -> Result<u32, ()> {
    if cursor.is_empty() || cursor.len() > 2048 {
        return Err(());
    }
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| ())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    if object.len() != 3
        || object.get("v").and_then(serde_json::Value::as_u64) != Some(2)
        || object.get("mailbox_id").and_then(serde_json::Value::as_str)
            != Some(mailbox_id.to_string().as_str())
    {
        return Err(());
    }
    object
        .get("position")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value <= 10_000)
        .ok_or(())
}

fn inbox_search_digest(text: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.inbox-search.v1");
    digest.update((text.len() as u64).to_be_bytes());
    digest.update(text.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

fn encode_inbox_search_cursor(mailbox_id: MailboxId, position: u32, search_digest: &str) -> String {
    URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "v": 2,
            "mailbox_id": mailbox_id,
            "position": position,
            "search_digest": search_digest,
        }))
        .expect("inbox search cursor consists only of serializable primitives"),
    )
}

fn decode_inbox_search_cursor(
    cursor: &str,
    mailbox_id: MailboxId,
    expected_search_digest: &str,
) -> Result<u32, ()> {
    if cursor.is_empty() || cursor.len() > 2048 {
        return Err(());
    }
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| ())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    if object.len() != 4
        || object.get("v").and_then(serde_json::Value::as_u64) != Some(2)
        || object.get("mailbox_id").and_then(serde_json::Value::as_str)
            != Some(mailbox_id.to_string().as_str())
        || object
            .get("search_digest")
            .and_then(serde_json::Value::as_str)
            != Some(expected_search_digest)
    {
        return Err(());
    }
    object
        .get("position")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value <= 10_000)
        .ok_or(())
}

fn inbox_api_error(error: InboxError, request_id: &str) -> Response {
    match error {
        InboxError::NotFound => resource_not_found(request_id),
        InboxError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_inbox_request",
            "the inbox request is invalid",
            false,
            request_id,
        ),
        InboxError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn draft_api_error(error: DraftError, request_id: &str) -> Response {
    match error {
        DraftError::NotFound => resource_not_found(request_id),
        DraftError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_draft_request",
            "the draft request is invalid",
            false,
            request_id,
        ),
        DraftError::Conflict => api_error(
            StatusCode::CONFLICT,
            "draft_conflict",
            "the draft changed or is no longer editable",
            false,
            request_id,
        ),
        DraftError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn human_draft_api_error(error: HumanDraftError, request_id: &str) -> Response {
    match error {
        HumanDraftError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "human_draft_forbidden",
            "the Human is not permitted to access drafts",
            false,
            request_id,
        ),
        HumanDraftError::NotFound => resource_not_found(request_id),
        HumanDraftError::InvalidRequest | HumanDraftError::InvalidPageSize => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_request",
            "the Human draft request is invalid",
            false,
            request_id,
        ),
        HumanDraftError::InvalidCursor => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_cursor",
            "the Human draft cursor is invalid",
            false,
            request_id,
        ),
        HumanDraftError::Conflict => api_error(
            StatusCode::CONFLICT,
            "draft_conflict",
            "the draft changed or is no longer editable",
            false,
            request_id,
        ),
        HumanDraftError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn resource_not_found(request_id: &str) -> Response {
    api_error(
        StatusCode::NOT_FOUND,
        "resource_not_found",
        "the requested resource does not exist",
        false,
        request_id,
    )
}

const fn mailbox_kind_wire(kind: MailboxKind) -> &'static str {
    match kind {
        MailboxKind::Human => "human",
        MailboxKind::Agent => "agent",
        MailboxKind::Shared => "shared",
        MailboxKind::Group => "group",
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComplianceCaseListQuery {
    status: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegalHoldListQuery {
    status: Option<String>,
    case_id: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

async fn list_compliance_cases(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<ComplianceCaseListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ComplianceCaseListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let status = parse_compliance_case_filter(query.status.as_deref())
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let limit = bounded_compliance_limit(query.limit)
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let after_case_id = query
        .cursor
        .as_deref()
        .map(|cursor| decode_compliance_cursor(cursor, "case", organization_id))
        .transpose()
        .map_err(|_| invalid_compliance_cursor(&request_id))?
        .map(ComplianceCaseId::from_uuid);
    if let Some(after_case_id) = after_case_id
        && !state
            .store
            .compliance_case_cursor_exists(organization_id, status, after_case_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
    {
        return Err(invalid_compliance_cursor(&request_id));
    }
    let mut cases = state
        .store
        .list_compliance_cases(organization_id, status, after_case_id, limit + 1)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    let has_more = cases.len() > limit as usize;
    if has_more {
        cases.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| cases.last())
        .flatten()
        .map(|case| encode_compliance_cursor("case", organization_id, case.id().as_uuid()));
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::Case,
        organization_id.to_string(),
        serde_json::json!({
            "operation": "list",
            "status": query.status,
            "result_count": cases.len(),
            "has_more": has_more,
            "cursor_present": after_case_id.is_some(),
        }),
        &request_id,
    )
    .await?;
    Ok(Json(ComplianceCaseListResponse {
        items: cases.into_iter().map(compliance_case_response).collect(),
        next_cursor,
        has_more,
    }))
}

async fn list_legal_holds(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<LegalHoldListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<LegalHoldListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let status = parse_legal_hold_filter(query.status.as_deref())
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let case_id = query
        .case_id
        .as_deref()
        .map(str::parse::<ComplianceCaseId>)
        .transpose()
        .map_err(|_| invalid_compliance_list_filter(&request_id))?;
    let limit = bounded_compliance_limit(query.limit)
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let after_hold_id = query
        .cursor
        .as_deref()
        .map(|cursor| decode_compliance_cursor(cursor, "hold", organization_id))
        .transpose()
        .map_err(|_| invalid_compliance_cursor(&request_id))?
        .map(LegalHoldId::from_uuid);
    if let Some(after_hold_id) = after_hold_id
        && !state
            .store
            .legal_hold_cursor_exists(organization_id, status, case_id, after_hold_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
    {
        return Err(invalid_compliance_cursor(&request_id));
    }
    let mut holds = state
        .store
        .list_legal_holds(organization_id, status, case_id, after_hold_id, limit + 1)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    let has_more = holds.len() > limit as usize;
    if has_more {
        holds.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| holds.last())
        .flatten()
        .map(|hold| encode_compliance_cursor("hold", organization_id, hold.id().as_uuid()));
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::LegalHold,
        organization_id.to_string(),
        serde_json::json!({
            "operation": "list",
            "status": query.status,
            "case_id": case_id,
            "result_count": holds.len(),
            "has_more": has_more,
            "cursor_present": after_hold_id.is_some(),
        }),
        &request_id,
    )
    .await?;
    Ok(Json(LegalHoldListResponse {
        items: holds.into_iter().map(legal_hold_response).collect(),
        next_cursor,
        has_more,
    }))
}

fn parse_compliance_case_filter(value: Option<&str>) -> Option<Option<ComplianceCaseStatus>> {
    match value {
        None => Some(None),
        Some("open") => Some(Some(ComplianceCaseStatus::Open)),
        Some("closed") => Some(Some(ComplianceCaseStatus::Closed)),
        Some(_) => None,
    }
}

fn parse_legal_hold_filter(value: Option<&str>) -> Option<Option<LegalHoldStatus>> {
    match value {
        None => Some(None),
        Some("active") => Some(Some(LegalHoldStatus::Active)),
        Some("released") => Some(Some(LegalHoldStatus::Released)),
        Some(_) => None,
    }
}

fn bounded_compliance_limit(limit: Option<i64>) -> Option<i64> {
    let limit = limit.unwrap_or(50);
    (1..=100).contains(&limit).then_some(limit)
}

fn encode_compliance_cursor(kind: &str, organization_id: OrganizationId, id: Uuid) -> String {
    format!("v1.{kind}.{organization_id}.{id}")
}

fn decode_compliance_cursor(
    cursor: &str,
    expected_kind: &str,
    organization_id: OrganizationId,
) -> Result<Uuid, ()> {
    let mut parts = cursor.split('.');
    if parts.next() != Some("v1")
        || parts.next() != Some(expected_kind)
        || parts
            .next()
            .and_then(|value| value.parse::<OrganizationId>().ok())
            != Some(organization_id)
    {
        return Err(());
    }
    let id = parts.next().and_then(|value| Uuid::parse_str(value).ok());
    if parts.next().is_some() {
        return Err(());
    }
    id.ok_or(())
}

fn invalid_compliance_list_filter(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_compliance_list_filter",
        "status, case_id, or limit is invalid",
        false,
        request_id,
    )
}

fn invalid_compliance_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::GONE,
        "compliance_cursor_expired_or_invalid",
        "the cursor is unavailable or does not match the tenant and active filters",
        false,
        request_id,
    )
}

async fn create_compliance_case(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<ComplianceCaseCreateRequest>,
) -> Result<(StatusCode, Json<ComplianceCaseResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let case = ComplianceGovernanceService::new(state.store.clone())
        .create_case(CreateComplianceCaseCommand {
            context: authorization,
            external_reference: body.external_reference,
            title: body.title,
            purpose: body.purpose,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| compliance_api_error(error, &request_id))?;
    Ok((StatusCode::CREATED, Json(compliance_case_response(case))))
}

async fn get_compliance_case(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<ComplianceCaseId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ComplianceCaseResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let case = state
        .store
        .get_compliance_case(authorization.organization_id, case_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::Case,
        case_id.to_string(),
        serde_json::json!({"operation": "get"}),
        &request_id,
    )
    .await?;
    Ok(Json(compliance_case_response(case)))
}

async fn close_compliance_case(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<ComplianceCaseId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ComplianceCaseResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let case = ComplianceGovernanceService::new(state.store.clone())
        .close_case(CloseComplianceCaseCommand {
            context: authorization,
            case_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| compliance_api_error(error, &request_id))?;
    Ok(Json(compliance_case_response(case)))
}

async fn apply_audit_legal_hold(
    State(state): State<Arc<AppState>>,
    Path(archive_id): Path<AuditArchiveId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<LegalHoldApplyRequest>,
) -> Result<(StatusCode, Json<LegalHoldResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let case_id = body.case_id.parse::<ComplianceCaseId>().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_compliance_case_id",
            "case_id must be a UUID",
            false,
            &request_id,
        )
    })?;
    let approval_request_id = parse_legal_hold_approval_id(&body.approval_request_id)
        .ok_or_else(|| invalid_legal_hold_approval_id(&request_id))?;
    let approval = ApprovalProof {
        organization_id: authorization.organization_id,
        request_id: approval_request_id,
        requested_by: authorization.principal_id,
        action: ApprovalAction::LegalHold,
        resource_kind: "audit.archive".into(),
        resource_id: archive_id.to_string(),
        now: Utc::now(),
    };
    ensure_legal_hold_approval(&state, &approval, &request_id).await?;
    if let Some(existing) = state
        .store
        .get_legal_hold_by_apply_approval(authorization.organization_id, approval_request_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
    {
        if exact_active_hold_replay(&existing, case_id, archive_id, &body.reason) {
            return Ok((StatusCode::OK, Json(legal_hold_response(existing))));
        }
        return Err(legal_hold_conflict(&request_id));
    }
    let lock = state
        .store
        .try_acquire_audit_legal_hold_lock(authorization.organization_id, archive_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| legal_hold_conflict(&request_id))?;
    let result = async {
        if let Some(existing) = state
            .store
            .get_legal_hold_by_apply_approval(authorization.organization_id, approval_request_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
        {
            return if exact_active_hold_replay(&existing, case_id, archive_id, &body.reason) {
                Ok(existing)
            } else {
                Err(legal_hold_conflict(&request_id))
            };
        }
        let record = state
            .store
            .get_audit_archive(authorization.organization_id, archive_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
            .ok_or_else(|| resource_not_found(&request_id))?;
        let archive_settings = state
            .audit_archive
            .as_ref()
            .ok_or_else(|| legal_hold_object_store_unavailable(&request_id))?;
        let archive = connect_legal_hold_archive_settings(archive_settings, &record)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let events = archive
            .set_legal_hold(&record.events.key, &record.events.version_id, true)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let manifest = archive
            .set_legal_hold(&record.manifest.key, &record.manifest.version_id, true)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        ComplianceGovernanceService::new(state.store.clone())
            .apply_hold(ApplyLegalHoldCommand {
                context: authorization,
                case_id,
                archive_id,
                reason: body.reason,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: true,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: request_id.clone(),
                now: Utc::now(),
            })
            .await
            .map_err(|error| compliance_api_error(error, &request_id))
    }
    .await;
    lock.release()
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    let hold = result?;
    Ok((StatusCode::CREATED, Json(legal_hold_response(hold))))
}

async fn get_legal_hold(
    State(state): State<Arc<AppState>>,
    Path(hold_id): Path<LegalHoldId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<LegalHoldResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let hold = state
        .store
        .get_legal_hold(authorization.organization_id, hold_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::LegalHold,
        hold_id.to_string(),
        serde_json::json!({"operation": "get"}),
        &request_id,
    )
    .await?;
    Ok(Json(legal_hold_response(hold)))
}

async fn release_audit_legal_hold(
    State(state): State<Arc<AppState>>,
    Path(hold_id): Path<LegalHoldId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<LegalHoldReleaseRequest>,
) -> Result<Json<LegalHoldResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = parse_legal_hold_approval_id(&body.approval_request_id)
        .ok_or_else(|| invalid_legal_hold_approval_id(&request_id))?;
    let approval = ApprovalProof {
        organization_id: authorization.organization_id,
        request_id: approval_request_id,
        requested_by: authorization.principal_id,
        action: ApprovalAction::LegalHold,
        resource_kind: "compliance.legal_hold".into(),
        resource_id: hold_id.to_string(),
        now: Utc::now(),
    };
    ensure_legal_hold_approval(&state, &approval, &request_id).await?;
    let current = state
        .store
        .get_legal_hold(authorization.organization_id, hold_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    if current.status() == LegalHoldStatus::Released {
        return if current.release_approval_request_id() == Some(approval_request_id) {
            Ok(Json(legal_hold_response(current)))
        } else {
            Err(legal_hold_conflict(&request_id))
        };
    }
    let archive_id = current.archive_id();
    let lock = state
        .store
        .try_acquire_audit_legal_hold_lock(authorization.organization_id, archive_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| legal_hold_conflict(&request_id))?;
    let result = async {
        let current = state
            .store
            .get_legal_hold(authorization.organization_id, hold_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
            .ok_or_else(|| resource_not_found(&request_id))?;
        if current.status() == LegalHoldStatus::Released {
            return if current.release_approval_request_id() == Some(approval_request_id) {
                Ok(current)
            } else {
                Err(legal_hold_conflict(&request_id))
            };
        }
        let record = state
            .store
            .get_audit_archive(authorization.organization_id, archive_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
            .ok_or_else(|| resource_not_found(&request_id))?;
        let archive_settings = state
            .audit_archive
            .as_ref()
            .ok_or_else(|| legal_hold_object_store_unavailable(&request_id))?;
        let archive = connect_legal_hold_archive_settings(archive_settings, &record)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let manifest = archive
            .set_legal_hold(&record.manifest.key, &record.manifest.version_id, false)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let events = archive
            .set_legal_hold(&record.events.key, &record.events.version_id, false)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        ComplianceGovernanceService::new(state.store.clone())
            .release_hold(ReleaseLegalHoldCommand {
                context: authorization,
                hold_id,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: false,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: request_id.clone(),
                now: Utc::now(),
            })
            .await
            .map_err(|error| compliance_api_error(error, &request_id))
    }
    .await;
    lock.release()
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    Ok(Json(legal_hold_response(result?)))
}

async fn authorize_audit_export(
    State(state): State<Arc<AppState>>,
    Path(archive_id): Path<AuditArchiveId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AuditExportGrantCreateRequest>,
) -> Result<(StatusCode, Json<AuditExportGrantResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_audit_export_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_approval_request_id",
                "approval_request_id must be a UUID",
                false,
                &request_id,
            )
        })?;
    let now = Utc::now();
    let approval = ApprovalProof {
        organization_id: authorization.organization_id,
        request_id: approval_request_id,
        requested_by: authorization.principal_id,
        action: ApprovalAction::AuditExport,
        resource_kind: "audit.archive".into(),
        resource_id: archive_id.to_string(),
        now,
    };
    let grant = AuditExportAuthorizationService::new(state.store.clone())
        .authorize(AuthorizeAuditExportCommand {
            context: authorization,
            archive_id,
            approval,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| audit_export_authorization_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(audit_export_grant_response(grant)),
    ))
}

async fn get_audit_export_grant(
    State(state): State<Arc<AppState>>,
    Path(grant_id): Path<AuditExportId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AuditExportGrantResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_audit_export_request(&state, &headers, connection, &request_id).await?;
    let grant = state
        .store
        .get_audit_export_grant(authorization.organization_id, grant_id)
        .await
        .map_err(|_| audit_export_store_unavailable(&request_id))?
        .filter(|grant| grant.requested_by() == authorization.principal_id)
        .ok_or_else(|| resource_not_found(&request_id))?;
    Ok(Json(audit_export_grant_response(grant)))
}

/// Upper bound on a single served archive artifact. It keeps one request from
/// buffering an unbounded object into memory.
const MAX_AUDIT_EXPORT_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

/// Serves one exact WORM archive artifact under an active export grant.
///
/// The object is fetched and independently reverified against the digest and
/// size recorded when the archive was created, and the retrieval is committed
/// to the append-only download ledger before any byte reaches the caller.
async fn download_audit_export_artifact(
    State(state): State<Arc<AppState>>,
    Path((grant_id, artifact)): Path<(AuditExportId, String)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_audit_export_request(&state, &headers, connection, &request_id).await?;
    let artifact = match artifact.as_str() {
        "events" => AuditExportArtifact::Events,
        "manifest" => AuditExportArtifact::Manifest,
        _ => return Err(resource_not_found(&request_id)),
    };
    let service = AuditExportDownloadService::new(state.store.clone());
    let authorized = service
        .resolve(ResolveAuditExportObjectCommand {
            context: authorization.clone(),
            grant_id,
            artifact,
            now: Utc::now(),
        })
        .await
        .map_err(|error| audit_export_download_api_error(error, &request_id))?;

    let archive_record = state
        .store
        .get_audit_archive(authorization.organization_id, authorized.archive_id)
        .await
        .map_err(|_| audit_export_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    let archive_settings = state
        .audit_archive
        .as_ref()
        .ok_or_else(|| audit_export_object_store_unavailable(&request_id))?;
    let archive = connect_legal_hold_archive_settings(archive_settings, &archive_record)
        .await
        .map_err(|_| audit_export_object_store_unavailable(&request_id))?;
    let retrieved = archive
        .get_verified_version(
            &authorized.object.key,
            &authorized.object.version_id,
            &authorized.object.sha256_hex,
            authorized.object.size_bytes,
            MAX_AUDIT_EXPORT_ARTIFACT_BYTES,
        )
        .await
        .map_err(|_| audit_export_object_store_unavailable(&request_id))?;

    service
        .record(RecordAuditExportDownloadCommand {
            context: authorization,
            retrieved: ArchivedObjectRecord {
                key: retrieved.key,
                version_id: retrieved.version_id,
                sha256_hex: retrieved.sha256_hex.clone(),
                size_bytes: retrieved.size_bytes,
            },
            authorized,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| audit_export_download_api_error(error, &request_id))?;

    let extension = match artifact {
        AuditExportArtifact::Events => "jsonl",
        AuditExportArtifact::Manifest => "json",
    };
    let file_name = format!("{grant_id}-{}.{}", artifact.as_str(), extension);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            (header::CONTENT_LENGTH, retrieved.size_bytes.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file_name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (
                HeaderName::from_static("x-copilotmail-artifact-sha256"),
                retrieved.sha256_hex,
            ),
        ],
        retrieved.bytes,
    )
        .into_response())
}

async fn authenticate_audit_export_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuditExport,
        "audit.export permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| audit_export_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "audit export authorization requires an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

async fn create_retention_policy(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RetentionPolicyCreateRequest>,
) -> Result<(StatusCode, Json<RetentionPolicyResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let resource_kind = retention_resource_kind_from_wire(&body.resource_kind)
        .ok_or_else(|| invalid_retention_request(&request_id))?;
    let draft = RetentionPolicyService::new(state.store.clone())
        .create(CreateRetentionPolicyCommand {
            context: authorization,
            policy_key: body.policy_key,
            display_name: body.display_name,
            resource_kind,
            minimum_retention_days: body.minimum_retention_days,
            archive_after_days: body.archive_after_days,
            delete_after_days: body.delete_after_days,
            rule: body.rule,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(retention_policy_response(draft.policy, None, draft.version)),
    ))
}

async fn append_retention_policy_version(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<RetentionPolicyId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RetentionPolicyVersionCreateRequest>,
) -> Result<(StatusCode, Json<RetentionPolicyVersionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    let version = RetentionPolicyService::new(state.store.clone())
        .append_version(AppendRetentionPolicyVersionCommand {
            context: authorization,
            policy_id,
            minimum_retention_days: body.minimum_retention_days,
            archive_after_days: body.archive_after_days,
            delete_after_days: body.delete_after_days,
            rule: body.rule,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(retention_policy_version_response(version)),
    ))
}

async fn activate_retention_policy_version(
    State(state): State<Arc<AppState>>,
    Path((policy_id, version_id)): Path<(RetentionPolicyId, RetentionPolicyVersionId)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RetentionPolicyActivationRequest>,
) -> Result<Json<RetentionPolicyVersionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| invalid_retention_request(&request_id))?;
    let now = Utc::now();
    let version = RetentionPolicyService::new(state.store.clone())
        .activate_version(ActivateRetentionPolicyVersionCommand {
            context: authorization.clone(),
            policy_id,
            policy_version_id: version_id,
            approval: ApprovalProof {
                organization_id: authorization.organization_id,
                request_id: approval_request_id,
                requested_by: authorization.principal_id,
                action: ApprovalAction::RetentionChange,
                resource_kind: "compliance.retention_policy_version".into(),
                resource_id: version_id.to_string(),
                now,
            },
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok(Json(retention_policy_version_response(version)))
}

async fn activate_agent_mail_policy_version(
    State(state): State<Arc<AppState>>,
    Path((agent_id, version_id)): Path<(PrincipalId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMailPolicyActivationRequest>,
) -> Result<Json<AgentMailPolicyActivationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_mail_policy_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| invalid_agent_mail_policy_request(&request_id))?;
    let now = Utc::now();
    let activation = AgentMailPolicyService::new(state.store.clone())
        .activate(ActivateAgentMailPolicyCommand {
            context: authorization.clone(),
            agent_id,
            policy_version_id: version_id,
            approval: ApprovalProof {
                organization_id: authorization.organization_id,
                request_id: approval_request_id,
                requested_by: authorization.principal_id,
                action: ApprovalAction::ProductionAgentMail,
                resource_kind: "agent.mail_policy_version".into(),
                resource_id: version_id.to_string(),
                now,
            },
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| agent_mail_policy_api_error(error, &request_id))?;
    Ok(Json(AgentMailPolicyActivationResponse {
        activation_id: activation.id().to_string(),
        organization_id: activation.organization_id().to_string(),
        agent_id: activation.agent_id().to_string(),
        policy_version: activation.policy_version(),
        approval_request_id: activation.approval_request_id().to_string(),
        activated_by: activation.activated_by().to_string(),
        activated_at: activation.activated_at().to_rfc3339(),
    }))
}

async fn create_agent_mail_policy_version(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<PrincipalId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMailPolicyVersionCreateRequest>,
) -> Result<(StatusCode, Json<AgentMailPolicyVersionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_mail_policy_request(&state, &headers, connection, &request_id).await?;
    let record = AgentMailPolicyService::new(state.store.clone())
        .create_version(CreateAgentMailPolicyVersionCommand {
            context: authorization,
            agent_id,
            allowed_recipient_domains: body.allowed_recipient_domains.into_iter().collect(),
            max_recipients_per_message: body.max_recipients_per_message,
            max_messages_per_hour: body.max_messages_per_hour,
            max_messages_per_recipient_per_day: body.max_messages_per_recipient_per_day,
            require_first_contact_approval: body.require_first_contact_approval,
            max_auto_replies_per_thread_per_day: body.max_auto_replies_per_thread_per_day,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_mail_policy_api_error(error, &request_id))?;
    let policy = &record.policy;
    Ok((
        StatusCode::CREATED,
        Json(AgentMailPolicyVersionResponse {
            policy_version_id: record.id.to_string(),
            organization_id: policy.organization_id().to_string(),
            agent_id: policy.agent_id().to_string(),
            version: policy.version(),
            allowed_recipient_domains: policy.allowed_recipient_domains().iter().cloned().collect(),
            max_recipients_per_message: policy.max_recipients_per_message(),
            max_messages_per_hour: policy.max_messages_per_hour(),
            max_messages_per_recipient_per_day: policy.max_messages_per_recipient_per_day(),
            require_first_contact_approval: policy.require_first_contact_approval(),
            max_auto_replies_per_thread_per_day: policy.max_auto_replies_per_thread_per_day(),
            created_by: record.created_by.to_string(),
            created_at: record.created_at.to_rfc3339(),
        }),
    ))
}

async fn get_agent_mail_policy_version(
    State(state): State<Arc<AppState>>,
    Path((agent_id, version_id)): Path<(PrincipalId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentMailPolicyVersionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_mail_policy_request(&state, &headers, connection, &request_id).await?;
    let record = AgentMailPolicyService::new(state.store.clone())
        .get_version(GetAgentMailPolicyVersionCommand {
            context: authorization,
            agent_id,
            policy_version_id: version_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_mail_policy_api_error(error, &request_id))?;
    let policy = &record.policy;
    Ok(Json(AgentMailPolicyVersionResponse {
        policy_version_id: record.id.to_string(),
        organization_id: policy.organization_id().to_string(),
        agent_id: policy.agent_id().to_string(),
        version: policy.version(),
        allowed_recipient_domains: policy.allowed_recipient_domains().iter().cloned().collect(),
        max_recipients_per_message: policy.max_recipients_per_message(),
        max_messages_per_hour: policy.max_messages_per_hour(),
        max_messages_per_recipient_per_day: policy.max_messages_per_recipient_per_day(),
        require_first_contact_approval: policy.require_first_contact_approval(),
        max_auto_replies_per_thread_per_day: policy.max_auto_replies_per_thread_per_day(),
        created_by: record.created_by.to_string(),
        created_at: record.created_at.to_rfc3339(),
    }))
}

async fn get_retention_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<RetentionPolicyId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<RetentionPolicyResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    let detail = RetentionPolicyService::new(state.store.clone())
        .get(GetRetentionPolicyCommand {
            context: authorization,
            policy_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok(Json(retention_policy_response(
        detail.policy,
        detail.active_version,
        detail.latest_version,
    )))
}

async fn authenticate_retention_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::RetentionManage,
        "retention.manage permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| retention_policy_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "retention policy management requires an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

async fn authenticate_agent_mail_policy_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| agent_mail_policy_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "Agent mail policy activation requires an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

async fn authenticate_compliance_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::LegalHoldManage,
        "legal_hold.manage permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| compliance_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "compliance operations require an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

async fn ensure_legal_hold_approval(
    state: &AppState,
    approval: &ApprovalProof,
    request_id: &str,
) -> Result<(), Response> {
    if !state
        .store
        .approval_permits(approval)
        .await
        .map_err(|_| compliance_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "legal_hold_approval_required",
            "an exact approved and unexpired Legal Hold request is required",
            false,
            request_id,
        ));
    }
    Ok(())
}

async fn record_compliance_read(
    state: &AppState,
    context: AuthorizationContext,
    kind: ComplianceReadKind,
    resource_id: String,
    details: serde_json::Value,
    request_id: &str,
) -> Result<(), Response> {
    ComplianceGovernanceService::new(state.store.clone())
        .record_read(RecordComplianceReadCommand {
            context,
            kind,
            resource_id,
            details,
            trace_id: request_id.into(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| compliance_api_error(error, request_id))
}

fn parse_legal_hold_approval_id(value: &str) -> Option<ApprovalRequestId> {
    value.parse().ok()
}

fn invalid_legal_hold_approval_id(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_approval_request_id",
        "approval_request_id must be a UUID",
        false,
        request_id,
    )
}

fn exact_active_hold_replay(
    hold: &LegalHold,
    case_id: ComplianceCaseId,
    archive_id: AuditArchiveId,
    reason: &str,
) -> bool {
    hold.case_id() == case_id
        && hold.archive_id() == archive_id
        && hold.reason() == reason
        && hold.status() == LegalHoldStatus::Active
}

fn compliance_case_response(case: ComplianceCase) -> ComplianceCaseResponse {
    ComplianceCaseResponse {
        case_id: case.id().to_string(),
        organization_id: case.organization_id().to_string(),
        external_reference: case.external_reference().into(),
        title: case.title().into(),
        purpose: case.purpose().into(),
        status: match case.status() {
            ComplianceCaseStatus::Open => "open",
            ComplianceCaseStatus::Closed => "closed",
        }
        .into(),
        created_by: case.created_by().to_string(),
        created_at: case.created_at().to_rfc3339(),
        closed_by: case.closed_by().map(|value| value.to_string()),
        closed_at: case.closed_at().map(|value| value.to_rfc3339()),
    }
}

fn legal_hold_response(hold: LegalHold) -> LegalHoldResponse {
    LegalHoldResponse {
        legal_hold_id: hold.id().to_string(),
        organization_id: hold.organization_id().to_string(),
        case_id: hold.case_id().to_string(),
        archive_id: hold.archive_id().to_string(),
        reason: hold.reason().into(),
        status: match hold.status() {
            LegalHoldStatus::Active => "active",
            LegalHoldStatus::Released => "released",
        }
        .into(),
        apply_approval_request_id: hold.apply_approval_request_id().to_string(),
        created_by: hold.created_by().to_string(),
        created_at: hold.created_at().to_rfc3339(),
        release_approval_request_id: hold
            .release_approval_request_id()
            .map(|value| value.to_string()),
        released_by: hold.released_by().map(|value| value.to_string()),
        released_at: hold.released_at().map(|value| value.to_rfc3339()),
    }
}

fn audit_export_grant_response(grant: AuditExportGrant) -> AuditExportGrantResponse {
    AuditExportGrantResponse {
        audit_export_grant_id: grant.id().to_string(),
        organization_id: grant.organization_id().to_string(),
        archive_id: grant.archive_id().to_string(),
        approval_request_id: grant.approval_request_id().to_string(),
        requested_by: grant.requested_by().to_string(),
        created_at: grant.created_at().to_rfc3339(),
        expires_at: grant.expires_at().to_rfc3339(),
    }
}

fn retention_resource_kind_from_wire(value: &str) -> Option<RetentionResourceKind> {
    match value {
        "audit_evidence" => Some(RetentionResourceKind::AuditEvidence),
        "mail_content" => Some(RetentionResourceKind::MailContent),
        _ => None,
    }
}

fn retention_policy_version_response(
    version: RetentionPolicyVersion,
) -> RetentionPolicyVersionResponse {
    RetentionPolicyVersionResponse {
        retention_policy_version_id: version.id().to_string(),
        version: version.version(),
        minimum_retention_days: version.minimum_retention_days(),
        archive_after_days: version.archive_after_days(),
        delete_after_days: version.delete_after_days(),
        rule: version.rule().clone(),
        created_by: version.created_by().to_string(),
        created_at: version.created_at().to_rfc3339(),
    }
}

fn retention_policy_response(
    policy: RetentionPolicy,
    active_version: Option<RetentionPolicyVersion>,
    latest_version: RetentionPolicyVersion,
) -> RetentionPolicyResponse {
    RetentionPolicyResponse {
        retention_policy_id: policy.id().to_string(),
        organization_id: policy.organization_id().to_string(),
        policy_key: policy.policy_key().into(),
        display_name: policy.display_name().into(),
        resource_kind: match policy.resource_kind() {
            RetentionResourceKind::AuditEvidence => "audit_evidence",
            RetentionResourceKind::MailContent => "mail_content",
        }
        .into(),
        active_version: active_version.map(retention_policy_version_response),
        latest_version: retention_policy_version_response(latest_version),
        created_by: policy.created_by().to_string(),
        created_at: policy.created_at().to_rfc3339(),
    }
}

fn retention_policy_api_error(error: RetentionPolicyError, request_id: &str) -> Response {
    match error {
        RetentionPolicyError::Domain(_) => invalid_retention_request(request_id),
        RetentionPolicyError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "retention_policy_operation_forbidden",
            "the retention policy operation is not authorized",
            false,
            request_id,
        ),
        RetentionPolicyError::NotFound => resource_not_found(request_id),
        RetentionPolicyError::Conflict
        | RetentionPolicyError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "retention_policy_conflict",
            "the retention policy changed or the approval was already consumed",
            false,
            request_id,
        ),
        RetentionPolicyError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => retention_policy_store_unavailable(request_id),
    }
}

fn invalid_retention_request(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_retention_policy",
        "the retention policy violates its key, duration, scope, or rule constraints",
        false,
        request_id,
    )
}

fn retention_policy_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "retention_policy_store_unavailable",
        "retention policy state is temporarily unavailable",
        true,
        request_id,
    )
}

fn agent_mail_policy_api_error(error: AgentMailPolicyError, request_id: &str) -> Response {
    match error {
        AgentMailPolicyError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_mail_policy_activation_forbidden",
            "the exact live approval and active Human authorization are required",
            false,
            request_id,
        ),
        AgentMailPolicyError::NotFound => api_error(
            StatusCode::NOT_FOUND,
            "agent_mail_policy_version_not_found",
            "the policy version is unavailable",
            false,
            request_id,
        ),
        AgentMailPolicyError::Conflict => api_error(
            StatusCode::CONFLICT,
            "agent_mail_policy_activation_conflict",
            "the approval or active policy state conflicts with this request",
            false,
            request_id,
        ),
        AgentMailPolicyError::Store(_) => agent_mail_policy_store_unavailable(request_id),
        AgentMailPolicyError::Domain(_) => invalid_agent_mail_policy_request(request_id),
    }
}

fn agent_mail_policy_query_api_error(
    error: AgentMailPolicyQueryError,
    request_id: &str,
) -> Response {
    match error {
        AgentMailPolicyQueryError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_mail_policy_query_forbidden",
            "the authenticated Agent cannot inspect mail policy state",
            false,
            request_id,
        ),
        AgentMailPolicyQueryError::Unavailable | AgentMailPolicyQueryError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_mail_policy_query_unavailable",
            "Agent mail policy state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

const fn agent_environment_wire(environment: AgentEnvironment) -> &'static str {
    match environment {
        AgentEnvironment::Development => "development",
        AgentEnvironment::Testing => "testing",
        AgentEnvironment::Production => "production",
    }
}

fn invalid_agent_mail_policy_request(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_agent_mail_policy_request",
        "the Agent mail policy activation request is invalid",
        false,
        request_id,
    )
}

fn agent_mail_policy_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "agent_mail_policy_store_unavailable",
        "Agent mail policy persistence is unavailable",
        true,
        request_id,
    )
}

fn audit_export_authorization_api_error(
    error: AuditExportAuthorizationError,
    request_id: &str,
) -> Response {
    match error {
        AuditExportAuthorizationError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "audit_export_approval_required",
            "an exact unexpired dual approval by the requesting Human is required",
            false,
            request_id,
        ),
        AuditExportAuthorizationError::NotFound => resource_not_found(request_id),
        AuditExportAuthorizationError::Conflict
        | AuditExportAuthorizationError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "audit_export_authorization_conflict",
            "the approval was already consumed by a different export authorization",
            false,
            request_id,
        ),
        AuditExportAuthorizationError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_audit_export_authorization",
            "the export authorization violates its validity constraints",
            false,
            request_id,
        ),
        AuditExportAuthorizationError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => audit_export_store_unavailable(request_id),
    }
}

fn audit_export_download_api_error(error: AuditExportDownloadError, request_id: &str) -> Response {
    match error {
        AuditExportDownloadError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "audit_export_grant_required",
            "an active export grant held by the requesting Human is required",
            false,
            request_id,
        ),
        AuditExportDownloadError::NotFound => resource_not_found(request_id),
        AuditExportDownloadError::Conflict
        | AuditExportDownloadError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "audit_export_download_conflict",
            "the retrieved object does not match the recorded archive evidence",
            false,
            request_id,
        ),
        AuditExportDownloadError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_audit_export_download",
            "the export download violates its validity constraints",
            false,
            request_id,
        ),
        AuditExportDownloadError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => audit_export_store_unavailable(request_id),
    }
}

fn audit_export_object_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "audit_export_object_store_unavailable",
        "the immutable archive object could not be retrieved and verified",
        true,
        request_id,
    )
}

fn audit_export_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "audit_export_store_unavailable",
        "audit export authorization state is temporarily unavailable",
        true,
        request_id,
    )
}

fn compliance_api_error(
    error: copilotmail_application::ComplianceGovernanceError,
    request_id: &str,
) -> Response {
    match error {
        copilotmail_application::ComplianceGovernanceError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_compliance_request",
            "the compliance request violates a domain invariant",
            false,
            request_id,
        ),
        copilotmail_application::ComplianceGovernanceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "compliance_operation_forbidden",
            "the compliance operation is not authorized",
            false,
            request_id,
        ),
        copilotmail_application::ComplianceGovernanceError::NotFound => {
            resource_not_found(request_id)
        }
        copilotmail_application::ComplianceGovernanceError::Conflict
        | copilotmail_application::ComplianceGovernanceError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => legal_hold_conflict(request_id),
        copilotmail_application::ComplianceGovernanceError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => compliance_store_unavailable(request_id),
    }
}

fn legal_hold_conflict(request_id: &str) -> Response {
    api_error(
        StatusCode::CONFLICT,
        "legal_hold_conflict",
        "the compliance state, approval, or archive operation conflicts with this request",
        true,
        request_id,
    )
}

fn compliance_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "compliance_store_unavailable",
        "compliance state is temporarily unavailable",
        true,
        request_id,
    )
}

fn legal_hold_object_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "legal_hold_object_store_unavailable",
        "immutable archive verification is temporarily unavailable",
        true,
        request_id,
    )
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleAssignmentHttpQuery {
    principal_id: Option<PrincipalId>,
    include_inactive: Option<bool>,
    cursor: Option<String>,
    limit: Option<i64>,
}

async fn list_roles(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<RoleListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to list roles",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let roles = RoleGovernanceService::new(state.store.clone())
        .list_roles(ListRolesCommand {
            context: authorization,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    Ok(Json(RoleListResponse {
        items: roles.into_iter().map(role_response).collect(),
    }))
}

async fn list_role_assignments(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<RoleAssignmentHttpQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<RoleAssignmentListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to list role assignments",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let include_inactive = query.include_inactive.unwrap_or(false);
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(invalid_role_assignment_filter(&request_id));
    }
    let after_assignment_id = query
        .cursor
        .as_deref()
        .map(|cursor| {
            decode_role_assignment_cursor(
                cursor,
                organization_id,
                query.principal_id,
                include_inactive,
            )
        })
        .transpose()
        .map_err(|_| invalid_role_assignment_cursor(&request_id))?
        .map(RoleAssignmentId::from_uuid);
    let now = Utc::now();
    if let Some(after_assignment_id) = after_assignment_id
        && !state
            .store
            .role_assignment_cursor_exists(
                RoleAssignmentListQuery {
                    organization_id,
                    principal_id: query.principal_id,
                    include_inactive,
                    after_assignment_id: None,
                    limit,
                    now,
                },
                after_assignment_id,
            )
            .await
            .map_err(|_| role_governance_store_unavailable(&request_id))?
    {
        return Err(invalid_role_assignment_cursor(&request_id));
    }
    let page = RoleGovernanceService::new(state.store.clone())
        .list_assignments(ListRoleAssignmentsCommand {
            context: authorization,
            principal_id: query.principal_id,
            include_inactive,
            after_assignment_id,
            limit,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    let next_cursor = page
        .has_more
        .then(|| page.items.last())
        .flatten()
        .map(|item| {
            encode_role_assignment_cursor(
                organization_id,
                query.principal_id,
                include_inactive,
                item.assignment_id.as_uuid(),
            )
        });
    Ok(Json(RoleAssignmentListResponse {
        items: page
            .items
            .into_iter()
            .map(|assignment| role_assignment_response(assignment, now))
            .collect(),
        next_cursor,
        has_more: page.has_more,
    }))
}

async fn grant_role_assignment(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RoleAssignmentGrantRequest>,
) -> Result<(StatusCode, Json<RoleAssignmentResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to grant roles",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let principal_id = body
        .principal_id
        .parse::<PrincipalId>()
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let role_id = body
        .role_id
        .parse::<RoleId>()
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let valid_until = DateTime::parse_from_rfc3339(&body.valid_until)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let now = Utc::now();
    let resource_id = role_assignment_approval_resource_id(principal_id, role_id, valid_until)
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let grant = RoleGovernanceService::new(state.store.clone())
        .grant(GrantRoleAssignmentCommand {
            context: authorization.clone(),
            principal_id,
            role_id,
            valid_until,
            approval: ApprovalProof {
                organization_id,
                request_id: approval_request_id,
                requested_by: authorization.principal_id,
                action: ApprovalAction::PrivilegeElevation,
                resource_kind: "authorization.role_assignment".into(),
                resource_id,
                now,
            },
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    let assignment = state
        .store
        .get_role_assignment(organization_id, grant.assignment_id)
        .await
        .map_err(|_| role_governance_store_unavailable(&request_id))?
        .ok_or_else(|| role_governance_store_unavailable(&request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(role_assignment_response(assignment, now)),
    ))
}

async fn revoke_role_assignment(
    State(state): State<Arc<AppState>>,
    Path(role_assignment_id): Path<RoleAssignmentId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(_body): Json<RoleAssignmentRevocationRequest>,
) -> Result<Json<RoleAssignmentResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to revoke roles",
        &request_id,
    )?;
    let now = Utc::now();
    let assignment = RoleGovernanceService::new(state.store.clone())
        .revoke(RevokeRoleAssignmentCommand {
            context: authorization,
            assignment_id: role_assignment_id,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    Ok(Json(role_assignment_response(assignment, now)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRequestListQuery {
    view: Option<String>,
    status: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

async fn list_approval_requests(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<ApprovalRequestListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ApprovalRequestListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let view = match query.view.as_deref().unwrap_or("actionable") {
        "actionable" => ApprovalRequestView::Actionable,
        "requested" => ApprovalRequestView::Requested,
        _ => return Err(invalid_approval_list_filter(&request_id)),
    };
    if view == ApprovalRequestView::Actionable {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::AuthorizationManage,
            "authorization.manage permission is required to read the approval inbox",
            &request_id,
        )?;
    }
    let status = parse_approval_status_filter(query.status.as_deref())
        .ok_or_else(|| invalid_approval_list_filter(&request_id))?;
    if view == ApprovalRequestView::Actionable
        && status.is_some()
        && status != Some(ApprovalStatus::Pending)
    {
        return Err(invalid_approval_list_filter(&request_id));
    }
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(invalid_approval_list_filter(&request_id));
    }
    let after_request_id = query
        .cursor
        .as_deref()
        .map(|cursor| decode_approval_cursor(cursor, organization_id, view, status))
        .transpose()
        .map_err(|_| invalid_approval_cursor(&request_id))?
        .map(ApprovalRequestId::from_uuid);
    let now = Utc::now();
    if let Some(after_request_id) = after_request_id
        && !state
            .store
            .approval_request_cursor_exists(
                organization_id,
                view,
                authorization.principal_id,
                status,
                after_request_id,
                now,
            )
            .await
            .map_err(|_| approval_store_unavailable(&request_id))?
    {
        return Err(invalid_approval_cursor(&request_id));
    }
    let page = ApprovalWorkflowService::new(state.store.clone())
        .list(ListApprovalRequestsCommand {
            context: authorization,
            view,
            status,
            after_request_id,
            limit,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    let next_cursor = page
        .has_more
        .then(|| page.items.last())
        .flatten()
        .map(|item| {
            encode_approval_cursor(organization_id, view, status, item.request_id.as_uuid())
        });
    Ok(Json(ApprovalRequestListResponse {
        items: page
            .items
            .into_iter()
            .map(approval_request_summary_response)
            .collect(),
        next_cursor,
        has_more: page.has_more,
    }))
}

async fn get_approval_request(
    State(state): State<Arc<AppState>>,
    Path(approval_request_id): Path<ApprovalRequestId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ApprovalRequestDetailResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    let detail = ApprovalWorkflowService::new(state.store.clone())
        .get(GetApprovalRequestCommand {
            context: authorization,
            request_id: approval_request_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok(Json(approval_request_detail_response(detail)))
}

async fn cancel_approval_request(
    State(state): State<Arc<AppState>>,
    Path(approval_request_id): Path<ApprovalRequestId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(_body): Json<ApprovalCancellationRequest>,
) -> Result<Json<ApprovalDecisionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    let result = ApprovalWorkflowService::new(state.store.clone())
        .cancel(CancelApprovalRequestCommand {
            context: authorization,
            request_id: approval_request_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok(Json(ApprovalDecisionResponse {
        approval_request_id: result.request_id.to_string(),
        status: approval_status_wire(result.status).into(),
        approval_count: result.approval_count,
    }))
}

async fn create_approval_request(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<ApprovalRequestCreateRequest>,
) -> Result<(StatusCode, Json<ApprovalRequestResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    let Some(action) = approval_action_from_wire(&body.action) else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_approval_action",
            "the approval action is not supported",
            false,
            &request_id,
        ));
    };
    let required = approval_request_permission(action);
    enforce_permission_and_scope(
        &identity,
        &authorization,
        required,
        "the requested high-risk action is not permitted",
        &request_id,
    )?;
    let expires_at = chrono::DateTime::parse_from_rfc3339(&body.expires_at)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_expiration",
                "expires_at must be an RFC 3339 instant",
                false,
                &request_id,
            )
        })?;
    let approval_request_id = ApprovalWorkflowService::new(state.store.clone())
        .request(RequestApprovalCommand {
            context: authorization,
            policy_key: body.policy_key,
            action,
            resource_kind: body.resource_kind,
            resource_id: body.resource_id,
            reason: body.reason,
            ticket_reference: body.ticket_reference,
            expires_at,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(ApprovalRequestResponse {
            approval_request_id: approval_request_id.to_string(),
            status: approval_status_wire(ApprovalStatus::Pending).into(),
        }),
    ))
}

async fn decide_approval_request(
    State(state): State<Arc<AppState>>,
    Path(approval_request_id): Path<ApprovalRequestId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<ApprovalDecisionRequest>,
) -> Result<Json<ApprovalDecisionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to decide approvals",
        &request_id,
    )?;
    let decision = match body.decision.as_str() {
        "approved" => ApprovalDecision::Approved,
        "rejected" => ApprovalDecision::Rejected,
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "invalid_approval_decision",
                "decision must be approved or rejected",
                false,
                &request_id,
            ));
        }
    };
    let result = ApprovalWorkflowService::new(state.store.clone())
        .decide(DecideApprovalCommand {
            context: authorization,
            request_id: approval_request_id,
            decision,
            reason: body.reason,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok(Json(ApprovalDecisionResponse {
        approval_request_id: result.request_id.to_string(),
        status: approval_status_wire(result.status).into(),
        approval_count: result.approval_count,
    }))
}

async fn human_session_csrf_guard(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(runtime) = state.human_session.as_ref() else {
        return next.run(request).await;
    };
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing-request-id")
        .to_owned();
    if human_oidc_redirect_route(request.method(), request.uri().path()) {
        let authorization_present = request
            .headers()
            .get_all(header::AUTHORIZATION)
            .iter()
            .next()
            .is_some();
        if authorization_present
            && any_human_session_cookie_present(request.headers(), runtime.secure_cookie)
        {
            return ambiguous_authentication(&request_id);
        }
        if authorization_present && bearer_token(request.headers()).is_err() {
            return invalid_access_token(&request_id);
        }
        return next.run(request).await;
    }
    let (cookie_client, session_token) =
        match select_presented_credential(request.headers(), Some(runtime.secure_cookie)) {
            Ok(PresentedCredential::BrowserSession { client, token }) => (client, token),
            Ok(PresentedCredential::Bearer(_) | PresentedCredential::Missing) => {
                return next.run(request).await;
            }
            Err(CredentialSelectionError::Ambiguous) => {
                return ambiguous_authentication(&request_id);
            }
            Err(CredentialSelectionError::InvalidBrowserSession) => {
                return invalid_human_session_cookie(&request_id);
            }
            Err(CredentialSelectionError::InvalidBearer) => {
                return invalid_access_token(&request_id);
            }
        };
    if !human_client_route_permitted(cookie_client, request.method(), request.uri().path()) {
        return human_client_route_denied(&request_id);
    }
    let safe_method = matches!(
        request.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS
    );
    let record = match HumanSessionService::new(state.store.clone())
        .resolve_session(ResolveHumanSessionCommand {
            session_token,
            now: Utc::now(),
        })
        .await
    {
        Ok(record) => record,
        Err(error) => return human_session_api_error(error, &request_id),
    };
    if record.client_key != cookie_client {
        return invalid_human_session_cookie(&request_id);
    }
    let Some(client) = runtime.client(record.client_key.as_str()) else {
        return invalid_human_session_cookie(&request_id);
    };
    if safe_method {
        return next.run(request).await;
    }
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    let origin = origins.next().and_then(|value| value.to_str().ok());
    if origin != Some(client.public_origin.as_str()) || origins.next().is_some() {
        return api_error(
            StatusCode::FORBIDDEN,
            "csrf_origin_rejected",
            "the request Origin does not match the Human browser session",
            false,
            &request_id,
        );
    }
    next.run(request).await
}

fn human_oidc_redirect_route(method: &Method, path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    matches!(
        segments.as_slice(),
        ["api", "v2", "auth", client, action]
            if matches!(*client, "admin" | "mail")
                && matches!(*action, "login" | "callback")
                && (method == Method::GET || method == Method::HEAD)
    )
}

fn human_client_route_permitted(client: HumanSessionClient, method: &Method, path: &str) -> bool {
    if human_oidc_redirect_route(method, path) {
        return true;
    }
    if let Some(required_client) = human_session_api_route_client(method, path) {
        return client == required_client;
    }
    let mail_route = human_mail_route(method, path);
    match client {
        HumanSessionClient::Admin => !mail_route,
        HumanSessionClient::Mail => mail_route,
    }
}

fn human_session_api_route_client(method: &Method, path: &str) -> Option<HumanSessionClient> {
    let segments = exact_path_segments(path)?;
    match segments.as_slice() {
        ["api", "v2", "auth", client, "session"]
            if method == Method::GET || method == Method::HEAD =>
        {
            human_session_client_from_segment(client)
        }
        ["api", "v2", "auth", client, "logout"] if method == Method::POST => {
            human_session_client_from_segment(client)
        }
        _ => None,
    }
}

fn human_session_client_from_segment(value: &str) -> Option<HumanSessionClient> {
    match value {
        "admin" => Some(HumanSessionClient::Admin),
        "mail" => Some(HumanSessionClient::Mail),
        _ => None,
    }
}

fn human_mail_route(method: &Method, path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    match segments.as_slice() {
        ["api", "v2", "mailboxes"] => method == Method::GET || method == Method::HEAD,
        ["api", "v2", "mailboxes", _, "messages"] => {
            method == Method::GET || method == Method::HEAD
        }
        ["api", "v2", "mailboxes", _, "messages", "search"] => method == Method::POST,
        ["api", "v2", "mailboxes", _, "messages", _] => {
            method == Method::GET || method == Method::HEAD || method == Method::PATCH
        }
        ["api", "v2", "mailboxes", _, "drafts"] => {
            method == Method::GET || method == Method::HEAD || method == Method::POST
        }
        ["api", "v2", "mailboxes", _, "drafts", _, "restore"] => method == Method::POST,
        ["api", "v2", "mailboxes", _, "submissions"] => method == Method::POST,
        ["api", "v2", "mail-submissions", _] => method == Method::GET || method == Method::HEAD,
        _ => false,
    }
}

#[allow(clippy::result_large_err)]
fn require_human_mail_browser_session(
    headers: &HeaderMap,
    secure_session_cookies: bool,
    request_id: &str,
) -> Result<(), Response> {
    match select_presented_credential(headers, Some(secure_session_cookies)) {
        Ok(PresentedCredential::BrowserSession {
            client: HumanSessionClient::Mail,
            ..
        }) => Ok(()),
        Ok(PresentedCredential::BrowserSession {
            client: HumanSessionClient::Admin,
            ..
        }) => Err(human_client_route_denied(request_id)),
        Ok(PresentedCredential::Bearer(_) | PresentedCredential::Missing) => {
            Err(invalid_human_session_cookie(request_id))
        }
        Err(CredentialSelectionError::Ambiguous) => Err(ambiguous_authentication(request_id)),
        Err(
            CredentialSelectionError::InvalidBrowserSession
            | CredentialSelectionError::InvalidBearer,
        ) => Err(invalid_human_session_cookie(request_id)),
    }
}

async fn authenticate_human_mail_session_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<AuthorizationContext, Response> {
    let secure_session_cookies = state
        .human_session
        .as_ref()
        .map(|runtime| runtime.secure_cookie)
        .ok_or_else(|| invalid_human_session_cookie(request_id))?;
    require_human_mail_browser_session(headers, secure_session_cookies, request_id)?;
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(invalid_human_session_cookie(request_id));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        request_id,
    )?;
    Ok(authorization)
}

fn exact_path_segments(path: &str) -> Option<Vec<&str>> {
    let path = path.strip_prefix('/')?;
    let segments = path.split('/').collect::<Vec<_>>();
    (!segments.is_empty() && segments.iter().all(|segment| !segment.is_empty())).then_some(segments)
}

async fn human_auth_response_headers(request: Request, next: Next) -> Response {
    let sensitive_human_route = request.uri().path().starts_with("/api/v2/auth/")
        || human_mail_route(request.method(), request.uri().path());
    let mut response = next.run(request).await;
    if sensitive_human_route {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        response.headers_mut().insert(
            header::HeaderName::from_static("referrer-policy"),
            header::HeaderValue::from_static("no-referrer"),
        );
    }
    response
}

async fn authenticate_api_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let secure_session_cookies = state
        .human_session
        .as_ref()
        .map(|runtime| runtime.secure_cookie);
    let credential = select_presented_credential(headers, secure_session_cookies).map_err(
        |error| match error {
            CredentialSelectionError::Ambiguous => ambiguous_authentication(request_id),
            CredentialSelectionError::InvalidBrowserSession => {
                invalid_human_session_cookie(request_id)
            }
            CredentialSelectionError::InvalidBearer => invalid_access_token(request_id),
        },
    )?;
    let (identity, session_organization_id) = match credential {
        PresentedCredential::BrowserSession {
            client: cookie_client,
            token: session_token,
        } => {
            let runtime = state
                .human_session
                .as_ref()
                .ok_or_else(|| invalid_human_session_cookie(request_id))?;
            let session = HumanSessionService::new(state.store.clone())
                .resolve_session(ResolveHumanSessionCommand {
                    session_token,
                    now: Utc::now(),
                })
                .await
                .map_err(|error| human_session_api_error(error, request_id))?;
            if session.client_key != cookie_client
                || runtime.client(session.client_key.as_str()).is_none()
            {
                return Err(invalid_human_session_cookie(request_id));
            }
            (
                AuthenticatedIdentity {
                    principal_id: session.principal_id,
                    principal_kind: AuthenticatedPrincipalKind::Human,
                    credential_id: session.oidc_jti,
                    scope_restriction: None,
                },
                Some(session.organization_id),
            )
        }
        PresentedCredential::Bearer(token) => {
            let identity = state
                .authenticator
                .authenticate_with_context(
                    token,
                    &AuthenticationContext {
                        client_certificate_fingerprint: connection
                            .and_then(|info| info.client_certificate_fingerprint),
                    },
                )
                .await
                .map_err(|error| match error {
                    AuthenticationError::InvalidCredential => invalid_access_token(request_id),
                    AuthenticationError::Unavailable => api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "authentication_unavailable",
                        "trusted identity verification is not available",
                        true,
                        request_id,
                    ),
                })?;
            if identity.principal_kind == AuthenticatedPrincipalKind::Human
                && state.human_session_required
            {
                return Err(api_error(
                    StatusCode::UNAUTHORIZED,
                    "human_session_required",
                    "Human API access requires a server-side browser session",
                    false,
                    request_id,
                ));
            }
            (identity, None)
        }
        PresentedCredential::Missing => {
            return Err(api_error(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "a Human browser session or bearer access token is required",
                false,
                request_id,
            ));
        }
    };
    let authorization = state
        .store
        .resolve_authorization(identity.principal_id, Utc::now())
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "authorization could not be evaluated",
                true,
                request_id,
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::UNAUTHORIZED,
                "principal_inactive",
                "the authenticated principal is not active",
                false,
                request_id,
            )
        })?;
    if session_organization_id.is_some_and(|expected| expected != authorization.organization_id) {
        return Err(invalid_human_session_cookie(request_id));
    }
    Ok((identity, authorization))
}

#[allow(clippy::result_large_err)]
fn enforce_permission_and_scope(
    identity: &AuthenticatedIdentity,
    authorization: &AuthorizationContext,
    permission: Permission,
    message: &'static str,
    request_id: &str,
) -> Result<(), Response> {
    if !authorization.permits(authorization.organization_id, permission) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            message,
            false,
            request_id,
        ));
    }
    if identity
        .scope_restriction
        .as_ref()
        .is_some_and(|scopes| !scopes.contains(&permission))
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "insufficient_scope",
            "the access token does not include the required permission",
            false,
            request_id,
        ));
    }
    Ok(())
}

fn approval_request_permission(action: ApprovalAction) -> Permission {
    match action {
        ApprovalAction::SendAs | ApprovalAction::SendOnBehalfOf | ApprovalAction::BulkDelete => {
            Permission::MailboxUse
        }
        ApprovalAction::ContentReview => Permission::ContentReview,
        ApprovalAction::AuditExport => Permission::AuditExport,
        ApprovalAction::RetentionChange => Permission::RetentionManage,
        ApprovalAction::PrivilegeElevation => Permission::AuthorizationManage,
        ApprovalAction::ProductionAgentMail => Permission::AuthorizationManage,
        ApprovalAction::AgentFirstContact => Permission::AgentOperate,
        ApprovalAction::LegalHold => Permission::LegalHoldManage,
        ApprovalAction::BreakGlass => Permission::BreakGlass,
    }
}

fn webhook_subscription_response(
    subscription: WebhookSubscriptionSummary,
) -> WebhookSubscriptionResponse {
    WebhookSubscriptionResponse {
        subscription_id: subscription.subscription_id.to_string(),
        organization_id: subscription.organization_id.to_string(),
        principal_id: subscription.principal_id.to_string(),
        endpoint_url: subscription.endpoint_url,
        event_types: subscription
            .event_types
            .into_iter()
            .map(|event_type| event_type.as_str().to_owned())
            .collect(),
        signing_key_reference: subscription.signing_key_reference,
        enabled: subscription.enabled,
        version: subscription.version,
        created_at: subscription.created_at.to_rfc3339(),
        updated_at: subscription.updated_at.to_rfc3339(),
    }
}

fn webhook_dead_letter_response(
    dead_letter: WebhookDeadLetterSummary,
) -> WebhookDeadLetterResponse {
    WebhookDeadLetterResponse {
        delivery_id: dead_letter.delivery_id.to_string(),
        subscription_id: dead_letter.subscription_id.to_string(),
        principal_id: dead_letter.principal_id.to_string(),
        event_id: dead_letter.event_id.to_string(),
        event_type: dead_letter.event_type,
        attempt: dead_letter.attempt,
        response_status: dead_letter.response_status,
        error_code: dead_letter.error_code,
        failed_at: dead_letter.failed_at.to_rfc3339(),
    }
}

fn agent_event_envelope_response(event: AgentCursorEvent) -> AgentEventEnvelopeResponse {
    AgentEventEnvelopeResponse {
        id: event.event_id.to_string(),
        organization_id: event.organization_id.to_string(),
        event_type: event.event_type.as_str().into(),
        schema_version: event.schema_version,
        occurred_at: event.occurred_at.to_rfc3339(),
        aggregate: AgentEventAggregateResponse {
            aggregate_type: event.aggregate_type,
            id: event.aggregate_id,
        },
        data: event.payload,
    }
}

fn webhook_subscription_query_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "webhook_subscription_unavailable",
        "webhook subscription state is temporarily unavailable",
        true,
        request_id,
    )
}

fn agent_event_cursor_api_error(error: AgentEventCursorError, request_id: &str) -> Response {
    match error {
        AgentEventCursorError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied Agent event cursor access",
            false,
            request_id,
        ),
        AgentEventCursorError::NotFound => resource_not_found(request_id),
        AgentEventCursorError::InvalidCursor => api_error(
            StatusCode::GONE,
            "event_cursor_expired_or_invalid",
            "the event cursor is unavailable or is not part of this subscription",
            false,
            request_id,
        ),
        AgentEventCursorError::InvalidPageSize
        | AgentEventCursorError::InvalidQuota
        | AgentEventCursorError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_event_cursor_request",
            "the Agent event cursor request is invalid",
            false,
            request_id,
        ),
        AgentEventCursorError::TenantCapacityReached => api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "agent_event_stream_tenant_capacity_reached",
            "the tenant Agent event stream capacity is reached",
            true,
            request_id,
        ),
        AgentEventCursorError::PrincipalCapacityReached => api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "agent_event_stream_principal_capacity_reached",
            "the Agent event stream capacity is reached",
            true,
            request_id,
        ),
        AgentEventCursorError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => webhook_subscription_query_unavailable(request_id),
        AgentEventCursorError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "event_cursor_conflict",
            "the subscription event feed changed while the page was read",
            true,
            request_id,
        ),
    }
}

fn webhook_dead_letter_api_error(
    error: ReplayWebhookDeadLetterError,
    request_id: &str,
) -> Response {
    match error {
        ReplayWebhookDeadLetterError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_webhook_replay",
            "the webhook replay request is invalid",
            false,
            request_id,
        ),
        ReplayWebhookDeadLetterError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied manual webhook replay",
            false,
            request_id,
        ),
        ReplayWebhookDeadLetterError::NotReplayable => api_error(
            StatusCode::CONFLICT,
            "webhook_delivery_not_replayable",
            "the delivery is not the latest dead letter for an enabled subscription",
            false,
            request_id,
        ),
        ReplayWebhookDeadLetterError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => webhook_subscription_query_unavailable(request_id),
        ReplayWebhookDeadLetterError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "webhook_replay_conflict",
            "the delivery changed before replay could be scheduled",
            true,
            request_id,
        ),
    }
}

fn audit_event_search_api_error(error: AuditEventSearchError, request_id: &str) -> Response {
    match error {
        AuditEventSearchError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "audit_event_search_forbidden",
            "the authorization context denied audit event search",
            false,
            request_id,
        ),
        AuditEventSearchError::InvalidFilter | AuditEventSearchError::Domain(_) => {
            invalid_audit_event_query(request_id)
        }
        AuditEventSearchError::InvalidCursor => api_error(
            StatusCode::GONE,
            "audit_cursor_expired_or_invalid",
            "the audit cursor is unavailable or does not match the active filters",
            false,
            request_id,
        ),
        AuditEventSearchError::Store(copilotmail_application::StoreError::Conflict) => api_error(
            StatusCode::CONFLICT,
            "audit_event_search_conflict",
            "the audit event search conflicted with current state",
            true,
            request_id,
        ),
        AuditEventSearchError::Store(
            copilotmail_application::StoreError::MissingReference
            | copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "audit_event_search_unavailable",
            "audit event search is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn webhook_subscription_api_error(error: WebhookSubscriptionError, request_id: &str) -> Response {
    match error {
        WebhookSubscriptionError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_webhook_subscription",
            "the webhook endpoint, event types, key reference, or state transition is invalid",
            false,
            request_id,
        ),
        WebhookSubscriptionError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied the webhook subscription operation",
            false,
            request_id,
        ),
        WebhookSubscriptionError::NotFound
        | WebhookSubscriptionError::Store(copilotmail_application::StoreError::MissingReference) => {
            resource_not_found(request_id)
        }
        WebhookSubscriptionError::Conflict
        | WebhookSubscriptionError::Store(copilotmail_application::StoreError::Conflict) => {
            api_error(
                StatusCode::CONFLICT,
                "webhook_subscription_conflict",
                "the webhook subscription changed concurrently",
                true,
                request_id,
            )
        }
        WebhookSubscriptionError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => webhook_subscription_query_unavailable(request_id),
    }
}

fn mail_domain_api_error(error: MailDomainGovernanceError, request_id: &str) -> Response {
    match error {
        MailDomainGovernanceError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_domain_operation",
            "the mail domain input or lifecycle transition is invalid",
            false,
            request_id,
        ),
        MailDomainGovernanceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied the mail domain operation",
            false,
            request_id,
        ),
        MailDomainGovernanceError::NotFound
        | MailDomainGovernanceError::Store(copilotmail_application::StoreError::MissingReference) => {
            api_error(
                StatusCode::NOT_FOUND,
                "resource_not_found",
                "the requested mail domain does not exist",
                false,
                request_id,
            )
        }
        MailDomainGovernanceError::Conflict
        | MailDomainGovernanceError::Store(copilotmail_application::StoreError::Conflict) => {
            api_error(
                StatusCode::CONFLICT,
                "mail_domain_conflict",
                "the mail domain or route changed concurrently",
                true,
                request_id,
            )
        }
        MailDomainGovernanceError::DnsEvidenceRequired => api_error(
            StatusCode::CONFLICT,
            "dns_evidence_required",
            "fresh complete DNS evidence is required before activation",
            false,
            request_id,
        ),
        MailDomainGovernanceError::ChallengeRotationNotAllowed => api_error(
            StatusCode::CONFLICT,
            "challenge_rotation_not_allowed",
            "active domains must be suspended before challenge rotation; retired domains cannot rotate",
            false,
            request_id,
        ),
        MailDomainGovernanceError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_domain_unavailable",
            "mail domain state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn mail_domain_query_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "mail_domain_query_unavailable",
        "mail domain read state is temporarily unavailable",
        true,
        request_id,
    )
}

fn mail_domain_response(domain: MailDomainSummary) -> MailDomainResponse {
    MailDomainResponse {
        mail_domain_id: domain.mail_domain_id.to_string(),
        organization_id: domain.organization_id.to_string(),
        name: domain.name.to_string(),
        status: mail_domain_status_wire(domain.status).into(),
        route_mode: match domain.route_mode {
            MailDomainRouteMode::DirectMx => "direct_mx",
            MailDomainRouteMode::SmartHost => "smart_host",
        }
        .into(),
        ownership_challenge_version: domain.ownership_challenge_version,
        route_version: domain.route_version,
        inbound_enabled: domain.inbound_enabled,
        outbound_enabled: domain.outbound_enabled,
        smart_host_ref: domain.smart_host_ref,
        dkim_key_ref: domain.dkim_key_ref,
        mail_engine_domain_ref: domain.mail_engine_domain_ref,
        mail_engine_projection_status: match domain.mail_engine_projection_status {
            MailEngineProjectionStatus::NotProjected => "not_projected",
            MailEngineProjectionStatus::Pending => "pending",
            MailEngineProjectionStatus::Ready => "ready",
            MailEngineProjectionStatus::Failed => "failed",
        }
        .into(),
        mail_engine_projected_enabled: domain.mail_engine_projected_enabled,
        mail_engine_projection_last_attempt_at: domain
            .mail_engine_projection_last_attempt_at
            .map(|value| value.to_rfc3339()),
        mail_engine_projection_succeeded_at: domain
            .mail_engine_projection_succeeded_at
            .map(|value| value.to_rfc3339()),
        mail_engine_projection_error_code: domain.mail_engine_projection_error_code,
        latest_dns_observed_at: domain
            .latest_dns_observed_at
            .map(|value| value.to_rfc3339()),
        latest_dns_valid_until: domain
            .latest_dns_valid_until
            .map(|value| value.to_rfc3339()),
        latest_dns_activation_ready: domain.latest_dns_activation_ready,
        latest_dns_failure_codes: domain.latest_dns_failure_codes,
    }
}

fn dns_api_error(error: DnsError, request_id: &str) -> Response {
    match error {
        DnsError::InvalidConfiguration
        | DnsError::InvalidOwnershipToken
        | DnsError::InvalidAnswer
        | DnsError::RecordLimitExceeded
        | DnsError::ExpiredAnswer
        | DnsError::InvalidEvidence(_) => api_error(
            StatusCode::BAD_REQUEST,
            "dns_verification_invalid",
            "the DNS verification request or answer is invalid",
            false,
            request_id,
        ),
        DnsError::ResolverInitialization | DnsError::Unavailable | DnsError::Timeout => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "dns_verification_unavailable",
            "DNS verification is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

const fn mail_domain_status_wire(status: MailDomainStatus) -> &'static str {
    match status {
        MailDomainStatus::Draft => "draft",
        MailDomainStatus::PendingDns => "pending_dns",
        MailDomainStatus::Verified => "verified",
        MailDomainStatus::Active => "active",
        MailDomainStatus::Suspended => "suspended",
        MailDomainStatus::Retired => "retired",
    }
}

fn approval_api_error(error: ApprovalWorkflowError, request_id: &str) -> Response {
    match error {
        ApprovalWorkflowError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_approval",
            "the approval request or decision is invalid",
            false,
            request_id,
        ),
        ApprovalWorkflowError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the approval policy or authorization context denied the operation",
            false,
            request_id,
        ),
        ApprovalWorkflowError::Store(copilotmail_application::StoreError::Conflict) => api_error(
            StatusCode::CONFLICT,
            "approval_conflict",
            "the approval was already decided, expired, or violates separation of duty",
            false,
            request_id,
        ),
        ApprovalWorkflowError::Store(copilotmail_application::StoreError::MissingReference) => {
            api_error(
                StatusCode::NOT_FOUND,
                "resource_not_found",
                "the requested approval policy or request does not exist",
                false,
                request_id,
            )
        }
        ApprovalWorkflowError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "approval_unavailable",
            "approval state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn role_governance_api_error(error: RoleGovernanceError, request_id: &str) -> Response {
    match error {
        RoleGovernanceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "role_governance_forbidden",
            "the authorization context denied role governance",
            false,
            request_id,
        ),
        RoleGovernanceError::NotFound => resource_not_found(request_id),
        RoleGovernanceError::LastActiveOwner => api_error(
            StatusCode::CONFLICT,
            "last_active_owner",
            "the last effective organization owner cannot be revoked",
            false,
            request_id,
        ),
        RoleGovernanceError::Conflict
        | RoleGovernanceError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "role_assignment_conflict",
            "the role assignment conflicts with current governance state",
            true,
            request_id,
        ),
        RoleGovernanceError::Domain(_) => invalid_role_assignment_request(request_id),
        RoleGovernanceError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => role_governance_store_unavailable(request_id),
    }
}

fn invalid_role_assignment_request(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_role_assignment",
        "the role assignment request is invalid",
        false,
        request_id,
    )
}

fn invalid_role_assignment_filter(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_role_assignment_filter",
        "the role assignment filters are invalid",
        false,
        request_id,
    )
}

fn invalid_role_assignment_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::GONE,
        "role_assignment_cursor_expired",
        "the role assignment cursor is invalid or no longer visible",
        false,
        request_id,
    )
}

fn role_governance_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "role_governance_unavailable",
        "role governance is temporarily unavailable",
        true,
        request_id,
    )
}

const fn approval_status_wire(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
        ApprovalStatus::Expired => "expired",
        ApprovalStatus::Cancelled => "cancelled",
    }
}

fn role_response(role: RoleSummary) -> RoleResponse {
    RoleResponse {
        role_id: role.role_id.to_string(),
        organization_id: role.organization_id.to_string(),
        system_role: role.system_role.as_str().into(),
        display_name: role.display_name,
        permissions: role
            .permissions
            .into_iter()
            .map(|permission| permission.as_str().to_owned())
            .collect(),
        created_at: role.created_at.to_rfc3339(),
    }
}

fn role_assignment_response(
    assignment: RoleAssignmentSummary,
    now: DateTime<Utc>,
) -> RoleAssignmentResponse {
    let active = assignment.is_active_at(now);
    RoleAssignmentResponse {
        role_assignment_id: assignment.assignment_id.to_string(),
        organization_id: assignment.organization_id.to_string(),
        principal_id: assignment.principal_id.to_string(),
        principal_kind: principal_kind_wire(assignment.principal_kind).into(),
        principal_status: principal_status_wire(assignment.principal_status).into(),
        principal_display_name: assignment.principal_display_name,
        role_id: assignment.role_id.to_string(),
        system_role: assignment.system_role.as_str().into(),
        role_display_name: assignment.role_display_name,
        created_by: assignment.created_by.to_string(),
        approval_request_id: assignment
            .approval_request_id
            .map(|approval_id| approval_id.to_string()),
        valid_from: assignment.valid_from.to_rfc3339(),
        valid_until: assignment.valid_until.map(|value| value.to_rfc3339()),
        revoked_at: assignment.revoked_at.map(|value| value.to_rfc3339()),
        created_at: assignment.created_at.to_rfc3339(),
        active,
    }
}

const fn principal_kind_wire(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Human => "human",
        PrincipalKind::Agent => "agent",
        PrincipalKind::Service => "service",
    }
}

const fn principal_status_wire(status: PrincipalStatus) -> &'static str {
    match status {
        PrincipalStatus::Draft => "draft",
        PrincipalStatus::Active => "active",
        PrincipalStatus::Suspended => "suspended",
        PrincipalStatus::Retired => "retired",
    }
}

fn encode_role_assignment_cursor(
    organization_id: OrganizationId,
    principal_id: Option<PrincipalId>,
    include_inactive: bool,
    id: Uuid,
) -> String {
    format!(
        "v1.role-assignment.{}.{}.{}.{}",
        principal_id
            .map(|principal_id| principal_id.to_string())
            .unwrap_or_else(|| "all".into()),
        if include_inactive { "all" } else { "active" },
        organization_id,
        id
    )
}

fn decode_role_assignment_cursor(
    cursor: &str,
    organization_id: OrganizationId,
    principal_id: Option<PrincipalId>,
    include_inactive: bool,
) -> Result<Uuid, ()> {
    let mut parts = cursor.split('.');
    let expected_principal = principal_id
        .map(|principal_id| principal_id.to_string())
        .unwrap_or_else(|| "all".into());
    if parts.next() != Some("v1")
        || parts.next() != Some("role-assignment")
        || parts.next() != Some(expected_principal.as_str())
        || parts.next() != Some(if include_inactive { "all" } else { "active" })
        || parts
            .next()
            .and_then(|value| value.parse::<OrganizationId>().ok())
            != Some(organization_id)
    {
        return Err(());
    }
    let id = parts.next().and_then(|value| Uuid::parse_str(value).ok());
    if parts.next().is_some() {
        return Err(());
    }
    id.ok_or(())
}

const fn approval_view_wire(view: ApprovalRequestView) -> &'static str {
    match view {
        ApprovalRequestView::Requested => "requested",
        ApprovalRequestView::Actionable => "actionable",
    }
}

fn parse_approval_status_filter(value: Option<&str>) -> Option<Option<ApprovalStatus>> {
    match value {
        None => Some(None),
        Some("pending") => Some(Some(ApprovalStatus::Pending)),
        Some("approved") => Some(Some(ApprovalStatus::Approved)),
        Some("rejected") => Some(Some(ApprovalStatus::Rejected)),
        Some("expired") => Some(Some(ApprovalStatus::Expired)),
        Some("cancelled") => Some(Some(ApprovalStatus::Cancelled)),
        Some(_) => None,
    }
}

fn encode_approval_cursor(
    organization_id: OrganizationId,
    view: ApprovalRequestView,
    status: Option<ApprovalStatus>,
    id: Uuid,
) -> String {
    format!(
        "v1.approval.{}.{}.{}.{}",
        approval_view_wire(view),
        status.map(approval_status_wire).unwrap_or("all"),
        organization_id,
        id
    )
}

fn decode_approval_cursor(
    cursor: &str,
    organization_id: OrganizationId,
    view: ApprovalRequestView,
    status: Option<ApprovalStatus>,
) -> Result<Uuid, ()> {
    let mut parts = cursor.split('.');
    if parts.next() != Some("v1")
        || parts.next() != Some("approval")
        || parts.next() != Some(approval_view_wire(view))
        || parts.next() != Some(status.map(approval_status_wire).unwrap_or("all"))
        || parts
            .next()
            .and_then(|value| value.parse::<OrganizationId>().ok())
            != Some(organization_id)
    {
        return Err(());
    }
    let id = parts.next().and_then(|value| Uuid::parse_str(value).ok());
    if parts.next().is_some() {
        return Err(());
    }
    id.ok_or(())
}

fn approval_request_summary_response(
    request: ApprovalRequestSummary,
) -> ApprovalRequestSummaryResponse {
    ApprovalRequestSummaryResponse {
        approval_request_id: request.request_id.to_string(),
        organization_id: request.organization_id.to_string(),
        policy_version_id: request.policy_version_id.to_string(),
        action: request.action.as_str().into(),
        resource_kind: request.resource_kind,
        resource_id: request.resource_id,
        requested_by: request.requested_by.to_string(),
        reason: request.reason,
        ticket_reference: request.ticket_reference,
        required_approvals: request.required_approvals,
        approval_count: request.approval_count,
        rejection_count: request.rejection_count,
        status: approval_status_wire(request.status).into(),
        created_at: request.created_at.to_rfc3339(),
        expires_at: request.expires_at.to_rfc3339(),
        completed_at: request.completed_at.map(|value| value.to_rfc3339()),
        cancelled_at: request.cancelled_at.map(|value| value.to_rfc3339()),
    }
}

fn approval_request_detail_response(
    detail: ApprovalRequestDetail,
) -> ApprovalRequestDetailResponse {
    ApprovalRequestDetailResponse {
        request: approval_request_summary_response(detail.request),
        steps: detail
            .steps
            .into_iter()
            .map(|step| ApprovalStepResponse {
                approval_step_id: step.step_id.to_string(),
                approver_principal_id: step.approver_principal_id.to_string(),
                decision: match step.decision {
                    ApprovalDecision::Approved => "approved",
                    ApprovalDecision::Rejected => "rejected",
                }
                .into(),
                reason: step.reason,
                decided_at: step.decided_at.to_rfc3339(),
            })
            .collect(),
    }
}

fn invalid_approval_list_filter(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_approval_list_filter",
        "view, status, or limit is invalid",
        false,
        request_id,
    )
}

fn invalid_approval_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::GONE,
        "approval_cursor_expired_or_invalid",
        "the cursor is unavailable or does not match the tenant and active filters",
        false,
        request_id,
    )
}

fn approval_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "approval_unavailable",
        "approval state is temporarily unavailable",
        true,
        request_id,
    )
}

async fn get_organization(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<OrganizationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;

    if authorization.organization_id != organization_id {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "resource_not_found",
            "the requested organization does not exist",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::OrganizationRead,
        "organization.read permission is required",
        &request_id,
    )?;

    let organization = state
        .store
        .get_organization(organization_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "organization_store_unavailable",
                "organization data is temporarily unavailable",
                true,
                &request_id,
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "resource_not_found",
                "the requested organization does not exist",
                false,
                &request_id,
            )
        })?;

    Ok(Json(OrganizationResponse {
        organization_id: organization.organization_id.to_string(),
        slug: organization.slug,
        display_name: organization.display_name,
    }))
}

async fn not_found(
    Extension(request_id): Extension<RequestId>,
) -> (StatusCode, Json<copilotmail_contracts::ErrorResponse>) {
    let request_id = request_id_value(&request_id);
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            code: "route_not_found".into(),
            message: "the requested API route does not exist".into(),
            request_id: request_id.clone(),
            trace_id: request_id,
            retryable: false,
            details: serde_json::Value::Null,
        }),
    )
}

enum PresentedCredential<'a> {
    BrowserSession {
        client: HumanSessionClient,
        token: String,
    },
    Bearer(&'a str),
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialSelectionError {
    Ambiguous,
    InvalidBrowserSession,
    InvalidBearer,
}

fn select_presented_credential<'a>(
    headers: &'a HeaderMap,
    secure_session_cookies: Option<bool>,
) -> Result<PresentedCredential<'a>, CredentialSelectionError> {
    let authorization_present = headers
        .get_all(header::AUTHORIZATION)
        .iter()
        .next()
        .is_some();
    let browser_cookie_present = secure_session_cookies.is_some_and(|secure| {
        [HumanSessionClient::Admin, HumanSessionClient::Mail]
            .into_iter()
            .any(|client| {
                browser_cookie_name_present(headers, human_session_cookie_name(client, secure))
            })
    });
    if browser_cookie_present && authorization_present {
        return Err(CredentialSelectionError::Ambiguous);
    }

    let mut browser_session = None;
    if let Some(secure) = secure_session_cookies {
        for client in [HumanSessionClient::Admin, HumanSessionClient::Mail] {
            let name = human_session_cookie_name(client, secure);
            let token = browser_cookie_value(headers, name)
                .map_err(|()| CredentialSelectionError::InvalidBrowserSession)?;
            if let Some(token) = token {
                if browser_session.is_some() {
                    return Err(CredentialSelectionError::Ambiguous);
                }
                browser_session = Some((client, token));
            }
        }
    }
    if let Some((client, token)) = browser_session {
        return Ok(PresentedCredential::BrowserSession { client, token });
    }

    match bearer_token(headers).map_err(|()| CredentialSelectionError::InvalidBearer)? {
        Some(token) => Ok(PresentedCredential::Bearer(token)),
        None => Ok(PresentedCredential::Missing),
    }
}

fn select_human_browser_session(
    headers: &HeaderMap,
    secure_session_cookies: bool,
) -> Result<(HumanSessionClient, String), CredentialSelectionError> {
    match select_presented_credential(headers, Some(secure_session_cookies))? {
        PresentedCredential::BrowserSession { client, token } => Ok((client, token)),
        PresentedCredential::Bearer(_) | PresentedCredential::Missing => {
            Err(CredentialSelectionError::InvalidBrowserSession)
        }
    }
}

fn any_human_session_cookie_present(headers: &HeaderMap, secure_session_cookies: bool) -> bool {
    [HumanSessionClient::Admin, HumanSessionClient::Mail]
        .into_iter()
        .any(|client| {
            browser_cookie_name_present(
                headers,
                human_session_cookie_name(client, secure_session_cookies),
            )
        })
}

fn bearer_token(headers: &HeaderMap) -> Result<Option<&str>, ()> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let value = value.to_str().map_err(|_| ())?;
    let (scheme, token) = value.split_once(' ').ok_or(())?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(());
    }
    Ok(Some(token))
}

fn browser_cookie_name_present(headers: &HeaderMap, name: &str) -> bool {
    headers.get_all(header::COOKIE).iter().any(|header_value| {
        header_value.to_str().ok().is_some_and(|value| {
            value.split(';').any(|pair| {
                let pair = pair.trim();
                pair == name
                    || pair
                        .strip_prefix(name)
                        .is_some_and(|suffix| suffix.starts_with('='))
            })
        })
    })
}

fn browser_cookie_value(headers: &HeaderMap, name: &str) -> Result<Option<String>, ()> {
    let mut found = None;
    for header_value in headers.get_all(header::COOKIE) {
        let value = header_value.to_str().map_err(|_| ())?;
        if value.len() > 8 * 1024 {
            return Err(());
        }
        for pair in value.split(';') {
            let Some((candidate_name, candidate_value)) = pair.trim().split_once('=') else {
                return Err(());
            };
            if candidate_name == name {
                if found.is_some()
                    || candidate_value.len() != 43
                    || !candidate_value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                    || URL_SAFE_NO_PAD
                        .decode(candidate_value)
                        .ok()
                        .filter(|decoded| decoded.len() == 32)
                        .is_none()
                {
                    return Err(());
                }
                found = Some(candidate_value.to_owned());
            }
        }
    }
    Ok(found)
}

fn browser_cookie_header(
    name: &str,
    value: &str,
    maximum_age_seconds: i64,
    secure: bool,
) -> Result<header::HeaderValue> {
    let same_site = match name {
        "__Host-copilotmail-admin-login"
        | "__Host-copilotmail-mail-login"
        | "copilotmail-admin-login"
        | "copilotmail-mail-login" => "Lax",
        "__Host-copilotmail-admin-session"
        | "__Host-copilotmail-mail-session"
        | "copilotmail-admin-session"
        | "copilotmail-mail-session" => "Strict",
        _ => anyhow::bail!("browser cookie name is invalid"),
    };
    anyhow::ensure!(
        value.is_empty()
            || value.len() == 43
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "browser cookie value is invalid"
    );
    anyhow::ensure!(
        (0..=28_800).contains(&maximum_age_seconds),
        "browser cookie lifetime is invalid"
    );
    let secure_attribute = if secure { "; Secure" } else { "" };
    let value = format!(
        "{name}={value}; Path=/; HttpOnly{secure_attribute}; SameSite={same_site}; Max-Age={maximum_age_seconds}"
    );
    header::HeaderValue::from_str(&value).context("build browser cookie")
}

fn clear_browser_cookie_header(name: &str, secure: bool) -> Result<header::HeaderValue> {
    browser_cookie_header(name, "", 0, secure)
}

fn browser_session_cookie_headers(
    client: HumanSessionClient,
    value: &str,
    maximum_age_seconds: i64,
    secure: bool,
) -> Result<[header::HeaderValue; 2]> {
    Ok([
        browser_cookie_header(
            human_session_cookie_name(client, secure),
            value,
            maximum_age_seconds,
            secure,
        )?,
        clear_browser_cookie_header(
            human_session_cookie_name(other_human_session_client(client), secure),
            secure,
        )?,
    ])
}

fn datetime_from_unix_seconds(value: u64) -> Option<DateTime<Utc>> {
    i64::try_from(value)
        .ok()
        .and_then(|value| DateTime::from_timestamp(value, 0))
}

fn unix_seconds_from_datetime(value: DateTime<Utc>) -> Option<u64> {
    u64::try_from(value.timestamp()).ok()
}

fn request_id_value(request_id: &RequestId) -> String {
    request_id
        .header_value()
        .to_str()
        .unwrap_or("invalid-request-id")
        .to_owned()
}

fn ambiguous_authentication(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "ambiguous_authentication",
        "send either a Human browser session or a bearer credential, not both",
        false,
        request_id,
    )
}

fn invalid_access_token(request_id: &str) -> Response {
    api_error(
        StatusCode::UNAUTHORIZED,
        "invalid_access_token",
        "the bearer access token is invalid",
        false,
        request_id,
    )
}

fn credential_selection_api_error(error: CredentialSelectionError, request_id: &str) -> Response {
    match error {
        CredentialSelectionError::Ambiguous => ambiguous_authentication(request_id),
        CredentialSelectionError::InvalidBrowserSession => invalid_human_session_cookie(request_id),
        CredentialSelectionError::InvalidBearer => invalid_access_token(request_id),
    }
}

fn human_client_route_denied(request_id: &str) -> Response {
    api_error(
        StatusCode::FORBIDDEN,
        "human_client_route_denied",
        "the Human browser client is not permitted to access this route",
        false,
        request_id,
    )
}

fn invalid_human_session_cookie(request_id: &str) -> Response {
    let mut response = api_error(
        StatusCode::UNAUTHORIZED,
        "invalid_human_session",
        "the Human browser session is missing, expired, or invalid",
        false,
        request_id,
    );
    response.headers_mut().remove(header::WWW_AUTHENTICATE);
    response
}

fn human_session_api_error(error: HumanSessionError, request_id: &str) -> Response {
    match error {
        HumanSessionError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_session_request",
            "the Human login or session request is invalid",
            false,
            request_id,
        ),
        HumanSessionError::NotFound => invalid_human_session_cookie(request_id),
        HumanSessionError::Conflict => api_error(
            StatusCode::CONFLICT,
            "human_session_conflict",
            "the Human login transaction or session was already consumed",
            false,
            request_id,
        ),
        HumanSessionError::RateLimited {
            retry_after_seconds,
        } => {
            let retry_after_seconds = retry_after_seconds.clamp(1, 60);
            let mut response = api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "human_login_rate_limited",
                "too many Human login attempts",
                true,
                request_id,
            );
            if let Ok(value) = header::HeaderValue::from_str(&retry_after_seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            response
        }
        HumanSessionError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser session persistence is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

fn oidc_flow_api_error(error: OidcFlowError, request_id: &str) -> Response {
    match error {
        OidcFlowError::ProviderUnavailable | OidcFlowError::EntropyUnavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "oidc_login_unavailable",
            "the enterprise identity provider is temporarily unavailable",
            true,
            request_id,
        ),
        OidcFlowError::InvalidAuthorizationResponse
        | OidcFlowError::AuthorizationRejected
        | OidcFlowError::TokenEndpointRejected
        | OidcFlowError::InvalidTokenResponse
        | OidcFlowError::InvalidAccessToken => {
            let mut response = api_error(
                StatusCode::UNAUTHORIZED,
                "oidc_login_rejected",
                "the enterprise identity provider rejected the login",
                false,
                request_id,
            );
            response.headers_mut().remove(header::WWW_AUTHENTICATE);
            response
        }
    }
}

fn api_error(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
    request_id: &str,
) -> Response {
    api_error_with_details(
        status,
        code,
        message,
        retryable,
        serde_json::Value::Null,
        request_id,
    )
}

fn api_error_with_details(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
    details: serde_json::Value,
    request_id: &str,
) -> Response {
    let mut response = (
        status,
        Json(ErrorResponse {
            code: code.into(),
            message: message.into(),
            request_id: request_id.into(),
            trace_id: request_id.into(),
            retryable,
            details,
        }),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
    }
    response
}

/// The operating-system event that requested a graceful process shutdown.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShutdownReason {
    CtrlC,
    Terminate,
}

impl ShutdownReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CtrlC => "ctrl_c",
            Self::Terminate => "sigterm",
        }
    }
}

/// Resolves with the reason corresponding to whichever signal future completes
/// first.
///
/// Keeping the selection independent from process-global signal registration
/// lets unit tests verify the semantics without sending a signal to the test
/// runner.
async fn select_shutdown_reason<C, T>(ctrl_c: C, terminate: T) -> std::io::Result<ShutdownReason>
where
    C: Future<Output = std::io::Result<()>>,
    T: Future<Output = std::io::Result<()>>,
{
    tokio::select! {
        result = ctrl_c => result.map(|()| ShutdownReason::CtrlC),
        result = terminate => result.map(|()| ShutdownReason::Terminate),
    }
}

/// Waits for Ctrl+C or SIGTERM on Unix hosts.
#[cfg(unix)]
async fn wait_for_shutdown_signal() -> std::io::Result<ShutdownReason> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let terminate = async move {
        terminate.recv().await.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "SIGTERM signal stream closed",
            )
        })
    };
    select_shutdown_reason(tokio::signal::ctrl_c(), terminate).await
}

/// Waits for Ctrl+C on Windows and other non-Unix hosts.
#[cfg(not(unix))]
async fn wait_for_shutdown_signal() -> std::io::Result<ShutdownReason> {
    select_shutdown_reason(
        tokio::signal::ctrl_c(),
        std::future::pending::<std::io::Result<()>>(),
    )
    .await
}

/// Completes when the platform's graceful-shutdown signal is received.
///
/// Unix services accept both SIGTERM (the normal container-orchestrator stop
/// signal) and Ctrl+C. Windows services retain Ctrl+C handling. Failure to
/// install or receive a handler is logged and completes this future so callers
/// shut down instead of continuing without a termination path.
async fn shutdown_signal() {
    match wait_for_shutdown_signal().await {
        Ok(reason) => {
            tracing::info!(
                shutdown_reason = reason.as_str(),
                "graceful shutdown signal received"
            );
        }
        Err(error) => {
            tracing::error!(%error, "failed to install or receive shutdown signal");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use opentelemetry::{propagation::TextMapPropagator, trace::TraceContextExt};
    use opentelemetry_sdk::propagation::TraceContextPropagator;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    #[test]
    fn fingerprint_compatibility_is_selected_only_for_the_fixed_legacy_plane() {
        let legacy = MailDataPlaneId::from_uuid(Uuid::from_u128(LEGACY_MAIL_DATA_PLANE_ID));
        let other =
            MailDataPlaneId::from_uuid(Uuid::from_u128(0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2102));

        assert!(matches!(
            mail_event_fingerprint_scheme(legacy).unwrap(),
            MailEventFingerprintScheme::LegacySource(_)
        ));
        assert_eq!(
            mail_event_fingerprint_scheme(other).unwrap(),
            MailEventFingerprintScheme::DataPlaneV2
        );
        assert_eq!(
            mail_data_plane_fingerprint_scheme(legacy),
            MailDataPlaneFingerprintScheme::LegacyV1
        );
        assert_eq!(
            mail_data_plane_fingerprint_scheme(other),
            MailDataPlaneFingerprintScheme::DataPlaneV2
        );
    }

    #[tokio::test]
    async fn shutdown_signal_selector_reports_ctrl_c_without_process_signal() {
        let reason = select_shutdown_reason(
            std::future::ready(Ok(())),
            std::future::pending::<std::io::Result<()>>(),
        )
        .await
        .unwrap();

        assert_eq!(reason, ShutdownReason::CtrlC);
        assert_eq!(reason.as_str(), "ctrl_c");
    }

    #[tokio::test]
    async fn shutdown_signal_selector_reports_sigterm_without_process_signal() {
        let reason = select_shutdown_reason(
            std::future::pending::<std::io::Result<()>>(),
            std::future::ready(Ok(())),
        )
        .await
        .unwrap();

        assert_eq!(reason, ShutdownReason::Terminate);
        assert_eq!(reason.as_str(), "sigterm");
    }

    #[tokio::test]
    async fn pinned_shutdown_signal_survives_a_work_branch_winning_first() {
        let (signal_sender, signal_receiver) = tokio::sync::oneshot::channel::<()>();
        let simulated_ctrl_c = async move {
            signal_receiver.await.map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "simulated signal sender closed",
                )
            })
        };
        let shutdown = select_shutdown_reason(
            simulated_ctrl_c,
            std::future::pending::<std::io::Result<()>>(),
        );
        tokio::pin!(shutdown);

        let work_branch_won = tokio::select! {
            _ = &mut shutdown => false,
            _ = std::future::ready(()) => true,
        };
        assert!(work_branch_won);

        signal_sender.send(()).unwrap();
        let reason = tokio::select! {
            result = &mut shutdown => result.unwrap(),
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                panic!("pinned shutdown future did not complete in a later select")
            }
        };
        assert_eq!(reason, ShutdownReason::CtrlC);
    }

    #[tokio::test]
    async fn liveness_does_not_depend_on_postgresql() {
        let response = health().await.0;
        assert_eq!(response.status, "ok");
        assert_eq!(response.service, SERVICE);
    }

    #[tokio::test]
    async fn system_info_declares_v2_principal_model() {
        let response = system_info().await.0;
        assert_eq!(response.api_version, "v2");
        assert_eq!(response.principal_model, "human_agent_service");
    }

    #[test]
    fn worker_error_backoff_is_exponential_bounded_and_resettable() {
        assert_eq!(worker_error_backoff(0), Duration::ZERO);
        assert_eq!(worker_error_backoff(1), Duration::from_secs(1));
        assert_eq!(worker_error_backoff(2), Duration::from_secs(2));
        assert_eq!(worker_error_backoff(5), Duration::from_secs(16));
        assert_eq!(worker_error_backoff(6), WORKER_ERROR_BACKOFF_MAX);
        assert_eq!(worker_error_backoff(u32::MAX), WORKER_ERROR_BACKOFF_MAX);
    }

    #[test]
    fn pkce_protection_is_authenticated_and_state_bound() {
        let protector = PkceProtector::new(&[7; 32]).unwrap();
        let verifier = "v".repeat(64);
        let state_sha256 = [3; 32];
        let protected = protector.protect(&verifier, &state_sha256).unwrap();
        assert_ne!(&protected[12..protected.len() - 16], verifier.as_bytes());
        assert_eq!(
            protector.unprotect(&protected, &state_sha256).unwrap(),
            verifier
        );

        let mut tampered = protected.clone();
        tampered[20] ^= 1;
        assert!(protector.unprotect(&tampered, &state_sha256).is_err());
        assert!(protector.unprotect(&protected, &[4; 32]).is_err());
    }

    #[test]
    fn browser_cookies_are_host_only_http_only_and_ambiguity_rejecting() {
        let token = URL_SAFE_NO_PAD.encode([9; 32]);
        assert_eq!(
            human_login_cookie_name(HumanSessionClient::Admin, true),
            "__Host-copilotmail-admin-login"
        );
        assert_eq!(
            human_session_cookie_name(HumanSessionClient::Mail, false),
            "copilotmail-mail-session"
        );
        let session_name = human_session_cookie_name(HumanSessionClient::Admin, true);
        let session_cookie = browser_cookie_header(session_name, &token, 900, true).unwrap();
        let session_cookie = session_cookie.to_str().unwrap();
        assert!(
            session_cookie.contains("; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=900")
        );
        assert!(!session_cookie.to_ascii_lowercase().contains("domain="));

        let login_name = human_login_cookie_name(HumanSessionClient::Admin, true);
        let login_cookie = browser_cookie_header(login_name, &token, 600, true).unwrap();
        let login_cookie = login_cookie.to_str().unwrap();
        assert!(login_cookie.contains("; Secure; SameSite=Lax; Max-Age=600"));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("theme=dark; {session_name}={token}")
                .parse()
                .unwrap(),
        );
        assert_eq!(
            browser_cookie_value(&headers, session_name).unwrap(),
            Some(token.clone())
        );
        headers.append(
            header::COOKIE,
            format!("{session_name}={token}").parse().unwrap(),
        );
        assert!(browser_cookie_value(&headers, session_name).is_err());
    }

    #[test]
    fn human_session_cookies_and_routes_are_strictly_client_partitioned() {
        let admin_token = URL_SAFE_NO_PAD.encode([7; 32]);
        let mail_token = URL_SAFE_NO_PAD.encode([8; 32]);
        let admin_name = human_session_cookie_name(HumanSessionClient::Admin, true);
        let mail_name = human_session_cookie_name(HumanSessionClient::Mail, true);

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{admin_name}={admin_token}").parse().unwrap(),
        );
        assert!(matches!(
            select_presented_credential(&headers, Some(true)),
            Ok(PresentedCredential::BrowserSession {
                client: HumanSessionClient::Admin,
                ..
            })
        ));

        headers.insert(
            header::COOKIE,
            format!("{admin_name}={admin_token}; {mail_name}={mail_token}")
                .parse()
                .unwrap(),
        );
        assert!(matches!(
            select_presented_credential(&headers, Some(true)),
            Err(CredentialSelectionError::Ambiguous)
        ));

        let mail_routes = [
            (Method::GET, "/api/v2/mailboxes"),
            (Method::GET, "/api/v2/mailboxes/m1/messages"),
            (Method::POST, "/api/v2/mailboxes/m1/messages/search"),
            (Method::GET, "/api/v2/mailboxes/m1/messages/msg1"),
            (Method::PATCH, "/api/v2/mailboxes/m1/messages/msg1"),
            (Method::GET, "/api/v2/mailboxes/m1/drafts"),
            (Method::POST, "/api/v2/mailboxes/m1/drafts"),
            (Method::POST, "/api/v2/mailboxes/m1/drafts/d1/restore"),
            (Method::POST, "/api/v2/mailboxes/m1/submissions"),
            (Method::GET, "/api/v2/mail-submissions/s1"),
        ];
        for (method, path) in mail_routes {
            assert!(human_client_route_permitted(
                HumanSessionClient::Mail,
                &method,
                path
            ));
            assert!(
                !human_client_route_permitted(HumanSessionClient::Admin, &method, path),
                "Admin browser sessions must not read mail content or compose mail"
            );
        }

        let admin_management_route = "/api/v2/organizations/o1/role-assignments";
        assert!(human_client_route_permitted(
            HumanSessionClient::Admin,
            &Method::POST,
            admin_management_route
        ));
        assert!(
            !human_client_route_permitted(
                HumanSessionClient::Mail,
                &Method::POST,
                admin_management_route
            ),
            "the client boundary runs before and cannot be overridden by admin RBAC"
        );
        assert!(human_client_route_permitted(
            HumanSessionClient::Admin,
            &Method::GET,
            "/api/v2/auth/admin/session"
        ));
        assert!(!human_client_route_permitted(
            HumanSessionClient::Mail,
            &Method::GET,
            "/api/v2/auth/admin/session"
        ));
        assert!(human_client_route_permitted(
            HumanSessionClient::Mail,
            &Method::POST,
            "/api/v2/auth/mail/logout"
        ));
        assert!(!human_client_route_permitted(
            HumanSessionClient::Mail,
            &Method::GET,
            "/api/v2/mailboxes/m1/messages/"
        ));

        let mut admin_headers = HeaderMap::new();
        admin_headers.insert(
            header::COOKIE,
            format!("{admin_name}={admin_token}").parse().unwrap(),
        );
        let denied =
            require_human_mail_browser_session(&admin_headers, true, "request-1").unwrap_err();
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        let mut mail_headers = HeaderMap::new();
        mail_headers.insert(
            header::COOKIE,
            format!("{mail_name}={mail_token}").parse().unwrap(),
        );
        assert!(require_human_mail_browser_session(&mail_headers, true, "request-1").is_ok());

        let mut bearer_headers = HeaderMap::new();
        bearer_headers.insert(header::AUTHORIZATION, "Bearer agent-token".parse().unwrap());
        let denied =
            require_human_mail_browser_session(&bearer_headers, true, "request-1").unwrap_err();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        assert!(!denied.headers().contains_key(header::WWW_AUTHENTICATE));
    }

    #[test]
    fn successful_login_sets_active_session_and_clears_the_other_client_cookie() {
        let token = URL_SAFE_NO_PAD.encode([7; 32]);
        let [active_cookie, cleared_cookie] =
            browser_session_cookie_headers(HumanSessionClient::Admin, &token, 900, true).unwrap();
        let mut headers = HeaderMap::new();
        headers.append(header::SET_COOKIE, active_cookie);
        headers.append(header::SET_COOKIE, cleared_cookie);

        let cookies = headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(cookies.len(), 2);
        assert!(cookies[0].starts_with(&format!("__Host-copilotmail-admin-session={token};")));
        assert!(cookies[0].contains("SameSite=Strict; Max-Age=900"));
        assert!(cookies[1].starts_with("__Host-copilotmail-mail-session=;"));
        assert!(cookies[1].contains("SameSite=Strict; Max-Age=0"));
    }

    #[tokio::test]
    async fn human_session_routes_fail_closed_without_configuration_or_storage_access() {
        let session = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .uri("/api/v2/auth/admin/session")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(session.status(), StatusCode::UNAUTHORIZED);
        assert!(!session.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(session.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(session.headers()["referrer-policy"], "no-referrer");

        let login = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .uri("/api/v2/auth/admin/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(login.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(login.headers()["referrer-policy"], "no-referrer");

        let logout = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v2/auth/admin/logout")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::UNAUTHORIZED);
        assert!(!logout.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(logout.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn human_login_rate_limit_response_is_bounded_and_retryable() {
        let response = human_session_api_error(
            HumanSessionError::RateLimited {
                retry_after_seconds: 600,
            },
            "request-1",
        );
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "60");
    }

    #[test]
    fn delivery_response_exposes_counts_without_recipient_addresses_or_engine_ids() {
        let response =
            mail_delivery_response(copilotmail_application::MailDeliverySnapshotRecord {
                snapshot_id: Uuid::now_v7(),
                organization_id: OrganizationId::new(),
                submission_id: MailSubmissionId::new(),
                source_state: "private-engine-state".into(),
                source_sha256: [7; 32],
                recipients: vec![
                    copilotmail_application::MailDeliveryRecipientWrite {
                        recipient_ref: [1; 32],
                        state: MailDeliveryRecipientState::Delivered,
                        smtp_reply: "250 2.0.0 Accepted".into(),
                    },
                    copilotmail_application::MailDeliveryRecipientWrite {
                        recipient_ref: [2; 32],
                        state: MailDeliveryRecipientState::PermanentFailure,
                        smtp_reply: "550 5.1.1 Unknown".into(),
                    },
                ],
                terminal: true,
                observed_at: Utc::now(),
            });
        assert_eq!(response.recipient_count, 2);
        assert_eq!(response.delivered_count, 1);
        assert_eq!(response.failed_count, 1);
        assert_eq!(response.recipients[0].recipient_ref.len(), 64);
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(!serialized.contains('@'));
        assert!(!serialized.contains("private-engine-state"));
    }

    #[test]
    fn request_headers_accept_only_valid_w3c_remote_trace_context() {
        let propagator = TraceContextPropagator::new();
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                .parse()
                .unwrap(),
        );
        let context = propagator.extract(&RequestHeaderExtractor(&headers));
        let span = context.span();
        let span_context = span.span_context();
        assert!(span_context.is_valid());
        assert!(span_context.is_remote());
        assert_eq!(
            span_context.trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );

        headers.insert("traceparent", "not-a-trace-context".parse().unwrap());
        let invalid = propagator.extract(&RequestHeaderExtractor(&headers));
        assert!(!invalid.span().span_context().is_valid());
    }

    #[test]
    fn legal_hold_approval_uses_the_dedicated_permission() {
        assert_eq!(
            approval_request_permission(ApprovalAction::LegalHold),
            Permission::LegalHoldManage
        );
    }

    #[test]
    fn compliance_cursors_are_versioned_kind_and_tenant_scoped() {
        let organization_id = OrganizationId::new();
        let id = Uuid::now_v7();
        let cursor = encode_compliance_cursor("case", organization_id, id);
        assert_eq!(
            decode_compliance_cursor(&cursor, "case", organization_id),
            Ok(id)
        );
        assert!(decode_compliance_cursor(&cursor, "hold", organization_id).is_err());
        assert!(decode_compliance_cursor(&cursor, "case", OrganizationId::new()).is_err());
        assert!(
            decode_compliance_cursor(&format!("{cursor}.extra"), "case", organization_id).is_err()
        );
        assert_eq!(bounded_compliance_limit(None), Some(50));
        assert_eq!(bounded_compliance_limit(Some(100)), Some(100));
        assert_eq!(bounded_compliance_limit(Some(101)), None);
        assert_eq!(
            parse_compliance_case_filter(Some("open")),
            Some(Some(ComplianceCaseStatus::Open))
        );
        assert_eq!(parse_legal_hold_filter(Some("unknown")), None);
    }

    #[test]
    fn approval_cursors_bind_tenant_view_and_status() {
        let organization_id = OrganizationId::new();
        let id = Uuid::now_v7();
        let cursor = encode_approval_cursor(
            organization_id,
            ApprovalRequestView::Requested,
            Some(ApprovalStatus::Pending),
            id,
        );
        assert_eq!(
            decode_approval_cursor(
                &cursor,
                organization_id,
                ApprovalRequestView::Requested,
                Some(ApprovalStatus::Pending),
            ),
            Ok(id)
        );
        assert!(
            decode_approval_cursor(
                &cursor,
                organization_id,
                ApprovalRequestView::Actionable,
                Some(ApprovalStatus::Pending),
            )
            .is_err()
        );
        assert!(
            decode_approval_cursor(
                &cursor,
                organization_id,
                ApprovalRequestView::Requested,
                Some(ApprovalStatus::Approved),
            )
            .is_err()
        );
        assert!(
            decode_approval_cursor(
                &cursor,
                OrganizationId::new(),
                ApprovalRequestView::Requested,
                Some(ApprovalStatus::Pending),
            )
            .is_err()
        );
    }

    #[test]
    fn role_assignment_cursors_bind_tenant_principal_and_activity_filter() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let assignment_id = RoleAssignmentId::new();
        let cursor = encode_role_assignment_cursor(
            organization_id,
            Some(principal_id),
            false,
            assignment_id.as_uuid(),
        );
        assert_eq!(
            decode_role_assignment_cursor(&cursor, organization_id, Some(principal_id), false,),
            Ok(assignment_id.as_uuid())
        );
        assert!(decode_role_assignment_cursor(&cursor, organization_id, None, false).is_err());
        assert!(
            decode_role_assignment_cursor(&cursor, organization_id, Some(principal_id), true,)
                .is_err()
        );
        assert!(
            decode_role_assignment_cursor(
                &cursor,
                OrganizationId::new(),
                Some(principal_id),
                false,
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn errors_include_and_propagate_request_id() {
        let request_id_header = HeaderName::from_static("x-request-id");
        let app = Router::new()
            .fallback(not_found)
            .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
            .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/missing")
                    .header("x-request-id", "test-request-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.headers()["x-request-id"], "test-request-123");
        let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
            .await
            .unwrap();
        let error: copilotmail_contracts::ErrorResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(error.code, "route_not_found");
        assert_eq!(error.request_id, "test-request-123");
        assert_eq!(error.trace_id, "test-request-123");
        assert!(!error.retryable);
    }

    #[test]
    fn bearer_parser_rejects_ambiguous_or_malformed_credentials() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer_token(&headers), Ok(None));
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert!(bearer_token(&headers).is_err());
        headers.insert(header::AUTHORIZATION, "Bearer ".parse().unwrap());
        assert!(bearer_token(&headers).is_err());
        headers.insert(
            header::AUTHORIZATION,
            "Bearer token with spaces".parse().unwrap(),
        );
        assert!(bearer_token(&headers).is_err());
        headers.insert(
            header::AUTHORIZATION,
            "bEaReR compact.token".parse().unwrap(),
        );
        assert_eq!(bearer_token(&headers), Ok(Some("compact.token")));
        headers.append(
            header::AUTHORIZATION,
            "Bearer second.token".parse().unwrap(),
        );
        assert!(bearer_token(&headers).is_err());
    }

    #[test]
    fn any_authorization_header_with_a_session_cookie_is_ambiguous() {
        let token = URL_SAFE_NO_PAD.encode([5; 32]);
        let cookie_name = human_session_cookie_name(HumanSessionClient::Mail, true);
        for authorization in [
            "Bearer valid.token",
            "bearer lower.case",
            "Bearer ",
            "Basic client-credential",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::COOKIE,
                format!("{cookie_name}={token}").parse().unwrap(),
            );
            headers.insert(header::AUTHORIZATION, authorization.parse().unwrap());
            assert!(matches!(
                select_presented_credential(&headers, Some(true)),
                Err(CredentialSelectionError::Ambiguous)
            ));
        }

        let mut duplicate = HeaderMap::new();
        duplicate.insert(
            header::COOKIE,
            format!("{cookie_name}={token}").parse().unwrap(),
        );
        duplicate.append(header::AUTHORIZATION, "Bearer first".parse().unwrap());
        duplicate.append(header::AUTHORIZATION, "Basic second".parse().unwrap());
        assert!(matches!(
            select_presented_credential(&duplicate, Some(true)),
            Err(CredentialSelectionError::Ambiguous)
        ));

        let mut malformed_without_cookie = HeaderMap::new();
        malformed_without_cookie.insert(header::AUTHORIZATION, "Basic only".parse().unwrap());
        assert!(matches!(
            select_presented_credential(&malformed_without_cookie, Some(true)),
            Err(CredentialSelectionError::InvalidBearer)
        ));
    }

    #[test]
    fn mail_engine_projection_errors_use_stable_non_secret_codes() {
        let cases = [
            (
                MailEngineError::InvalidConfiguration,
                "mail_engine_invalid_configuration",
            ),
            (
                MailEngineError::CredentialUnavailable,
                "mail_engine_credential_unavailable",
            ),
            (MailEngineError::Unavailable, "mail_engine_unavailable"),
            (
                MailEngineError::InvalidSession,
                "mail_engine_invalid_session",
            ),
            (
                MailEngineError::InvalidResponse,
                "mail_engine_invalid_response",
            ),
            (MailEngineError::Rejected, "mail_engine_rejected"),
            (
                MailEngineError::ProjectionConflict,
                "mail_engine_projection_conflict",
            ),
        ];
        for (error, expected) in cases {
            let code = mail_engine_error_code(error);
            assert_eq!(code, expected);
            assert!(code.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'.' | b'-')
            }));
        }
    }

    #[test]
    fn webhook_retry_delay_is_exponential_and_bounded() {
        assert_eq!(webhook_retry_delay(0), chrono::Duration::seconds(5));
        assert_eq!(webhook_retry_delay(1), chrono::Duration::seconds(10));
        assert_eq!(webhook_retry_delay(8), chrono::Duration::seconds(1_280));
        assert_eq!(webhook_retry_delay(100), chrono::Duration::seconds(1_280));
    }

    fn disconnected_store() -> PgControlPlaneStore {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        PgControlPlaneStore::from_pool(pool)
    }

    fn router_with_test_ingress(settings: MailEventIngressSettings) -> Router {
        router_with_state(
            AppState {
                store: disconnected_store(),
                authenticator: Arc::new(UnconfiguredAuthenticator),
                authentication_ready: false,
                human_auth_required: false,
                human_session: None,
                human_session_required: false,
                agent_token_service: None,
                agent_authentication_ready: false,
                agent_auth_required: false,
                agent_event_stream_tenant_limit: 100,
                agent_event_stream_principal_limit: 5,
                agent_ingress_tenant_requests_per_minute: 6_000,
                agent_ingress_principal_requests_per_minute: 600,
                agent_event_stream_metrics: AgentEventStreamMetrics::default(),
                mail_data_plane_id: Some(MailDataPlaneId::from_uuid(Uuid::from_u128(
                    0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2101,
                ))),
                mail_engine: None,
                mail_engine_required: false,
                mail_content: None,
                mail_content_required: false,
                mail_drafts: None,
                mail_draft_content: None,
                mail_drafts_required: false,
                mail_event_ingress: Some(settings),
                mail_event_ingress_required: true,
                audit_archive: None,
            },
            None,
        )
    }

    #[tokio::test]
    async fn mail_event_ingress_authenticates_before_body_and_verifies_exact_hmac() {
        let bearer = b"0123456789abcdef0123456789abcdef";
        let signature_key = b"abcdef0123456789abcdef0123456789";
        let prefix =
            std::env::temp_dir().join(format!("copilotmail-ingress-test-{}", Uuid::now_v7()));
        let bearer_file = prefix.with_extension("bearer");
        let signature_file = prefix.with_extension("hmac");
        tokio::fs::write(&bearer_file, [bearer.as_slice(), b"\n"].concat())
            .await
            .unwrap();
        tokio::fs::write(&signature_file, signature_key)
            .await
            .unwrap();
        let settings = MailEventIngressSettings {
            bearer_token_file: bearer_file.clone(),
            signature_key_file: signature_file.clone(),
        };

        let unauthenticated = router_with_test_ingress(settings.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/v2/mail-engine/stalwart/events")
                    .body(Body::from(vec![b'x'; 2 * 1024 * 1024]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

        let body = br#"{"events":[{"id":"event-1","createdAt":"2026-07-26T08:00:00Z","type":"delivery.delivered","data":{"accountId":42,"documentId":991,"mailboxId":[7]}}]}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(signature_key).unwrap();
        mac.update(body);
        let signature =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        let verified_but_unsupported = router_with_test_ingress(settings.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/v2/mail-engine/stalwart/events")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", String::from_utf8_lossy(bearer)),
                    )
                    .header("x-signature", signature)
                    .body(Body::from(body.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(verified_but_unsupported.status(), StatusCode::BAD_REQUEST);

        let bad_signature = router_with_test_ingress(settings)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/v2/mail-engine/stalwart/events")
                    .header(
                        header::AUTHORIZATION,
                        format!("Bearer {}", String::from_utf8_lossy(bearer)),
                    )
                    .header(
                        "x-signature",
                        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                    )
                    .body(Body::from(body.as_slice()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bad_signature.status(), StatusCode::UNAUTHORIZED);

        tokio::fs::remove_file(bearer_file).await.unwrap();
        tokio::fs::remove_file(signature_file).await.unwrap();
    }

    #[tokio::test]
    async fn protected_organization_route_fails_closed_without_authentication() {
        let organization_id = OrganizationId::new();
        let app = router(disconnected_store());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v2/organizations/{organization_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");

        let app = router(disconnected_store());
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v2/organizations/{organization_id}"))
                    .header(header::AUTHORIZATION, "Bearer unverified-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v2/organizations/{organization_id}/audit-events"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .uri("/api/v2/agent/mail-policy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let subscription_id = SubscriptionId::new();
        let message_id = Uuid::now_v7();
        for (method, uri, body) in [
            (
                "PUT",
                format!(
                    "/api/v2/agent/mailboxes/{}/processing-subscription",
                    MailboxId::new()
                ),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/agent/processing-subscriptions/{subscription_id}/claims"),
                r#"{"lease_seconds":60}"#.into(),
            ),
            (
                "PATCH",
                format!("/api/v2/agent/processing-subscriptions/{subscription_id}"),
                r#"{"expected_status":"active","status":"paused"}"#.into(),
            ),
            (
                "GET",
                format!("/api/v2/agent/processing-subscriptions/{subscription_id}/dead-letters"),
                String::new(),
            ),
            (
                "POST",
                format!(
                    "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/lease-extension"
                ),
                format!(r#"{{"lease_id":"{}","lease_seconds":60}}"#, LeaseId::new()),
            ),
            (
                "POST",
                format!(
                    "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/acknowledgement"
                ),
                format!(r#"{{"lease_id":"{}"}}"#, LeaseId::new()),
            ),
            (
                "POST",
                format!(
                    "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/nack"
                ),
                format!(
                    r#"{{"lease_id":"{}","reason_code":"temporary_failure"}}"#,
                    LeaseId::new()
                ),
            ),
        ] {
            let response = router(disconnected_store())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn approval_routes_fail_closed_before_touching_storage() {
        let organization_id = OrganizationId::new();
        let approval_request_id = ApprovalRequestId::new();
        let create = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v2/approval-requests")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{
                            "policy_key":"mail.send_as.external",
                            "action":"mail.send_as",
                            "resource_kind":"mailbox",
                            "resource_id":"mailbox-1",
                            "reason":"review",
                            "ticket_reference":"SEC-1",
                            "expires_at":"2026-07-26T00:00:00Z"
                        }"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::UNAUTHORIZED);

        let decide = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v2/approval-requests/{}/decisions",
                        ApprovalRequestId::new()
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"decision":"approved","reason":"reviewed"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(decide.status(), StatusCode::UNAUTHORIZED);

        for (method, uri, body) in [
            (
                "GET",
                format!(
                    "/api/v2/organizations/{organization_id}/approval-requests?view=actionable"
                ),
                String::new(),
            ),
            (
                "GET",
                format!("/api/v2/approval-requests/{approval_request_id}"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/approval-requests/{approval_request_id}/cancellation"),
                "{}".into(),
            ),
        ] {
            let response = router(disconnected_store())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn role_governance_routes_fail_closed_before_touching_storage() {
        let organization_id = OrganizationId::new();
        let assignment_id = RoleAssignmentId::new();
        for (method, uri, body) in [
            (
                "GET",
                format!("/api/v2/organizations/{organization_id}/roles"),
                String::new(),
            ),
            (
                "GET",
                format!("/api/v2/organizations/{organization_id}/role-assignments"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/organizations/{organization_id}/role-assignments"),
                r#"{
                    "principal_id":"018f13b9-1111-7bc8-965d-48a28866f073",
                    "role_id":"018f13b9-2222-7bc8-965d-48a28866f073",
                    "valid_until":"2026-08-01T00:00:00.000Z",
                    "approval_request_id":"018f13b9-3333-7bc8-965d-48a28866f073"
                }"#
                .into(),
            ),
            (
                "POST",
                format!("/api/v2/role-assignments/{assignment_id}/revocation"),
                "{}".into(),
            ),
        ] {
            let response = router(disconnected_store())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn compliance_routes_fail_closed_before_database_or_object_store_access() {
        let organization_id = OrganizationId::new();
        let case_id = ComplianceCaseId::new();
        let archive_id = AuditArchiveId::new();
        let grant_id = AuditExportId::new();
        let retention_policy_id = RetentionPolicyId::new();
        let retention_version_id = RetentionPolicyVersionId::new();
        let agent_id = PrincipalId::new();
        let agent_mail_policy_version_id = Uuid::now_v7();
        let hold_id = LegalHoldId::new();
        let approval_request_id = ApprovalRequestId::new();
        let requests = [
            (
                "GET",
                format!("/api/v2/organizations/{organization_id}/compliance-cases"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/organizations/{organization_id}/compliance-cases"),
                r#"{"external_reference":"CASE-1","title":"Evidence hold","purpose":"Investigation"}"#.to_owned(),
            ),
            (
                "GET",
                format!("/api/v2/compliance-cases/{case_id}"),
                String::new(),
            ),
            (
                "GET",
                format!("/api/v2/organizations/{organization_id}/legal-holds"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/compliance-cases/{case_id}/closure"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/audit-archives/{archive_id}/legal-holds"),
                format!(
                    r#"{{"case_id":"{case_id}","approval_request_id":"{approval_request_id}","reason":"Evidence preservation"}}"#
                ),
            ),
            (
                "POST",
                format!("/api/v2/audit-archives/{archive_id}/exports"),
                format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
            ),
            (
                "GET",
                format!("/api/v2/audit-export-grants/{grant_id}"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/audit-export-grants/{grant_id}/artifacts/events"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/audit-export-grants/{grant_id}/artifacts/manifest"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/organizations/{organization_id}/retention-policies"),
                r#"{"policy_key":"audit.standard","display_name":"Audit standard","resource_kind":"audit_evidence","minimum_retention_days":2555,"archive_after_days":30,"delete_after_days":3650,"rule":{}}"#.to_owned(),
            ),
            (
                "GET",
                format!("/api/v2/retention-policies/{retention_policy_id}"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/retention-policies/{retention_policy_id}/versions"),
                r#"{"minimum_retention_days":2555,"archive_after_days":30,"delete_after_days":3650,"rule":{}}"#.to_owned(),
            ),
            (
                "POST",
                format!("/api/v2/retention-policies/{retention_policy_id}/versions/{retention_version_id}/activation"),
                format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
            ),
            (
                "POST",
                format!("/api/v2/agents/{agent_id}/mail-policy-versions/{agent_mail_policy_version_id}/activation"),
                format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
            ),
            (
                "POST",
                format!("/api/v2/agents/{agent_id}/mail-policy-versions"),
                r#"{"allowed_recipient_domains":["internal.example"],"max_recipients_per_message":5,"max_messages_per_hour":100,"max_messages_per_recipient_per_day":10,"require_first_contact_approval":true,"max_auto_replies_per_thread_per_day":2}"#.to_owned(),
            ),
            (
                "GET",
                format!("/api/v2/agents/{agent_id}/mail-policy-versions/{agent_mail_policy_version_id}"),
                String::new(),
            ),
            (
                "GET",
                format!("/api/v2/legal-holds/{hold_id}"),
                String::new(),
            ),
            (
                "POST",
                format!("/api/v2/legal-holds/{hold_id}/release"),
                format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
            ),
        ];
        for (method, uri, body) in requests {
            let response = router(disconnected_store())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn mailbox_resource_route_fails_closed_without_authentication() {
        let mailbox_id = MailboxId::new();
        let message_id = Uuid::now_v7();
        let submission_id = MailSubmissionId::new();
        for uri in [
            "/api/v2/mailboxes".to_owned(),
            format!("/api/v2/mailboxes/{mailbox_id}"),
            format!("/api/v2/mailboxes/{mailbox_id}/messages"),
            format!("/api/v2/mailboxes/{mailbox_id}/messages/{message_id}"),
            format!("/api/v2/mailboxes/{mailbox_id}/drafts"),
            format!(
                "/api/v2/agent/mailboxes/{mailbox_id}/messages/{}/content",
                Uuid::now_v7()
            ),
            format!("/api/v2/mail-submissions/{submission_id}"),
        ] {
            let human_mail = human_mail_route(&Method::GET, &uri);
            let response = router(disconnected_store())
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            if human_mail {
                assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            }
        }
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .uri("/api/v2/mailboxes")
                    .header(header::AUTHORIZATION, "Bearer unverified-agent-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .uri("/api/v2/mailboxes?limit=not-a-number&provider=forbidden")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "authentication must fail closed before parsing directory filters or touching storage"
        );
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!(
                        "/api/v2/mailboxes/{mailbox_id}/messages/{message_id}"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"is_read":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v2/mailboxes/{mailbox_id}/messages/search"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"text":"quarterly report","cursor":null,"page_size":30}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v2/mailboxes/{mailbox_id}/drafts"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"draft_id":null,"expected_version":null,"to":["recipient@example.test"],"cc":[],"bcc":[],"subject":"Status","text_body":"Ready."}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let draft_id = DraftId::new();
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v2/mailboxes/{mailbox_id}/drafts/{draft_id}/restore"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"expected_version":"not-an-integer"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "authentication must fail before parsing a draft restore body"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v2/mailboxes/{mailbox_id}/submissions"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("idempotency-key", "test-submission-key-000001")
                    .body(Body::from(format!(
                        r#"{{"draft_id":"{}","expected_draft_version":2}}"#,
                        copilotmail_domain::DraftId::new()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/api/v2/agent/mailboxes/{mailbox_id}/sandbox-submissions"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("idempotency-key", "test-sandbox-key-00000001")
                    .body(Body::from(format!(
                        r#"{{"draft_id":"{}","expected_draft_version":2}}"#,
                        copilotmail_domain::DraftId::new()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn inbox_cursor_is_mailbox_bound_and_bounded() {
        let mailbox_id = MailboxId::new();
        let cursor = encode_inbox_cursor(mailbox_id, 50);
        assert_eq!(decode_inbox_cursor(&cursor, mailbox_id), Ok(50));
        assert_eq!(decode_inbox_cursor(&cursor, MailboxId::new()), Err(()));
        assert_eq!(decode_inbox_cursor("", mailbox_id), Err(()));
        assert_eq!(
            decode_inbox_cursor(
                &URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&serde_json::json!({
                        "v": 2,
                        "mailbox_id": mailbox_id,
                        "position": 10_001
                    }))
                    .unwrap()
                ),
                mailbox_id
            ),
            Err(())
        );

        let digest = inbox_search_digest("quarterly report");
        let search_cursor = encode_inbox_search_cursor(mailbox_id, 30, &digest);
        assert_eq!(
            decode_inbox_search_cursor(&search_cursor, mailbox_id, &digest),
            Ok(30)
        );
        assert_eq!(
            decode_inbox_search_cursor(
                &search_cursor,
                mailbox_id,
                &inbox_search_digest("different query")
            ),
            Err(())
        );
        assert_eq!(
            decode_inbox_search_cursor(&search_cursor, MailboxId::new(), &digest),
            Err(())
        );
        let decoded = URL_SAFE_NO_PAD.decode(search_cursor).unwrap();
        let decoded = String::from_utf8(decoded).unwrap();
        assert!(!decoded.contains("quarterly report"));
        assert!(!decoded.contains("query_state"));
    }

    #[tokio::test]
    async fn mailbox_management_routes_fail_closed_without_authentication() {
        let organization_id = OrganizationId::new();
        let mailbox_id = MailboxId::new();
        let create = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v2/organizations/{organization_id}/mailboxes"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"kind":"shared","primary_address":"support@example.test"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create.status(), StatusCode::UNAUTHORIZED);

        let grant = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/v2/mailboxes/{mailbox_id}/delegations"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(format!(
                        r#"{{
                            "grantee_principal_id":"{}",
                            "capabilities":["mailbox.read"],
                            "reason":"support rotation",
                            "valid_until":"2026-07-26T00:00:00Z"
                        }}"#,
                        PrincipalId::new()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(grant.status(), StatusCode::UNAUTHORIZED);

        let revoke = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!(
                        "/api/v2/mailbox-delegations/{}",
                        DelegationId::new()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(revoke.status(), StatusCode::UNAUTHORIZED);

        let binding = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"account_ref":"42","expected_version":0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(binding.status(), StatusCode::UNAUTHORIZED);

        let malformed_binding = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(malformed_binding.status(), StatusCode::UNAUTHORIZED);

        let oversized_binding = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!(
                        "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding"
                    ))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(vec![b'x'; 16 * 1024 + 1]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized_binding.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mail_domain_routes_fail_closed_before_dns_or_storage_access() {
        let organization_id = OrganizationId::new();
        let mail_domain_id = MailDomainId::new();
        let requests = [
            (
                "GET",
                format!("/api/v2/organizations/{organization_id}/mail-domains"),
                "",
            ),
            ("GET", format!("/api/v2/mail-domains/{mail_domain_id}"), ""),
            (
                "POST",
                format!("/api/v2/organizations/{organization_id}/mail-domains"),
                r#"{"name":"example.test","route_mode":"direct_mx"}"#,
            ),
            (
                "POST",
                format!("/api/v2/mail-domains/{mail_domain_id}/dns-verifications"),
                r#"{"domain":"example.test","ownership_token":"cmv1_abcdefghijklmnopqrstuvwxyz123456","dkim_selector":"mail"}"#,
            ),
            (
                "POST",
                format!("/api/v2/mail-domains/{mail_domain_id}/ownership-challenge"),
                r#"{"expected_version":1}"#,
            ),
            (
                "POST",
                format!("/api/v2/mail-domains/{mail_domain_id}/activation"),
                "{}",
            ),
            (
                "POST",
                format!("/api/v2/mail-domains/{mail_domain_id}/suspension"),
                r#"{"reason_code":"security_incident"}"#,
            ),
            (
                "PUT",
                format!("/api/v2/mail-domains/{mail_domain_id}/route"),
                r#"{"expected_version":1,"inbound_enabled":false,"outbound_enabled":false,"smart_host_ref":null,"dkim_key_ref":null}"#,
            ),
        ];
        for (method, uri, body) in requests {
            let response = router(disconnected_store())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn webhook_subscription_routes_fail_closed_before_storage_access() {
        let organization_id = OrganizationId::new();
        let subscription_id = WebhookSubscriptionId::new();
        let event_id = Uuid::now_v7();
        let requests = [
            (
                "GET",
                format!("/api/v2/organizations/{organization_id}/webhook-subscriptions"),
                "",
            ),
            (
                "POST",
                format!("/api/v2/organizations/{organization_id}/webhook-subscriptions"),
                r#"{"principal_id":"00000000-0000-0000-0000-000000000001","endpoint_url":"https://hooks.example.test/events","event_types":["message.received.v1"],"signing_key_reference":"agent-hook-1"}"#,
            ),
            (
                "GET",
                format!("/api/v2/webhook-subscriptions/{subscription_id}"),
                "",
            ),
            (
                "PUT",
                format!("/api/v2/webhook-subscriptions/{subscription_id}"),
                r#"{"expected_version":1,"endpoint_url":"https://hooks.example.test/events","event_types":["message.received.v1"],"signing_key_reference":"agent-hook-1","enabled":false}"#,
            ),
            (
                "GET",
                format!("/api/v2/webhook-subscriptions/{subscription_id}/events?limit=50"),
                "",
            ),
            (
                "GET",
                format!("/api/v2/webhook-subscriptions/{subscription_id}/stream"),
                "",
            ),
            (
                "GET",
                format!("/api/v2/organizations/{organization_id}/webhook-dead-letters"),
                "",
            ),
            (
                "POST",
                format!(
                    "/api/v2/webhook-subscriptions/{subscription_id}/dead-letters/{event_id}/replay"
                ),
                "{}",
            ),
        ];
        for (method, uri, body) in requests {
            let response = router(disconnected_store())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[test]
    fn agent_event_cursor_is_versioned_and_subscription_scoped() {
        let subscription_id = WebhookSubscriptionId::new();
        let event_id = Uuid::now_v7();
        let cursor = format_agent_event_cursor(subscription_id, event_id);
        assert_eq!(
            parse_agent_event_cursor(&cursor, subscription_id).unwrap(),
            event_id
        );
        assert!(parse_agent_event_cursor(&cursor, WebhookSubscriptionId::new()).is_err());
        assert!(parse_agent_event_cursor("v2.invalid", subscription_id).is_err());
    }

    #[test]
    fn agent_processing_dead_letter_cursor_is_versioned_and_subscription_scoped() {
        let subscription_id = SubscriptionId::new();
        let message_id = Uuid::now_v7();
        let cursor = format_agent_processing_dead_letter_cursor(subscription_id, message_id);
        assert_eq!(
            parse_agent_processing_dead_letter_cursor(&cursor, subscription_id),
            Ok(message_id)
        );
        assert!(parse_agent_processing_dead_letter_cursor(&cursor, SubscriptionId::new()).is_err());
        assert!(
            parse_agent_processing_dead_letter_cursor(&format!("{cursor}.extra"), subscription_id)
                .is_err()
        );
        assert_eq!(
            parse_agent_processing_subscription_status("paused", "request-id").unwrap(),
            AgentProcessingSubscriptionStatus::Paused
        );
        assert!(parse_agent_processing_subscription_status("deleted", "request-id").is_err());
    }

    #[test]
    fn agent_event_stream_resume_sources_must_match_and_remain_subscription_scoped() {
        let subscription_id = WebhookSubscriptionId::new();
        let event_id = Uuid::now_v7();
        let cursor = format_agent_event_cursor(subscription_id, event_id);
        let header = header::HeaderValue::from_str(&cursor).unwrap();
        assert_eq!(
            resolve_agent_stream_cursor(Some(&cursor), Some(&header), subscription_id).unwrap(),
            Some(event_id)
        );
        assert_eq!(
            resolve_agent_stream_cursor(None, Some(&header), subscription_id).unwrap(),
            Some(event_id)
        );
        let other_cursor = format_agent_event_cursor(subscription_id, Uuid::now_v7());
        let other_header = header::HeaderValue::from_str(&other_cursor).unwrap();
        assert_eq!(
            resolve_agent_stream_cursor(Some(&cursor), Some(&other_header), subscription_id),
            Err(AgentEventCursorParseError::ConflictingResumeSources)
        );
        assert_eq!(
            resolve_agent_stream_cursor(Some(&cursor), None, WebhookSubscriptionId::new()),
            Err(AgentEventCursorParseError::ScopeMismatch)
        );
    }

    #[test]
    fn audit_event_cursor_is_positive_versioned_and_tenant_scoped() {
        let organization_id = OrganizationId::new();
        let cursor = format_audit_event_cursor(organization_id, 42);
        assert_eq!(
            parse_audit_event_cursor(&cursor, organization_id).unwrap(),
            42
        );
        assert_eq!(
            parse_audit_event_cursor(&cursor, OrganizationId::new()),
            Err(AuditEventCursorParseError::ScopeMismatch)
        );
        for invalid in [
            "v2.invalid.1",
            "v1.00000000-0000-0000-0000-000000000000.0",
            "v1.00000000-0000-0000-0000-000000000000.-1",
            "v1.00000000-0000-0000-0000-000000000000.1.extra",
        ] {
            assert!(parse_audit_event_cursor(invalid, organization_id).is_err());
        }
    }

    #[test]
    fn audit_maintenance_ids_are_stable_scoped_and_uuid_v8() {
        let organization_id = OrganizationId::new().to_string();
        let first = stable_maintenance_uuid(
            "checkpoint",
            &[organization_id.as_bytes(), b"42", b"audit-key-1"],
        );
        let replay = stable_maintenance_uuid(
            "checkpoint",
            &[organization_id.as_bytes(), b"42", b"audit-key-1"],
        );
        let another_head = stable_maintenance_uuid(
            "checkpoint",
            &[organization_id.as_bytes(), b"43", b"audit-key-1"],
        );
        let archive = stable_maintenance_uuid(
            "archive",
            &[organization_id.as_bytes(), b"42", b"audit-key-1"],
        );
        assert_eq!(first, replay);
        assert_ne!(first, another_head);
        assert_ne!(first, archive);
        assert_eq!(first.get_version_num(), 8);
    }

    #[test]
    fn audit_archive_metadata_carries_stable_artifact_trace_without_sensitive_fields() {
        let organization_id = OrganizationId::new();
        let checkpoint_id = AuditCheckpointId::new();
        let archive_id = AuditArchiveId::new();
        let trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
        assert_eq!(
            trace_id,
            stable_audit_artifact_trace_id(organization_id, checkpoint_id)
        );
        assert_ne!(
            trace_id,
            stable_audit_artifact_trace_id(organization_id, AuditCheckpointId::new())
        );
        assert_eq!(trace_id.parse::<Uuid>().unwrap().get_version_num(), 8);
        let metadata =
            audit_archive_object_metadata(organization_id, checkpoint_id, archive_id, 1, &trace_id);

        assert_eq!(metadata["organization-id"], organization_id.to_string());
        assert_eq!(metadata["checkpoint-id"], checkpoint_id.to_string());
        assert_eq!(metadata["archive-id"], archive_id.to_string());
        assert_eq!(metadata["trace-id"], trace_id);
        assert_eq!(metadata.len(), 5);
        assert!(
            metadata
                .keys()
                .all(|key| !key.contains("secret") && !key.contains("content"))
        );
    }

    #[tokio::test]
    async fn audit_maintenance_metrics_expose_alertable_fail_closed_state() {
        let organization_id = OrganizationId::new();
        let metrics = AuditMaintenanceMetrics::with_limits(organization_id, 2, 60, 30);
        assert!(metrics.readiness().is_ok());

        metrics.cycle_started();
        metrics.failed();
        assert!(metrics.readiness().is_ok());
        metrics.cycle_started();
        metrics.failed();
        assert_eq!(metrics.readiness(), Err("failure_threshold_exceeded"));

        let rendered = metrics.render();
        assert!(rendered.contains("copilotmail_audit_maintenance_failures_total"));
        assert!(rendered.contains(&format!("organization_id=\"{organization_id}\"")));
        assert!(rendered.contains("copilotmail_audit_maintenance_consecutive_failures"));
        let metrics_response = audit_maintenance_observability_router(metrics.clone())
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(metrics_response.status(), StatusCode::OK);
        assert_eq!(
            metrics_response.headers()[header::CONTENT_TYPE],
            "text/plain; version=0.0.4; charset=utf-8"
        );
        let readiness_response = audit_maintenance_observability_router(metrics)
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(readiness_response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let pending = AuditMaintenanceMetrics::with_limits(organization_id, 3, 60, 30);
        pending.state.lock().unwrap().pending_checkpoint_age_seconds = 61;
        assert_eq!(pending.readiness(), Err("pending_checkpoint_overdue"));

        let slow = AuditMaintenanceMetrics::with_limits(organization_id, 3, 60, 30);
        slow.cycle_started();
        slow.state.lock().unwrap().cycle_started_at = Utc::now().timestamp() - 31;
        assert_eq!(slow.readiness(), Err("cycle_duration_exceeded"));

        let passive = AuditMaintenanceMetrics::with_limits(organization_id, 3, 60, 30);
        passive.cycle_started();
        passive.lock_conflict();
        assert!(passive.readiness().is_ok());
        assert!(
            passive
                .render()
                .contains("copilotmail_audit_maintenance_lock_conflicts_total")
        );
    }

    #[tokio::test]
    async fn agent_token_endpoint_fails_closed_and_disables_caching_when_unconfigured() {
        let app = router(disconnected_store());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/oauth2/token")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(
                        "grant_type=client_credentials&client_id=invalid&\
                         client_assertion_type=invalid&client_assertion=invalid",
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::PRAGMA], "no-cache");
    }

    #[tokio::test]
    async fn agent_jwks_endpoint_fails_closed_when_authorization_server_is_unconfigured() {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .uri("/oauth2/jwks.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[tokio::test]
    async fn malformed_agent_token_form_uses_oauth_error_and_disables_caching() {
        let app = router(disconnected_store());
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/oauth2/token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::PRAGMA], "no-cache");
    }

    #[test]
    fn agent_event_stream_metrics_are_bounded_and_content_free() {
        let metrics = AgentEventStreamMetrics::default();
        metrics.opened();
        metrics
            .tenant_rejected_total
            .fetch_add(2, Ordering::Relaxed);
        metrics
            .principal_rejected_total
            .fetch_add(3, Ordering::Relaxed);
        metrics.stream_errors_total.fetch_add(1, Ordering::Relaxed);
        metrics
            .ingress_allowed_total
            .fetch_add(4, Ordering::Relaxed);
        metrics
            .ingress_principal_rejected_total
            .fetch_add(5, Ordering::Relaxed);
        let rendered = metrics.render();
        assert!(rendered.contains("copilotmail_agent_event_stream_active 1"));
        assert!(rendered.contains("scope=\"tenant\"} 2"));
        assert!(rendered.contains("scope=\"principal\"} 3"));
        assert!(rendered.contains("outcome=\"allowed\"} 4"));
        assert!(rendered.contains("outcome=\"principal_limited\"} 5"));
        assert!(!rendered.contains("organization_id"));
        assert!(!rendered.contains("principal_id"));
        metrics.closed();
        assert!(
            metrics
                .render()
                .contains("copilotmail_agent_event_stream_active 0")
        );
    }

    #[test]
    fn agent_ingress_rejection_has_bounded_retry_after() {
        let response = agent_ingress_limited_response(
            "agent_ingress_principal_rate_limited",
            "Agent event ingress rate is exhausted",
            600,
            "request-id",
        );
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "60");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn agent_mail_submission_requires_agent_operate_in_rbac_and_token_scope() {
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let mut authorization = AuthorizationContext {
            principal_id,
            organization_id,
            permissions: [Permission::MailboxUse].into_iter().collect(),
        };
        let mut identity = AuthenticatedIdentity {
            principal_id,
            principal_kind: AuthenticatedPrincipalKind::Agent,
            credential_id: Some("agent-credential".into()),
            scope_restriction: Some([Permission::MailboxUse].into_iter().collect()),
        };
        assert_eq!(
            enforce_mail_submission_scopes(&identity, &authorization, "request")
                .expect_err("Agent RBAC must include agent.operate")
                .status(),
            StatusCode::FORBIDDEN
        );
        authorization.permissions.insert(Permission::AgentOperate);
        assert_eq!(
            enforce_mail_submission_scopes(&identity, &authorization, "request")
                .expect_err("Agent token scope must include agent.operate")
                .status(),
            StatusCode::FORBIDDEN
        );
        identity
            .scope_restriction
            .as_mut()
            .unwrap()
            .insert(Permission::AgentOperate);
        assert!(enforce_mail_submission_scopes(&identity, &authorization, "request").is_ok());
    }

    #[tokio::test]
    async fn agent_mail_admission_errors_are_stable_minimal_and_actionable() {
        let cases = [
            (
                AgentMailAdmissionFailure::RecipientPolicy,
                StatusCode::UNPROCESSABLE_ENTITY,
                "agent_mail_recipient_policy_denied",
                json!({"scope": "recipient_policy"}),
                None,
            ),
            (
                AgentMailAdmissionFailure::HourlyQuota,
                StatusCode::TOO_MANY_REQUESTS,
                "agent_mail_hourly_quota_exceeded",
                json!({"scope": "agent_hour"}),
                Some("60"),
            ),
            (
                AgentMailAdmissionFailure::RecipientDailyQuota,
                StatusCode::TOO_MANY_REQUESTS,
                "agent_mail_recipient_daily_quota_exceeded",
                json!({"scope": "recipient_day"}),
                Some("3600"),
            ),
            (
                AgentMailAdmissionFailure::ThreadCircuitBreaker,
                StatusCode::TOO_MANY_REQUESTS,
                "agent_mail_thread_circuit_breaker_open",
                json!({"scope": "thread_day"}),
                Some("3600"),
            ),
            (
                AgentMailAdmissionFailure::AutomaticReplyLoop,
                StatusCode::CONFLICT,
                "agent_mail_automatic_reply_blocked",
                json!({"scope": "automatic_reply_loop"}),
                None,
            ),
            (
                AgentMailAdmissionFailure::DuplicateContentCircuitBreaker,
                StatusCode::TOO_MANY_REQUESTS,
                "agent_mail_duplicate_content_circuit_breaker_open",
                json!({"scope": "identical_body_day"}),
                Some("3600"),
            ),
            (
                AgentMailAdmissionFailure::SandboxRequired,
                StatusCode::CONFLICT,
                "agent_mail_sandbox_required",
                json!({"scope": "testing_environment"}),
                None,
            ),
        ];
        for (reason, status, code, details, retry_after) in cases {
            let response = agent_mail_admission_api_error(reason, "admission-request");
            assert_eq!(response.status(), status);
            assert_eq!(
                response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok()),
                retry_after
            );
            let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
            let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
            assert_eq!(error.code, code);
            assert_eq!(error.details, details);
            assert_eq!(error.retryable, retry_after.is_some());
            assert!(
                !body
                    .windows(b"recipient@example.test".len())
                    .any(|window| window == b"recipient@example.test")
            );
        }

        let approval = agent_mail_admission_api_error(
            AgentMailAdmissionFailure::FirstContactApproval,
            "approval-request",
        );
        assert_eq!(approval.status(), StatusCode::CONFLICT);
        let body = to_bytes(approval.into_body(), 16 * 1024).await.unwrap();
        let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(error.code, "agent_first_contact_approval_required");
        assert_eq!(error.details["approval_action"], "agent.first_contact");
        assert_eq!(error.details["resource_kind"], "agent.mail_draft");
        assert!(
            !body
                .windows(b"recipient@example.test".len())
                .any(|window| window == b"recipient@example.test")
        );
    }

    #[test]
    fn own_agent_mail_policy_response_is_aggregate_and_privacy_preserving() {
        let organization_id = OrganizationId::new();
        let agent_id = PrincipalId::new();
        let observed_at = "2026-07-27T02:15:00Z".parse::<DateTime<Utc>>().unwrap();
        let response = own_agent_mail_policy_response(
            AgentMailPolicyRuntimeSnapshot {
                organization_id,
                agent_id,
                environment: AgentEnvironment::Production,
                active_policy: Some(
                    copilotmail_domain::AgentMailPolicyVersion::new(
                        organization_id,
                        agent_id,
                        4,
                        ["internal.example".into()],
                        10,
                        25,
                        3,
                        true,
                        2,
                    )
                    .unwrap(),
                ),
                observed_at,
                hourly_window_started_at: "2026-07-27T01:15:00Z".parse::<DateTime<Utc>>().unwrap(),
                admitted_messages_in_hour: 7,
            },
            vec!["agent.operate".into(), "mailbox.use".into()],
        );
        assert_eq!(response.admission_state, "active");
        assert_eq!(response.hourly_usage.remaining_messages, Some(18));
        let body = serde_json::to_string(&response).unwrap();
        for forbidden in [
            "recipient_ref",
            "recipient@example.test",
            "thread_ref",
            "approval_request_id",
            "message_id",
            "engine_",
        ] {
            assert!(!body.contains(forbidden), "{forbidden} leaked: {body}");
        }
    }

    #[tokio::test]
    async fn signing_key_manifest_resolves_relative_secret_files() {
        let directory = std::env::temp_dir().join(format!("copilotmail-keyset-{}", Uuid::now_v7()));
        tokio::fs::create_dir(&directory).await.unwrap();
        tokio::fs::write(directory.join("active-private.pem"), b"private")
            .await
            .unwrap();
        tokio::fs::write(directory.join("active-public.pem"), b"public")
            .await
            .unwrap();
        tokio::fs::write(
            directory.join("keyset.json"),
            br#"{
                "active_key_id": "active-2026-07",
                "keys": [{
                    "key_id": "active-2026-07",
                    "private_key_file": "active-private.pem",
                    "public_key_file": "active-public.pem"
                }]
            }"#,
        )
        .await
        .unwrap();
        let settings = AgentOAuthSettings {
            issuer: "https://mail.example.test".into(),
            audience: "https://mail.example.test/api/v2".into(),
            token_endpoint: "https://mail.example.test/oauth2/token".into(),
            signing_key_id: None,
            private_key_file: None,
            public_key_file: None,
            signing_key_set_file: Some(directory.join("keyset.json")),
            signing_key_reload_seconds: 30,
            access_token_ttl_seconds: 300,
            allow_insecure_http: false,
        };
        let loaded = load_agent_signing_key_set(&settings).await.unwrap();
        assert_eq!(loaded.active_key_id, "active-2026-07");
        assert_eq!(loaded.keys.len(), 1);
        assert_eq!(
            loaded.keys[0].private_key_pem.as_deref(),
            Some(b"private".as_slice())
        );
        assert_eq!(loaded.keys[0].public_key_pem, b"public");
        assert_eq!(loaded.digest.len(), 32);
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
