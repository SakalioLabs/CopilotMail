//! V2 HTTP control plane and durable background worker.

#![forbid(unsafe_code)]

mod agent_inspector_http;
mod http;
mod tls;
#[cfg(test)]
use agent_inspector_http::{agent_inspector_response, agent_mail_submission_evidence_response};
use agent_inspector_http::{get_agent_inspector, get_agent_mail_submission_evidence};
use http::*;

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
    AgentIngressRateLimiter, AgentInspectorError, AgentInspectorService, AgentInspectorSnapshot,
    AgentMailAdmissionFailure, AgentMailContentError, AgentMailContentService,
    AgentMailPolicyError, AgentMailPolicyQueryError, AgentMailPolicyQueryService,
    AgentMailPolicyRuntimeSnapshot, AgentMailPolicyService, AgentMailSandboxError,
    AgentMailSandboxService, AgentMailSubmissionEvidence, AgentMailSubmissionEvidenceError,
    AgentMailSubmissionEvidenceService, AgentMessageProcessingResult, AgentProcessingError,
    AgentProcessingService, AgentProcessingSubscriptionStatus, AppendApprovalPolicyVersionCommand,
    AppendRetentionPolicyVersionCommand, ApplyLegalHoldCommand, ApprovalPolicyService,
    ApprovalProof, ApprovalQueryStore, ApprovalRequestDetail, ApprovalRequestSummary,
    ApprovalRequestView, ApprovalWorkflowError, ApprovalWorkflowService, ArchivedObjectRecord,
    AuditArchiveRecord, AuditCheckpointQueryStore, AuditEventSearchError, AuditEventSearchService,
    AuditEventSummary, AuditExportArtifact, AuditExportAuthorizationError,
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
    ExtendAgentMessageLeaseCommand, GetAgentInspectorCommand, GetAgentMailPolicyVersionCommand,
    GetAgentMailSubmissionEvidenceCommand, GetApprovalRequestCommand, GetMessageCommand,
    GetOwnAgentMailPolicyCommand, GetRetentionPolicyCommand, GrantMailboxDelegationCommand,
    GrantRoleAssignmentCommand, HumanDraftError, HumanDraftService, HumanInboxMessageSummary,
    HumanMailboxDirectoryError, HumanMailboxDirectoryService, HumanSentError, HumanSentService,
    HumanSessionClient, HumanSessionError, HumanSessionGovernanceError,
    HumanSessionGovernanceService, HumanSessionRevocationCause, HumanSessionService, InboxError,
    InboxService, IngestMailEngineEventsCommand, IssueScimTokenCommand, IssueScimTokenService,
    LegalHoldObjectEvidence, ListAgentProcessingDeadLettersCommand, ListApprovalRequestsCommand,
    ListHumanDraftsCommand, ListHumanMailboxDirectoryCommand, ListHumanSentSubmissionsCommand,
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
    RevokeOwnBrowserSessionsCommand, RevokePrincipalBrowserSessionsCommand,
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
    AgentEventCursorPageResponse, AgentEventEnvelopeResponse, AgentInspectorControlPlaneResponse,
    AgentInspectorOwnerResponse, AgentInspectorProfileResponse, AgentInspectorResponse,
    AgentInspectorTokenAccessResponse, AgentMailContentResponse, AgentMailPolicyActivationRequest,
    AgentMailPolicyActivationResponse, AgentMailPolicyVersionCreateRequest,
    AgentMailPolicyVersionResponse, AgentMailSandboxCreateRequest, AgentMailSandboxResponse,
    AgentMailSubmissionEvidenceAdmissionResponse,
    AgentMailSubmissionEvidenceAuditReferenceResponse, AgentMailSubmissionEvidenceDeliveryResponse,
    AgentMailSubmissionEvidencePolicyResponse, AgentMailSubmissionEvidenceResponse,
    AgentMailSubmissionEvidenceSubmissionResponse, AgentMessageAcknowledgementRequest,
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
    HumanSentDeliverySummaryResponse, HumanSentListItemResponse, HumanSentListResponse,
    HumanSessionResponse, InboxAddressResponse, InboxAttachmentResponse,
    InboxMessageSummaryResponse, InboxPageResponse, InboxSearchRequest, LegalHoldApplyRequest,
    LegalHoldListResponse, LegalHoldReleaseRequest, LegalHoldResponse,
    LogoutAllHumanSessionsRequest, MailDeliveryRecipientResponse, MailDeliveryResponse,
    MailDomainActionResponse, MailDomainChallengeRotationRequest,
    MailDomainChallengeRotationResponse, MailDomainCreateRequest, MailDomainCreateResponse,
    MailDomainDnsVerificationRequest, MailDomainDnsVerificationResponse, MailDomainListResponse,
    MailDomainResponse, MailDomainRouteResponse, MailDomainRouteUpdateRequest,
    MailDomainSuspendRequest, MailEngineMailboxBindingResponse,
    MailEngineMailboxBindingUpdateRequest, MailEventIngressResponse, MailSubmissionCreateRequest,
    MailSubmissionResponse, MailboxCreateRequest, MailboxCreateResponse,
    MailboxDelegationCreateRequest, MailboxDelegationResponse, MailboxDelegationRevokedResponse,
    MailboxResponse, MessageDetailResponse, MessageStateResponse, MessageStateUpdateRequest,
    OidcOwnerBindingResponse, OrganizationBootstrapResponse, OrganizationResponse,
    OwnAgentMailHourlyUsageResponse, OwnAgentMailPolicyLimitsResponse, OwnAgentMailPolicyResponse,
    ReadinessResponse, RetentionPolicyActivationRequest, RetentionPolicyCreateRequest,
    RetentionPolicyResponse, RetentionPolicyVersionCreateRequest, RetentionPolicyVersionResponse,
    RevokePrincipalBrowserSessionsRequest, RoleAssignmentGrantRequest, RoleAssignmentListResponse,
    RoleAssignmentResponse, RoleAssignmentRevocationRequest, RoleListResponse, RoleResponse,
    ScimTokenIssuedResponse, ScimTokenRevokedResponse, SystemInfoResponse, TextDraftResponse,
    TextDraftSaveRequest, WebhookDeadLetterListResponse, WebhookDeadLetterReplayRequest,
    WebhookDeadLetterReplayResponse, WebhookDeadLetterResponse, WebhookSubscriptionCreateRequest,
    WebhookSubscriptionListResponse, WebhookSubscriptionResponse, WebhookSubscriptionUpdateRequest,
    parse_closed_json,
};
use copilotmail_dns::{DnsError, DnsVerifier, DnsVerifierConfig, HickoryDnsLookup};
use copilotmail_domain::{
    AgentEnvironment, AgentRiskLevel, ApprovalAction, ApprovalDecision, ApprovalRequestId,
    ApprovalStatus, AuditArchiveId, AuditCheckpointId, AuditExportGrant, AuditExportId,
    AuditOutcome, AuthenticationBinding, AuthorizationContext, ComplianceCase, ComplianceCaseId,
    ComplianceCaseStatus, CredentialId, DelegationId, DraftId, LeaseId, LegalHold, LegalHoldId,
    LegalHoldStatus, MailDataPlaneId, MailDomainId, MailDomainName, MailDomainRouteMode,
    MailDomainStatus, MailEngineProjectionStatus, MailSubmissionId, MailboxCapability, MailboxId,
    MailboxKind, OrganizationId, Permission, PrincipalId, PrincipalKind, PrincipalStatus,
    RetentionPolicy, RetentionPolicyId, RetentionPolicyVersion, RetentionPolicyVersionId,
    RetentionResourceKind, RoleAssignmentId, RoleId, ScimTokenId, SubscriptionId,
    WebhookSubscriptionId,
};
use copilotmail_mail_engine::{
    MailEngineWebhookError, StalwartJmapClient, StalwartJmapConfig, StalwartWebhookParser,
};
use copilotmail_object_store::{
    ArchivedObjectEvidence, S3WormArchive, S3WormArchiveConfig, WormArchiveError, WormObjectSource,
    WormRetentionMode,
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

mod audit_maintenance;
mod cli_admin;
mod cli_audit;
mod cli_setup;
mod serve;
mod shutdown;
mod state;
mod web_auth;
mod web_errors;
mod web_router;
mod web_session;
mod web_token;

pub use audit_maintenance::*;
pub use cli_admin::*;
pub use cli_audit::*;
pub use cli_setup::*;
pub use serve::*;
pub(crate) use shutdown::*;
pub(crate) use state::*;
pub(crate) use web_auth::*;
pub(crate) use web_errors::*;
pub(crate) use web_router::*;
pub(crate) use web_session::*;
pub(crate) use web_token::*;

#[cfg(test)]
mod tests;
