//! Application use cases and infrastructure ports for the V2 control plane.

#![forbid(unsafe_code)]

mod agent_event_cursor;
mod agent_identity;
mod agent_ingress;
mod agent_inspector;
mod agent_mail_content;
mod agent_mail_policy;
mod agent_mail_policy_query;
mod agent_mail_sandbox;
mod agent_mail_submission_evidence;
mod agent_mcp;
mod agent_processing;
mod approval;
mod audit_archive;
mod audit_checkpoint;
mod audit_export;
mod audit_query;
mod authentication;
mod bind_oidc_owner;
mod bootstrap_organization;
mod compliance;
mod draft;
mod events;
mod human_draft;
mod human_login_failure;
mod human_mailbox_directory;
mod human_sent;
mod human_session;
mod human_session_governance;
mod inbox;
mod mail_delivery;
mod mail_domain;
mod mail_event_ingestion;
pub mod mail_ports;
mod mail_submission;
mod mailbox_authorization;
mod ports;
mod register_principal;
mod retention;
mod role_governance;
mod scim;
mod webhook_delivery;
mod webhook_subscription;

pub use agent_event_cursor::{
    AgentEventCursorError, AgentEventCursorPage, AgentEventCursorService,
    OpenAgentEventStreamCommand, PullAgentEventsCommand,
};
pub use agent_identity::{
    AddAgentCredentialCommand, AddAgentCredentialResult, AddAgentCredentialService,
    AddAgentMtlsCredentialCommand, AddAgentMtlsCredentialService, AgentIdentityError,
    RegisterAgentCommand, RegisterAgentResult, RegisterAgentService, RevokeAgentCredentialCommand,
    RevokeAgentCredentialService,
};
pub use agent_ingress::{AgentIngressRateLimitError, AgentIngressRateLimiter};
pub use agent_inspector::{
    AGENT_INSPECTOR_SNAPSHOT_VERSION, AgentInspectorAgent, AgentInspectorControlPlane,
    AgentInspectorError, AgentInspectorMailAdmissionState, AgentInspectorOwner,
    AgentInspectorProfile, AgentInspectorReviewState, AgentInspectorService,
    AgentInspectorSnapshot, AgentInspectorTokenAccess, AgentInspectorTokenAccessState,
    GetAgentInspectorCommand,
};
pub use agent_mail_content::{
    AgentMailContent, AgentMailContentError, AgentMailContentService, ReadAgentMailContentCommand,
};
pub use agent_mail_policy::{
    ActivateAgentMailPolicyCommand, AgentMailPolicyError, AgentMailPolicyService,
    CreateAgentMailPolicyVersionCommand, GetAgentMailPolicyVersionCommand,
};
pub use agent_mail_policy_query::{
    AgentMailPolicyQueryError, AgentMailPolicyQueryService, GetOwnAgentMailPolicyCommand,
};
pub use agent_mail_sandbox::{
    AgentMailSandboxError, AgentMailSandboxService, CaptureAgentMailSandboxCommand,
    CapturedAgentMailSandboxSubmission,
};
pub use agent_mail_submission_evidence::{
    AgentMailSubmissionAdmissionEvidence, AgentMailSubmissionAuditReference,
    AgentMailSubmissionDeliveryEvidence, AgentMailSubmissionEvidence,
    AgentMailSubmissionEvidenceError, AgentMailSubmissionEvidenceService,
    AgentMailSubmissionStatusEvidence, GetAgentMailSubmissionEvidenceCommand,
};
pub use agent_mcp::{AgentMcpSessionError, AgentMcpSessionService, OpenAgentMcpSessionCommand};
pub use agent_processing::{
    AcknowledgeAgentMessageCommand, AgentProcessingDeadLetterPage, AgentProcessingError,
    AgentProcessingService, ChangeAgentProcessingSubscriptionCommand, ClaimAgentMessageCommand,
    EnsureAgentProcessingSubscriptionCommand, ExtendAgentMessageLeaseCommand,
    ListAgentProcessingDeadLettersCommand, NackAgentMessageCommand,
};
pub use approval::{
    AppendApprovalPolicyVersionCommand, ApprovalGateService, ApprovalPolicyService,
    ApprovalRequestPage, ApprovalWorkflowError, ApprovalWorkflowService,
    CancelApprovalRequestCommand, CreateApprovalPolicyCommand, CreateApprovalPolicyResult,
    DecideApprovalCommand, DecideApprovalResult, GetApprovalRequestCommand,
    ListApprovalRequestsCommand, RequestApprovalCommand,
};
pub use audit_archive::{
    RecordAuditArchiveCommand, RecordAuditArchiveError, RecordAuditArchiveResult,
    RecordAuditArchiveService,
};
pub use audit_checkpoint::{
    CreateAuditCheckpointCommand, CreateAuditCheckpointError, CreateAuditCheckpointResult,
    CreateAuditCheckpointService,
};
pub use audit_export::{
    AuditExportAuthorizationError, AuditExportAuthorizationService, AuditExportDownloadError,
    AuditExportDownloadService, AuthorizeAuditExportCommand, AuthorizedAuditExportObject,
    RecordAuditExportDownloadCommand, ResolveAuditExportObjectCommand,
};
pub use audit_query::{
    AuditEventSearchError, AuditEventSearchPage, AuditEventSearchService, SearchAuditEventsCommand,
};
pub use authentication::{
    AccessTokenAuthenticator, AuthenticatedIdentity, AuthenticatedPrincipalKind,
    AuthenticationContext, AuthenticationError,
};
pub use bind_oidc_owner::{
    BindOidcOwnerCommand, BindOidcOwnerError, BindOidcOwnerResult, BindOidcOwnerService,
};
pub use bootstrap_organization::{
    BootstrapOrganizationCommand, BootstrapOrganizationError, BootstrapOrganizationResult,
    BootstrapOrganizationService,
};
pub use compliance::{
    ApplyLegalHoldCommand, CloseComplianceCaseCommand, ComplianceGovernanceError,
    ComplianceGovernanceService, ComplianceReadKind, CreateComplianceCaseCommand,
    RecordComplianceReadCommand, ReleaseLegalHoldCommand,
};
pub use draft::{DraftError, DraftService, SaveTextDraftCommand, SavedTextDraft};
pub use events::PendingEvent;
pub use human_draft::{
    HumanDraftAttemptListItem, HumanDraftAttemptPage, HumanDraftAttemptService, HumanDraftError,
    HumanDraftListItem, HumanDraftPage, HumanDraftService, ListHumanDraftAttemptsCommand,
    ListHumanDraftsCommand, RestoreHumanDraftCommand, RestoredHumanDraft,
};
pub use human_login_failure::{
    HumanLoginFailureError, HumanLoginFailureService, RecordHumanLoginFailureCommand,
};
pub use human_mailbox_directory::{
    HumanMailboxDirectoryError, HumanMailboxDirectoryItem, HumanMailboxDirectoryPage,
    HumanMailboxDirectoryService, ListHumanMailboxDirectoryCommand,
};
pub use human_sent::{
    HumanSentDeliverySummary, HumanSentError, HumanSentListItem, HumanSentPage, HumanSentService,
    ListHumanSentSubmissionsCommand,
};
pub use human_session::{
    BeginHumanLoginCommand, ConsumeHumanLoginCommand, ConsumedHumanLogin,
    CreateHumanSessionCommand, HumanSessionError, HumanSessionService, ListOwnHumanSessionsCommand,
    OwnHumanSessionListEntry, ResolveHumanSessionCommand, RevokeHumanSessionCommand,
    RevokedHumanSession,
};
pub use human_session_governance::{
    AcceptedBrowserSessionRevocation, HumanSessionGovernanceError, HumanSessionGovernanceService,
    RevokeOwnBrowserSessionsCommand, RevokePrincipalBrowserSessionsCommand,
};
pub use inbox::{
    GetMessageCommand, HumanInboxMessageSummary, HumanInboxPage, HumanMessageDetail, InboxError,
    InboxService, ListInboxCommand, SearchInboxCommand, UpdateMessageStateCommand,
};
pub use mail_delivery::{
    MailDeliveryReconciliationError, MailDeliveryReconciliationResult,
    MailDeliveryReconciliationService, ReconcileMailDeliveryCommand,
};
pub use mail_domain::{
    ActivateMailDomainCommand, MailDomainGovernanceError, MailDomainGovernanceService,
    RecordMailDomainDnsCommand, RegisterMailDomainCommand, RegisterMailDomainResult,
    RotateMailDomainChallengeCommand, RotateMailDomainChallengeResult, SuspendMailDomainCommand,
    UpdateMailDomainRouteCommand, UpdateMailDomainRouteResult,
};
pub use mail_event_ingestion::{
    BindMailEngineMailboxCommand, BindMailEngineMailboxResult, InboundMailEventIngestionService,
    IngestInboundMailEventsCommand, IngestInboundMailEventsResult, IngestMailEngineEventsCommand,
    LegacyMailEventSource, MailDataPlaneFingerprintScheme, MailEngineEventIngestionService,
    MailEngineMailboxBindingService, MailEventFingerprintScheme, MailEventIngestionError,
};
pub use mail_submission::{
    ExecuteMailSubmissionCommand, MailSubmissionError, MailSubmissionExecutionError,
    MailSubmissionExecutionResult, MailSubmissionExecutor, MailSubmissionService,
    RequestMailSubmissionCommand,
};
pub use mailbox_authorization::{
    AuthorizeMailboxActionService, CreateMailboxCommand, CreateMailboxResult,
    GrantMailboxDelegationCommand, GrantMailboxDelegationResult, MailboxAuthorizationError,
    MailboxDelegationService, MailboxManagementService, RevokeMailboxDelegationCommand,
};
pub use ports::{
    ActiveApprovalPolicy, AgentCredentialStore, AgentCursorEvent, AgentEventCursorQueryStore,
    AgentEventStreamLease, AgentEventStreamLeaseAcquisition, AgentIngressRateLimitDecision,
    AgentIngressRateLimitStore, AgentInspectorControlPlaneCandidate,
    AgentInspectorPrincipalCandidate, AgentInspectorProfileCandidate,
    AgentInspectorSnapshotCandidate, AgentInspectorTokenAccessCandidate, AgentMailAdmissionFailure,
    AgentMailAdmissionRecord, AgentMailContentQueryStore, AgentMailContentTarget,
    AgentMailPolicyQueryStore, AgentMailPolicyRuntimeSnapshot, AgentMailPolicyVersionRecord,
    AgentMailSandboxSubmissionCreation, AgentMailSandboxSubmissionRecord,
    AgentMailSandboxSubmissionWrite, AgentMailSubmissionAdmissionEvidenceCandidate,
    AgentMailSubmissionAuditReferenceCandidate, AgentMailSubmissionDeliveryEvidenceCandidate,
    AgentMailSubmissionEvidenceCandidate, AgentMessageClaimWrite, AgentMessageProcessingOperation,
    AgentMessageProcessingResult, AgentMessageProcessingState, AgentMessageTransitionWrite,
    AgentProcessingDeadLetterBatch, AgentProcessingDeadLetterQuery,
    AgentProcessingDeadLetterRecord, AgentProcessingQueryStore,
    AgentProcessingSubscriptionChangeResult, AgentProcessingSubscriptionChangeWrite,
    AgentProcessingSubscriptionRecord, AgentProcessingSubscriptionStatus,
    ApprovalCancellationWrite, ApprovalDecisionRecord, ApprovalDecisionWrite, ApprovalProof,
    ApprovalQueryStore, ApprovalRequestDetail, ApprovalRequestListQuery, ApprovalRequestSummary,
    ApprovalRequestView, ApprovalStepSummary, ApprovedRequestEvidence, ArchivedObjectRecord,
    AuditArchiveRecord, AuditCheckpointQueryStore, AuditEventQueryStore, AuditEventSearch,
    AuditEventSummary, AuditExportArtifact, AuditExportDownloadRecord, AuditExportQueryStore,
    AuthorizationStore, ComplianceQueryStore, ControlPlaneStore, ControlPlaneTransaction,
    DraftCreationAttemptWrite, ExistingAgentCredential, ExistingAgentMtlsCredential,
    ExistingAuditCheckpoint, ExistingHumanIdentity, ExistingOidcOwnerBinding,
    ExistingOrganizationBootstrap, ExistingScimGroup, ExistingScimUser, ExternalIdentityResolver,
    HumanDraftAttemptListBatch, HumanDraftAttemptListCandidate, HumanDraftAttemptListQuery,
    HumanDraftAttemptListRecheck, HumanDraftListBatch, HumanDraftListCandidate,
    HumanDraftListItemRecheck, HumanDraftListQuery, HumanDraftListRecheck, HumanDraftQueryStore,
    HumanDraftReadRecheck, HumanDraftRestoreQuery, HumanDraftRestoreResolution,
    HumanDraftRestoreTarget, HumanLoginAdmissionResult, HumanLoginFailureQuery,
    HumanLoginFailureReason, HumanLoginFailureRecord, HumanLoginFailureStore,
    HumanLoginFailureWrite, HumanLoginTransactionRecord, HumanLoginTransactionStore,
    HumanLoginTransactionWrite, HumanMailContentQueryStore, HumanMailContentTarget,
    HumanMailMessageProjection, HumanMailboxDirectoryBatch, HumanMailboxDirectoryCandidate,
    HumanMailboxDirectoryQuery, HumanMailboxDirectoryStore, HumanSentDeliveryEvidence,
    HumanSentListBatch, HumanSentListCandidate, HumanSentListItemRecheck, HumanSentListQuery,
    HumanSentListRecheck, HumanSentQueryStore, HumanSessionClient, HumanSessionGovernanceExecution,
    HumanSessionGovernanceExecutionWrite, HumanSessionListItem, HumanSessionQueryStore,
    HumanSessionRecord, HumanSessionRevocationCause, HumanSessionRevocationResult,
    HumanSessionWrite, InboundMailEventInsert, InboundMailEventRecord, LegalHoldObjectEvidence,
    MailDeliveryQueryStore, MailDeliveryRecipientState, MailDeliveryRecipientWrite,
    MailDeliveryReconciliationTarget, MailDeliverySnapshotInsert, MailDeliverySnapshotRecord,
    MailDeliverySnapshotWrite, MailDomainDnsObservation, MailDomainQueryStore, MailDomainSummary,
    MailDraftMetadataStore, MailDraftRecipientEvidence, MailDraftRecord, MailDraftStatus,
    MailDraftWrite, MailEngineMailboxBinding, MailEngineMailboxBindingWrite,
    MailEventIngestionStore, MailEventIngestionTransaction, MailReplyTarget,
    MailReplyTargetQueryStore, MailSubmissionIntentCreation, MailSubmissionIntentRecord,
    MailSubmissionIntentWrite, MailSubmissionLease, MailSubmissionQueryStore, MailSubmissionStatus,
    MailSubmissionTerminalTransition, MailSubmissionTerminalWrite, MailboxAuthorizationStore,
    MailboxQueryStore, MailboxSummary, OrganizationQueryStore, OrganizationSummary,
    OutboundMailEventInsert, OutboundMailEventKind, OutboundMailEventRecord,
    RetentionPolicyActivationRecord, RetentionPolicyDetail, RetentionPolicyQueryStore,
    RetentionPolicyRecord, RevokeActiveHumanSessions, RoleAssignmentGrantRecord,
    RoleAssignmentListQuery, RoleAssignmentRevocationResult, RoleAssignmentSummary,
    RoleGovernanceQueryStore, RoleSummary, ScimCredential, ScimDirectoryQueryStore, StoreError,
    WebhookDeadLetterQueryStore, WebhookDeadLetterSummary, WebhookSubscriptionQueryStore,
    WebhookSubscriptionSummary,
};
pub use register_principal::{
    RegisterPrincipalCommand, RegisterPrincipalError, RegisterPrincipalService,
};
pub use retention::{
    ActivateRetentionPolicyVersionCommand, AppendRetentionPolicyVersionCommand,
    CreateRetentionPolicyCommand, GetRetentionPolicyCommand, RetentionPolicyDraft,
    RetentionPolicyError, RetentionPolicyService,
};
pub use role_governance::{
    GrantRoleAssignmentCommand, ListRoleAssignmentsCommand, ListRolesCommand,
    RevokeRoleAssignmentCommand, RoleAssignmentPage, RoleGovernanceError, RoleGovernanceService,
    role_assignment_approval_resource_id,
};
pub use scim::{
    DeleteScimGroupCommand, IssueScimTokenCommand, IssueScimTokenService,
    ProvisionScimGroupCommand, ProvisionScimGroupResult, ProvisionScimUserCommand,
    ProvisionScimUserResult, ReplaceScimGroupCommand, ReplaceScimUserCommand,
    RevokeScimTokenCommand, RevokeScimTokenService, ScimGroupService, ScimProvisioningError,
    ScimUserService,
};
pub use webhook_delivery::{
    ReplayWebhookDeadLetterCommand, ReplayWebhookDeadLetterError, ReplayWebhookDeadLetterResult,
    WebhookDeadLetterService,
};
pub use webhook_subscription::{
    CreateWebhookSubscriptionCommand, UpdateWebhookSubscriptionCommand, WebhookSubscriptionError,
    WebhookSubscriptionMutationResult, WebhookSubscriptionService,
};
