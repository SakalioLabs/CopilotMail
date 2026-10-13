//! PostgreSQL implementation of CopilotMail control-plane persistence ports.

#![forbid(unsafe_code)]

mod repositories;
mod schema;
mod transaction_authorization;
mod transactions;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use copilotmail_application::{
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
    AuditEventSummary, AuditExportDownloadRecord, AuditExportQueryStore, AuthorizationStore,
    ComplianceQueryStore, ControlPlaneStore, ControlPlaneTransaction, DraftCreationAttemptWrite,
    ExistingAgentCredential, ExistingAgentMtlsCredential, ExistingAuditCheckpoint,
    ExistingHumanIdentity, ExistingOidcOwnerBinding, ExistingOrganizationBootstrap,
    ExistingScimGroup, ExistingScimUser, ExternalIdentityResolver, HumanDraftAttemptListBatch,
    HumanDraftAttemptListCandidate, HumanDraftAttemptListQuery, HumanDraftAttemptListRecheck,
    HumanDraftListBatch, HumanDraftListCandidate, HumanDraftListQuery, HumanDraftListRecheck,
    HumanDraftQueryStore, HumanDraftReadRecheck, HumanDraftRestoreQuery,
    HumanDraftRestoreResolution, HumanDraftRestoreTarget, HumanLoginAdmissionResult,
    HumanLoginFailureQuery, HumanLoginFailureReason, HumanLoginFailureRecord,
    HumanLoginFailureStore, HumanLoginFailureWrite, HumanLoginTransactionRecord,
    HumanLoginTransactionStore, HumanLoginTransactionWrite, HumanMailContentQueryStore,
    HumanMailContentTarget, HumanMailMessageProjection, HumanMailboxDirectoryBatch,
    HumanMailboxDirectoryCandidate, HumanMailboxDirectoryQuery, HumanMailboxDirectoryStore,
    HumanSentDeliveryEvidence, HumanSentListBatch, HumanSentListCandidate, HumanSentListQuery,
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
    OutboundMailEventInsert, OutboundMailEventKind, OutboundMailEventRecord, PendingEvent,
    RetentionPolicyActivationRecord, RetentionPolicyDetail, RetentionPolicyQueryStore,
    RetentionPolicyRecord, RevokeActiveHumanSessions, RoleAssignmentGrantRecord,
    RoleAssignmentListQuery, RoleAssignmentRevocationResult, RoleAssignmentSummary,
    RoleGovernanceQueryStore, RoleSummary, ScimCredential, ScimDirectoryQueryStore, StoreError,
    WebhookDeadLetterQueryStore, WebhookDeadLetterSummary, WebhookSubscriptionQueryStore,
    WebhookSubscriptionSummary,
};
use copilotmail_audit::{
    AuditArtifactError, AuditCheckpointCandidate, AuditExportRecord, SignedAuditCheckpoint,
    decode_sha256,
};
use copilotmail_domain::{
    AgentCredential, AgentEnvironment, AgentMailPolicyActivation, AgentMailPolicyActivationId,
    AgentMailPolicyVersion, AgentMtlsCredential, AgentProfile, AgentRiskLevel, ApprovalAction,
    ApprovalDecision, ApprovalPolicyVersion, ApprovalRequest, ApprovalRequestId, ApprovalStatus,
    AuditArchiveId, AuditCheckpointId, AuditEvent, AuditExportGrant, AuditExportId, AuditOutcome,
    AuthenticationBinding, AuthorizationContext, ComplianceCase, ComplianceCaseId,
    ComplianceCaseStatus, CredentialId, DelegationId, DraftCreationAttemptId, DraftId,
    HumanLoginFailureId, HumanLoginTransactionId, HumanProfile, HumanSessionId,
    HumanSessionRevocationId, LeaseId, LegalHold, LegalHoldId, LegalHoldStatus, MailDataPlaneId,
    MailDomain, MailDomainId, MailDomainName, MailDomainRoute, MailDomainRouteMode,
    MailDomainStatus, MailEngineProjectionStatus, MailSubmissionId, Mailbox, MailboxAccessSnapshot,
    MailboxCapability, MailboxDelegation, MailboxId, MailboxKind, MailboxMembership,
    MailboxMembershipRole, Organization, OrganizationId, OrganizationSlug, Permission, PolicyId,
    PolicyVersionId, Principal, PrincipalId, PrincipalKind, PrincipalStatus,
    ResolvedMailboxDelegation, RetentionPolicy, RetentionPolicyActivationId, RetentionPolicyId,
    RetentionPolicyVersion, RetentionPolicyVersionId, RetentionResourceKind, Role, RoleAssignment,
    RoleAssignmentId, RoleId, ScimGroupId, ScimTokenId, SubscriptionId, SystemRole,
    WebhookEventType, WebhookSubscription, WebhookSubscriptionId,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{
    PgPool, Postgres, Transaction, migrate::Migrator, pool::PoolConnection, postgres::PgPoolOptions,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// PostgreSQL-backed control-plane store.
#[derive(Clone)]
pub struct PgControlPlaneStore {
    pool: PgPool,
}

/// Owns the PostgreSQL session holding one tenant's audit-maintenance lock.
///
/// Dropping without an explicit successful release evicts the connection from
/// the pool, so a session lock can never leak into a later borrower.
pub struct AuditMaintenanceLock {
    connection: Option<PoolConnection<Postgres>>,
    key: i64,
}

/// Owns the PostgreSQL session that serializes remote Legal Hold changes for
/// one exact tenant archive across all control-plane hosts.
pub struct AuditLegalHoldLock {
    connection: Option<PoolConnection<Postgres>>,
    key: i64,
}

impl AuditLegalHoldLock {
    pub async fn release(mut self) -> Result<(), sqlx::Error> {
        let mut connection = self
            .connection
            .take()
            .expect("audit Legal Hold lock connection is present");
        let unlocked = sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
            .bind(self.key)
            .fetch_one(&mut *connection)
            .await;
        match unlocked {
            Ok(true) => Ok(()),
            Ok(false) => {
                connection.close_on_drop();
                Err(sqlx::Error::Protocol(
                    "PostgreSQL audit Legal Hold advisory lock was not held".into(),
                ))
            }
            Err(error) => {
                connection.close_on_drop();
                Err(error)
            }
        }
    }
}

impl Drop for AuditLegalHoldLock {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.as_mut() {
            connection.close_on_drop();
        }
    }
}

impl AuditMaintenanceLock {
    /// Releases the advisory lock and returns the healthy session to the pool.
    pub async fn release(mut self) -> Result<(), sqlx::Error> {
        let mut connection = self
            .connection
            .take()
            .expect("audit maintenance lock connection is present");
        let unlocked = sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
            .bind(self.key)
            .fetch_one(&mut *connection)
            .await;
        match unlocked {
            Ok(true) => Ok(()),
            Ok(false) => {
                connection.close_on_drop();
                Err(sqlx::Error::Protocol(
                    "PostgreSQL audit maintenance advisory lock was not held".into(),
                ))
            }
            Err(error) => {
                connection.close_on_drop();
                Err(error)
            }
        }
    }
}

impl Drop for AuditMaintenanceLock {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.as_mut() {
            connection.close_on_drop();
        }
    }
}

/// Evidence returned after recomputing one tenant's complete audit chain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuditChainVerificationReport {
    pub organization_id: OrganizationId,
    pub event_count: u64,
    pub first_event_id: Option<Uuid>,
    pub last_event_id: Option<Uuid>,
    pub head_hash: Option<String>,
    pub hash_version: Option<i16>,
}

/// A database read failure or a precise audit-chain integrity violation.
#[derive(Debug, Error)]
pub enum AuditChainVerificationError {
    #[error("read audit chain: {0}")]
    Database(#[from] sqlx::Error),
    #[error(
        "audit chain integrity violation at event {event_id} sequence {chain_sequence}: {reason}"
    )]
    Integrity {
        event_id: Uuid,
        chain_sequence: i64,
        reason: String,
    },
}

#[derive(Clone, Debug)]
pub struct StoredAuditCheckpoint {
    pub checkpoint: SignedAuditCheckpoint,
    pub archive_object_key: String,
}

#[derive(Debug, Error)]
pub enum AuditArtifactStoreError {
    #[error("read audit artifact: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Artifact(#[from] AuditArtifactError),
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct AuditChainRow {
    id: Uuid,
    organization_id: Uuid,
    chain_sequence: i64,
    hash_version: i16,
    hash_payload: String,
    previous_hash: Option<Vec<u8>>,
    event_hash: Vec<u8>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct AuditEventSummaryRow {
    event_id: Uuid,
    organization_id: Uuid,
    actor_principal_id: Uuid,
    actor_kind: String,
    on_behalf_of_principal_id: Option<Uuid>,
    credential_id: Option<String>,
    subject_principal_id: Option<Uuid>,
    action: String,
    resource_type: String,
    resource_id: String,
    outcome: String,
    trace_id: String,
    request_id: String,
    source_ip: Option<String>,
    client_id: Option<String>,
    session_id: Option<String>,
    token_jti: Option<String>,
    policy_id: Option<Uuid>,
    policy_version_id: Option<Uuid>,
    approval_request_id: Option<Uuid>,
    ticket_reference: Option<String>,
    environment: Option<String>,
    region: Option<String>,
    details: Value,
    occurred_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
    chain_sequence: i64,
    hash_version: i16,
    previous_hash_hex: Option<String>,
    event_hash_hex: String,
    signature_batch_id: Option<Uuid>,
}

#[derive(Debug, sqlx::FromRow)]
struct StoredAuditCheckpointRow {
    signed_payload: String,
    signature_algorithm: String,
    signature: Vec<u8>,
    object_key: String,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct MailDomainSummaryRow {
    mail_domain_id: Uuid,
    organization_id: Uuid,
    name: String,
    status: String,
    route_mode: String,
    ownership_challenge_version: i64,
    route_version: i64,
    inbound_enabled: bool,
    outbound_enabled: bool,
    smart_host_ref: Option<String>,
    dkim_key_ref: Option<String>,
    mail_engine_domain_ref: Option<String>,
    mail_engine_projection_status: String,
    mail_engine_projected_enabled: Option<bool>,
    mail_engine_projection_last_attempt_at: Option<DateTime<Utc>>,
    mail_engine_projection_succeeded_at: Option<DateTime<Utc>>,
    mail_engine_projection_error_code: Option<String>,
    latest_dns_observed_at: Option<DateTime<Utc>>,
    latest_dns_valid_until: Option<DateTime<Utc>>,
    latest_dns_activation_ready: bool,
    latest_dns_failure_codes: Vec<String>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct WebhookSubscriptionRow {
    subscription_id: Uuid,
    organization_id: Uuid,
    principal_id: Uuid,
    endpoint_url: String,
    event_types: Vec<String>,
    signing_key_reference: String,
    enabled: bool,
    version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct WebhookDeadLetterRow {
    delivery_id: Uuid,
    subscription_id: Uuid,
    organization_id: Uuid,
    principal_id: Uuid,
    event_id: Uuid,
    event_type: String,
    attempt: i32,
    response_status: Option<i32>,
    error_code: String,
    failed_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct AgentCursorEventRow {
    event_id: Uuid,
    organization_id: Uuid,
    event_type: String,
    aggregate_type: String,
    aggregate_id: String,
    schema_version: i32,
    payload: Value,
    occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct ApprovalRequestSummaryRow {
    id: Uuid,
    organization_id: Uuid,
    policy_version_id: Uuid,
    action: String,
    resource_kind: String,
    resource_id: String,
    requested_by: Uuid,
    reason: String,
    ticket_reference: Option<String>,
    required_approvals: i16,
    approval_count: i64,
    rejection_count: i64,
    effective_status: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    cancelled_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct ApprovalStepSummaryRow {
    id: Uuid,
    approver_principal_id: Uuid,
    decision: String,
    reason: String,
    decided_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct RoleAssignmentSummaryRow {
    id: Uuid,
    organization_id: Uuid,
    principal_id: Uuid,
    principal_kind: String,
    principal_status: String,
    principal_display_name: String,
    role_id: Uuid,
    system_key: String,
    role_display_name: String,
    created_by: Uuid,
    approval_request_id: Option<Uuid>,
    valid_from: DateTime<Utc>,
    valid_until: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct AuditExportGrantRow {
    id: Uuid,
    organization_id: Uuid,
    archive_id: Uuid,
    approval_request_id: Uuid,
    requested_by: Uuid,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct HumanLoginTransactionRow {
    id: Uuid,
    client_key: String,
    return_to: String,
    encrypted_pkce_verifier: Vec<u8>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct HumanSessionRow {
    id: Uuid,
    organization_id: Uuid,
    principal_id: Uuid,
    login_transaction_id: Uuid,
    client_key: String,
    oidc_jti: Option<String>,
    upstream_expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct MailDraftRow {
    id: Uuid,
    organization_id: Uuid,
    mailbox_id: Uuid,
    created_by: Uuid,
    mail_data_plane_id: Uuid,
    engine_account_ref: String,
    engine_email_ref: String,
    engine_thread_ref: String,
    reply_to_message_id: Option<Uuid>,
    reply_to_automatic: bool,
    body_ref: Vec<u8>,
    content_sha256: Vec<u8>,
    recipient_refs: Option<Vec<Vec<u8>>>,
    recipient_domains: Option<Vec<String>>,
    version: i64,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct MailSubmissionIntentRow {
    id: Uuid,
    organization_id: Uuid,
    requested_by: Uuid,
    mailbox_id: Uuid,
    draft_id: Uuid,
    draft_version: i64,
    mail_data_plane_id: Uuid,
    request_sha256: Vec<u8>,
    status: String,
    state_version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct AgentMailSandboxSubmissionRow {
    id: Uuid,
    organization_id: Uuid,
    agent_id: Uuid,
    mailbox_id: Uuid,
    draft_id: Uuid,
    draft_version: i64,
    request_sha256: Vec<u8>,
    recipient_count: i32,
    captured_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct AgentMailAdmissionRow {
    id: Uuid,
    organization_id: Uuid,
    agent_id: Uuid,
    submission_id: Uuid,
    policy_version_id: Uuid,
    policy_version: i32,
    recipient_count: i32,
    first_contact_count: i32,
    admitted_at: DateTime<Utc>,
}

/// One raw, tenant-bound database projection for the privileged Agent
/// Inspector. The mapper below deliberately validates every nullable Owner
/// field and every persisted enum before application code can disclose it.
#[derive(Clone, Debug, sqlx::FromRow)]
struct AgentInspectorSnapshotRow {
    organization_id: Uuid,
    agent_id: Uuid,
    agent_kind: String,
    agent_status: String,
    agent_display_name: String,
    agent_created_at: DateTime<Utc>,
    profile_owner_principal_id: Uuid,
    owner_id: Option<Uuid>,
    owner_organization_id: Option<Uuid>,
    owner_kind: Option<String>,
    owner_status: Option<String>,
    owner_display_name: Option<String>,
    environment: String,
    risk_level: String,
    purpose: String,
    review_due_at: DateTime<Utc>,
    active_credential_count: i64,
    next_credential_expiry_at: Option<DateTime<Utc>>,
    last_authenticated_at: Option<DateTime<Utc>>,
    effective_scopes: Vec<String>,
    credential_integrity_error: bool,
    has_active_production_mail_policy: bool,
    policy_integrity_error: bool,
    active_processing_subscription_count: i64,
    paused_processing_subscription_count: i64,
    retired_processing_subscription_count: i64,
    subscription_integrity_error: bool,
    last_processing_activity_at: Option<DateTime<Utc>>,
    observed_at: DateTime<Utc>,
}

/// One raw, tenant-bound production-Agent mail evidence projection. All
/// identifiers are internal until the mapper proves the exact immutable
/// admission, historical approvals, delivery counters, and audit references
/// are coherent.
#[derive(Clone, Debug, sqlx::FromRow)]
struct AgentMailSubmissionEvidenceRow {
    organization_id: Uuid,
    agent_id: Uuid,
    submission_id: Uuid,
    submission_status: String,
    submission_created_at: DateTime<Utc>,
    submission_updated_at: DateTime<Utc>,
    submission_accepted_at: Option<DateTime<Utc>>,
    admission_id: Uuid,
    admitted_at: DateTime<Utc>,
    policy_activation_id: Uuid,
    policy_activated_at: DateTime<Utc>,
    policy_activation_timing_valid: bool,
    policy_version_id: Uuid,
    policy_version: i32,
    policy_activation_approval_request_id: Uuid,
    policy_activation_approval_verified: bool,
    recipient_count: i32,
    first_contact_count: i32,
    first_contact_approval_request_id: Option<Uuid>,
    first_contact_approval_verified: bool,
    delivery_snapshot_id: Option<Uuid>,
    delivery_observed_at: Option<DateTime<Utc>>,
    delivery_terminal: Option<bool>,
    delivery_recipient_count: Option<i32>,
    delivery_delivered_count: Option<i32>,
    delivery_failed_count: Option<i32>,
    delivery_queued_count: Option<i32>,
    delivery_unknown_count: Option<i32>,
    requested_audit_event_id: Option<Uuid>,
    requested_audit_occurred_at: Option<DateTime<Utc>>,
    requested_audit_event_count: Option<i64>,
    admitted_audit_event_id: Option<Uuid>,
    admitted_audit_occurred_at: Option<DateTime<Utc>>,
    admitted_audit_event_count: Option<i64>,
    terminal_audit_event_id: Option<Uuid>,
    terminal_audit_action: Option<String>,
    terminal_audit_outcome: Option<String>,
    terminal_audit_occurred_at: Option<DateTime<Utc>>,
    terminal_audit_event_count: i64,
    observed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct AgentMessageProcessingOperationRow {
    id: Uuid,
    organization_id: Uuid,
    subscription_id: Uuid,
    message_id: Uuid,
    principal_id: Uuid,
    lease_id: Uuid,
    operation: String,
    request_sha256: Vec<u8>,
    result_state: String,
    attempt_count: i32,
    lease_expires_at: Option<DateTime<Utc>>,
    reason_code: Option<String>,
    occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct MailSubmissionLeaseRow {
    id: Uuid,
    organization_id: Uuid,
    requested_by: Uuid,
    mailbox_id: Uuid,
    draft_id: Uuid,
    draft_version: i64,
    mail_data_plane_id: Uuid,
    engine_account_ref: String,
    engine_email_ref: String,
    primary_address: String,
    lease_id: Uuid,
    lease_expires_at: DateTime<Utc>,
    attempt_count: i32,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct MailDeliveryTargetRow {
    id: Uuid,
    organization_id: Uuid,
    requested_by: Uuid,
    mailbox_id: Uuid,
    mail_data_plane_id: Uuid,
    engine_account_ref: String,
    engine_email_ref: String,
    engine_identity_ref: String,
    engine_submission_ref: String,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct MailDeliverySnapshotRow {
    id: Uuid,
    organization_id: Uuid,
    submission_id: Uuid,
    source_state: String,
    source_sha256: Vec<u8>,
    terminal: bool,
    observed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, sqlx::FromRow)]
struct MailDeliveryRecipientRow {
    recipient_ref: Vec<u8>,
    state: String,
    smtp_reply: String,
}

#[derive(Debug, sqlx::FromRow)]
struct RetentionPolicyRow {
    id: Uuid,
    organization_id: Uuid,
    policy_key: String,
    display_name: String,
    resource_kind: String,
    active_version_id: Option<Uuid>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    latest_version: i32,
}

#[derive(Debug, sqlx::FromRow)]
struct RetentionPolicyVersionRow {
    id: Uuid,
    policy_id: Uuid,
    organization_id: Uuid,
    version: i32,
    minimum_retention_days: i32,
    archive_after_days: Option<i32>,
    delete_after_days: Option<i32>,
    rule: Value,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct RetentionPolicyActivationRow {
    id: Uuid,
    organization_id: Uuid,
    policy_id: Uuid,
    policy_version_id: Uuid,
    approval_request_id: Uuid,
    activated_by: Uuid,
    activated_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
struct RetentionPolicyDetailRow {
    policy_id: Uuid,
    organization_id: Uuid,
    policy_key: String,
    display_name: String,
    resource_kind: String,
    created_by: Uuid,
    policy_created_at: DateTime<Utc>,
    latest_id: Uuid,
    latest_version: i32,
    latest_minimum_retention_days: i32,
    latest_archive_after_days: Option<i32>,
    latest_delete_after_days: Option<i32>,
    latest_rule: Value,
    latest_created_by: Uuid,
    latest_created_at: DateTime<Utc>,
    active_id: Option<Uuid>,
    active_version: Option<i32>,
    active_minimum_retention_days: Option<i32>,
    active_archive_after_days: Option<i32>,
    active_delete_after_days: Option<i32>,
    active_rule: Option<Value>,
    active_created_by: Option<Uuid>,
    active_created_at: Option<DateTime<Utc>>,
}

struct AuditChainVerificationState {
    organization_id: OrganizationId,
    event_count: u64,
    first_event_id: Option<Uuid>,
    last_event_id: Option<Uuid>,
    expected_previous_hash: Option<Vec<u8>>,
    hash_version: Option<i16>,
}

impl PgControlPlaneStore {
    /// Connects to PostgreSQL with a bounded pool and acquisition timeout.
    pub async fn connect(database_url: &str, max_connections: u32) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    /// Wraps an existing pool, primarily for integration tests and embedding.
    #[must_use]
    pub const fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns the underlying pool for read-only health integrations.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Applies all versioned control-plane migrations.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        MIGRATOR.run(&self.pool).await
    }

    /// Verifies that a database connection can execute a minimal query.
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Attempts to become the sole audit-maintenance process for one tenant.
    ///
    /// The lock is session scoped and therefore spans export and object-store
    /// I/O without holding a database transaction or an MVCC snapshot.
    pub async fn try_acquire_audit_maintenance_lock(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<AuditMaintenanceLock>, sqlx::Error> {
        let mut digest = Sha256::new();
        digest.update(b"copilotmail.audit-maintenance-lock.v1");
        digest.update(organization_id.as_uuid().as_bytes());
        let digest = digest.finalize();
        let key = i64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 digest always contains eight bytes"),
        );
        let mut connection = self.pool.acquire().await?;
        let acquired = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
            .bind(key)
            .fetch_one(&mut *connection)
            .await?;
        if acquired {
            Ok(Some(AuditMaintenanceLock {
                connection: Some(connection),
                key,
            }))
        } else {
            Ok(None)
        }
    }

    /// Attempts to serialize Legal Hold changes for one exact immutable
    /// archive while S3 and PostgreSQL are updated as a recoverable saga.
    pub async fn try_acquire_audit_legal_hold_lock(
        &self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditLegalHoldLock>, sqlx::Error> {
        let mut digest = Sha256::new();
        digest.update(b"copilotmail.audit-legal-hold-lock.v1");
        digest.update(organization_id.as_uuid().as_bytes());
        digest.update(archive_id.as_uuid().as_bytes());
        let digest = digest.finalize();
        let key = i64::from_be_bytes(
            digest[..8]
                .try_into()
                .expect("SHA-256 digest always contains eight bytes"),
        );
        let mut connection = self.pool.acquire().await?;
        let acquired = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
            .bind(key)
            .fetch_one(&mut *connection)
            .await?;
        if acquired {
            Ok(Some(AuditLegalHoldLock {
                connection: Some(connection),
                key,
            }))
        } else {
            Ok(None)
        }
    }

    /// Binds a deployment-specific canonical HTTP(S) origin to one registered
    /// data plane. The append-only database row makes retry exact and rejects
    /// both ID-to-origin changes and one origin being split across two IDs.
    pub async fn bind_mail_data_plane_origin(
        &self,
        data_plane_id: MailDataPlaneId,
        expected_adapter_kind: &str,
        origin: &str,
        bound_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        if !valid_mail_data_plane_adapter_kind(expected_adapter_kind) {
            return Err(StoreError::Conflict);
        }
        let canonical_origin = canonical_mail_data_plane_origin(origin)?;
        sqlx::query_scalar(
            r#"
            WITH inserted AS (
                INSERT INTO mail_data_plane_origin_bindings(
                    data_plane_id, canonical_origin, bound_at
                )
                SELECT plane.id, $3, $4
                  FROM mail_data_planes AS plane
                 WHERE plane.id = $1
                   AND plane.adapter_kind = $2
                   AND $4 >= plane.created_at
                ON CONFLICT(data_plane_id) DO NOTHING
                RETURNING data_plane_id
            )
            SELECT EXISTS(
                SELECT 1
                  FROM mail_data_planes AS plane
                  JOIN mail_data_plane_origin_bindings AS binding
                    ON binding.data_plane_id = plane.id
                 WHERE plane.id = $1
                   AND plane.adapter_kind = $2
                   AND binding.canonical_origin = $3
            )
            "#,
        )
        .bind(data_plane_id.as_uuid())
        .bind(expected_adapter_kind)
        .bind(canonical_origin)
        .bind(bound_at)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    /// Verifies the complete immutable data-plane identity before remote I/O.
    /// This method never performs trust-on-first-use registration.
    pub async fn mail_data_plane_registered(
        &self,
        data_plane_id: MailDataPlaneId,
        expected_adapter_kind: &str,
        origin: &str,
    ) -> Result<bool, StoreError> {
        if !valid_mail_data_plane_adapter_kind(expected_adapter_kind) {
            return Err(StoreError::Conflict);
        }
        let canonical_origin = canonical_mail_data_plane_origin(origin)?;
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM mail_data_planes AS plane
                  JOIN mail_data_plane_origin_bindings AS binding
                    ON binding.data_plane_id = plane.id
                 WHERE plane.id = $1
                   AND plane.adapter_kind = $2
                   AND binding.canonical_origin = $3
            )
            "#,
        )
        .bind(data_plane_id.as_uuid())
        .bind(expected_adapter_kind)
        .bind(canonical_origin)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }
    /// Returns true when all tables required by this binary are present.
    pub async fn schema_ready(&self) -> Result<bool, sqlx::Error> {
        schema::ready(&self.pool).await
    }

    /// Reads a tenant's persisted hash material in bounded pages and
    /// independently recomputes the complete chain without mutating data.
    pub async fn verify_audit_chain(
        &self,
        organization_id: OrganizationId,
    ) -> Result<AuditChainVerificationReport, AuditChainVerificationError> {
        const PAGE_SIZE: i64 = 1_000;
        let mut transaction = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await?;
        let mut state = AuditChainVerificationState::new(organization_id);
        let mut last_sequence = 0_i64;

        loop {
            let rows = sqlx::query_as::<_, AuditChainRow>(
                r#"
                SELECT id, organization_id, chain_sequence, hash_version,
                       hash_payload, previous_hash, event_hash
                  FROM audit_events
                 WHERE organization_id = $1
                   AND chain_sequence > $2
                 ORDER BY chain_sequence
                 LIMIT $3
                "#,
            )
            .bind(organization_id.as_uuid())
            .bind(last_sequence)
            .bind(PAGE_SIZE)
            .fetch_all(&mut *transaction)
            .await?;

            for row in &rows {
                state.verify_row(row)?;
                last_sequence = row.chain_sequence;
            }
            if rows.len() < usize::try_from(PAGE_SIZE).expect("positive page size") {
                break;
            }
        }

        transaction.commit().await?;
        Ok(state.finish())
    }

    /// Loads one immutable signed checkpoint for portable export.
    pub async fn get_signed_audit_checkpoint(
        &self,
        organization_id: OrganizationId,
        checkpoint_id: AuditCheckpointId,
    ) -> Result<Option<StoredAuditCheckpoint>, AuditArtifactStoreError> {
        let row = sqlx::query_as::<_, StoredAuditCheckpointRow>(
            r#"
            SELECT signed_payload, signature_algorithm, signature, object_key
              FROM audit_checkpoints
             WHERE organization_id = $1
               AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(checkpoint_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        row.map(stored_audit_checkpoint_from_row).transpose()
    }

    /// Returns the oldest checkpoint without committed WORM archive evidence.
    ///
    /// Maintenance workers resume this record before signing a newer chain
    /// head, preserving the exact export manifest across remote/DB retries.
    pub async fn oldest_unarchived_audit_checkpoint(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<StoredAuditCheckpoint>, AuditArtifactStoreError> {
        let row = sqlx::query_as::<_, StoredAuditCheckpointRow>(
            r#"
            SELECT checkpoint.signed_payload,
                   checkpoint.signature_algorithm,
                   checkpoint.signature,
                   checkpoint.object_key
              FROM audit_checkpoints AS checkpoint
              LEFT JOIN audit_archives AS archive
                ON archive.organization_id = checkpoint.organization_id
               AND archive.checkpoint_id = checkpoint.id
             WHERE checkpoint.organization_id = $1
               AND archive.id IS NULL
             ORDER BY checkpoint.chain_sequence_end, checkpoint.id
             LIMIT 1
            "#,
        )
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        row.map(stored_audit_checkpoint_from_row).transpose()
    }

    /// Loads a bounded, ordered page of portable chain material.
    pub async fn load_audit_export_page(
        &self,
        organization_id: OrganizationId,
        after_sequence: i64,
        through_sequence: i64,
        limit: i64,
    ) -> Result<Vec<AuditExportRecord>, AuditArtifactStoreError> {
        if after_sequence < 0 || through_sequence <= after_sequence || !(1..=1_000).contains(&limit)
        {
            return Err(AuditArtifactError::InvalidFormat.into());
        }
        let rows = sqlx::query_as::<_, AuditChainRow>(
            r#"
            SELECT id, organization_id, chain_sequence, hash_version,
                   hash_payload, previous_hash, event_hash
              FROM audit_events
             WHERE organization_id = $1
               AND chain_sequence > $2
               AND chain_sequence <= $3
             ORDER BY chain_sequence
             LIMIT $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(after_sequence)
        .bind(through_sequence)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                AuditExportRecord::from_hash_bytes(
                    row.id,
                    OrganizationId::from_uuid(row.organization_id),
                    row.chain_sequence,
                    row.hash_version,
                    row.hash_payload,
                    row.previous_hash.as_deref(),
                    &row.event_hash,
                )
                .map_err(AuditArtifactStoreError::from)
            })
            .collect()
    }

    /// Claims a recoverable batch of unpublished outbox events.
    pub async fn load_mail_domain_projection_target(
        &self,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomainProjectionTarget>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            mail_domain_id: Uuid,
            organization_id: Uuid,
            name: String,
            status: String,
            mail_data_plane_id: Option<Uuid>,
            mail_engine_domain_ref: Option<String>,
            mail_engine_projection_last_attempt_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id AS mail_domain_id, organization_id, name, status,
                   mail_data_plane_id, mail_engine_domain_ref,
                   mail_engine_projection_last_attempt_at
              FROM mail_domains
             WHERE id = $1
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(MailDomainProjectionTarget {
                mail_domain_id: MailDomainId::from_uuid(row.mail_domain_id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                name: MailDomainName::parse(row.name).map_err(|error| {
                    tracing::error!(%error, "invalid mail domain name persisted");
                    StoreError::Unavailable
                })?,
                status: mail_domain_status_from_str(&row.status)?,
                mail_data_plane_id: row.mail_data_plane_id.map(MailDataPlaneId::from_uuid),
                mail_engine_domain_ref: row.mail_engine_domain_ref,
                projection_last_attempt_at: row.mail_engine_projection_last_attempt_at,
            })
        })
        .transpose()
    }

    pub async fn mark_mail_domain_projection_attempt(
        &self,
        mail_domain_id: MailDomainId,
        expected_status: MailDomainStatus,
        expected_data_plane_id: MailDataPlaneId,
        expected_previous_attempt_at: Option<DateTime<Utc>>,
        attempted_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mail_domains
               SET mail_engine_projection_status = 'pending',
                   mail_engine_projection_last_attempt_at = $5,
                   mail_engine_projection_error_code = NULL
             WHERE id = $1
               AND status = $2
               AND mail_data_plane_id = $3
               AND mail_engine_projection_status <> 'ready'
               AND mail_engine_projection_last_attempt_at IS NOT DISTINCT FROM $4
               AND ($4::timestamptz IS NULL OR $5 > $4)
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(mail_domain_status(expected_status))
        .bind(expected_data_plane_id.as_uuid())
        .bind(expected_previous_attempt_at)
        .bind(attempted_at)
        .execute(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn complete_mail_domain_projection(
        &self,
        mail_domain_id: MailDomainId,
        expected_status: MailDomainStatus,
        expected_data_plane_id: MailDataPlaneId,
        expected_attempted_at: DateTime<Utc>,
        engine_ref: &str,
        projected_enabled: bool,
        completed_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        if !valid_engine_ref(engine_ref) || completed_at < expected_attempted_at {
            return Err(StoreError::Conflict);
        }
        let result = sqlx::query(
            r#"
            UPDATE mail_domains
               SET mail_engine_domain_ref = $5,
                   mail_engine_projection_status = 'ready',
                   mail_engine_projected_enabled = $6,
                   mail_engine_projection_succeeded_at = $7,
                   mail_engine_projection_error_code = NULL
             WHERE id = $1
               AND status = $2
               AND mail_data_plane_id = $3
               AND mail_engine_projection_status = 'pending'
               AND mail_engine_projection_last_attempt_at = $4
               AND (
                   mail_engine_domain_ref IS NULL
                   OR mail_engine_domain_ref = $5
               )
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(mail_domain_status(expected_status))
        .bind(expected_data_plane_id.as_uuid())
        .bind(expected_attempted_at)
        .bind(engine_ref)
        .bind(projected_enabled)
        .bind(completed_at)
        .execute(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn fail_mail_domain_projection(
        &self,
        mail_domain_id: MailDomainId,
        expected_status: MailDomainStatus,
        expected_data_plane_id: MailDataPlaneId,
        expected_attempted_at: DateTime<Utc>,
        error_code: &str,
        failed_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        if !valid_stable_code(error_code) || failed_at < expected_attempted_at {
            return Err(StoreError::Conflict);
        }
        let result = sqlx::query(
            r#"
            UPDATE mail_domains
               SET mail_engine_projection_status = 'failed',
                   mail_engine_projection_error_code = $5
             WHERE id = $1
               AND status = $2
               AND mail_data_plane_id = $3
               AND mail_engine_projection_status = 'pending'
               AND mail_engine_projection_last_attempt_at = $4
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(mail_domain_status(expected_status))
        .bind(expected_data_plane_id.as_uuid())
        .bind(expected_attempted_at)
        .bind(error_code)
        .execute(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }
    /// Returns only non-terminal, tenant-bound subscriptions that existed when
    /// the event occurred. Successful deliveries are never selected again.
    pub async fn pending_webhook_delivery_targets(
        &self,
        organization_id: OrganizationId,
        event_id: Uuid,
        event_type: &str,
        occurred_at: DateTime<Utc>,
    ) -> Result<Vec<WebhookDeliveryTarget>, sqlx::Error> {
        sqlx::query_as::<_, WebhookDeliveryTarget>(
            r#"
            SELECT subscription.id AS subscription_id,
                   subscription.organization_id,
                   subscription.endpoint_url,
                   subscription.signing_key_reference,
                   COALESCE((
                       SELECT count(*)::integer
                         FROM webhook_deliveries AS cycle_attempt
                        WHERE cycle_attempt.subscription_id = subscription.id
                          AND cycle_attempt.event_id = $2
                          AND cycle_attempt.attempt > COALESCE((
                              SELECT max(replay_marker.attempt)
                                FROM webhook_deliveries AS replay_marker
                               WHERE replay_marker.subscription_id = subscription.id
                                 AND replay_marker.event_id = $2
                                 AND replay_marker.error_code = 'manual_replay_requested'
                          ), 0)
                   ), 0) AS previous_attempts
              FROM webhook_subscriptions AS subscription
              JOIN principals AS principal
                ON principal.id = subscription.principal_id
               AND principal.organization_id = subscription.organization_id
               AND principal.status = 'active'
              LEFT JOIN LATERAL (
                    SELECT attempt, status
                      FROM webhook_deliveries
                     WHERE subscription_id = subscription.id
                       AND event_id = $2
                     ORDER BY attempt DESC
                     LIMIT 1
              ) AS last_delivery ON true
             WHERE subscription.organization_id = $1
               AND subscription.enabled
               AND subscription.created_at <= $4
               AND subscription.event_types @> ARRAY[$3]::text[]
               AND (
                   last_delivery.status IS NULL
                   OR last_delivery.status NOT IN ('delivered', 'dead_lettered')
               )
             ORDER BY subscription.id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(event_id)
        .bind(event_type)
        .bind(occurred_at)
        .fetch_all(&self.pool)
        .await
    }

    /// Appends one delivery result under a subscription row lock. Response
    /// bodies and exception strings are intentionally not accepted.
    pub async fn append_webhook_delivery(
        &self,
        subscription_id: Uuid,
        event_id: Uuid,
        outcome: WebhookDeliveryOutcome,
        attempted_at: DateTime<Utc>,
    ) -> Result<Option<i32>, StoreError> {
        let (status, response_status, next_attempt_at, error_code, delivered_at) = match outcome {
            WebhookDeliveryOutcome::Delivered { response_status }
                if (200..=299).contains(&response_status) =>
            {
                (
                    "delivered",
                    Some(response_status),
                    None,
                    None,
                    Some(attempted_at),
                )
            }
            WebhookDeliveryOutcome::Retrying {
                response_status,
                error_code,
                next_attempt_at,
            } if next_attempt_at > attempted_at && valid_stable_code(&error_code) => (
                "retrying",
                response_status,
                Some(next_attempt_at),
                Some(error_code),
                None,
            ),
            WebhookDeliveryOutcome::DeadLettered {
                response_status,
                error_code,
            } if valid_stable_code(&error_code) => (
                "dead_lettered",
                response_status,
                None,
                Some(error_code),
                None,
            ),
            _ => return Err(StoreError::Conflict),
        };
        if response_status.is_some_and(|value| !(100..=599).contains(&value)) {
            return Err(StoreError::Conflict);
        }

        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        let locked = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM webhook_subscriptions WHERE id = $1 FOR UPDATE",
        )
        .bind(subscription_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if locked.is_none() {
            transaction.rollback().await.map_err(map_store_error)?;
            return Ok(None);
        }
        let attempt = sqlx::query_scalar::<_, i32>(
            r#"
            SELECT COALESCE(max(attempt), 0) + 1
              FROM webhook_deliveries
             WHERE subscription_id = $1 AND event_id = $2
            "#,
        )
        .bind(subscription_id)
        .bind(event_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        sqlx::query(
            r#"
            INSERT INTO webhook_deliveries(
                id, subscription_id, event_id, attempt, status,
                response_status, next_attempt_at, error_code, delivered_at,
                created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(subscription_id)
        .bind(event_id)
        .bind(attempt)
        .bind(status)
        .bind(response_status.map(i32::from))
        .bind(next_attempt_at)
        .bind(error_code)
        .bind(delivered_at)
        .bind(attempted_at)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(Some(attempt))
    }

    /// Claims a recoverable batch without allowing one data-plane worker to
    /// consume another plane's work. Global events remain claimable by any
    /// configured worker; a worker without mail configuration claims only them.
    pub async fn claim_outbox_batch(
        &self,
        worker_id: Uuid,
        worker_data_plane_id: Option<MailDataPlaneId>,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, sqlx::Error> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            event_type: String,
            aggregate_type: String,
            aggregate_id: String,
            schema_version: i32,
            payload: Value,
            mail_data_plane_id: Option<Uuid>,
            occurred_at: DateTime<Utc>,
            attempts: i32,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            WITH candidates AS (
                SELECT id
                  FROM outbox_events
                 WHERE published_at IS NULL
                   AND next_attempt_at <= now()
                   AND (locked_at IS NULL OR locked_at < now() - interval '5 minutes')
                   AND (
                       ($1::uuid IS NULL AND mail_data_plane_id IS NULL)
                       OR (
                           $1::uuid IS NOT NULL
                           AND (mail_data_plane_id IS NULL OR mail_data_plane_id = $1)
                       )
                   )
                 ORDER BY occurred_at, id
                 FOR UPDATE SKIP LOCKED
                 LIMIT $2
            )
            UPDATE outbox_events AS event
               SET locked_at = now(),
                   locked_by = $3,
                   attempts = attempts + 1
              FROM candidates
             WHERE event.id = candidates.id
            RETURNING event.id,
                      event.event_type,
                      event.aggregate_type,
                      event.aggregate_id,
                      event.schema_version,
                      event.payload,
                      event.mail_data_plane_id,
                      event.occurred_at,
                      event.attempts
            "#,
        )
        .bind(worker_data_plane_id.map(MailDataPlaneId::as_uuid))
        .bind(limit)
        .bind(worker_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| OutboxRecord {
                id: row.id,
                event_type: row.event_type,
                aggregate_type: row.aggregate_type,
                aggregate_id: row.aggregate_id,
                schema_version: row.schema_version,
                payload: row.payload,
                data_plane_id: row.mail_data_plane_id.map(MailDataPlaneId::from_uuid),
                occurred_at: row.occurred_at,
                attempts: row.attempts,
            })
            .collect())
    }

    /// Marks a claimed outbox event as published by the same worker.
    pub async fn mark_outbox_published(
        &self,
        event_id: Uuid,
        worker_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            r#"
            UPDATE outbox_events
               SET published_at = now(),
                   locked_at = NULL,
                   locked_by = NULL,
                   next_attempt_at = now(),
                   last_error = NULL
             WHERE id = $1
               AND locked_by = $2
               AND published_at IS NULL
            "#,
        )
        .bind(event_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Releases a claim after publishing fails so it can be retried.
    pub async fn release_outbox_event(
        &self,
        event_id: Uuid,
        worker_id: Uuid,
        error_code: &str,
    ) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            r#"
            UPDATE outbox_events
               SET locked_at = NULL,
                   locked_by = NULL,
                   next_attempt_at = now()
                       + interval '5 seconds'
                         * power(2, least(attempts, 8)),
                   last_error = left($3, 200)
             WHERE id = $1
               AND locked_by = $2
               AND published_at IS NULL
            "#,
        )
        .bind(event_id)
        .bind(worker_id)
        .bind(error_code)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

async fn increment_agent_ingress_window(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    principal_id: Option<PrincipalId>,
    window_started_at: DateTime<Utc>,
    window_ends_at: DateTime<Utc>,
    observed_at: DateTime<Utc>,
    limit: i32,
) -> Result<bool, sqlx::Error> {
    let incremented = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO agent_ingress_rate_limit_windows(
            organization_id, principal_id, window_started_at,
            window_ends_at, request_count, updated_at
        )
        VALUES ($1, $2, $3, $4, 1, $5)
        ON CONFLICT (organization_id, principal_id, window_started_at)
        DO UPDATE
           SET request_count = agent_ingress_rate_limit_windows.request_count + 1,
               updated_at = EXCLUDED.updated_at
         WHERE agent_ingress_rate_limit_windows.request_count < $6
        RETURNING request_count
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(principal_id.map(PrincipalId::as_uuid))
    .bind(window_started_at)
    .bind(window_ends_at)
    .bind(observed_at)
    .bind(limit)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(incremented.is_some())
}

#[derive(sqlx::FromRow)]
struct ScimUserRow {
    organization_id: Uuid,
    principal_id: Uuid,
    external_id: String,
    user_name: String,
    display_name: String,
    active: bool,
    version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ScimGroupRow {
    organization_id: Uuid,
    group_id: Uuid,
    external_id: String,
    display_name: String,
    members: Vec<Uuid>,
    version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Current control-plane truth used to reconcile one engine domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailDomainProjectionTarget {
    pub mail_domain_id: MailDomainId,
    pub organization_id: OrganizationId,
    pub name: MailDomainName,
    pub status: MailDomainStatus,
    pub mail_data_plane_id: Option<MailDataPlaneId>,
    pub mail_engine_domain_ref: Option<String>,
    pub projection_last_attempt_at: Option<DateTime<Utc>>,
}

/// An outbox event leased by a worker.
#[derive(Debug, Clone)]
pub struct OutboxRecord {
    /// Stable event identifier.
    pub id: Uuid,
    /// Versioned event type.
    pub event_type: String,
    /// Aggregate type.
    pub aggregate_type: String,
    /// Aggregate identifier.
    pub aggregate_id: String,
    /// Payload schema version.
    pub schema_version: i32,
    /// Sanitized JSON payload.
    pub payload: Value,
    /// Exact owning mail data plane, or `None` for a global control event.
    pub data_plane_id: Option<MailDataPlaneId>,
    /// Business timestamp.
    pub occurred_at: DateTime<Utc>,
    /// Number of delivery claims.
    pub attempts: i32,
}

impl OutboxRecord {
    /// Every externally routable event carries its authoritative tenant in the
    /// sanitized payload. Missing or malformed values are not webhook-routable.
    #[must_use]
    pub fn organization_id(&self) -> Option<OrganizationId> {
        self.payload
            .get("organization_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .map(OrganizationId::from_uuid)
    }
}

/// Enabled subscription that has not reached a terminal result for an event.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct WebhookDeliveryTarget {
    pub subscription_id: Uuid,
    pub organization_id: Uuid,
    pub endpoint_url: String,
    pub signing_key_reference: String,
    pub previous_attempts: i32,
}

/// Sanitized append-only delivery outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookDeliveryOutcome {
    Delivered {
        response_status: u16,
    },
    Retrying {
        response_status: Option<u16>,
        error_code: String,
        next_attempt_at: DateTime<Utc>,
    },
    DeadLettered {
        response_status: Option<u16>,
        error_code: String,
    },
}

struct PgControlPlaneTransaction {
    transaction: Option<Transaction<'static, Postgres>>,
    bound_authentication: Option<AuthenticationBinding>,
}

impl PgControlPlaneTransaction {
    fn new(transaction: Transaction<'static, Postgres>) -> Self {
        Self {
            transaction: Some(transaction),
            bound_authentication: None,
        }
    }

    fn transaction(&mut self) -> Result<&mut Transaction<'static, Postgres>, StoreError> {
        self.transaction.as_mut().ok_or(StoreError::Unavailable)
    }
}

#[async_trait]
impl ControlPlaneStore for PgControlPlaneStore {
    async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
        let transaction = self.pool.begin().await.map_err(map_store_error)?;
        Ok(Box::new(PgControlPlaneTransaction::new(transaction)))
    }
}

#[async_trait]
impl MailEventIngestionStore for PgControlPlaneStore {
    async fn begin_mail_event_ingestion(
        &self,
    ) -> Result<Box<dyn MailEventIngestionTransaction>, StoreError> {
        let transaction = self.pool.begin().await.map_err(map_store_error)?;
        Ok(Box::new(PgControlPlaneTransaction::new(transaction)))
    }
}

fn valid_mail_data_plane_adapter_kind(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        })
}
fn canonical_mail_data_plane_origin(value: &str) -> Result<String, StoreError> {
    if value != value.trim() || value.chars().any(char::is_control) {
        return Err(StoreError::Conflict);
    }
    let parsed = Url::parse(value).map_err(|_| StoreError::Conflict)?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.cannot_be_a_base()
        || parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(StoreError::Conflict);
    }
    let canonical = parsed.origin().ascii_serialization();
    if canonical == "null" || canonical.len() > 512 {
        return Err(StoreError::Conflict);
    }
    Ok(canonical)
}
fn principal_kind(value: PrincipalKind) -> &'static str {
    match value {
        PrincipalKind::Human => "human",
        PrincipalKind::Agent => "agent",
        PrincipalKind::Service => "service",
    }
}

const fn human_session_client(value: HumanSessionClient) -> &'static str {
    value.as_str()
}

fn human_session_client_from_str(value: &str) -> Result<HumanSessionClient, StoreError> {
    match value {
        "admin" => Ok(HumanSessionClient::Admin),
        "mail" => Ok(HumanSessionClient::Mail),
        unknown => {
            tracing::error!(
                client_key = unknown,
                "unknown Human session client persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

const fn human_session_revocation_cause(value: HumanSessionRevocationCause) -> &'static str {
    value.as_str()
}

fn human_session_revocation_cause_from_str(
    value: &str,
) -> Result<HumanSessionRevocationCause, StoreError> {
    match value {
        "logout" => Ok(HumanSessionRevocationCause::Logout),
        "security_response" => Ok(HumanSessionRevocationCause::SecurityResponse),
        "principal_deactivated" => Ok(HumanSessionRevocationCause::PrincipalDeactivated),
        "upstream_revoked" => Ok(HumanSessionRevocationCause::UpstreamRevoked),
        unknown => {
            tracing::error!(
                revocation_cause = unknown,
                "unknown Human session revocation cause persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

fn human_login_transaction_from_row(
    row: HumanLoginTransactionRow,
) -> Result<HumanLoginTransactionRecord, StoreError> {
    if !(32..=4096).contains(&row.encrypted_pkce_verifier.len())
        || row.return_to.is_empty()
        || row.return_to.len() > 1024
        || row.return_to != row.return_to.trim()
        || !row.return_to.starts_with('/')
        || row.return_to.starts_with("//")
        || row.return_to.contains('\\')
        || row.return_to.chars().any(char::is_control)
        || row.expires_at <= row.created_at
        || row.expires_at > row.created_at + chrono::Duration::minutes(10)
    {
        tracing::error!("invalid Human login transaction persisted");
        return Err(StoreError::Unavailable);
    }
    Ok(HumanLoginTransactionRecord {
        login_transaction_id: HumanLoginTransactionId::from_uuid(row.id),
        client_key: human_session_client_from_str(&row.client_key)?,
        return_to: row.return_to,
        encrypted_pkce_verifier: row.encrypted_pkce_verifier,
        created_at: row.created_at,
        expires_at: row.expires_at,
    })
}

fn human_session_from_row(row: HumanSessionRow) -> Result<HumanSessionRecord, StoreError> {
    if row.upstream_expires_at <= row.created_at
        || row.expires_at <= row.created_at
        || row.expires_at > row.upstream_expires_at
        || row.expires_at > row.created_at + chrono::Duration::hours(8)
        || row.oidc_jti.as_deref().is_some_and(|jti| {
            jti.is_empty()
                || jti.len() > 200
                || jti != jti.trim()
                || jti.chars().any(char::is_control)
        })
    {
        tracing::error!("invalid Human browser session persisted");
        return Err(StoreError::Unavailable);
    }
    Ok(HumanSessionRecord {
        session_id: HumanSessionId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        login_transaction_id: HumanLoginTransactionId::from_uuid(row.login_transaction_id),
        client_key: human_session_client_from_str(&row.client_key)?,
        oidc_jti: row.oidc_jti,
        upstream_expires_at: row.upstream_expires_at,
        created_at: row.created_at,
        expires_at: row.expires_at,
    })
}

fn principal_kind_from_str(value: &str) -> Result<PrincipalKind, StoreError> {
    match value {
        "human" => Ok(PrincipalKind::Human),
        "agent" => Ok(PrincipalKind::Agent),
        "service" => Ok(PrincipalKind::Service),
        _ => Err(StoreError::Unavailable),
    }
}

fn agent_environment(value: AgentEnvironment) -> &'static str {
    match value {
        AgentEnvironment::Development => "development",
        AgentEnvironment::Testing => "testing",
        AgentEnvironment::Production => "production",
    }
}

fn agent_risk_level(value: AgentRiskLevel) -> &'static str {
    match value {
        AgentRiskLevel::Low => "low",
        AgentRiskLevel::Medium => "medium",
        AgentRiskLevel::High => "high",
    }
}

fn agent_environment_from_str(value: &str) -> Result<AgentEnvironment, StoreError> {
    match value {
        "development" => Ok(AgentEnvironment::Development),
        "testing" => Ok(AgentEnvironment::Testing),
        "production" => Ok(AgentEnvironment::Production),
        unknown => {
            tracing::error!(
                environment = unknown,
                "invalid Agent Inspector environment persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

fn agent_risk_level_from_str(value: &str) -> Result<AgentRiskLevel, StoreError> {
    match value {
        "low" => Ok(AgentRiskLevel::Low),
        "medium" => Ok(AgentRiskLevel::Medium),
        "high" => Ok(AgentRiskLevel::High),
        unknown => {
            tracing::error!(
                risk_level = unknown,
                "invalid Agent Inspector risk level persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

fn agent_inspector_snapshot_from_row(
    row: AgentInspectorSnapshotRow,
) -> Result<AgentInspectorSnapshotCandidate, StoreError> {
    if row.credential_integrity_error
        || row.policy_integrity_error
        || row.subscription_integrity_error
        || row.organization_id.is_nil()
        || row.agent_id.is_nil()
        || row.profile_owner_principal_id.is_nil()
        || row.review_due_at <= row.agent_created_at
        || !valid_agent_inspector_display_name(&row.agent_display_name)
        || !valid_agent_inspector_purpose(&row.purpose)
        || row.last_processing_activity_at.is_some_and(|activity_at| {
            activity_at > row.observed_at + chrono::Duration::seconds(30)
        })
    {
        tracing::error!("invalid Agent Inspector aggregate persisted");
        return Err(StoreError::Unavailable);
    }

    let agent_kind = principal_kind_from_str(&row.agent_kind)?;
    let agent_status = principal_status_from_str(&row.agent_status)?;
    if agent_kind != PrincipalKind::Agent {
        tracing::error!("Agent Inspector target was not persisted as an Agent");
        return Err(StoreError::Unavailable);
    }

    let owner_id = row.owner_id.ok_or_else(|| {
        tracing::error!("Agent Inspector profile owner is missing");
        StoreError::Unavailable
    })?;
    let owner_organization_id = row.owner_organization_id.ok_or_else(|| {
        tracing::error!("Agent Inspector profile owner tenant is missing");
        StoreError::Unavailable
    })?;
    let owner_kind = row.owner_kind.as_deref().ok_or_else(|| {
        tracing::error!("Agent Inspector profile owner kind is missing");
        StoreError::Unavailable
    })?;
    let owner_status = row.owner_status.as_deref().ok_or_else(|| {
        tracing::error!("Agent Inspector profile owner status is missing");
        StoreError::Unavailable
    })?;
    let owner_display_name = row.owner_display_name.as_deref().ok_or_else(|| {
        tracing::error!("Agent Inspector profile owner display name is missing");
        StoreError::Unavailable
    })?;
    let owner_kind = principal_kind_from_str(owner_kind)?;
    let owner_status = principal_status_from_str(owner_status)?;
    if owner_id.is_nil()
        || owner_id != row.profile_owner_principal_id
        || owner_id == row.agent_id
        || owner_organization_id != row.organization_id
        || owner_kind != PrincipalKind::Human
        || !valid_agent_inspector_display_name(owner_display_name)
    {
        tracing::error!("Agent Inspector profile owner violates identity invariants");
        return Err(StoreError::Unavailable);
    }

    let environment = agent_environment_from_str(&row.environment)?;
    let risk_level = agent_risk_level_from_str(&row.risk_level)?;
    let active_credential_count =
        u32::try_from(row.active_credential_count).map_err(|_| StoreError::Unavailable)?;
    if row.next_credential_expiry_at.is_some() != (active_credential_count > 0)
        || row
            .next_credential_expiry_at
            .is_some_and(|expires_at| expires_at <= row.observed_at)
        || row.last_authenticated_at.is_some_and(|authenticated_at| {
            authenticated_at > row.observed_at + chrono::Duration::seconds(30)
        })
    {
        tracing::error!("Agent Inspector credential aggregate violates time invariants");
        return Err(StoreError::Unavailable);
    }

    let mut effective_scopes = BTreeSet::new();
    for value in row.effective_scopes {
        let permission = permission_from_str(&value).ok_or_else(|| {
            tracing::error!(
                scope = value,
                "invalid Agent Inspector effective scope persisted"
            );
            StoreError::Unavailable
        })?;
        if !Permission::AGENT_TOKEN.contains(&permission) || !effective_scopes.insert(permission) {
            tracing::error!("Agent Inspector effective scopes violate Agent token bounds");
            return Err(StoreError::Unavailable);
        }
    }
    if (active_credential_count == 0 || agent_status != PrincipalStatus::Active)
        && !effective_scopes.is_empty()
    {
        tracing::error!("Agent Inspector effective scopes lack a valid active identity");
        return Err(StoreError::Unavailable);
    }

    let active_processing_subscription_count =
        u32::try_from(row.active_processing_subscription_count)
            .map_err(|_| StoreError::Unavailable)?;
    let paused_processing_subscription_count =
        u32::try_from(row.paused_processing_subscription_count)
            .map_err(|_| StoreError::Unavailable)?;
    let retired_processing_subscription_count =
        u32::try_from(row.retired_processing_subscription_count)
            .map_err(|_| StoreError::Unavailable)?;

    Ok(AgentInspectorSnapshotCandidate {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        agent: AgentInspectorPrincipalCandidate {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.agent_id),
            kind: agent_kind,
            status: agent_status,
            display_name: row.agent_display_name,
        },
        owner: AgentInspectorPrincipalCandidate {
            organization_id: OrganizationId::from_uuid(owner_organization_id),
            principal_id: PrincipalId::from_uuid(owner_id),
            kind: owner_kind,
            status: owner_status,
            display_name: owner_display_name.into(),
        },
        profile: AgentInspectorProfileCandidate {
            principal_id: PrincipalId::from_uuid(row.agent_id),
            owner_principal_id: PrincipalId::from_uuid(row.profile_owner_principal_id),
            environment,
            risk_level,
            purpose: row.purpose,
            review_due_at: row.review_due_at,
        },
        token_access: AgentInspectorTokenAccessCandidate {
            active_credential_count,
            next_credential_expiry_at: row.next_credential_expiry_at,
            last_authenticated_at: row.last_authenticated_at,
            effective_scopes,
        },
        control_plane: AgentInspectorControlPlaneCandidate {
            has_active_production_mail_policy: row.has_active_production_mail_policy,
            active_processing_subscription_count,
            paused_processing_subscription_count,
            retired_processing_subscription_count,
            last_processing_activity_at: row.last_processing_activity_at,
        },
        observed_at: row.observed_at,
    })
}

/// Each persisted terminal submission state has one exact Audit action and
/// outcome. This keeps a same-action denied or failed record from being
/// mistaken for the durable terminal proof.
const fn expected_terminal_audit(
    status: MailSubmissionStatus,
) -> Option<(&'static str, &'static str)> {
    match status {
        MailSubmissionStatus::Pending | MailSubmissionStatus::Reconciling => None,
        MailSubmissionStatus::Accepted => Some(("mail.submission.accepted", "success")),
        MailSubmissionStatus::Rejected => Some(("mail.submission.rejected", "failure")),
        MailSubmissionStatus::ManualReview => Some(("mail.submission.review_required", "failure")),
    }
}

/// Validates the persisted business-state chronology before an evidence view
/// can disclose it. Admission is an immutable row inserted after the initial
/// intent, so a pending intent's `updated_at` is deliberately not treated as
/// an admission upper bound. `accepted_at` remains internal: it is needed to
/// bind a delivery observation, not to widen the Human-facing response
/// contract.
fn valid_agent_mail_submission_timeline(
    submission_created_at: DateTime<Utc>,
    admitted_at: DateTime<Utc>,
    submission_updated_at: DateTime<Utc>,
    submission_accepted_at: Option<DateTime<Utc>>,
    submission_status: MailSubmissionStatus,
    observed_at: DateTime<Utc>,
) -> bool {
    if submission_created_at > admitted_at || submission_updated_at > observed_at {
        return false;
    }

    match (submission_status, submission_accepted_at) {
        (MailSubmissionStatus::Accepted, Some(accepted_at)) => {
            // New terminal writes assign both facts from one database clock.
            // Older immutable records may retain separate times, so this
            // reader keeps the causality bound rather than rewriting history.
            admitted_at <= accepted_at && accepted_at <= observed_at
        }
        (MailSubmissionStatus::Accepted, None) => false,
        (_, None) => true,
        (_, Some(_)) => false,
    }
}

/// Audit timestamps are facts about the immutable business transition they
/// prove, rather than merely events that happened before this read.
struct AgentMailSubmissionAuditTimeline {
    requested_at: DateTime<Utc>,
    admitted_audit_at: DateTime<Utc>,
    terminal_audit_at: Option<DateTime<Utc>>,
    submission_created_at: DateTime<Utc>,
    admitted_at: DateTime<Utc>,
    submission_updated_at: DateTime<Utc>,
    submission_accepted_at: Option<DateTime<Utc>>,
    observed_at: DateTime<Utc>,
}

fn valid_agent_mail_submission_audit_timeline(timeline: &AgentMailSubmissionAuditTimeline) -> bool {
    timeline.requested_at == timeline.submission_created_at
        && timeline.admitted_audit_at == timeline.admitted_at
        && timeline.requested_at <= timeline.admitted_audit_at
        && timeline.admitted_audit_at <= timeline.observed_at
        && timeline.terminal_audit_at.is_none_or(|terminal_at| {
            terminal_at >= timeline.admitted_at
                && terminal_at <= timeline.submission_updated_at
                && terminal_at <= timeline.observed_at
                && timeline
                    .submission_accepted_at
                    .is_none_or(|accepted_at| terminal_at <= accepted_at)
        })
}

/// A delivery snapshot is only evidence after the accepted submission state
/// it observes. Admissions alone are insufficient for that claim.
fn valid_agent_mail_submission_delivery_timeline(
    delivery_observed_at: DateTime<Utc>,
    admitted_at: DateTime<Utc>,
    submission_accepted_at: Option<DateTime<Utc>>,
    observed_at: DateTime<Utc>,
) -> bool {
    submission_accepted_at.is_some_and(|accepted_at| {
        admitted_at <= accepted_at
            && accepted_at <= delivery_observed_at
            && delivery_observed_at <= observed_at
    })
}

fn agent_mail_submission_evidence_from_row(
    row: AgentMailSubmissionEvidenceRow,
) -> Result<AgentMailSubmissionEvidenceCandidate, StoreError> {
    if row.organization_id.is_nil()
        || row.agent_id.is_nil()
        || row.submission_id.is_nil()
        || row.admission_id.is_nil()
        || row.policy_activation_id.is_nil()
        || row.policy_version_id.is_nil()
        || row.policy_activation_approval_request_id.is_nil()
        || !row.policy_activation_timing_valid
        || !row.policy_activation_approval_verified
        || !row.first_contact_approval_verified
    {
        tracing::error!("invalid Agent mail submission evidence aggregate persisted");
        return Err(StoreError::Unavailable);
    }

    let submission_status = match row.submission_status.as_str() {
        "pending" => MailSubmissionStatus::Pending,
        "reconciling" => MailSubmissionStatus::Reconciling,
        "accepted" => MailSubmissionStatus::Accepted,
        "rejected" => MailSubmissionStatus::Rejected,
        "manual_review" => MailSubmissionStatus::ManualReview,
        unknown => {
            tracing::error!(
                status = unknown,
                "unknown Agent evidence submission status persisted"
            );
            return Err(StoreError::Unavailable);
        }
    };
    if row.policy_activated_at > row.admitted_at
        || row.policy_activated_at > row.observed_at
        || !valid_agent_mail_submission_timeline(
            row.submission_created_at,
            row.admitted_at,
            row.submission_updated_at,
            row.submission_accepted_at,
            submission_status,
            row.observed_at,
        )
    {
        tracing::error!("invalid Agent mail submission evidence timeline persisted");
        return Err(StoreError::Unavailable);
    }
    let recipient_count =
        u16::try_from(row.recipient_count).map_err(|_| StoreError::Unavailable)?;
    let first_contact_count =
        u16::try_from(row.first_contact_count).map_err(|_| StoreError::Unavailable)?;
    if !(1..=100).contains(&recipient_count)
        || first_contact_count > recipient_count
        || (first_contact_count == 0 && row.first_contact_approval_request_id.is_some())
        || (first_contact_count > 0 && row.first_contact_approval_request_id.is_none())
        || row
            .first_contact_approval_request_id
            .is_some_and(|request_id| request_id.is_nil())
    {
        tracing::error!("invalid Agent mail admission counters or first-contact proof persisted");
        return Err(StoreError::Unavailable);
    }

    let (requested_audit_event_id, requested_audit_occurred_at, requested_audit_event_count) = (
        row.requested_audit_event_id,
        row.requested_audit_occurred_at,
        row.requested_audit_event_count,
    );
    let (admitted_audit_event_id, admitted_audit_occurred_at, admitted_audit_event_count) = (
        row.admitted_audit_event_id,
        row.admitted_audit_occurred_at,
        row.admitted_audit_event_count,
    );
    let (requested_audit_event_id, requested_audit_occurred_at) = match (
        requested_audit_event_id,
        requested_audit_occurred_at,
        requested_audit_event_count,
    ) {
        (Some(event_id), Some(occurred_at), Some(1))
            if !event_id.is_nil() && occurred_at <= row.observed_at =>
        {
            (event_id, occurred_at)
        }
        _ => {
            tracing::error!("Agent submission request audit reference is incomplete or ambiguous");
            return Err(StoreError::Unavailable);
        }
    };
    let (admitted_audit_event_id, admitted_audit_occurred_at) = match (
        admitted_audit_event_id,
        admitted_audit_occurred_at,
        admitted_audit_event_count,
    ) {
        (Some(event_id), Some(occurred_at), Some(1))
            if !event_id.is_nil() && occurred_at <= row.observed_at =>
        {
            (event_id, occurred_at)
        }
        _ => {
            tracing::error!(
                "Agent submission admission audit reference is incomplete or ambiguous"
            );
            return Err(StoreError::Unavailable);
        }
    };

    let mut audit_references = vec![
        AgentMailSubmissionAuditReferenceCandidate {
            event_id: requested_audit_event_id,
            action: "mail.submission.requested".into(),
            occurred_at: requested_audit_occurred_at,
        },
        AgentMailSubmissionAuditReferenceCandidate {
            event_id: admitted_audit_event_id,
            action: "agent.mail.admitted".into(),
            occurred_at: admitted_audit_occurred_at,
        },
    ];
    let mut terminal_audit_occurred_at = None;
    match expected_terminal_audit(submission_status) {
        Some((expected_action, expected_outcome)) => match (
            row.terminal_audit_event_id,
            row.terminal_audit_action.as_deref(),
            row.terminal_audit_outcome.as_deref(),
            row.terminal_audit_occurred_at,
            row.terminal_audit_event_count,
        ) {
            (Some(event_id), Some(action), Some(outcome), Some(occurred_at), 1)
                if !event_id.is_nil()
                    && action == expected_action
                    && outcome == expected_outcome
                    && occurred_at <= row.observed_at =>
            {
                terminal_audit_occurred_at = Some(occurred_at);
                audit_references.push(AgentMailSubmissionAuditReferenceCandidate {
                    event_id,
                    action: action.into(),
                    occurred_at,
                });
            }
            _ => {
                tracing::error!(
                    "Agent submission terminal audit reference is incomplete or ambiguous"
                );
                return Err(StoreError::Unavailable);
            }
        },
        None if row.terminal_audit_event_count != 0
            || row.terminal_audit_event_id.is_some()
            || row.terminal_audit_action.is_some()
            || row.terminal_audit_outcome.is_some()
            || row.terminal_audit_occurred_at.is_some() =>
        {
            tracing::error!("nonterminal Agent submission has terminal audit evidence");
            return Err(StoreError::Unavailable);
        }
        None => {}
    }

    if !valid_agent_mail_submission_audit_timeline(&AgentMailSubmissionAuditTimeline {
        requested_at: requested_audit_occurred_at,
        admitted_audit_at: admitted_audit_occurred_at,
        terminal_audit_at: terminal_audit_occurred_at,
        submission_created_at: row.submission_created_at,
        admitted_at: row.admitted_at,
        submission_updated_at: row.submission_updated_at,
        submission_accepted_at: row.submission_accepted_at,
        observed_at: row.observed_at,
    }) {
        tracing::error!("Agent submission Audit evidence is not causally bound to state");
        return Err(StoreError::Unavailable);
    }

    let delivery = match (
        row.delivery_snapshot_id,
        row.delivery_observed_at,
        row.delivery_terminal,
        row.delivery_recipient_count,
        row.delivery_delivered_count,
        row.delivery_failed_count,
        row.delivery_queued_count,
        row.delivery_unknown_count,
    ) {
        (None, None, None, None, None, None, None, None) => None,
        (
            Some(snapshot_id),
            Some(observed_at),
            Some(terminal),
            Some(delivery_recipient_count),
            Some(delivered_count),
            Some(failed_count),
            Some(queued_count),
            Some(unknown_count),
        ) => {
            let delivery_recipient_count =
                u16::try_from(delivery_recipient_count).map_err(|_| StoreError::Unavailable)?;
            let delivered_count =
                u16::try_from(delivered_count).map_err(|_| StoreError::Unavailable)?;
            let failed_count = u16::try_from(failed_count).map_err(|_| StoreError::Unavailable)?;
            let queued_count = u16::try_from(queued_count).map_err(|_| StoreError::Unavailable)?;
            let unknown_count =
                u16::try_from(unknown_count).map_err(|_| StoreError::Unavailable)?;
            let count_sum = u32::from(delivered_count)
                + u32::from(failed_count)
                + u32::from(queued_count)
                + u32::from(unknown_count);
            if snapshot_id.is_nil()
                || submission_status != MailSubmissionStatus::Accepted
                || !valid_agent_mail_submission_delivery_timeline(
                    observed_at,
                    row.admitted_at,
                    row.submission_accepted_at,
                    row.observed_at,
                )
                || delivery_recipient_count != recipient_count
                || !(1..=100).contains(&delivery_recipient_count)
                || count_sum != u32::from(delivery_recipient_count)
                || terminal != (queued_count == 0 && unknown_count == 0)
            {
                tracing::error!("invalid Agent submission delivery evidence persisted");
                return Err(StoreError::Unavailable);
            }
            Some(AgentMailSubmissionDeliveryEvidenceCandidate {
                snapshot_id,
                observed_at,
                terminal,
                recipient_count: delivery_recipient_count,
                delivered_count,
                failed_count,
                queued_count,
                unknown_count,
            })
        }
        _ => {
            tracing::error!("partial Agent submission delivery projection persisted");
            return Err(StoreError::Unavailable);
        }
    };

    Ok(AgentMailSubmissionEvidenceCandidate {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        agent_id: PrincipalId::from_uuid(row.agent_id),
        submission_id: MailSubmissionId::from_uuid(row.submission_id),
        submission_status,
        submission_created_at: row.submission_created_at,
        submission_updated_at: row.submission_updated_at,
        submission_accepted_at: row.submission_accepted_at,
        admission: AgentMailSubmissionAdmissionEvidenceCandidate {
            admission_id: row.admission_id,
            admitted_at: row.admitted_at,
            policy_activation_id: row.policy_activation_id,
            policy_activated_at: row.policy_activated_at,
            policy_version_id: row.policy_version_id,
            policy_version: u32::try_from(row.policy_version)
                .map_err(|_| StoreError::Unavailable)?,
            policy_activation_approval_request_id: ApprovalRequestId::from_uuid(
                row.policy_activation_approval_request_id,
            ),
            policy_activation_approval_verified: row.policy_activation_approval_verified,
            recipient_count,
            first_contact_count,
            first_contact_approval_request_id: row
                .first_contact_approval_request_id
                .map(ApprovalRequestId::from_uuid),
            first_contact_approval_verified: row.first_contact_approval_verified,
        },
        delivery,
        audit_references,
        observed_at: row.observed_at,
    })
}

fn valid_agent_inspector_display_name(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 200
        && !value.chars().any(char::is_control)
        && value.split_whitespace().collect::<Vec<_>>().join(" ") == value
}

fn valid_agent_inspector_purpose(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 1_000
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn scim_user_from_row(row: ScimUserRow) -> ExistingScimUser {
    ExistingScimUser {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        external_id: row.external_id,
        user_name: row.user_name,
        display_name: row.display_name,
        active: row.active,
        version: row.version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn scim_group_from_row(row: ScimGroupRow) -> ExistingScimGroup {
    ExistingScimGroup {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        group_id: ScimGroupId::from_uuid(row.group_id),
        external_id: row.external_id,
        display_name: row.display_name,
        members: row
            .members
            .into_iter()
            .map(PrincipalId::from_uuid)
            .collect(),
        version: row.version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

async fn insert_scim_group_members(
    transaction: &mut Transaction<'static, Postgres>,
    group: &ExistingScimGroup,
) -> Result<(), StoreError> {
    for member in &group.members {
        sqlx::query(
            r#"
            INSERT INTO scim_group_members(
                group_id, organization_id, principal_id, created_at
            )
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(group.group_id.as_uuid())
        .bind(group.organization_id.as_uuid())
        .bind(member.as_uuid())
        .bind(group.updated_at)
        .execute(&mut **transaction)
        .await
        .map_err(map_store_error)?;
    }
    Ok(())
}

fn principal_status(value: PrincipalStatus) -> &'static str {
    match value {
        PrincipalStatus::Draft => "draft",
        PrincipalStatus::Active => "active",
        PrincipalStatus::Suspended => "suspended",
        PrincipalStatus::Retired => "retired",
    }
}

fn principal_status_from_str(value: &str) -> Result<PrincipalStatus, StoreError> {
    match value {
        "draft" => Ok(PrincipalStatus::Draft),
        "active" => Ok(PrincipalStatus::Active),
        "suspended" => Ok(PrincipalStatus::Suspended),
        "retired" => Ok(PrincipalStatus::Retired),
        _ => Err(StoreError::Unavailable),
    }
}

fn system_role_from_str(value: &str) -> Result<SystemRole, StoreError> {
    match value {
        "org_owner" => Ok(SystemRole::OrgOwner),
        "mail_admin" => Ok(SystemRole::MailAdmin),
        "security_admin" => Ok(SystemRole::SecurityAdmin),
        "compliance_auditor" => Ok(SystemRole::ComplianceAuditor),
        "content_reviewer" => Ok(SystemRole::ContentReviewer),
        "helpdesk" => Ok(SystemRole::Helpdesk),
        "agent_operator" => Ok(SystemRole::AgentOperator),
        "employee" => Ok(SystemRole::Employee),
        "break_glass" => Ok(SystemRole::BreakGlass),
        _ => Err(StoreError::Unavailable),
    }
}

fn audit_outcome(value: AuditOutcome) -> &'static str {
    match value {
        AuditOutcome::Success => "success",
        AuditOutcome::Failure => "failure",
        AuditOutcome::Denied => "denied",
    }
}

fn audit_outcome_from_str(value: &str) -> Result<AuditOutcome, StoreError> {
    match value {
        "success" => Ok(AuditOutcome::Success),
        "failure" => Ok(AuditOutcome::Failure),
        "denied" => Ok(AuditOutcome::Denied),
        _ => Err(StoreError::Conflict),
    }
}

fn stored_audit_checkpoint_from_row(
    row: StoredAuditCheckpointRow,
) -> Result<StoredAuditCheckpoint, AuditArtifactStoreError> {
    Ok(StoredAuditCheckpoint {
        checkpoint: SignedAuditCheckpoint::from_storage(
            &row.signed_payload,
            row.signature_algorithm,
            &row.signature,
        )?,
        archive_object_key: row.object_key,
    })
}

fn audit_event_summary_from_row(
    row: AuditEventSummaryRow,
) -> Result<AuditEventSummary, StoreError> {
    if row.event_hash_hex.len() != 64
        || row
            .previous_hash_hex
            .as_ref()
            .is_some_and(|hash| hash.len() != 64)
        || !matches!(row.actor_kind.as_str(), "human" | "agent" | "service")
        || !row.details.is_object()
        || row.chain_sequence <= 0
        || row.hash_version <= 0
    {
        return Err(StoreError::Conflict);
    }
    Ok(AuditEventSummary {
        event_id: row.event_id,
        organization_id: OrganizationId::from_uuid(row.organization_id),
        actor_principal_id: PrincipalId::from_uuid(row.actor_principal_id),
        actor_kind: row.actor_kind,
        on_behalf_of_principal_id: row.on_behalf_of_principal_id.map(PrincipalId::from_uuid),
        credential_id: row.credential_id,
        subject_principal_id: row.subject_principal_id.map(PrincipalId::from_uuid),
        action: row.action,
        resource_type: row.resource_type,
        resource_id: row.resource_id,
        outcome: audit_outcome_from_str(&row.outcome)?,
        trace_id: row.trace_id,
        request_id: row.request_id,
        source_ip: row.source_ip,
        client_id: row.client_id,
        session_id: row.session_id,
        token_jti: row.token_jti,
        policy_id: row.policy_id.map(PolicyId::from_uuid),
        policy_version_id: row.policy_version_id.map(PolicyVersionId::from_uuid),
        approval_request_id: row
            .approval_request_id
            .map(copilotmail_domain::ApprovalRequestId::from_uuid),
        ticket_reference: row.ticket_reference,
        environment: row.environment,
        region: row.region,
        details: row.details,
        occurred_at: row.occurred_at,
        received_at: row.received_at,
        chain_sequence: row.chain_sequence,
        hash_version: row.hash_version,
        previous_hash_hex: row.previous_hash_hex,
        event_hash_hex: row.event_hash_hex,
        signature_batch_id: row.signature_batch_id,
    })
}

fn permission_from_str(value: &str) -> Option<Permission> {
    let permission = match value {
        "organization.read" => Permission::OrganizationRead,
        "organization.manage" => Permission::OrganizationManage,
        "identity.manage" => Permission::IdentityManage,
        "authorization.manage" => Permission::AuthorizationManage,
        "mail.admin" => Permission::MailAdmin,
        "security.admin" => Permission::SecurityAdmin,
        "session.revoke_others" => Permission::SessionRevokeOthers,
        "audit.read" => Permission::AuditRead,
        "audit.export" => Permission::AuditExport,
        "legal_hold.manage" => Permission::LegalHoldManage,
        "retention.manage" => Permission::RetentionManage,
        "content.review" => Permission::ContentReview,
        "agent.operate" => Permission::AgentOperate,
        "agent.mail.read" => Permission::AgentMailRead,
        "mailbox.use" => Permission::MailboxUse,
        "break_glass.invoke" => Permission::BreakGlass,
        unknown => {
            tracing::error!(permission = unknown, "unknown permission denied");
            return None;
        }
    };
    Some(permission)
}

const fn compliance_case_status(status: ComplianceCaseStatus) -> &'static str {
    match status {
        ComplianceCaseStatus::Open => "open",
        ComplianceCaseStatus::Closed => "closed",
    }
}

fn audit_export_grant_from_row(row: AuditExportGrantRow) -> Result<AuditExportGrant, StoreError> {
    AuditExportGrant::restore(
        AuditExportId::from_uuid(row.id),
        OrganizationId::from_uuid(row.organization_id),
        AuditArchiveId::from_uuid(row.archive_id),
        copilotmail_domain::ApprovalRequestId::from_uuid(row.approval_request_id),
        PrincipalId::from_uuid(row.requested_by),
        row.created_at,
        row.expires_at,
    )
    .map_err(|error| {
        tracing::error!(%error, "invalid audit export grant persisted");
        StoreError::Unavailable
    })
}

fn agent_message_processing_result_from_row(
    row: AgentMessageProcessingOperationRow,
    replayed: bool,
) -> Result<AgentMessageProcessingResult, StoreError> {
    let operation = match row.operation.as_str() {
        "claimed" => AgentMessageProcessingOperation::Claimed,
        "lease_extended" => AgentMessageProcessingOperation::LeaseExtended,
        "acknowledged" => AgentMessageProcessingOperation::Acknowledged,
        "nacked" => AgentMessageProcessingOperation::Nacked,
        "dead_lettered" => AgentMessageProcessingOperation::DeadLettered,
        _ => return Err(StoreError::Conflict),
    };
    let state = match row.result_state.as_str() {
        "pending" => AgentMessageProcessingState::Pending,
        "leased" => AgentMessageProcessingState::Leased,
        "acknowledged" => AgentMessageProcessingState::Acknowledged,
        "dead_lettered" => AgentMessageProcessingState::DeadLettered,
        _ => return Err(StoreError::Conflict),
    };
    if row.request_sha256.len() != 32
        || row.attempt_count <= 0
        || (state == AgentMessageProcessingState::Leased) != row.lease_expires_at.is_some()
    {
        return Err(StoreError::Conflict);
    }
    Ok(AgentMessageProcessingResult {
        operation_id: row.id,
        organization_id: OrganizationId::from_uuid(row.organization_id),
        subscription_id: SubscriptionId::from_uuid(row.subscription_id),
        message_id: row.message_id,
        principal_id: PrincipalId::from_uuid(row.principal_id),
        lease_id: LeaseId::from_uuid(row.lease_id),
        operation,
        state,
        attempt_count: u32::try_from(row.attempt_count).map_err(|_| StoreError::Conflict)?,
        lease_expires_at: row.lease_expires_at,
        reason_code: row.reason_code,
        occurred_at: row.occurred_at,
        replayed,
    })
}

fn retention_resource_kind_from_str(value: &str) -> Result<RetentionResourceKind, StoreError> {
    match value {
        "audit.evidence" => Ok(RetentionResourceKind::AuditEvidence),
        "mail.content" => Ok(RetentionResourceKind::MailContent),
        unknown => {
            tracing::error!(
                resource_kind = unknown,
                "unknown retention resource kind persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

fn retention_policy_record_from_row(
    row: RetentionPolicyRow,
) -> Result<RetentionPolicyRecord, StoreError> {
    let latest_version = u32::try_from(row.latest_version).map_err(|_| StoreError::Unavailable)?;
    let policy = RetentionPolicy::restore(
        RetentionPolicyId::from_uuid(row.id),
        OrganizationId::from_uuid(row.organization_id),
        row.policy_key,
        row.display_name,
        retention_resource_kind_from_str(&row.resource_kind)?,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(|error| {
        tracing::error!(%error, "invalid retention policy persisted");
        StoreError::Unavailable
    })?;
    Ok(RetentionPolicyRecord {
        policy,
        active_version_id: row
            .active_version_id
            .map(RetentionPolicyVersionId::from_uuid),
        latest_version,
    })
}

fn retention_policy_version_from_row(
    row: RetentionPolicyVersionRow,
) -> Result<RetentionPolicyVersion, StoreError> {
    RetentionPolicyVersion::restore(
        RetentionPolicyVersionId::from_uuid(row.id),
        RetentionPolicyId::from_uuid(row.policy_id),
        OrganizationId::from_uuid(row.organization_id),
        u32::try_from(row.version).map_err(|_| StoreError::Unavailable)?,
        u32::try_from(row.minimum_retention_days).map_err(|_| StoreError::Unavailable)?,
        row.archive_after_days
            .map(u32::try_from)
            .transpose()
            .map_err(|_| StoreError::Unavailable)?,
        row.delete_after_days
            .map(u32::try_from)
            .transpose()
            .map_err(|_| StoreError::Unavailable)?,
        row.rule,
        PrincipalId::from_uuid(row.created_by),
        row.created_at,
    )
    .map_err(|error| {
        tracing::error!(%error, "invalid retention policy version persisted");
        StoreError::Unavailable
    })
}

fn retention_policy_activation_from_row(
    row: RetentionPolicyActivationRow,
) -> RetentionPolicyActivationRecord {
    RetentionPolicyActivationRecord {
        id: RetentionPolicyActivationId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        policy_id: RetentionPolicyId::from_uuid(row.policy_id),
        policy_version_id: RetentionPolicyVersionId::from_uuid(row.policy_version_id),
        approval_request_id: copilotmail_domain::ApprovalRequestId::from_uuid(
            row.approval_request_id,
        ),
        activated_by: PrincipalId::from_uuid(row.activated_by),
        activated_at: row.activated_at,
    }
}

fn retention_policy_detail_from_row(
    row: RetentionPolicyDetailRow,
) -> Result<RetentionPolicyDetail, StoreError> {
    let policy_id = RetentionPolicyId::from_uuid(row.policy_id);
    let organization_id = OrganizationId::from_uuid(row.organization_id);
    let policy = RetentionPolicy::restore(
        policy_id,
        organization_id,
        row.policy_key,
        row.display_name,
        retention_resource_kind_from_str(&row.resource_kind)?,
        PrincipalId::from_uuid(row.created_by),
        row.policy_created_at,
    )
    .map_err(|error| {
        tracing::error!(%error, "invalid retention policy detail persisted");
        StoreError::Unavailable
    })?;
    let latest_version = retention_policy_version_from_row(RetentionPolicyVersionRow {
        id: row.latest_id,
        policy_id: row.policy_id,
        organization_id: row.organization_id,
        version: row.latest_version,
        minimum_retention_days: row.latest_minimum_retention_days,
        archive_after_days: row.latest_archive_after_days,
        delete_after_days: row.latest_delete_after_days,
        rule: row.latest_rule,
        created_by: row.latest_created_by,
        created_at: row.latest_created_at,
    })?;
    let active_version = match (
        row.active_id,
        row.active_version,
        row.active_minimum_retention_days,
        row.active_rule,
        row.active_created_by,
        row.active_created_at,
    ) {
        (None, None, None, None, None, None) => None,
        (
            Some(id),
            Some(version),
            Some(minimum_retention_days),
            Some(rule),
            Some(created_by),
            Some(created_at),
        ) => Some(retention_policy_version_from_row(
            RetentionPolicyVersionRow {
                id,
                policy_id: row.policy_id,
                organization_id: row.organization_id,
                version,
                minimum_retention_days,
                archive_after_days: row.active_archive_after_days,
                delete_after_days: row.active_delete_after_days,
                rule,
                created_by,
                created_at,
            },
        )?),
        _ => {
            tracing::error!(%organization_id, %policy_id, "partial active retention version persisted");
            return Err(StoreError::Unavailable);
        }
    };
    Ok(RetentionPolicyDetail {
        policy,
        active_version,
        latest_version,
    })
}

fn parse_compliance_case_status(value: &str) -> Result<ComplianceCaseStatus, StoreError> {
    match value {
        "open" => Ok(ComplianceCaseStatus::Open),
        "closed" => Ok(ComplianceCaseStatus::Closed),
        unknown => {
            tracing::error!(status = unknown, "unknown compliance case status persisted");
            Err(StoreError::Unavailable)
        }
    }
}

const fn legal_hold_status(status: LegalHoldStatus) -> &'static str {
    match status {
        LegalHoldStatus::Active => "active",
        LegalHoldStatus::Released => "released",
    }
}

fn parse_legal_hold_status(value: &str) -> Result<LegalHoldStatus, StoreError> {
    match value {
        "active" => Ok(LegalHoldStatus::Active),
        "released" => Ok(LegalHoldStatus::Released),
        unknown => {
            tracing::error!(status = unknown, "unknown legal hold status persisted");
            Err(StoreError::Unavailable)
        }
    }
}

const fn mailbox_kind(kind: MailboxKind) -> &'static str {
    match kind {
        MailboxKind::Human => "human",
        MailboxKind::Agent => "agent",
        MailboxKind::Shared => "shared",
        MailboxKind::Group => "group",
    }
}

fn mailbox_kind_from_str(value: &str) -> Option<MailboxKind> {
    match value {
        "human" => Some(MailboxKind::Human),
        "agent" => Some(MailboxKind::Agent),
        "shared" => Some(MailboxKind::Shared),
        "group" => Some(MailboxKind::Group),
        unknown => {
            tracing::error!(kind = unknown, "unknown mailbox kind denied");
            None
        }
    }
}

const fn mailbox_membership_role(role: MailboxMembershipRole) -> &'static str {
    match role {
        MailboxMembershipRole::Owner => "owner",
        MailboxMembershipRole::Member => "member",
        MailboxMembershipRole::Delegate => "delegate",
    }
}

fn mailbox_membership_role_from_str(value: &str) -> Option<MailboxMembershipRole> {
    match value {
        "owner" => Some(MailboxMembershipRole::Owner),
        "member" => Some(MailboxMembershipRole::Member),
        "delegate" => Some(MailboxMembershipRole::Delegate),
        unknown => {
            tracing::error!(role = unknown, "unknown mailbox membership role denied");
            None
        }
    }
}

fn mailbox_capability_from_str(value: &str) -> Option<MailboxCapability> {
    match value {
        "mailbox.read" => Some(MailboxCapability::Read),
        "mailbox.send" => Some(MailboxCapability::Send),
        "mailbox.send_as" => Some(MailboxCapability::SendAs),
        unknown => {
            tracing::error!(scope = unknown, "unknown mailbox capability denied");
            None
        }
    }
}

fn mail_domain_summary_from_row(
    row: MailDomainSummaryRow,
) -> Result<MailDomainSummary, StoreError> {
    Ok(MailDomainSummary {
        mail_domain_id: MailDomainId::from_uuid(row.mail_domain_id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        name: MailDomainName::parse(row.name).map_err(|error| {
            tracing::error!(%error, "invalid mail domain name persisted");
            StoreError::Unavailable
        })?,
        status: mail_domain_status_from_str(&row.status)?,
        route_mode: mail_domain_route_mode_from_str(&row.route_mode)?,
        ownership_challenge_version: row.ownership_challenge_version,
        route_version: row.route_version,
        inbound_enabled: row.inbound_enabled,
        outbound_enabled: row.outbound_enabled,
        smart_host_ref: row.smart_host_ref,
        dkim_key_ref: row.dkim_key_ref,
        mail_engine_domain_ref: row.mail_engine_domain_ref,
        mail_engine_projection_status: mail_engine_projection_status_from_str(
            &row.mail_engine_projection_status,
        )?,
        mail_engine_projected_enabled: row.mail_engine_projected_enabled,
        mail_engine_projection_last_attempt_at: row.mail_engine_projection_last_attempt_at,
        mail_engine_projection_succeeded_at: row.mail_engine_projection_succeeded_at,
        mail_engine_projection_error_code: row.mail_engine_projection_error_code,
        latest_dns_observed_at: row.latest_dns_observed_at,
        latest_dns_valid_until: row.latest_dns_valid_until,
        latest_dns_activation_ready: row.latest_dns_activation_ready,
        latest_dns_failure_codes: row.latest_dns_failure_codes,
    })
}

fn webhook_event_types_from_strings(
    values: &[String],
) -> Result<BTreeSet<WebhookEventType>, StoreError> {
    let event_types = values
        .iter()
        .map(|value| WebhookEventType::parse(value).map_err(|_| StoreError::Unavailable))
        .collect::<Result<BTreeSet<_>, _>>()?;
    if event_types.len() != values.len() || event_types.is_empty() {
        return Err(StoreError::Unavailable);
    }
    Ok(event_types)
}

fn webhook_event_type_names(values: &BTreeSet<WebhookEventType>) -> Vec<&'static str> {
    values.iter().map(|value| value.as_str()).collect()
}

fn webhook_subscription_from_row(
    row: WebhookSubscriptionRow,
) -> Result<WebhookSubscription, StoreError> {
    Ok(WebhookSubscription {
        id: WebhookSubscriptionId::from_uuid(row.subscription_id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        endpoint_url: row.endpoint_url,
        event_types: webhook_event_types_from_strings(&row.event_types)?,
        signing_key_reference: row.signing_key_reference,
        enabled: row.enabled,
        version: row.version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn webhook_subscription_summary_from_row(
    row: WebhookSubscriptionRow,
) -> Result<WebhookSubscriptionSummary, StoreError> {
    let event_types = webhook_event_types_from_strings(&row.event_types)?;
    Ok(WebhookSubscriptionSummary {
        subscription_id: WebhookSubscriptionId::from_uuid(row.subscription_id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        endpoint_url: row.endpoint_url,
        event_types: event_types.into_iter().collect(),
        signing_key_reference: row.signing_key_reference,
        enabled: row.enabled,
        version: row.version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

const fn mail_domain_status(status: MailDomainStatus) -> &'static str {
    match status {
        MailDomainStatus::Draft => "draft",
        MailDomainStatus::PendingDns => "pending_dns",
        MailDomainStatus::Verified => "verified",
        MailDomainStatus::Active => "active",
        MailDomainStatus::Suspended => "suspended",
        MailDomainStatus::Retired => "retired",
    }
}

fn mail_domain_status_from_str(value: &str) -> Result<MailDomainStatus, StoreError> {
    match value {
        "draft" => Ok(MailDomainStatus::Draft),
        "pending_dns" => Ok(MailDomainStatus::PendingDns),
        "verified" => Ok(MailDomainStatus::Verified),
        "active" => Ok(MailDomainStatus::Active),
        "suspended" => Ok(MailDomainStatus::Suspended),
        "retired" => Ok(MailDomainStatus::Retired),
        unknown => {
            tracing::error!(status = unknown, "unknown mail domain status persisted");
            Err(StoreError::Unavailable)
        }
    }
}

const fn mail_domain_route_mode(mode: MailDomainRouteMode) -> &'static str {
    match mode {
        MailDomainRouteMode::DirectMx => "direct_mx",
        MailDomainRouteMode::SmartHost => "smart_host",
    }
}

fn mail_domain_route_mode_from_str(value: &str) -> Result<MailDomainRouteMode, StoreError> {
    match value {
        "direct_mx" => Ok(MailDomainRouteMode::DirectMx),
        "smart_host" => Ok(MailDomainRouteMode::SmartHost),
        unknown => {
            tracing::error!(
                route_mode = unknown,
                "unknown mail domain route mode persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

fn mail_engine_projection_status_from_str(
    value: &str,
) -> Result<MailEngineProjectionStatus, StoreError> {
    match value {
        "not_projected" => Ok(MailEngineProjectionStatus::NotProjected),
        "pending" => Ok(MailEngineProjectionStatus::Pending),
        "ready" => Ok(MailEngineProjectionStatus::Ready),
        "failed" => Ok(MailEngineProjectionStatus::Failed),
        unknown => {
            tracing::error!(
                projection_status = unknown,
                "unknown mail engine projection status persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

fn sha256_array(value: &[u8], field: &'static str) -> Result<[u8; 32], StoreError> {
    value.try_into().map_err(|_| {
        tracing::error!(
            field,
            length = value.len(),
            "invalid SHA-256 digest persisted"
        );
        StoreError::Unavailable
    })
}

fn valid_engine_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.chars().any(char::is_control)
        && !value.contains('/')
}

fn valid_stable_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        })
}

const fn approval_status(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
        ApprovalStatus::Expired => "expired",
        ApprovalStatus::Cancelled => "cancelled",
    }
}

const fn approval_decision(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Approved => "approved",
        ApprovalDecision::Rejected => "rejected",
    }
}

const fn approval_request_view(view: ApprovalRequestView) -> &'static str {
    match view {
        ApprovalRequestView::Requested => "requested",
        ApprovalRequestView::Actionable => "actionable",
    }
}

fn approval_status_from_str(value: &str) -> Result<ApprovalStatus, StoreError> {
    match value {
        "pending" => Ok(ApprovalStatus::Pending),
        "approved" => Ok(ApprovalStatus::Approved),
        "rejected" => Ok(ApprovalStatus::Rejected),
        "expired" => Ok(ApprovalStatus::Expired),
        "cancelled" => Ok(ApprovalStatus::Cancelled),
        unknown => {
            tracing::error!(status = unknown, "unknown approval status persisted");
            Err(StoreError::Unavailable)
        }
    }
}

fn approval_decision_from_str(value: &str) -> Result<ApprovalDecision, StoreError> {
    match value {
        "approved" => Ok(ApprovalDecision::Approved),
        "rejected" => Ok(ApprovalDecision::Rejected),
        unknown => {
            tracing::error!(decision = unknown, "unknown approval decision persisted");
            Err(StoreError::Unavailable)
        }
    }
}

fn approval_request_summary_from_row(
    row: ApprovalRequestSummaryRow,
) -> Result<ApprovalRequestSummary, StoreError> {
    Ok(ApprovalRequestSummary {
        request_id: copilotmail_domain::ApprovalRequestId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        policy_version_id: PolicyVersionId::from_uuid(row.policy_version_id),
        action: approval_action_from_str(&row.action).ok_or(StoreError::Unavailable)?,
        resource_kind: row.resource_kind,
        resource_id: row.resource_id,
        requested_by: PrincipalId::from_uuid(row.requested_by),
        reason: row.reason,
        ticket_reference: row.ticket_reference,
        required_approvals: u8::try_from(row.required_approvals)
            .map_err(|_| StoreError::Unavailable)?,
        approval_count: u8::try_from(row.approval_count).map_err(|_| StoreError::Unavailable)?,
        rejection_count: u8::try_from(row.rejection_count).map_err(|_| StoreError::Unavailable)?,
        status: approval_status_from_str(&row.effective_status)?,
        created_at: row.created_at,
        expires_at: row.expires_at,
        completed_at: row.completed_at,
        cancelled_at: row.cancelled_at,
    })
}

fn role_assignment_summary_from_row(
    row: RoleAssignmentSummaryRow,
) -> Result<RoleAssignmentSummary, StoreError> {
    Ok(RoleAssignmentSummary {
        assignment_id: RoleAssignmentId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        principal_id: PrincipalId::from_uuid(row.principal_id),
        principal_kind: principal_kind_from_str(&row.principal_kind)?,
        principal_status: principal_status_from_str(&row.principal_status)?,
        principal_display_name: row.principal_display_name,
        role_id: RoleId::from_uuid(row.role_id),
        system_role: system_role_from_str(&row.system_key)?,
        role_display_name: row.role_display_name,
        created_by: PrincipalId::from_uuid(row.created_by),
        approval_request_id: row
            .approval_request_id
            .map(copilotmail_domain::ApprovalRequestId::from_uuid),
        valid_from: row.valid_from,
        valid_until: row.valid_until,
        revoked_at: row.revoked_at,
        created_at: row.created_at,
    })
}

fn approval_step_summary_from_row(
    row: ApprovalStepSummaryRow,
) -> Result<ApprovalStepSummary, StoreError> {
    Ok(ApprovalStepSummary {
        step_id: copilotmail_domain::ApprovalStepId::from_uuid(row.id),
        approver_principal_id: PrincipalId::from_uuid(row.approver_principal_id),
        decision: approval_decision_from_str(&row.decision)?,
        reason: row.reason,
        decided_at: row.decided_at,
    })
}

fn approval_action_from_str(value: &str) -> Option<ApprovalAction> {
    match value {
        "mail.send_as" => Some(ApprovalAction::SendAs),
        "mail.send_on_behalf_of" => Some(ApprovalAction::SendOnBehalfOf),
        "compliance.content_review" => Some(ApprovalAction::ContentReview),
        "audit.export" => Some(ApprovalAction::AuditExport),
        "compliance.retention_change" => Some(ApprovalAction::RetentionChange),
        "authorization.privilege_elevation" => Some(ApprovalAction::PrivilegeElevation),
        "compliance.legal_hold" => Some(ApprovalAction::LegalHold),
        "security.break_glass" => Some(ApprovalAction::BreakGlass),
        "mail.bulk_delete" => Some(ApprovalAction::BulkDelete),
        "agent.production_mail" => Some(ApprovalAction::ProductionAgentMail),
        "agent.first_contact" => Some(ApprovalAction::AgentFirstContact),
        "session.revoke_others" => Some(ApprovalAction::SessionRevokeOthers),
        unknown => {
            tracing::error!(action = unknown, "unknown approval action denied");
            None
        }
    }
}

#[cfg(test)]
fn verify_audit_chain_rows(
    organization_id: OrganizationId,
    rows: &[AuditChainRow],
) -> Result<AuditChainVerificationReport, AuditChainVerificationError> {
    let mut state = AuditChainVerificationState::new(organization_id);
    for row in rows {
        state.verify_row(row)?;
    }
    Ok(state.finish())
}

impl AuditChainVerificationState {
    const fn new(organization_id: OrganizationId) -> Self {
        Self {
            organization_id,
            event_count: 0,
            first_event_id: None,
            last_event_id: None,
            expected_previous_hash: None,
            hash_version: None,
        }
    }

    fn verify_row(&mut self, row: &AuditChainRow) -> Result<(), AuditChainVerificationError> {
        let expected_sequence =
            i64::try_from(self.event_count + 1).expect("audit event count cannot exceed i64");
        let violation = |reason: String| AuditChainVerificationError::Integrity {
            event_id: row.id,
            chain_sequence: row.chain_sequence,
            reason,
        };

        if row.organization_id != self.organization_id.as_uuid() {
            return Err(violation("event belongs to another organization".into()));
        }
        if row.chain_sequence != expected_sequence {
            return Err(violation(format!(
                "expected sequence {expected_sequence}, found {}",
                row.chain_sequence
            )));
        }
        if row.hash_version != 1 {
            return Err(violation(format!(
                "unsupported hash version {}",
                row.hash_version
            )));
        }
        if row.previous_hash != self.expected_previous_hash {
            return Err(violation("previous hash does not match prior event".into()));
        }

        let expected_event_hash =
            calculate_audit_event_hash(self.expected_previous_hash.as_deref(), &row.hash_payload);
        if row.event_hash != expected_event_hash {
            return Err(violation(
                "event hash does not match persisted hash payload".into(),
            ));
        }
        self.first_event_id.get_or_insert(row.id);
        self.last_event_id = Some(row.id);
        self.expected_previous_hash = Some(row.event_hash.clone());
        self.hash_version.get_or_insert(row.hash_version);
        self.event_count += 1;
        Ok(())
    }

    fn finish(self) -> AuditChainVerificationReport {
        AuditChainVerificationReport {
            organization_id: self.organization_id,
            event_count: self.event_count,
            first_event_id: self.first_event_id,
            last_event_id: self.last_event_id,
            head_hash: self.expected_previous_hash.as_deref().map(encode_hex),
            hash_version: self.hash_version,
        }
    }
}

fn calculate_audit_event_hash(previous_hash: Option<&[u8]>, hash_payload: &str) -> Vec<u8> {
    let mut digest = Sha256::new();
    if let Some(previous_hash) = previous_hash {
        digest.update(previous_hash);
    }
    digest.update(hash_payload.as_bytes());
    digest.finalize().to_vec()
}

fn encode_hex(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Loads the current Human mailbox authorization snapshot without binding the
/// result to a particular mail data plane. Content-free historical evidence
/// (such as the sent-directory) remains valid after a safe binding rotation.
async fn load_human_sent_access_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    mailbox_id: MailboxId,
    now: DateTime<Utc>,
) -> Result<Option<MailboxAccessSnapshot>, StoreError> {
    #[derive(sqlx::FromRow)]
    struct AccessRow {
        mailbox_id: Uuid,
        organization_id: Uuid,
        membership: Option<String>,
    }
    #[derive(sqlx::FromRow)]
    struct DelegationRow {
        id: Uuid,
        grantor_principal_id: Uuid,
        scopes: Vec<String>,
    }

    let row = sqlx::query_as::<_, AccessRow>(
        r#"
        SELECT mailbox.id AS mailbox_id, mailbox.organization_id,
               (
                   SELECT membership.role
                     FROM mailbox_memberships AS membership
                    WHERE membership.organization_id = mailbox.organization_id
                      AND membership.mailbox_id = mailbox.id
                      AND membership.principal_id = principal.id
                      AND membership.valid_from <= $4
                      AND (
                          membership.valid_until IS NULL
                          OR membership.valid_until > $4
                      )
                    ORDER BY CASE membership.role
                        WHEN 'owner' THEN 1
                        WHEN 'member' THEN 2
                        ELSE 3
                    END
                    LIMIT 1
               ) AS membership
          FROM mailboxes AS mailbox
          JOIN principals AS principal
            ON principal.id = $2
           AND principal.organization_id = mailbox.organization_id
           AND principal.kind = 'human'
           AND principal.status = 'active'
         WHERE mailbox.organization_id = $1
           AND mailbox.id = $3
           AND EXISTS (
               SELECT 1
                 FROM role_assignments AS assignment
                 JOIN roles AS role
                   ON role.id = assignment.role_id
                  AND role.organization_id = assignment.organization_id
                 JOIN role_permissions AS permission
                   ON permission.role_id = role.id
                  AND permission.permission = 'mailbox.use'
                WHERE assignment.organization_id = $1
                  AND assignment.principal_id = $2
                  AND assignment.revoked_at IS NULL
                  AND assignment.valid_from <= $4
                  AND (
                      assignment.valid_until IS NULL
                      OR assignment.valid_until > $4
                  )
           )
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(mailbox_id.as_uuid())
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_store_error)?;
    let Some(row) = row else {
        return Ok(None);
    };

    let delegation_rows = sqlx::query_as::<_, DelegationRow>(
        r#"
        SELECT id, grantor_principal_id, scopes
          FROM delegations
         WHERE organization_id = $1
           AND mailbox_id = $2
           AND grantee_principal_id = $3
           AND revoked_at IS NULL
           AND valid_from <= $4
           AND valid_until > $4
         ORDER BY id
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(mailbox_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(now)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_store_error)?;

    let membership = row
        .membership
        .as_deref()
        .map(|role| mailbox_membership_role_from_str(role).ok_or(StoreError::Unavailable))
        .transpose()?;
    let delegations = delegation_rows
        .into_iter()
        .map(|delegation| {
            let capabilities = delegation
                .scopes
                .into_iter()
                .map(|scope| mailbox_capability_from_str(&scope).ok_or(StoreError::Unavailable))
                .collect::<Result<BTreeSet<_>, StoreError>>()?;
            Ok(ResolvedMailboxDelegation {
                delegation_id: DelegationId::from_uuid(delegation.id),
                grantor_principal_id: PrincipalId::from_uuid(delegation.grantor_principal_id),
                capabilities,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    Ok(Some(MailboxAccessSnapshot {
        organization_id: OrganizationId::from_uuid(row.organization_id),
        mailbox_id: MailboxId::from_uuid(row.mailbox_id),
        membership,
        delegations,
    }))
}

async fn load_human_draft_access_snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    mailbox_id: MailboxId,
    expected_data_plane_id: MailDataPlaneId,
    now: DateTime<Utc>,
) -> Result<
    Option<(
        MailboxSummary,
        MailEngineMailboxBinding,
        MailboxAccessSnapshot,
    )>,
    StoreError,
> {
    #[derive(sqlx::FromRow)]
    struct AccessRow {
        mailbox_id: Uuid,
        organization_id: Uuid,
        kind: String,
        primary_address: String,
        mailbox_created_at: DateTime<Utc>,
        mail_data_plane_id: Uuid,
        account_ref: String,
        binding_version: i64,
        binding_updated_at: DateTime<Utc>,
        membership: Option<String>,
    }
    #[derive(sqlx::FromRow)]
    struct DelegationRow {
        id: Uuid,
        grantor_principal_id: Uuid,
        scopes: Vec<String>,
    }

    let row = sqlx::query_as::<_, AccessRow>(
        r#"
        SELECT mailbox.id AS mailbox_id, mailbox.organization_id,
               mailbox.kind, mailbox.primary_address,
               mailbox.created_at AS mailbox_created_at,
               binding.mail_data_plane_id, binding.account_ref,
               binding.version AS binding_version,
               binding.updated_at AS binding_updated_at,
               (
                   SELECT membership.role
                     FROM mailbox_memberships AS membership
                    WHERE membership.organization_id = mailbox.organization_id
                      AND membership.mailbox_id = mailbox.id
                      AND membership.principal_id = principal.id
                      AND membership.valid_from <= $5
                      AND (
                          membership.valid_until IS NULL
                          OR membership.valid_until > $5
                      )
                    ORDER BY CASE membership.role
                        WHEN 'owner' THEN 1
                        WHEN 'member' THEN 2
                        ELSE 3
                    END
                    LIMIT 1
               ) AS membership
          FROM mailboxes AS mailbox
          JOIN principals AS principal
            ON principal.id = $2
           AND principal.organization_id = mailbox.organization_id
           AND principal.kind = 'human'
           AND principal.status = 'active'
          JOIN mail_engine_mailbox_bindings AS binding
            ON binding.organization_id = mailbox.organization_id
           AND binding.mailbox_id = mailbox.id
           AND binding.mail_data_plane_id = $4
         WHERE mailbox.organization_id = $1
           AND mailbox.id = $3
           AND EXISTS (
               SELECT 1
                 FROM role_assignments AS assignment
                 JOIN roles AS role
                   ON role.id = assignment.role_id
                  AND role.organization_id = assignment.organization_id
                 JOIN role_permissions AS permission
                   ON permission.role_id = role.id
                  AND permission.permission = 'mailbox.use'
                WHERE assignment.organization_id = $1
                  AND assignment.principal_id = $2
                  AND assignment.revoked_at IS NULL
                  AND assignment.valid_from <= $5
                  AND (
                      assignment.valid_until IS NULL
                      OR assignment.valid_until > $5
                  )
           )
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(mailbox_id.as_uuid())
    .bind(expected_data_plane_id.as_uuid())
    .bind(now)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_store_error)?;
    let Some(row) = row else {
        return Ok(None);
    };

    let delegation_rows = sqlx::query_as::<_, DelegationRow>(
        r#"
        SELECT id, grantor_principal_id, scopes
          FROM delegations
         WHERE organization_id = $1
           AND mailbox_id = $2
           AND grantee_principal_id = $3
           AND revoked_at IS NULL
           AND valid_from <= $4
           AND valid_until > $4
         ORDER BY id
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(mailbox_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(now)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_store_error)?;

    let membership = row
        .membership
        .as_deref()
        .map(|role| mailbox_membership_role_from_str(role).ok_or(StoreError::Unavailable))
        .transpose()?;
    let delegations = delegation_rows
        .into_iter()
        .map(|delegation| {
            let capabilities = delegation
                .scopes
                .into_iter()
                .map(|scope| mailbox_capability_from_str(&scope).ok_or(StoreError::Unavailable))
                .collect::<Result<BTreeSet<_>, StoreError>>()?;
            Ok(ResolvedMailboxDelegation {
                delegation_id: DelegationId::from_uuid(delegation.id),
                grantor_principal_id: PrincipalId::from_uuid(delegation.grantor_principal_id),
                capabilities,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let kind = mailbox_kind_from_str(&row.kind).ok_or(StoreError::Unavailable)?;
    let primary_address = copilotmail_domain::MailboxAddress::parse(row.primary_address)
        .map_err(|_| StoreError::Unavailable)?;
    let resolved_organization_id = OrganizationId::from_uuid(row.organization_id);
    let resolved_mailbox_id = MailboxId::from_uuid(row.mailbox_id);
    let data_plane_id = MailDataPlaneId::from_uuid(row.mail_data_plane_id);
    Ok(Some((
        MailboxSummary {
            mailbox_id: resolved_mailbox_id,
            organization_id: resolved_organization_id,
            kind,
            primary_address,
            created_at: row.mailbox_created_at,
        },
        MailEngineMailboxBinding {
            organization_id: resolved_organization_id,
            mailbox_id: resolved_mailbox_id,
            data_plane_id,
            account_ref: row.account_ref,
            version: row.binding_version,
            updated_at: row.binding_updated_at,
        },
        MailboxAccessSnapshot {
            organization_id: resolved_organization_id,
            mailbox_id: resolved_mailbox_id,
            membership,
            delegations,
        },
    )))
}

fn human_draft_access_permits_read_and_send(
    authorization: &MailboxAccessSnapshot,
    organization_id: OrganizationId,
    mailbox_id: MailboxId,
) -> bool {
    authorization
        .authorize(organization_id, mailbox_id, MailboxCapability::Read, None)
        .is_some()
        && authorization
            .authorize(organization_id, mailbox_id, MailboxCapability::Send, None)
            .is_some()
}

fn mail_draft_from_row(row: MailDraftRow) -> Result<MailDraftRecord, StoreError> {
    let body_ref: [u8; 32] = row.body_ref.try_into().map_err(|value: Vec<u8>| {
        tracing::error!(
            digest_bytes = value.len(),
            "invalid mail draft body reference persisted"
        );
        StoreError::Unavailable
    })?;
    let content_sha256: [u8; 32] = row.content_sha256.try_into().map_err(|value: Vec<u8>| {
        tracing::error!(
            digest_bytes = value.len(),
            "invalid mail draft digest persisted"
        );
        StoreError::Unavailable
    })?;
    let status = match row.status.as_str() {
        "active" => MailDraftStatus::Active,
        "frozen" => MailDraftStatus::Frozen,
        "sent" => MailDraftStatus::Sent,
        unknown => {
            tracing::error!(status = unknown, "unknown mail draft status persisted");
            return Err(StoreError::Unavailable);
        }
    };
    let recipient_refs = row.recipient_refs.unwrap_or_default();
    let recipient_domains = row.recipient_domains.unwrap_or_default();
    if recipient_refs.len() != recipient_domains.len() {
        tracing::error!("mail draft recipient evidence has mismatched cardinality");
        return Err(StoreError::Unavailable);
    }
    let recipients = recipient_refs
        .into_iter()
        .zip(recipient_domains)
        .map(|(recipient_ref, domain)| {
            let recipient_ref = recipient_ref.try_into().map_err(|value: Vec<u8>| {
                tracing::error!(
                    digest_bytes = value.len(),
                    "invalid mail draft recipient reference persisted"
                );
                StoreError::Unavailable
            })?;
            Ok(MailDraftRecipientEvidence {
                recipient_ref,
                domain,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    Ok(MailDraftRecord {
        draft_id: DraftId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        mailbox_id: MailboxId::from_uuid(row.mailbox_id),
        created_by: PrincipalId::from_uuid(row.created_by),
        data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
        engine_account_ref: row.engine_account_ref,
        engine_email_ref: row.engine_email_ref,
        engine_thread_ref: row.engine_thread_ref,
        reply_to_message_id: row.reply_to_message_id,
        reply_to_automatic: row.reply_to_automatic,
        body_ref,
        content_sha256,
        recipients,
        version: row.version,
        status,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn mail_submission_intent_from_row(
    row: MailSubmissionIntentRow,
) -> Result<MailSubmissionIntentRecord, StoreError> {
    let request_sha256: [u8; 32] = row.request_sha256.try_into().map_err(|value: Vec<u8>| {
        tracing::error!(
            digest_bytes = value.len(),
            "invalid mail submission request digest persisted"
        );
        StoreError::Unavailable
    })?;
    let status = match row.status.as_str() {
        "pending" => MailSubmissionStatus::Pending,
        "reconciling" => MailSubmissionStatus::Reconciling,
        "accepted" => MailSubmissionStatus::Accepted,
        "rejected" => MailSubmissionStatus::Rejected,
        "manual_review" => MailSubmissionStatus::ManualReview,
        unknown => {
            tracing::error!(status = unknown, "unknown mail submission status persisted");
            return Err(StoreError::Unavailable);
        }
    };
    Ok(MailSubmissionIntentRecord {
        submission_id: MailSubmissionId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        requested_by: PrincipalId::from_uuid(row.requested_by),
        mailbox_id: MailboxId::from_uuid(row.mailbox_id),
        draft_id: DraftId::from_uuid(row.draft_id),
        draft_version: row.draft_version,
        data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
        request_sha256,
        status,
        state_version: row.state_version,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn agent_mail_sandbox_submission_from_row(
    row: AgentMailSandboxSubmissionRow,
) -> Result<AgentMailSandboxSubmissionRecord, StoreError> {
    let _: [u8; 32] = row.request_sha256.try_into().map_err(|value: Vec<u8>| {
        tracing::error!(
            digest_bytes = value.len(),
            "invalid Agent mail sandbox request digest persisted"
        );
        StoreError::Unavailable
    })?;
    Ok(AgentMailSandboxSubmissionRecord {
        submission_id: row.id,
        organization_id: OrganizationId::from_uuid(row.organization_id),
        agent_id: PrincipalId::from_uuid(row.agent_id),
        mailbox_id: MailboxId::from_uuid(row.mailbox_id),
        draft_id: DraftId::from_uuid(row.draft_id),
        draft_version: row.draft_version,
        recipient_count: u16::try_from(row.recipient_count).map_err(|_| StoreError::Unavailable)?,
        captured_at: row.captured_at,
    })
}

fn agent_mail_admission_from_row(
    row: AgentMailAdmissionRow,
) -> Result<AgentMailAdmissionRecord, StoreError> {
    Ok(AgentMailAdmissionRecord {
        id: row.id,
        organization_id: OrganizationId::from_uuid(row.organization_id),
        agent_id: PrincipalId::from_uuid(row.agent_id),
        submission_id: MailSubmissionId::from_uuid(row.submission_id),
        policy_version_id: row.policy_version_id,
        policy_version: u32::try_from(row.policy_version).map_err(|_| StoreError::Unavailable)?,
        recipient_count: u16::try_from(row.recipient_count).map_err(|_| StoreError::Unavailable)?,
        first_contact_count: u16::try_from(row.first_contact_count)
            .map_err(|_| StoreError::Unavailable)?,
        admitted_at: row.admitted_at,
    })
}

fn mail_delivery_recipient_from_row(
    row: MailDeliveryRecipientRow,
) -> Result<MailDeliveryRecipientWrite, StoreError> {
    Ok(MailDeliveryRecipientWrite {
        recipient_ref: digest_from_bytes(row.recipient_ref, "mail delivery recipient")?,
        state: parse_mail_delivery_recipient_state(&row.state)?,
        smtp_reply: row.smtp_reply,
    })
}

const fn mail_delivery_recipient_state(state: MailDeliveryRecipientState) -> &'static str {
    match state {
        MailDeliveryRecipientState::Queued => "queued",
        MailDeliveryRecipientState::Delivered => "delivered",
        MailDeliveryRecipientState::PermanentFailure => "permanent_failure",
        MailDeliveryRecipientState::Unknown => "unknown",
    }
}

fn parse_mail_delivery_recipient_state(
    state: &str,
) -> Result<MailDeliveryRecipientState, StoreError> {
    match state {
        "queued" => Ok(MailDeliveryRecipientState::Queued),
        "delivered" => Ok(MailDeliveryRecipientState::Delivered),
        "permanent_failure" => Ok(MailDeliveryRecipientState::PermanentFailure),
        "unknown" => Ok(MailDeliveryRecipientState::Unknown),
        unknown => {
            tracing::error!(
                state = unknown,
                "unknown mail delivery recipient state persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

const fn outbound_mail_event_kind(kind: OutboundMailEventKind) -> &'static str {
    match kind {
        OutboundMailEventKind::Delivered => "delivered",
        OutboundMailEventKind::PermanentFailure => "permanent_failure",
        OutboundMailEventKind::Rescheduled => "rescheduled",
        OutboundMailEventKind::Completed => "completed",
    }
}

fn digest_from_bytes(value: Vec<u8>, label: &str) -> Result<[u8; 32], StoreError> {
    value.try_into().map_err(|value: Vec<u8>| {
        tracing::error!(
            digest_bytes = value.len(),
            evidence = label,
            "invalid SHA-256 evidence persisted"
        );
        StoreError::Unavailable
    })
}

fn mail_submission_lease_from_row(
    row: MailSubmissionLeaseRow,
) -> Result<MailSubmissionLease, StoreError> {
    let authorized_from =
        copilotmail_domain::MailboxAddress::parse(&row.primary_address).map_err(|error| {
            tracing::error!(
                error = %error,
                "invalid mailbox address persisted for mail submission"
            );
            StoreError::Unavailable
        })?;
    if row.attempt_count <= 0 || row.draft_version <= 0 {
        tracing::error!("invalid mail submission lease persisted");
        return Err(StoreError::Unavailable);
    }
    Ok(MailSubmissionLease {
        submission_id: MailSubmissionId::from_uuid(row.id),
        organization_id: OrganizationId::from_uuid(row.organization_id),
        requested_by: PrincipalId::from_uuid(row.requested_by),
        mailbox_id: MailboxId::from_uuid(row.mailbox_id),
        draft_id: DraftId::from_uuid(row.draft_id),
        draft_version: row.draft_version,
        data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
        engine_account_ref: row.engine_account_ref,
        engine_email_ref: row.engine_email_ref,
        authorized_from,
        lease_id: row.lease_id,
        lease_expires_at: row.lease_expires_at,
        attempt_count: row.attempt_count,
    })
}

fn map_agent_mail_admission_error(error: sqlx::Error) -> StoreError {
    let reason =
        error
            .as_database_error()
            .and_then(|database_error| match database_error.constraint() {
                Some("agent_mail_recipient_policy") => {
                    Some(AgentMailAdmissionFailure::RecipientPolicy)
                }
                Some("agent_mail_hourly_quota") => Some(AgentMailAdmissionFailure::HourlyQuota),
                Some("agent_mail_recipient_daily_quota") => {
                    Some(AgentMailAdmissionFailure::RecipientDailyQuota)
                }
                Some("agent_mail_thread_circuit_breaker") => {
                    Some(AgentMailAdmissionFailure::ThreadCircuitBreaker)
                }
                Some("agent_mail_automatic_reply_loop") => {
                    Some(AgentMailAdmissionFailure::AutomaticReplyLoop)
                }
                Some("agent_mail_duplicate_content_circuit_breaker") => {
                    Some(AgentMailAdmissionFailure::DuplicateContentCircuitBreaker)
                }
                Some("agent_mail_testing_requires_sandbox") => {
                    Some(AgentMailAdmissionFailure::SandboxRequired)
                }
                Some("agent_mail_first_contact_approval") => {
                    Some(AgentMailAdmissionFailure::FirstContactApproval)
                }
                _ => None,
            });
    reason.map_or_else(|| map_store_error(error), StoreError::AgentMailAdmission)
}

fn agent_processing_subscription_status(
    status: &str,
) -> Result<AgentProcessingSubscriptionStatus, StoreError> {
    match status {
        "active" => Ok(AgentProcessingSubscriptionStatus::Active),
        "paused" => Ok(AgentProcessingSubscriptionStatus::Paused),
        "retired" => Ok(AgentProcessingSubscriptionStatus::Retired),
        unknown => {
            tracing::error!(
                status = unknown,
                "unknown Agent processing subscription status persisted"
            );
            Err(StoreError::Unavailable)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn agent_processing_subscription_change_result(
    operation_id: Uuid,
    organization_id: Uuid,
    subscription_id: Uuid,
    principal_id: Uuid,
    mailbox_id: Uuid,
    previous_status: &str,
    result_status: &str,
    created_at: DateTime<Utc>,
    occurred_at: DateTime<Utc>,
    replayed: bool,
) -> Result<AgentProcessingSubscriptionChangeResult, StoreError> {
    Ok(AgentProcessingSubscriptionChangeResult {
        operation_id,
        subscription: AgentProcessingSubscriptionRecord {
            subscription_id: SubscriptionId::from_uuid(subscription_id),
            organization_id: OrganizationId::from_uuid(organization_id),
            principal_id: PrincipalId::from_uuid(principal_id),
            mailbox_id: MailboxId::from_uuid(mailbox_id),
            status: agent_processing_subscription_status(result_status)?,
            created_at,
            updated_at: occurred_at,
        },
        previous_status: agent_processing_subscription_status(previous_status)?,
        occurred_at,
        replayed,
    })
}

fn map_store_error(error: sqlx::Error) -> StoreError {
    let Some(database_error) = error.as_database_error() else {
        tracing::error!(error = %error, "PostgreSQL operation failed");
        return StoreError::Unavailable;
    };
    match database_error.code().as_deref() {
        Some("23503") => StoreError::MissingReference,
        Some("23505" | "23514") => StoreError::Conflict,
        _ => {
            tracing::error!(
                code = database_error.code().as_deref(),
                constraint = database_error.constraint(),
                "PostgreSQL rejected control-plane write"
            );
            StoreError::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain_rows(organization_id: OrganizationId, payloads: &[&str]) -> Vec<AuditChainRow> {
        let mut previous_hash: Option<Vec<u8>> = None;
        payloads
            .iter()
            .enumerate()
            .map(|(index, payload)| {
                let event_hash = calculate_audit_event_hash(previous_hash.as_deref(), payload);
                let row = AuditChainRow {
                    id: Uuid::now_v7(),
                    organization_id: organization_id.as_uuid(),
                    chain_sequence: i64::try_from(index + 1).unwrap(),
                    hash_version: 1,
                    hash_payload: (*payload).into(),
                    previous_hash: previous_hash.clone(),
                    event_hash: event_hash.clone(),
                };
                previous_hash = Some(event_hash);
                row
            })
            .collect()
    }

    #[test]
    fn verifies_a_complete_tenant_chain() {
        let organization_id = OrganizationId::new();
        let rows = chain_rows(organization_id, &[r#"{"id":1}"#, r#"{"id":2}"#]);

        let report = verify_audit_chain_rows(organization_id, &rows).unwrap();

        assert_eq!(report.event_count, 2);
        assert_eq!(report.first_event_id, Some(rows[0].id));
        assert_eq!(report.last_event_id, Some(rows[1].id));
        assert_eq!(report.head_hash.as_deref().map(str::len), Some(64));
        assert_eq!(report.hash_version, Some(1));
    }

    #[test]
    fn rejects_payload_tampering() {
        let organization_id = OrganizationId::new();
        let mut rows = chain_rows(organization_id, &[r#"{"id":1}"#, r#"{"id":2}"#]);
        rows[1].hash_payload.push(' ');

        let error = verify_audit_chain_rows(organization_id, &rows).unwrap_err();

        assert!(error.to_string().contains("event hash does not match"));
    }

    #[test]
    fn rejects_sequence_gaps_and_cross_tenant_rows() {
        let organization_id = OrganizationId::new();
        let mut rows = chain_rows(organization_id, &[r#"{"id":1}"#, r#"{"id":2}"#]);
        rows[1].chain_sequence = 3;
        assert!(verify_audit_chain_rows(organization_id, &rows).is_err());

        rows[1].chain_sequence = 2;
        rows[1].organization_id = OrganizationId::new().as_uuid();
        assert!(verify_audit_chain_rows(organization_id, &rows).is_err());
    }

    #[test]
    fn production_agent_terminal_audit_outcomes_are_state_bound() {
        assert_eq!(
            expected_terminal_audit(MailSubmissionStatus::Accepted),
            Some(("mail.submission.accepted", "success"))
        );
        assert_eq!(
            expected_terminal_audit(MailSubmissionStatus::Rejected),
            Some(("mail.submission.rejected", "failure"))
        );
        assert_eq!(
            expected_terminal_audit(MailSubmissionStatus::ManualReview),
            Some(("mail.submission.review_required", "failure"))
        );
        assert_eq!(expected_terminal_audit(MailSubmissionStatus::Pending), None);
        assert_eq!(
            expected_terminal_audit(MailSubmissionStatus::Reconciling),
            None
        );
    }

    #[test]
    fn production_agent_evidence_timestamps_are_causally_bound() {
        let created_at = Utc::now();
        let admitted_at = created_at + chrono::Duration::seconds(1);
        let accepted_at = admitted_at + chrono::Duration::seconds(1);
        let updated_at = accepted_at + chrono::Duration::seconds(1);
        let delivery_observed_at = updated_at + chrono::Duration::seconds(1);
        let observed_at = delivery_observed_at + chrono::Duration::seconds(1);

        assert!(valid_agent_mail_submission_timeline(
            created_at,
            admitted_at,
            updated_at,
            Some(accepted_at),
            MailSubmissionStatus::Accepted,
            observed_at,
        ));
        assert!(valid_agent_mail_submission_timeline(
            created_at,
            admitted_at,
            created_at,
            None,
            MailSubmissionStatus::Pending,
            observed_at,
        ));
        assert!(valid_agent_mail_submission_audit_timeline(
            &AgentMailSubmissionAuditTimeline {
                requested_at: created_at,
                admitted_audit_at: admitted_at,
                terminal_audit_at: Some(accepted_at),
                submission_created_at: created_at,
                admitted_at,
                submission_updated_at: updated_at,
                submission_accepted_at: Some(accepted_at),
                observed_at,
            },
        ));
        assert!(valid_agent_mail_submission_delivery_timeline(
            delivery_observed_at,
            admitted_at,
            Some(accepted_at),
            observed_at,
        ));

        assert!(!valid_agent_mail_submission_audit_timeline(
            &AgentMailSubmissionAuditTimeline {
                requested_at: admitted_at,
                admitted_audit_at: admitted_at,
                terminal_audit_at: Some(accepted_at),
                submission_created_at: created_at,
                admitted_at,
                submission_updated_at: updated_at,
                submission_accepted_at: Some(accepted_at),
                observed_at,
            },
        ));
        assert!(!valid_agent_mail_submission_audit_timeline(
            &AgentMailSubmissionAuditTimeline {
                requested_at: created_at,
                admitted_audit_at: admitted_at,
                terminal_audit_at: Some(admitted_at - chrono::Duration::nanoseconds(1)),
                submission_created_at: created_at,
                admitted_at,
                submission_updated_at: updated_at,
                submission_accepted_at: Some(accepted_at),
                observed_at,
            },
        ));
        assert!(!valid_agent_mail_submission_delivery_timeline(
            accepted_at - chrono::Duration::nanoseconds(1),
            admitted_at,
            Some(accepted_at),
            observed_at,
        ));
    }
}
