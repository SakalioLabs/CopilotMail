//! PostgreSQL implementation of CopilotMail control-plane persistence ports.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use copilotmail_application::{
    ActiveApprovalPolicy, AgentCredentialStore, AgentCursorEvent, AgentEventCursorQueryStore,
    AgentEventStreamLease, AgentEventStreamLeaseAcquisition, AgentIngressRateLimitDecision,
    AgentIngressRateLimitStore, AgentMailAdmissionFailure, AgentMailAdmissionRecord,
    AgentMailContentQueryStore, AgentMailContentTarget, AgentMailPolicyQueryStore,
    AgentMailPolicyRuntimeSnapshot, AgentMailPolicyVersionRecord,
    AgentMailSandboxSubmissionCreation, AgentMailSandboxSubmissionRecord,
    AgentMailSandboxSubmissionWrite, AgentMessageClaimWrite, AgentMessageProcessingOperation,
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
    ComplianceQueryStore, ControlPlaneStore, ControlPlaneTransaction, ExistingAgentCredential,
    ExistingAgentMtlsCredential, ExistingAuditCheckpoint, ExistingHumanIdentity,
    ExistingOidcOwnerBinding, ExistingOrganizationBootstrap, ExistingScimGroup, ExistingScimUser,
    ExternalIdentityResolver, HumanDraftListBatch, HumanDraftListCandidate, HumanDraftListQuery,
    HumanDraftListRecheck, HumanDraftQueryStore, HumanDraftReadRecheck, HumanDraftRestoreQuery,
    HumanDraftRestoreResolution, HumanDraftRestoreTarget, HumanLoginAdmissionResult,
    HumanLoginTransactionRecord, HumanLoginTransactionStore, HumanLoginTransactionWrite,
    HumanMailContentQueryStore, HumanMailContentTarget, HumanMailMessageProjection,
    HumanMailboxDirectoryBatch, HumanMailboxDirectoryCandidate, HumanMailboxDirectoryQuery,
    HumanMailboxDirectoryStore, HumanSessionClient, HumanSessionQueryStore, HumanSessionRecord,
    HumanSessionRevocationCause, HumanSessionRevocationResult, HumanSessionWrite,
    InboundMailEventInsert, InboundMailEventRecord, LegalHoldObjectEvidence,
    MailDeliveryQueryStore, MailDeliveryRecipientState, MailDeliveryRecipientWrite,
    MailDeliveryReconciliationTarget, MailDeliverySnapshotInsert, MailDeliverySnapshotRecord,
    MailDeliverySnapshotWrite, MailDomainDnsObservation, MailDomainQueryStore, MailDomainSummary,
    MailDraftMetadataStore, MailDraftRecipientEvidence, MailDraftRecord, MailDraftStatus,
    MailDraftWrite, MailEngineMailboxBinding, MailEngineMailboxBindingWrite,
    MailEventIngestionStore, MailEventIngestionTransaction, MailReplyTarget,
    MailReplyTargetQueryStore, MailSubmissionIntentCreation, MailSubmissionIntentRecord,
    MailSubmissionIntentWrite, MailSubmissionLease, MailSubmissionQueryStore, MailSubmissionStatus,
    MailSubmissionTerminalWrite, MailboxAuthorizationStore, MailboxQueryStore, MailboxSummary,
    OrganizationQueryStore, OrganizationSummary, OutboundMailEventInsert, OutboundMailEventKind,
    OutboundMailEventRecord, PendingEvent, RetentionPolicyActivationRecord, RetentionPolicyDetail,
    RetentionPolicyQueryStore, RetentionPolicyRecord, RoleAssignmentGrantRecord,
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
    ApprovalDecision, ApprovalPolicyVersion, ApprovalRequest, ApprovalStatus, AuditArchiveId,
    AuditCheckpointId, AuditEvent, AuditExportGrant, AuditExportId, AuditOutcome,
    AuthorizationContext, ComplianceCase, ComplianceCaseId, ComplianceCaseStatus, CredentialId,
    DelegationId, DraftId, HumanLoginTransactionId, HumanProfile, HumanSessionId,
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
        sqlx::query_scalar(
            r#"
            SELECT to_regclass('public.organizations') IS NOT NULL
               AND to_regclass('public.organization_bootstraps') IS NOT NULL
               AND to_regclass('public.oidc_owner_bindings') IS NOT NULL
               AND to_regclass('public.scim_tokens') IS NOT NULL
               AND to_regclass('public.scim_users') IS NOT NULL
               AND to_regclass('public.scim_groups') IS NOT NULL
               AND to_regclass('public.scim_group_members') IS NOT NULL
               AND to_regclass('public.agent_private_key_credentials') IS NOT NULL
               AND to_regclass('public.agent_mtls_credentials') IS NOT NULL
               AND to_regclass('public.oauth_client_assertion_jtis') IS NOT NULL
               AND to_regclass('public.roles') IS NOT NULL
               AND EXISTS (
                   SELECT 1
                     FROM pg_constraint AS permission_check
                    WHERE permission_check.conrelid = 'public.role_permissions'::regclass
                      AND permission_check.conname = 'role_permissions_permission_check'
                      AND permission_check.contype = 'c'
                      AND permission_check.convalidated
                      AND pg_get_constraintdef(permission_check.oid)
                              LIKE '%agent.mail.read%'
               )               AND to_regclass('public.mailboxes') IS NOT NULL
               AND to_regclass('public.mail_domains') IS NOT NULL
               AND to_regclass('public.mail_domain_dns_observations') IS NOT NULL
               AND to_regclass('public.mail_domain_routes') IS NOT NULL
               AND to_regclass('public.mail_data_planes') IS NOT NULL
               AND EXISTS (
                   SELECT 1
                     FROM mail_data_planes
                    WHERE id = '018f305d-3d0f-7b2a-9d68-7f4a3f6e2101'::uuid
                      AND adapter_kind = 'stalwart_jmap'
               )
               AND to_regclass('public.mail_data_plane_origin_bindings') IS NOT NULL
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_data_planes', 'mail_data_planes_pkey', 'p', ARRAY['id']::text[]),
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_pkey', 'p', ARRAY['data_plane_id']::text[]),
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_canonical_origin_key', 'u', ARRAY['canonical_origin']::text[]),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_account_unique', 'u', ARRAY['mail_data_plane_id', 'account_ref']::text[]),
                         ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_account_unique', 'u', ARRAY['mail_data_plane_id', 'account_ref']::text[]),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_event_unique', 'u', ARRAY['mail_data_plane_id', 'source_event_id']::text[]),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_message_unique', 'u', ARRAY['mail_data_plane_id', 'account_ref', 'document_ref']::text[]),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_event_unique', 'u', ARRAY['mail_data_plane_id', 'source_event_id']::text[]),
                         ('mail_drafts', 'mail_drafts_data_plane_email_unique', 'u', ARRAY['mail_data_plane_id', 'engine_account_ref', 'engine_email_ref']::text[]),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_source_unique', 'u', ARRAY['organization_id', 'submission_id', 'mail_data_plane_id', 'source_sha256']::text[]),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_account_unique', 'u', ARRAY['source', 'account_ref']::text[]),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_event_unique', 'u', ARRAY['source', 'source_event_id']::text[]),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_message_unique', 'u', ARRAY['source', 'account_ref', 'document_ref']::text[]),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_event_unique', 'u', ARRAY['source', 'source_event_id']::text[]),
                         ('mail_drafts', 'mail_drafts_engine_email_unique', 'u', ARRAY['engine_source', 'engine_account_ref', 'engine_email_ref']::text[])
                     ) AS required_key(table_name, constraint_name, constraint_type, column_names)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_constraint AS constraint_row
                         WHERE constraint_row.conrelid =
                                   to_regclass('public.' || required_key.table_name)
                           AND constraint_row.conname = required_key.constraint_name
                           AND constraint_row.contype::text = required_key.constraint_type
                           AND constraint_row.convalidated
                           AND ARRAY(
                               SELECT attribute.attname::text
                                 FROM unnest(constraint_row.conkey)
                                      WITH ORDINALITY AS key_column(attnum, position)
                                 JOIN pg_attribute AS attribute
                                   ON attribute.attrelid = constraint_row.conrelid
                                  AND attribute.attnum = key_column.attnum
                                WHERE NOT attribute.attisdropped
                                ORDER BY key_column.position
                           ) = required_key.column_names
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_data_plane_id_fkey', 'data_plane_id'),
                         ('mail_domains', 'mail_domains_data_plane_fkey', 'mail_data_plane_id'),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_fkey', 'mail_data_plane_id'),
                         ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_fkey', 'mail_data_plane_id'),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_fkey', 'mail_data_plane_id'),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_fkey', 'mail_data_plane_id'),
                         ('mail_drafts', 'mail_drafts_data_plane_fkey', 'mail_data_plane_id'),
                         ('mail_submission_intents', 'mail_submission_intents_data_plane_fkey', 'mail_data_plane_id'),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_fkey', 'mail_data_plane_id'),
                         ('agent_mail_admissions', 'agent_mail_admissions_data_plane_fkey', 'mail_data_plane_id'),
                         ('outbox_events', 'outbox_events_data_plane_fkey', 'mail_data_plane_id')
                     ) AS required_fkey(table_name, constraint_name, column_name)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_constraint AS constraint_row
                         WHERE constraint_row.conrelid =
                                   to_regclass('public.' || required_fkey.table_name)
                           AND constraint_row.conname = required_fkey.constraint_name
                           AND constraint_row.contype = 'f'
                           AND constraint_row.convalidated
                           AND constraint_row.confrelid = 'public.mail_data_planes'::regclass
                           AND ARRAY(
                               SELECT attribute.attname::text
                                 FROM unnest(constraint_row.conkey)
                                      WITH ORDINALITY AS key_column(attnum, position)
                                 JOIN pg_attribute AS attribute
                                   ON attribute.attrelid = constraint_row.conrelid
                                  AND attribute.attnum = key_column.attnum
                                ORDER BY key_column.position
                           ) = ARRAY[required_fkey.column_name]::text[]
                           AND ARRAY(
                               SELECT attribute.attname::text
                                 FROM unnest(constraint_row.confkey)
                                      WITH ORDINALITY AS key_column(attnum, position)
                                 JOIN pg_attribute AS attribute
                                   ON attribute.attrelid = constraint_row.confrelid
                                  AND attribute.attnum = key_column.attnum
                                ORDER BY key_column.position
                           ) = ARRAY['id']::text[]
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_data_planes', 'mail_data_planes_non_nil_id_check', ARRAY['%00000000-0000-0000-0000-000000000000%']::text[]),
                         ('agent_private_key_credentials', 'agent_private_key_credentials_allowed_scopes_check', ARRAY['%cardinality(allowed_scopes) >= 1%', '%cardinality(allowed_scopes) <= 4%', '%agent.mail.read%']::text[]),
                         ('agent_mtls_credentials', 'agent_mtls_credentials_allowed_scopes_check', ARRAY['%cardinality(allowed_scopes) >= 1%', '%cardinality(allowed_scopes) <= 4%', '%agent.mail.read%']::text[]),
                         ('mail_data_planes', 'mail_data_planes_adapter_kind_check', ARRAY['%lower(adapter_kind)%', '%^[a-z][a-z0-9._-]{0,63}$%']::text[]),
                         ('mail_data_planes', 'mail_data_planes_label_check', ARRAY['%btrim(label)%', '%[[:cntrl:]]%']::text[]),
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_origin_shape_check', ARRAY['%lower(canonical_origin)%', '%https?://%']::text[]),
                         ('mail_domains', 'mail_domains_data_plane_assignment_check', ARRAY['%mail_data_plane_assigned_at >= created_at%']::text[]),
                         ('mail_domains', 'mail_domains_engine_ref_data_plane_check', ARRAY['%mail_engine_domain_ref IS NULL%', '%mail_data_plane_id IS NOT NULL%']::text[]),
                         ('mail_domains', 'mail_domains_projection_data_plane_check', ARRAY['%mail_engine_projection_status%', '%mail_engine_domain_ref IS NOT NULL%']::text[]),
                         ('outbox_events', 'outbox_events_required_data_plane_route_check', ARRAY['%mail_engine.mailbox_binding.updated.v2%', '%mail_domain.suspended.v1%', '%mail_data_plane_id IS NOT NULL%']::text[]),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_check', ARRAY['%stalwart%']::text[]),
                         ('mail_engine_account_owners', 'mail_engine_account_owners_source_check', ARRAY['%stalwart%']::text[]),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_check', ARRAY['%stalwart%']::text[]),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_check', ARRAY['%stalwart%']::text[]),
                         ('mail_drafts', 'mail_drafts_engine_source_check', ARRAY['%stalwart%']::text[]),
                         ('mail_submission_intents', 'mail_submission_intents_engine_source_check', ARRAY['%stalwart%']::text[]),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_source_check', ARRAY['%stalwart_jmap%']::text[])
                     ) AS required_check(table_name, constraint_name, definition_patterns)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_constraint AS constraint_row
                         WHERE constraint_row.conrelid =
                                   to_regclass('public.' || required_check.table_name)
                           AND constraint_row.conname = required_check.constraint_name
                           AND constraint_row.contype = 'c'
                           AND constraint_row.convalidated
                           AND pg_get_constraintdef(constraint_row.oid)
                                   LIKE ALL(required_check.definition_patterns)
                    )
               )               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_data_planes', 'mail_data_planes_reject_update_delete', 'reject_mail_data_plane_mutation', 27),
                         ('mail_data_planes', 'mail_data_planes_reject_truncate', 'reject_mail_data_plane_mutation', 34),
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_validate_insert', 'validate_mail_data_plane_origin_binding_insert', 7),
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_reject_update_delete', 'reject_mail_data_plane_origin_binding_mutation', 27),
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_reject_truncate', 'reject_mail_data_plane_origin_binding_mutation', 34),
                         ('mail_domains', 'mail_domains_guard_data_plane_identity', 'guard_mail_domain_data_plane_identity', 19),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_guard_data_plane', 'guard_mail_engine_binding_data_plane', 19),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_validate_data_plane', 'validate_inbound_mail_engine_event_data_plane', 7),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_validate_data_plane', 'validate_outbound_mail_engine_event_data_plane', 7),
                         ('agent_mail_admissions', 'agent_mail_admissions_validate_data_plane', 'validate_agent_mail_admission_data_plane', 7),
                         ('agent_mail_admissions', 'agent_mail_admissions_validate_insert', 'validate_agent_mail_admission_insert', 7),
                         ('agent_mail_admissions', 'agent_mail_admissions_materialize_recipients', 'materialize_agent_mail_admission_recipients', 5),
                         ('agent_mail_admissions', 'agent_mail_admissions_reject_update', 'reject_agent_mail_admission_mutation', 19),
                         ('agent_mail_admissions', 'agent_mail_admissions_reject_delete', 'reject_agent_mail_admission_mutation', 11),
                         ('agent_mail_admissions', 'agent_mail_admissions_reject_truncate', 'reject_agent_mail_admission_mutation', 34),
                         ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_update', 'reject_agent_mail_admission_mutation', 19),
                         ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_delete', 'reject_agent_mail_admission_mutation', 11),
                         ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_truncate', 'reject_agent_mail_admission_mutation', 34),
                         ('outbox_events', 'outbox_events_validate_data_plane_route', 'validate_outbox_event_data_plane_route', 7),
                         ('outbox_events', 'outbox_events_guard_identity', 'guard_outbox_event_identity', 19),
                         ('mail_drafts', 'mail_drafts_validate_insert', 'validate_mail_draft_insert', 7),
                         ('mail_drafts', 'mail_drafts_validate_update', 'validate_mail_draft_update', 19),
                         ('mail_submission_intents', 'mail_submission_intents_validate_insert', 'validate_mail_submission_intent_insert', 7),
                         ('mail_submission_intents', 'mail_submission_intents_validate_update', 'validate_mail_submission_intent_update', 19),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_validate_insert', 'validate_mail_delivery_snapshot_insert', 7)
                     ) AS required_trigger(table_name, trigger_name, function_name, trigger_type)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_trigger AS trigger
                         WHERE trigger.tgrelid =
                                   to_regclass('public.' || required_trigger.table_name)
                           AND trigger.tgname = required_trigger.trigger_name
                           AND NOT trigger.tgisinternal
                           AND trigger.tgenabled = 'O'
                           AND trigger.tgtype = required_trigger.trigger_type
                           AND trigger.tgnargs = 0
                           AND trigger.tgqual IS NULL
                           AND trigger.tgfoid = to_regprocedure(
                               'public.' || required_trigger.function_name || '()'
                           )
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_domains', 'mail_domains_guard_data_plane_identity', ARRAY['mail_data_plane_assigned_at', 'mail_data_plane_id', 'mail_engine_domain_ref']::text[]),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_guard_data_plane', ARRAY['mail_data_plane_id']::text[]),
                         ('mail_drafts', 'mail_drafts_validate_insert', ARRAY[]::text[]),
                         ('mail_drafts', 'mail_drafts_validate_update', ARRAY[]::text[]),
                         ('mail_submission_intents', 'mail_submission_intents_validate_insert', ARRAY[]::text[]),
                         ('mail_submission_intents', 'mail_submission_intents_validate_update', ARRAY[]::text[]),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_validate_insert', ARRAY[]::text[]),
                         ('agent_mail_admissions', 'agent_mail_admissions_validate_data_plane', ARRAY[]::text[]),
                         ('agent_mail_admissions', 'agent_mail_admissions_validate_insert', ARRAY[]::text[]),
                         ('agent_mail_admissions', 'agent_mail_admissions_materialize_recipients', ARRAY[]::text[]),
                         ('agent_mail_admissions', 'agent_mail_admissions_reject_update', ARRAY[]::text[]),
                         ('agent_mail_admissions', 'agent_mail_admissions_reject_delete', ARRAY[]::text[]),
                         ('agent_mail_admissions', 'agent_mail_admissions_reject_truncate', ARRAY[]::text[]),
                         ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_update', ARRAY[]::text[]),
                         ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_delete', ARRAY[]::text[]),
                         ('agent_mail_admission_recipients', 'agent_mail_admission_recipients_reject_truncate', ARRAY[]::text[]),
                         ('outbox_events', 'outbox_events_validate_data_plane_route', ARRAY[]::text[]),
                         ('outbox_events', 'outbox_events_guard_identity', ARRAY[]::text[])
                     ) AS required_trigger_columns(table_name, trigger_name, column_names)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_trigger AS trigger
                         WHERE trigger.tgrelid =
                                   to_regclass('public.' || required_trigger_columns.table_name)
                           AND trigger.tgname = required_trigger_columns.trigger_name
                           AND ARRAY(
                               SELECT attribute.attname::text
                                 FROM unnest(trigger.tgattr::smallint[]) AS trigger_column(attnum)
                                 JOIN pg_attribute AS attribute
                                   ON attribute.attrelid = trigger.tgrelid
                                  AND attribute.attnum = trigger_column.attnum
                                WHERE NOT attribute.attisdropped
                                ORDER BY attribute.attname
                           ) = required_trigger_columns.column_names
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_data_plane_origin_bindings', 'mail_data_plane_origin_bindings_origin_shape_check'),
                         ('mail_domains', 'mail_domains_data_plane_assignment_check'),
                         ('mail_domains', 'mail_domains_engine_ref_data_plane_check'),
                         ('mail_domains', 'mail_domains_projection_data_plane_check'),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_fkey'),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_data_plane_account_unique'),
                         ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_fkey'),
                         ('mail_engine_account_owners', 'mail_engine_account_owners_data_plane_account_unique'),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_fkey'),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_event_unique'),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_data_plane_message_unique'),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_fkey'),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_data_plane_event_unique'),
                         ('mail_drafts', 'mail_drafts_data_plane_fkey'),
                         ('mail_drafts', 'mail_drafts_data_plane_email_unique'),
                         ('mail_submission_intents', 'mail_submission_intents_data_plane_fkey'),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_fkey'),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_data_plane_source_unique'),
                         ('agent_mail_admissions', 'agent_mail_admissions_data_plane_fkey'),
                         ('outbox_events', 'outbox_events_data_plane_fkey'),
                         ('outbox_events', 'outbox_events_required_data_plane_route_check'),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_check'),
                         ('mail_engine_mailbox_bindings', 'mail_engine_mailbox_bindings_source_account_unique'),
                         ('mail_engine_account_owners', 'mail_engine_account_owners_source_check'),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_check'),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_event_unique'),
                         ('inbound_mail_engine_events', 'inbound_mail_engine_events_source_message_unique'),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_check'),
                         ('outbound_mail_engine_events', 'outbound_mail_engine_events_source_event_unique'),
                         ('mail_drafts', 'mail_drafts_engine_source_check'),
                         ('mail_drafts', 'mail_drafts_engine_email_unique'),
                         ('mail_submission_intents', 'mail_submission_intents_engine_source_check'),
                         ('mail_delivery_snapshots', 'mail_delivery_snapshots_source_check')
                     ) AS required_constraint(table_name, constraint_name)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_constraint AS constraint_row
                         WHERE constraint_row.conrelid =
                                   to_regclass('public.' || required_constraint.table_name)
                           AND constraint_row.conname = required_constraint.constraint_name
                           AND constraint_row.convalidated
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_domains_data_plane_engine_ref_unique', 'mail_domains', true, ARRAY['mail_data_plane_id', 'mail_engine_domain_ref']::text[], 'mail_engine_domain_refISNOTNULL'),
                         ('outbox_unpublished_data_plane_idx', 'outbox_events', false, ARRAY['mail_data_plane_id', 'next_attempt_at', 'occurred_at', 'id']::text[], 'published_atISNULL')
                     ) AS required_index(index_name, table_name, is_unique, column_names, predicate)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_index AS index_row
                         WHERE index_row.indexrelid =
                                   to_regclass('public.' || required_index.index_name)
                           AND index_row.indrelid =
                                   to_regclass('public.' || required_index.table_name)
                           AND index_row.indisunique = required_index.is_unique
                           AND index_row.indisvalid
                           AND index_row.indisready
                           AND ARRAY(
                               SELECT attribute.attname::text
                                 FROM unnest(index_row.indkey::smallint[])
                                      WITH ORDINALITY AS key_column(attnum, position)
                                 JOIN pg_attribute AS attribute
                                   ON attribute.attrelid = index_row.indrelid
                                  AND attribute.attnum = key_column.attnum
                                WHERE key_column.position <= index_row.indnkeyatts
                                ORDER BY key_column.position
                           ) = required_index.column_names
                           AND trim(both '()' from regexp_replace(
                                   pg_get_expr(index_row.indpred, index_row.indrelid),
                                   '\s+',
                                   '',
                                   'g'
                               )) = required_index.predicate
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_domains', 'mail_data_plane_id'),
                         ('mail_domains', 'mail_data_plane_assigned_at'),
                         ('outbox_events', 'mail_data_plane_id')
                     ) AS nullable_column(table_name, column_name)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_attribute AS attribute
                         WHERE attribute.attrelid =
                                   to_regclass('public.' || nullable_column.table_name)
                           AND attribute.attname = nullable_column.column_name
                           AND NOT attribute.attnotnull
                           AND NOT attribute.atthasdef
                           AND NOT attribute.attisdropped
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('mail_engine_mailbox_bindings'),
                         ('mail_engine_account_owners'),
                         ('inbound_mail_engine_events'),
                         ('outbound_mail_engine_events'),
                         ('mail_drafts'),
                         ('mail_submission_intents'),
                         ('mail_delivery_snapshots'),
                         ('agent_mail_admissions')
                     ) AS legacy_default(table_name)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_attribute AS attribute
                          JOIN pg_attrdef AS default_row
                            ON default_row.adrelid = attribute.attrelid
                           AND default_row.adnum = attribute.attnum
                         WHERE attribute.attrelid =
                                   to_regclass('public.' || legacy_default.table_name)
                           AND attribute.attname = 'mail_data_plane_id'
                           AND attribute.atttypid = 'uuid'::regtype
                           AND attribute.attnotnull
                           AND NOT attribute.attisdropped
                           AND pg_get_expr(default_row.adbin, default_row.adrelid) =
                                   '''018f305d-3d0f-7b2a-9d68-7f4a3f6e2101''::uuid'
                    )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM (VALUES
                         ('validate_mail_draft_insert', ARRAY['%binding.mail_data_plane_id = NEW.mail_data_plane_id%', '%inbound.mail_data_plane_id = NEW.mail_data_plane_id%']::text[]),
                         ('validate_mail_draft_update', ARRAY['%NEW.mail_data_plane_id <> OLD.mail_data_plane_id%']::text[]),
                         ('validate_mail_submission_intent_insert', ARRAY['%draft.mail_data_plane_id = NEW.mail_data_plane_id%']::text[]),
                         ('validate_mail_submission_intent_update', ARRAY['%NEW.mail_data_plane_id <> OLD.mail_data_plane_id%']::text[]),
                         ('validate_mail_delivery_snapshot_insert', ARRAY['%submission.mail_data_plane_id = NEW.mail_data_plane_id%']::text[]),
                         ('validate_agent_mail_admission_data_plane', ARRAY['%intent.mail_data_plane_id = NEW.mail_data_plane_id%', '%draft.mail_data_plane_id = NEW.mail_data_plane_id%']::text[])
                     ) AS required_function(function_name, definition_patterns)
                    WHERE NOT EXISTS (
                        SELECT 1
                          FROM pg_proc AS function_row
                         WHERE function_row.oid = to_regprocedure(
                                   'public.' || required_function.function_name || '()'
                               )
                           AND pg_get_functiondef(function_row.oid)
                                   LIKE ALL(required_function.definition_patterns)
                    )
               )
               AND EXISTS (
                   SELECT 1
                     FROM pg_proc AS function_row
                    WHERE function_row.oid =
                              to_regprocedure('public.validate_agent_mail_admission_insert()')
                      AND regexp_replace(pg_get_functiondef(function_row.oid), '\s+', '', 'g')
                              LIKE '%ANDprior.mail_data_plane_id=NEW.mail_data_plane_id%'
                      AND regexp_replace(pg_get_functiondef(function_row.oid), '\s+', '', 'g')
                              LIKE '%ANDadmission.mail_data_plane_id=NEW.mail_data_plane_id%'
                      AND regexp_replace(pg_get_functiondef(function_row.oid), '\s+', '', 'g')
                              LIKE $legacy_thread_digest$%current_thread_ref:=digest(convert_to(NEW.organization_id::text||':'||NEW.agent_id::text||':'||current_draft.engine_thread_ref,'UTF8'),'sha256');%$legacy_thread_digest$
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM mail_domains
                    WHERE (mail_data_plane_id IS NULL) <>
                              (mail_data_plane_assigned_at IS NULL)
                       OR (mail_engine_domain_ref IS NOT NULL AND mail_data_plane_id IS NULL)
                       OR (
                           mail_engine_projection_status = 'ready'
                           AND (
                               mail_data_plane_id IS NULL
                               OR mail_data_plane_assigned_at IS NULL
                               OR mail_engine_domain_ref IS NULL
                           )
                       )
               )
               AND NOT EXISTS (
                   SELECT 1
                     FROM outbox_events
                    WHERE event_type IN (
                        'mail_engine.mailbox_binding.updated.v1',
                        'mail_engine.mailbox_binding.updated.v2',
                        'message.received.v1',
                        'mail_engine.delivery_trace.observed.v1',
                        'mail.submission.requested.v1',
                        'agent.mail.admitted.v1',
                        'mail.submission.accepted.v1',
                        'mail.submission.rejected.v1',
                        'mail.submission.review_required.v1',
                        'mail.delivery.reconciliation.requested.v1',
                        'message.delivery_status_changed.v1',
                        'mail_domain.activated.v1',
                        'mail_domain.suspended.v1'
                    )
                      AND mail_data_plane_id IS NULL
               )               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.mail_domains'::regclass
                      AND attname = 'mail_engine_projection_status'
                      AND attnotnull
                      AND NOT attisdropped
               )
               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.mail_domains'::regclass
                      AND attname = 'ownership_challenge_version'
                      AND attnotnull
                      AND NOT attisdropped
               )
               AND to_regclass('public.delegations') IS NOT NULL
               AND to_regclass('public.policies') IS NOT NULL
               AND to_regclass('public.policy_versions') IS NOT NULL
               AND to_regclass('public.approval_requests') IS NOT NULL
               AND to_regclass('public.approval_steps') IS NOT NULL
               AND to_regclass('public.audit_event_catalog') IS NOT NULL
               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.mailbox_memberships'::regclass
                      AND attname = 'organization_id'
                      AND NOT attisdropped
               )
               AND to_regclass('public.audit_events') IS NOT NULL
               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.audit_events'::regclass
                      AND attname = 'chain_sequence'
                      AND attnotnull
                      AND NOT attisdropped
               )
               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.audit_checkpoints'::regclass
                      AND attname = 'signed_payload'
                      AND attnotnull
                      AND NOT attisdropped
               )
               AND to_regclass('public.audit_archives') IS NOT NULL
               AND to_regclass('public.compliance_cases') IS NOT NULL
               AND to_regclass('public.legal_holds') IS NOT NULL
               AND to_regclass('public.outbox_events') IS NOT NULL
               AND to_regclass('public.mail_engine_mailbox_bindings') IS NOT NULL
               AND to_regclass('public.mail_engine_account_owners') IS NOT NULL
               AND EXISTS (
                   SELECT 1
                     FROM pg_class AS owner_table
                     JOIN pg_namespace AS owner_namespace
                       ON owner_namespace.oid = owner_table.relnamespace
                    WHERE owner_namespace.nspname = 'public'
                      AND owner_table.relname = 'mail_engine_account_owners'
                      AND owner_table.relkind = 'r'
                      AND EXISTS (
                          SELECT 1
                            FROM pg_constraint AS owner_primary_key
                           WHERE owner_primary_key.conrelid = owner_table.oid
                             AND owner_primary_key.contype = 'p'
                             AND ARRAY(
                                 SELECT attribute.attname::text
                                   FROM unnest(owner_primary_key.conkey)
                                        WITH ORDINALITY
                                        AS key_column(attnum, position)
                                   JOIN pg_attribute AS attribute
                                     ON attribute.attrelid = owner_table.oid
                                    AND attribute.attnum = key_column.attnum
                                  WHERE NOT attribute.attisdropped
                                  ORDER BY key_column.position
                             ) = ARRAY['source', 'account_ref']::text[]
                      )
                      AND EXISTS (
                          SELECT 1
                            FROM pg_constraint AS owner_data_plane_unique
                           WHERE owner_data_plane_unique.conrelid = owner_table.oid
                             AND owner_data_plane_unique.contype = 'u'
                             AND owner_data_plane_unique.conname =
                                 'mail_engine_account_owners_data_plane_account_unique'
                             AND ARRAY(
                                 SELECT attribute.attname::text
                                   FROM unnest(owner_data_plane_unique.conkey)
                                        WITH ORDINALITY
                                        AS key_column(attnum, position)
                                   JOIN pg_attribute AS attribute
                                     ON attribute.attrelid = owner_table.oid
                                    AND attribute.attnum = key_column.attnum
                                  WHERE NOT attribute.attisdropped
                                  ORDER BY key_column.position
                             ) = ARRAY['mail_data_plane_id', 'account_ref']::text[]
                      )
               )
               AND EXISTS (
                   SELECT 1
                     FROM pg_trigger AS account_owner_trigger
                    WHERE account_owner_trigger.tgrelid =
                              'public.mail_engine_mailbox_bindings'::regclass
                      AND account_owner_trigger.tgname =
                              'mail_engine_mailbox_bindings_enforce_account_owner'
                      AND NOT account_owner_trigger.tgisinternal
                      AND account_owner_trigger.tgenabled = 'O'
                      AND account_owner_trigger.tgtype = 23
                      AND account_owner_trigger.tgnargs = 0
                      AND account_owner_trigger.tgqual IS NULL
                      AND account_owner_trigger.tgfoid =
                          'public.enforce_mail_engine_account_owner()'::regprocedure
                      AND ARRAY(
                          SELECT attribute.attname::text
                            FROM unnest(account_owner_trigger.tgattr::smallint[])
                                 AS trigger_column(attnum)
                            JOIN pg_attribute AS attribute
                              ON attribute.attrelid = account_owner_trigger.tgrelid
                             AND attribute.attnum = trigger_column.attnum
                           WHERE NOT attribute.attisdropped
                           ORDER BY attribute.attname
                      ) = ARRAY[
                          'account_ref',
                          'mail_data_plane_id',
                          'mailbox_id',
                          'organization_id',
                          'source'
                      ]::text[]
                )
                AND EXISTS (
                    SELECT 1
                      FROM pg_trigger AS owner_mutation_trigger
                     WHERE owner_mutation_trigger.tgrelid =
                               'public.mail_engine_account_owners'::regclass
                       AND owner_mutation_trigger.tgname =
                               'mail_engine_account_owners_reject_update_delete'
                       AND NOT owner_mutation_trigger.tgisinternal
                       AND owner_mutation_trigger.tgenabled = 'O'
                       AND owner_mutation_trigger.tgtype = 27
                       AND owner_mutation_trigger.tgnargs = 0
                       AND cardinality(
                               owner_mutation_trigger.tgattr::smallint[]
                           ) = 0
                       AND owner_mutation_trigger.tgqual IS NULL
                       AND owner_mutation_trigger.tgfoid =
                           'public.reject_mail_engine_account_owner_mutation()'::regprocedure
                )
                AND EXISTS (
                    SELECT 1
                      FROM pg_trigger AS owner_truncate_trigger
                     WHERE owner_truncate_trigger.tgrelid =
                               'public.mail_engine_account_owners'::regclass
                       AND owner_truncate_trigger.tgname =
                               'mail_engine_account_owners_reject_truncate'
                       AND NOT owner_truncate_trigger.tgisinternal
                       AND owner_truncate_trigger.tgenabled = 'O'
                       AND owner_truncate_trigger.tgtype = 34
                       AND owner_truncate_trigger.tgnargs = 0
                       AND cardinality(
                               owner_truncate_trigger.tgattr::smallint[]
                           ) = 0
                       AND owner_truncate_trigger.tgqual IS NULL
                       AND owner_truncate_trigger.tgfoid =
                           'public.reject_mail_engine_account_owner_mutation()'::regprocedure
                )
                AND EXISTS (
                    SELECT 1
                      FROM pg_trigger AS owner_insert_guard
                      JOIN pg_constraint AS owner_insert_constraint
                        ON owner_insert_constraint.oid =
                               owner_insert_guard.tgconstraint
                     WHERE owner_insert_guard.tgrelid =
                               'public.mail_engine_account_owners'::regclass
                       AND owner_insert_guard.tgname =
                               'mail_engine_account_owners_require_current_binding'
                       AND NOT owner_insert_guard.tgisinternal
                       AND owner_insert_guard.tgenabled = 'O'
                       AND owner_insert_guard.tgtype = 5
                       AND owner_insert_guard.tgnargs = 0
                       AND cardinality(
                               owner_insert_guard.tgattr::smallint[]
                           ) = 0
                       AND owner_insert_guard.tgqual IS NULL
                       AND owner_insert_guard.tgdeferrable
                       AND owner_insert_guard.tginitdeferred
                       AND owner_insert_guard.tgfoid =
                           'public.require_current_binding_for_account_owner()'::regprocedure
                       AND owner_insert_constraint.conrelid =
                               owner_insert_guard.tgrelid
                       AND owner_insert_constraint.conname =
                               owner_insert_guard.tgname
                       AND owner_insert_constraint.contype = 't'
                       AND owner_insert_constraint.condeferrable
                       AND owner_insert_constraint.condeferred
                )
                AND NOT EXISTS (
                    SELECT 1
                      FROM mail_engine_mailbox_bindings AS binding
                      LEFT JOIN mail_engine_account_owners AS owner
                        ON owner.mail_data_plane_id = binding.mail_data_plane_id
                       AND owner.source = binding.source
                       AND owner.account_ref = binding.account_ref
                     WHERE owner.account_ref IS NULL
                        OR owner.organization_id
                               IS DISTINCT FROM binding.organization_id
                        OR owner.mailbox_id IS DISTINCT FROM binding.mailbox_id
                )
                AND to_regclass('public.inbound_mail_engine_events') IS NOT NULL
               AND EXISTS (
                   SELECT 1
                     FROM pg_trigger
                    WHERE tgrelid = 'public.inbound_mail_engine_events'::regclass
                      AND tgname = 'inbound_mail_engine_events_reject_update_delete'
                      AND NOT tgisinternal
               )
               AND to_regclass('public.webhook_subscriptions') IS NOT NULL
               AND to_regclass('public.webhook_deliveries') IS NOT NULL
               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.webhook_subscriptions'::regclass
                      AND attname = 'version'
                      AND attnotnull
                      AND NOT attisdropped
               )
               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.webhook_deliveries'::regclass
                      AND attname = 'error_code'
                      AND NOT attisdropped
               )
               AND EXISTS (
                   SELECT 1
                     FROM pg_trigger
                    WHERE tgrelid = 'public.webhook_deliveries'::regclass
                      AND tgname = 'webhook_deliveries_reject_update_delete'
                      AND NOT tgisinternal
               )
               AND EXISTS (
                   SELECT 1
                     FROM audit_event_catalog
                    WHERE action = 'agent.webhook_delivery.replayed'
                      AND resource_type = 'agent.webhook_delivery'
                      AND risk = 'critical'
                      AND privileged
               )
               AND EXISTS (
                   SELECT 1
                     FROM audit_event_catalog
                    WHERE action = 'agent.event_cursor.read'
                      AND resource_type = 'agent.event_cursor'
                      AND risk = 'medium'
                      AND privileged
               )
               AND EXISTS (
                   SELECT 1
                     FROM audit_event_catalog
                    WHERE action = 'agent.event_stream.opened'
                      AND resource_type = 'agent.event_stream'
                      AND risk = 'medium'
                      AND privileged
               )
               AND EXISTS (
                   SELECT 1
                     FROM audit_event_catalog
                    WHERE action = 'audit.events.queried'
                      AND resource_type = 'audit.query'
                      AND risk = 'high'
                      AND privileged
               )
               AND EXISTS (
                   SELECT 1
                     FROM audit_event_catalog
                    WHERE action = 'mail_engine.mailbox_binding.updated'
                      AND resource_type = 'mail_engine.mailbox_binding'
                      AND risk = 'high'
                      AND privileged
               )
               AND EXISTS (
                   SELECT 1
                     FROM pg_attribute
                    WHERE attrelid = 'public.outbox_events'::regclass
                      AND attname = 'next_attempt_at'
                      AND attnotnull
                      AND NOT attisdropped
               )
            "#,
        )
        .fetch_one(&self.pool)
        .await
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

#[async_trait]
impl AuditCheckpointQueryStore for PgControlPlaneStore {
    async fn audit_checkpoint_candidate(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<AuditCheckpointCandidate>, StoreError> {
        let report = self
            .verify_audit_chain(organization_id)
            .await
            .map_err(|error| {
                tracing::error!(%organization_id, %error, "audit chain is not checkpointable");
                StoreError::Unavailable
            })?;
        let (Some(first_event_id), Some(last_event_id), Some(root_hash), Some(hash_version)) = (
            report.first_event_id,
            report.last_event_id,
            report.head_hash,
            report.hash_version,
        ) else {
            return Ok(None);
        };
        let chain_sequence_end =
            i64::try_from(report.event_count).map_err(|_| StoreError::Unavailable)?;
        let (period_start, period_end): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
            r#"
            SELECT min(occurred_at), max(occurred_at)
              FROM audit_events
             WHERE organization_id = $1
               AND chain_sequence <= $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(chain_sequence_end)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(Some(AuditCheckpointCandidate {
            organization_id,
            chain_sequence_end,
            event_count: report.event_count,
            first_event_id,
            last_event_id,
            period_start,
            period_end,
            root_hash,
            hash_version,
        }))
    }
}

#[async_trait]
impl AuditEventQueryStore for PgControlPlaneStore {
    async fn active_human_audit_reader_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE organization_id = $1
                   AND id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn audit_event_search_anchor_exists(
        &self,
        search: &AuditEventSearch,
        chain_sequence: i64,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM audit_events
                 WHERE organization_id = $1
                   AND occurred_at >= $2
                   AND occurred_at <= $3
                   AND ($4::text IS NULL OR action = $4)
                   AND ($5::uuid IS NULL OR actor_principal_id = $5)
                   AND ($6::text IS NULL OR resource_type = $6)
                   AND ($7::text IS NULL OR resource_id = $7)
                   AND ($8::text IS NULL OR outcome = $8)
                   AND ($9::text IS NULL OR request_id = $9)
                   AND chain_sequence = $10
            )
            "#,
        )
        .bind(search.organization_id.as_uuid())
        .bind(search.occurred_from)
        .bind(search.occurred_to)
        .bind(search.action.as_deref())
        .bind(search.actor_principal_id.map(|id| id.as_uuid()))
        .bind(search.resource_type.as_deref())
        .bind(search.resource_id.as_deref())
        .bind(search.outcome.map(audit_outcome))
        .bind(search.request_id.as_deref())
        .bind(chain_sequence)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn search_audit_events(
        &self,
        search: &AuditEventSearch,
    ) -> Result<Vec<AuditEventSummary>, StoreError> {
        if !(1..=101).contains(&search.limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, AuditEventSummaryRow>(
            r#"
            SELECT event.id AS event_id,
                   event.organization_id,
                   event.actor_principal_id,
                   actor.kind AS actor_kind,
                   event.on_behalf_of_principal_id,
                   event.credential_id,
                   event.subject_principal_id,
                   event.action,
                   event.resource_type,
                   event.resource_id,
                   event.outcome,
                   event.trace_id,
                   event.request_id,
                   CASE
                       WHEN event.source_ip IS NULL THEN NULL
                       ELSE host(event.source_ip)
                   END AS source_ip,
                   event.client_id,
                   event.session_id,
                   event.token_jti,
                   event.policy_id,
                   event.policy_version_id,
                   event.approval_request_id,
                   event.ticket_reference,
                   event.environment,
                   event.region,
                   event.details,
                   event.occurred_at,
                   event.received_at,
                   event.chain_sequence,
                   event.hash_version,
                   encode(event.previous_hash, 'hex') AS previous_hash_hex,
                   encode(event.event_hash, 'hex') AS event_hash_hex,
                   event.signature_batch_id
              FROM audit_events AS event
              JOIN principals AS actor
                ON actor.id = event.actor_principal_id
               AND actor.organization_id = event.organization_id
             WHERE event.organization_id = $1
               AND event.occurred_at >= $2
               AND event.occurred_at <= $3
               AND ($4::text IS NULL OR event.action = $4)
               AND ($5::uuid IS NULL OR event.actor_principal_id = $5)
               AND ($6::text IS NULL OR event.resource_type = $6)
               AND ($7::text IS NULL OR event.resource_id = $7)
               AND ($8::text IS NULL OR event.outcome = $8)
               AND ($9::text IS NULL OR event.request_id = $9)
               AND ($10::bigint IS NULL OR event.chain_sequence < $10)
             ORDER BY event.chain_sequence DESC
             LIMIT $11
            "#,
        )
        .bind(search.organization_id.as_uuid())
        .bind(search.occurred_from)
        .bind(search.occurred_to)
        .bind(search.action.as_deref())
        .bind(search.actor_principal_id.map(|id| id.as_uuid()))
        .bind(search.resource_type.as_deref())
        .bind(search.resource_id.as_deref())
        .bind(search.outcome.map(audit_outcome))
        .bind(search.request_id.as_deref())
        .bind(search.before_chain_sequence)
        .bind(search.limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter().map(audit_event_summary_from_row).collect()
    }
}

#[async_trait]
impl HumanLoginTransactionStore for PgControlPlaneStore {
    async fn admit_human_login_transaction(
        &self,
        write: &HumanLoginTransactionWrite,
        admission_limit: u32,
        cleanup_limit: u32,
    ) -> Result<HumanLoginAdmissionResult, StoreError> {
        if !(1..=100_000).contains(&admission_limit) || !(1..=1_000).contains(&cleanup_limit) {
            return Err(StoreError::Conflict);
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        let admitted_window = sqlx::query_scalar::<_, DateTime<Utc>>(
            r#"
            WITH database_window AS (
                SELECT date_trunc('minute', statement_timestamp()) AS window_started_at
            )
            INSERT INTO human_login_admission_windows(
                client_key, window_started_at, admitted_count
            )
            SELECT $1, database_window.window_started_at, 1
              FROM database_window
            ON CONFLICT (client_key, window_started_at) DO UPDATE
               SET admitted_count =
                   human_login_admission_windows.admitted_count + 1
             WHERE human_login_admission_windows.admitted_count < $2
            RETURNING window_started_at
            "#,
        )
        .bind(human_session_client(write.client_key))
        .bind(i64::from(admission_limit))
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if admitted_window.is_none() {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanLoginAdmissionResult::RateLimited {
                retry_after_seconds: 60,
            });
        }

        let cleanup_limit = i64::from(cleanup_limit);
        sqlx::query(
            r#"
            WITH stale AS (
                SELECT client_key, window_started_at
                  FROM human_login_admission_windows
                 WHERE window_started_at
                       < date_trunc('minute', statement_timestamp())
                 ORDER BY window_started_at, client_key
                 FOR UPDATE SKIP LOCKED
                 LIMIT $1
            )
            DELETE FROM human_login_admission_windows AS admission
             USING stale
             WHERE admission.client_key = stale.client_key
               AND admission.window_started_at = stale.window_started_at
            "#,
        )
        .bind(cleanup_limit)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;

        let expired_login_ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT login.id
              FROM human_login_transactions AS login
             WHERE login.expires_at <= statement_timestamp()
               AND NOT EXISTS (
                   SELECT 1
                     FROM human_browser_sessions AS session
                    WHERE session.login_transaction_id = login.id
               )
             ORDER BY login.expires_at, login.id
             FOR UPDATE OF login SKIP LOCKED
             LIMIT $1
            "#,
        )
        .bind(cleanup_limit)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !expired_login_ids.is_empty() {
            sqlx::query(
                r#"
                DELETE FROM human_login_transaction_consumptions
                 WHERE login_transaction_id = ANY($1)
                "#,
            )
            .bind(expired_login_ids.as_slice())
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
            sqlx::query(
                r#"
                DELETE FROM human_login_transactions AS login
                 WHERE login.id = ANY($1)
                   AND login.expires_at <= statement_timestamp()
                   AND NOT EXISTS (
                       SELECT 1
                         FROM human_browser_sessions AS session
                        WHERE session.login_transaction_id = login.id
                   )
                "#,
            )
            .bind(expired_login_ids.as_slice())
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        }

        sqlx::query(
            r#"
            INSERT INTO human_login_transactions(
                id, client_key, return_to, state_sha256, browser_nonce_sha256,
                encrypted_pkce_verifier, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(write.login_transaction_id.as_uuid())
        .bind(human_session_client(write.client_key))
        .bind(&write.return_to)
        .bind(write.state_sha256.as_slice())
        .bind(write.browser_nonce_sha256.as_slice())
        .bind(&write.encrypted_pkce_verifier)
        .bind(write.created_at)
        .bind(write.expires_at)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanLoginAdmissionResult::Admitted)
    }

    async fn consume_human_login_transaction(
        &self,
        state_sha256: &[u8; 32],
        browser_nonce_sha256: &[u8; 32],
        consumed_at: DateTime<Utc>,
    ) -> Result<Option<HumanLoginTransactionRecord>, StoreError> {
        let row = sqlx::query_as::<_, HumanLoginTransactionRow>(
            r#"
            WITH candidate AS (
                SELECT login.id, login.client_key, login.return_to,
                       login.encrypted_pkce_verifier,
                       login.created_at, login.expires_at
                  FROM human_login_transactions AS login
                 WHERE login.state_sha256 = $1
                   AND login.browser_nonce_sha256 = $2
                   AND login.created_at <= $3
                   AND login.expires_at > $3
                   AND NOT EXISTS (
                       SELECT 1
                         FROM human_login_transaction_consumptions AS consumed
                        WHERE consumed.login_transaction_id = login.id
                   )
                   FOR UPDATE
            ),
            consumed AS (
                INSERT INTO human_login_transaction_consumptions(
                    login_transaction_id, consumed_at
                )
                SELECT id, $3 FROM candidate
                ON CONFLICT (login_transaction_id) DO NOTHING
                RETURNING login_transaction_id
            )
            SELECT candidate.id, candidate.client_key, candidate.return_to,
                   candidate.encrypted_pkce_verifier,
                   candidate.created_at, candidate.expires_at
              FROM candidate
              JOIN consumed
                ON consumed.login_transaction_id = candidate.id
            "#,
        )
        .bind(state_sha256.as_slice())
        .bind(browser_nonce_sha256.as_slice())
        .bind(consumed_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(human_login_transaction_from_row).transpose()
    }
}

#[async_trait]
impl HumanSessionQueryStore for PgControlPlaneStore {
    async fn resolve_human_session_by_token_sha256(
        &self,
        token_sha256: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<Option<HumanSessionRecord>, StoreError> {
        let row = sqlx::query_as::<_, HumanSessionRow>(
            r#"
            SELECT session.id, session.organization_id, session.principal_id,
                   session.login_transaction_id, session.client_key,
                   session.oidc_jti, session.upstream_expires_at,
                   session.created_at, session.expires_at
              FROM human_browser_sessions AS session
              JOIN principals AS principal
                ON principal.id = session.principal_id
               AND principal.organization_id = session.organization_id
             WHERE session.token_sha256 = $1
               AND session.created_at <= $2
               AND session.expires_at > $2
               AND session.upstream_expires_at > $2
               AND principal.kind = 'human'
               AND principal.status = 'active'
               AND NOT EXISTS (
                   SELECT 1
                     FROM human_browser_session_revocations AS revocation
                    WHERE revocation.session_id = session.id
               )
            "#,
        )
        .bind(token_sha256.as_slice())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(human_session_from_row).transpose()
    }
}

#[async_trait]
impl AuthorizationStore for PgControlPlaneStore {
    async fn resolve_authorization(
        &self,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<Option<AuthorizationContext>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct AuthorizationRow {
            organization_id: Uuid,
            permissions: Vec<String>,
        }

        let row = sqlx::query_as::<_, AuthorizationRow>(
            r#"
            SELECT principal.organization_id,
                   COALESCE(
                       array_agg(DISTINCT permission.permission)
                           FILTER (WHERE permission.permission IS NOT NULL),
                       ARRAY[]::text[]
                   ) AS permissions
              FROM principals AS principal
              LEFT JOIN role_assignments AS assignment
                ON assignment.principal_id = principal.id
               AND assignment.organization_id = principal.organization_id
               AND assignment.revoked_at IS NULL
               AND assignment.valid_from <= $2
               AND (assignment.valid_until IS NULL OR assignment.valid_until > $2)
              LEFT JOIN role_permissions AS permission
                ON permission.role_id = assignment.role_id
             WHERE principal.id = $1
               AND principal.status = 'active'
             GROUP BY principal.organization_id
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;

        Ok(row.map(|row| AuthorizationContext {
            principal_id,
            organization_id: OrganizationId::from_uuid(row.organization_id),
            permissions: row
                .permissions
                .into_iter()
                .filter_map(|value| permission_from_str(&value))
                .collect::<BTreeSet<_>>(),
        }))
    }
}

#[async_trait]
impl MailboxAuthorizationStore for PgControlPlaneStore {
    async fn principal_can_submit_mail(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals AS principal
                  LEFT JOIN agent_profiles AS agent
                    ON agent.principal_id = principal.id
                 WHERE principal.id = $1
                   AND principal.organization_id = $2
                   AND principal.status = 'active'
                   AND (
                       principal.kind = 'human'
                       OR (
                           principal.kind = 'agent'
                           AND (
                               agent.environment IN ('development', 'testing')
                               OR (
                                   agent.environment = 'production'
                                   AND EXISTS (
                                       SELECT 1
                                         FROM agent_mail_policy_activations AS activation
                                        WHERE activation.organization_id = principal.organization_id
                                          AND activation.agent_id = principal.id
                                   )
                               )
                           )
                       )
                   )
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn active_production_agent_mail_policy(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyVersion>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct ActivePolicyRow {
            version: i32,
            allowed_recipient_domains: Vec<String>,
            max_recipients_per_message: i32,
            max_messages_per_hour: i32,
            max_messages_per_recipient_per_day: i32,
            require_first_contact_approval: bool,
            max_auto_replies_per_thread_per_day: i32,
        }
        let row = sqlx::query_as::<_, ActivePolicyRow>(
            r#"
            SELECT policy.version, policy.allowed_recipient_domains,
                   policy.max_recipients_per_message,
                   policy.max_messages_per_hour,
                   policy.max_messages_per_recipient_per_day,
                   policy.require_first_contact_approval,
                   policy.max_auto_replies_per_thread_per_day
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
              JOIN LATERAL (
                  SELECT activation.policy_version_id
                    FROM agent_mail_policy_activations AS activation
                   WHERE activation.organization_id = principal.organization_id
                     AND activation.agent_id = principal.id
                   ORDER BY activation.policy_version DESC
                   LIMIT 1
              ) AS activation ON true
              JOIN agent_mail_policy_versions AS policy
                ON policy.id = activation.policy_version_id
               AND policy.organization_id = principal.organization_id
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
               AND profile.environment = 'production'
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            AgentMailPolicyVersion::new(
                organization_id,
                principal_id,
                u32::try_from(row.version).map_err(|_| StoreError::Unavailable)?,
                row.allowed_recipient_domains,
                u16::try_from(row.max_recipients_per_message)
                    .map_err(|_| StoreError::Unavailable)?,
                u32::try_from(row.max_messages_per_hour).map_err(|_| StoreError::Unavailable)?,
                u16::try_from(row.max_messages_per_recipient_per_day)
                    .map_err(|_| StoreError::Unavailable)?,
                row.require_first_contact_approval,
                u8::try_from(row.max_auto_replies_per_thread_per_day)
                    .map_err(|_| StoreError::Unavailable)?,
            )
            .map_err(|_| StoreError::Unavailable)
        })
        .transpose()
    }

    async fn active_agent_environment(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<copilotmail_domain::AgentEnvironment>, StoreError> {
        let environment = sqlx::query_scalar::<_, String>(
            r#"
            SELECT profile.environment
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        environment
            .map(|value| match value.as_str() {
                "development" => Ok(copilotmail_domain::AgentEnvironment::Development),
                "testing" => Ok(copilotmail_domain::AgentEnvironment::Testing),
                "production" => Ok(copilotmail_domain::AgentEnvironment::Production),
                unknown => {
                    tracing::error!(environment = unknown, "unknown Agent environment persisted");
                    Err(StoreError::Unavailable)
                }
            })
            .transpose()
    }

    async fn resolve_mailbox_access(
        &self,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        now: DateTime<Utc>,
    ) -> Result<Option<MailboxAccessSnapshot>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct MailboxAccessRow {
            organization_id: Uuid,
            membership: Option<String>,
        }
        #[derive(sqlx::FromRow)]
        struct DelegationRow {
            id: Uuid,
            grantor_principal_id: Uuid,
            scopes: Vec<String>,
        }

        let mailbox = sqlx::query_as::<_, MailboxAccessRow>(
            r#"
            SELECT mailbox.organization_id,
                   (
                       SELECT membership.role
                         FROM mailbox_memberships AS membership
                        WHERE membership.organization_id = mailbox.organization_id
                          AND membership.mailbox_id = mailbox.id
                          AND membership.principal_id = principal.id
                          AND membership.valid_from <= $3
                          AND (
                              membership.valid_until IS NULL
                              OR membership.valid_until > $3
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
                ON principal.id = $1
               AND principal.organization_id = mailbox.organization_id
               AND principal.status = 'active'
             WHERE mailbox.id = $2
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        let Some(mailbox) = mailbox else {
            return Ok(None);
        };

        let delegations = sqlx::query_as::<_, DelegationRow>(
            r#"
            SELECT id, grantor_principal_id, scopes
              FROM delegations
             WHERE organization_id = $1
               AND mailbox_id = $2
               AND grantee_principal_id = $3
               AND revoked_at IS NULL
               AND valid_from <= $4
               AND valid_until > $4
            "#,
        )
        .bind(mailbox.organization_id)
        .bind(mailbox_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;

        let membership = mailbox
            .membership
            .as_deref()
            .and_then(mailbox_membership_role_from_str);
        let delegations = delegations
            .into_iter()
            .map(|row| ResolvedMailboxDelegation {
                delegation_id: DelegationId::from_uuid(row.id),
                grantor_principal_id: PrincipalId::from_uuid(row.grantor_principal_id),
                capabilities: row
                    .scopes
                    .into_iter()
                    .filter_map(|scope| mailbox_capability_from_str(&scope))
                    .collect(),
            })
            .collect();
        Ok(Some(MailboxAccessSnapshot {
            organization_id: OrganizationId::from_uuid(mailbox.organization_id),
            mailbox_id,
            membership,
            delegations,
        }))
    }
}

#[async_trait]
impl HumanMailboxDirectoryStore for PgControlPlaneStore {
    async fn list_human_mailbox_directory(
        &self,
        query: &HumanMailboxDirectoryQuery,
    ) -> Result<HumanMailboxDirectoryBatch, StoreError> {
        if !(1..=50).contains(&query.limit) {
            return Err(StoreError::Conflict);
        }

        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;

        let active_human = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(query.principal_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !active_human {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanMailboxDirectoryBatch {
                active_human: false,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let cursor_valid = if let Some(after_mailbox_id) = query.after_mailbox_id {
            sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                      FROM mailboxes AS mailbox
                      JOIN mail_engine_mailbox_bindings AS binding
                        ON binding.organization_id = mailbox.organization_id
                       AND binding.mailbox_id = mailbox.id
                     WHERE mailbox.id = $1
                       AND mailbox.organization_id = $2
                       AND (
                           EXISTS (
                               SELECT 1
                                 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = mailbox.organization_id
                                  AND membership.mailbox_id = mailbox.id
                                  AND membership.principal_id = $3
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $4
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $4
                                  )
                           )
                           OR EXISTS (
                               SELECT 1
                                 FROM delegations AS delegation
                                WHERE delegation.organization_id = mailbox.organization_id
                                  AND delegation.mailbox_id = mailbox.id
                                  AND delegation.grantee_principal_id = $3
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $4
                                  AND delegation.valid_until > $4
                           )
                       )
                )
                "#,
            )
            .bind(after_mailbox_id.as_uuid())
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(query.now)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            true
        };
        if !cursor_valid {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanMailboxDirectoryBatch {
                active_human: true,
                cursor_valid: false,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct MailboxRow {
            id: Uuid,
            organization_id: Uuid,
            kind: String,
            primary_address: String,
            created_at: DateTime<Utc>,
            mail_data_plane_id: Uuid,
        }
        let mut rows = sqlx::query_as::<_, MailboxRow>(
            r#"
            SELECT mailbox.id, mailbox.organization_id, mailbox.kind,
                   mailbox.primary_address, mailbox.created_at,
                   binding.mail_data_plane_id
              FROM mailboxes AS mailbox
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = mailbox.organization_id
               AND binding.mailbox_id = mailbox.id
              JOIN principals AS principal
                ON principal.id = $2
               AND principal.organization_id = mailbox.organization_id
               AND principal.kind = 'human'
               AND principal.status = 'active'
             WHERE mailbox.organization_id = $1
               AND ($3::uuid IS NULL OR mailbox.id > $3)
               AND (
                   EXISTS (
                       SELECT 1
                         FROM mailbox_memberships AS membership
                        WHERE membership.organization_id = mailbox.organization_id
                          AND membership.mailbox_id = mailbox.id
                          AND membership.principal_id = principal.id
                          AND membership.role IN ('owner', 'member')
                          AND membership.valid_from <= $4
                          AND (
                              membership.valid_until IS NULL
                              OR membership.valid_until > $4
                          )
                   )
                   OR EXISTS (
                       SELECT 1
                         FROM delegations AS delegation
                        WHERE delegation.organization_id = mailbox.organization_id
                          AND delegation.mailbox_id = mailbox.id
                          AND delegation.grantee_principal_id = principal.id
                          AND 'mailbox.read' = ANY(delegation.scopes)
                          AND delegation.revoked_at IS NULL
                          AND delegation.valid_from <= $4
                          AND delegation.valid_until > $4
                   )
               )
             ORDER BY mailbox.id
             LIMIT $5
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.after_mailbox_id.map(MailboxId::as_uuid))
        .bind(query.now)
        .bind(i64::from(query.limit) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let has_more = rows.len() > usize::from(query.limit);
        rows.truncate(usize::from(query.limit));

        let mailbox_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
        #[derive(sqlx::FromRow)]
        struct MembershipRow {
            mailbox_id: Uuid,
            role: String,
        }
        let membership_rows = if mailbox_ids.is_empty() {
            Vec::new()
        } else {
            sqlx::query_as::<_, MembershipRow>(
                r#"
                SELECT DISTINCT ON (membership.mailbox_id)
                       membership.mailbox_id, membership.role
                  FROM mailbox_memberships AS membership
                 WHERE membership.organization_id = $1
                   AND membership.principal_id = $2
                   AND membership.mailbox_id = ANY($3)
                   AND membership.valid_from <= $4
                   AND (
                       membership.valid_until IS NULL
                       OR membership.valid_until > $4
                   )
                 ORDER BY membership.mailbox_id,
                          CASE membership.role
                              WHEN 'owner' THEN 1
                              WHEN 'member' THEN 2
                              ELSE 3
                          END
                "#,
            )
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(&mailbox_ids)
            .bind(query.now)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_store_error)?
        };
        let mut memberships = BTreeMap::new();
        for row in membership_rows {
            let role =
                mailbox_membership_role_from_str(&row.role).ok_or(StoreError::Unavailable)?;
            memberships.insert(row.mailbox_id, role);
        }

        #[derive(sqlx::FromRow)]
        struct DelegationRow {
            mailbox_id: Uuid,
            id: Uuid,
            grantor_principal_id: Uuid,
            scopes: Vec<String>,
        }
        let delegation_rows = if mailbox_ids.is_empty() {
            Vec::new()
        } else {
            sqlx::query_as::<_, DelegationRow>(
                r#"
                SELECT mailbox_id, id, grantor_principal_id, scopes
                  FROM delegations
                 WHERE organization_id = $1
                   AND grantee_principal_id = $2
                   AND mailbox_id = ANY($3)
                   AND revoked_at IS NULL
                   AND valid_from <= $4
                   AND valid_until > $4
                 ORDER BY mailbox_id, id
                "#,
            )
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(&mailbox_ids)
            .bind(query.now)
            .fetch_all(&mut *transaction)
            .await
            .map_err(map_store_error)?
        };
        let mut delegations = BTreeMap::<Uuid, Vec<ResolvedMailboxDelegation>>::new();
        for row in delegation_rows {
            let capabilities = row
                .scopes
                .into_iter()
                .map(|scope| mailbox_capability_from_str(&scope).ok_or(StoreError::Unavailable))
                .collect::<Result<BTreeSet<_>, StoreError>>()?;
            delegations
                .entry(row.mailbox_id)
                .or_default()
                .push(ResolvedMailboxDelegation {
                    delegation_id: DelegationId::from_uuid(row.id),
                    grantor_principal_id: PrincipalId::from_uuid(row.grantor_principal_id),
                    capabilities,
                });
        }

        let candidates = rows
            .into_iter()
            .map(|row| {
                let kind = mailbox_kind_from_str(&row.kind).ok_or(StoreError::Unavailable)?;
                let primary_address =
                    copilotmail_domain::MailboxAddress::parse(row.primary_address)
                        .map_err(|_| StoreError::Unavailable)?;
                let mailbox_id = MailboxId::from_uuid(row.id);
                let organization_id = OrganizationId::from_uuid(row.organization_id);
                Ok(HumanMailboxDirectoryCandidate {
                    mailbox: MailboxSummary {
                        mailbox_id,
                        organization_id,
                        kind,
                        primary_address,
                        created_at: row.created_at,
                    },
                    data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
                    authorization: MailboxAccessSnapshot {
                        organization_id,
                        mailbox_id,
                        membership: memberships.remove(&row.id),
                        delegations: delegations.remove(&row.id).unwrap_or_default(),
                    },
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanMailboxDirectoryBatch {
            active_human: true,
            cursor_valid: true,
            candidates,
            has_more,
        })
    }
}

#[async_trait]
impl AgentMailPolicyQueryStore for PgControlPlaneStore {
    async fn get_agent_mail_policy_runtime(
        &self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyRuntimeSnapshot>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct RuntimeRow {
            environment: String,
            policy_version: Option<i32>,
            allowed_recipient_domains: Option<Vec<String>>,
            max_recipients_per_message: Option<i32>,
            max_messages_per_hour: Option<i32>,
            max_messages_per_recipient_per_day: Option<i32>,
            require_first_contact_approval: Option<bool>,
            max_auto_replies_per_thread_per_day: Option<i32>,
            observed_at: DateTime<Utc>,
            hourly_window_started_at: DateTime<Utc>,
            admitted_messages_in_hour: i64,
        }

        let row = sqlx::query_as::<_, RuntimeRow>(
            r#"
            WITH runtime_clock AS MATERIALIZED (
                SELECT clock_timestamp() AS observed_at
            )
            SELECT profile.environment,
                   policy.version AS policy_version,
                   policy.allowed_recipient_domains,
                   policy.max_recipients_per_message,
                   policy.max_messages_per_hour,
                   policy.max_messages_per_recipient_per_day,
                   policy.require_first_contact_approval,
                   policy.max_auto_replies_per_thread_per_day,
                   runtime_clock.observed_at,
                   runtime_clock.observed_at - interval '1 hour'
                       AS hourly_window_started_at,
                   (
                       SELECT count(*)
                         FROM agent_mail_admissions AS admission
                        WHERE admission.organization_id = principal.organization_id
                          AND admission.agent_id = principal.id
                          AND admission.admitted_at
                              > runtime_clock.observed_at - interval '1 hour'
                          AND admission.admitted_at <= runtime_clock.observed_at
                   ) AS admitted_messages_in_hour
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
              CROSS JOIN runtime_clock
              LEFT JOIN LATERAL (
                  SELECT activation.policy_version_id
                    FROM agent_mail_policy_activations AS activation
                   WHERE activation.organization_id = principal.organization_id
                     AND activation.agent_id = principal.id
                     AND profile.environment = 'production'
                   ORDER BY activation.policy_version DESC
                   LIMIT 1
              ) AS activation ON true
              LEFT JOIN agent_mail_policy_versions AS policy
                ON policy.id = activation.policy_version_id
               AND policy.organization_id = principal.organization_id
               AND policy.agent_id = principal.id
             WHERE principal.id = $1
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(agent_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;

        row.map(|row| {
            let environment = match row.environment.as_str() {
                "development" => AgentEnvironment::Development,
                "testing" => AgentEnvironment::Testing,
                "production" => AgentEnvironment::Production,
                _ => return Err(StoreError::Unavailable),
            };
            let active_policy = match (
                row.policy_version,
                row.allowed_recipient_domains,
                row.max_recipients_per_message,
                row.max_messages_per_hour,
                row.max_messages_per_recipient_per_day,
                row.require_first_contact_approval,
                row.max_auto_replies_per_thread_per_day,
            ) {
                (None, None, None, None, None, None, None) => None,
                (
                    Some(version),
                    Some(allowed_recipient_domains),
                    Some(max_recipients_per_message),
                    Some(max_messages_per_hour),
                    Some(max_messages_per_recipient_per_day),
                    Some(require_first_contact_approval),
                    Some(max_auto_replies_per_thread_per_day),
                ) => Some(
                    AgentMailPolicyVersion::new(
                        organization_id,
                        agent_id,
                        u32::try_from(version).map_err(|_| StoreError::Unavailable)?,
                        allowed_recipient_domains,
                        u16::try_from(max_recipients_per_message)
                            .map_err(|_| StoreError::Unavailable)?,
                        u32::try_from(max_messages_per_hour)
                            .map_err(|_| StoreError::Unavailable)?,
                        u16::try_from(max_messages_per_recipient_per_day)
                            .map_err(|_| StoreError::Unavailable)?,
                        require_first_contact_approval,
                        u8::try_from(max_auto_replies_per_thread_per_day)
                            .map_err(|_| StoreError::Unavailable)?,
                    )
                    .map_err(|_| StoreError::Unavailable)?,
                ),
                _ => return Err(StoreError::Unavailable),
            };
            Ok(AgentMailPolicyRuntimeSnapshot {
                organization_id,
                agent_id,
                environment,
                active_policy,
                observed_at: row.observed_at,
                hourly_window_started_at: row.hourly_window_started_at,
                admitted_messages_in_hour: u32::try_from(row.admitted_messages_in_hour)
                    .map_err(|_| StoreError::Unavailable)?,
            })
        })
        .transpose()
    }
}

#[async_trait]
impl AgentProcessingQueryStore for PgControlPlaneStore {
    async fn get_agent_processing_subscription(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        subscription_id: SubscriptionId,
    ) -> Result<Option<AgentProcessingSubscriptionRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            mailbox_id: Uuid,
            status: String,
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT subscription.id, subscription.organization_id,
                   subscription.principal_id, subscription.mailbox_id,
                   subscription.status, subscription.created_at,
                   subscription.updated_at
              FROM agent_subscriptions AS subscription
              JOIN principals AS principal
                ON principal.id = subscription.principal_id
               AND principal.organization_id = subscription.organization_id
             WHERE subscription.id = $1
               AND subscription.organization_id = $2
               AND subscription.principal_id = $3
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(AgentProcessingSubscriptionRecord {
                subscription_id: SubscriptionId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                principal_id: PrincipalId::from_uuid(row.principal_id),
                mailbox_id: MailboxId::from_uuid(row.mailbox_id),
                status: agent_processing_subscription_status(&row.status)?,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        })
        .transpose()
    }

    async fn list_agent_processing_dead_letters(
        &self,
        query: &AgentProcessingDeadLetterQuery,
    ) -> Result<AgentProcessingDeadLetterBatch, StoreError> {
        if !(1..=101).contains(&query.limit) {
            return Err(StoreError::Conflict);
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        let authorized = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM agent_subscriptions AS subscription
                  JOIN principals AS principal
                    ON principal.id = subscription.principal_id
                   AND principal.organization_id = subscription.organization_id
                 WHERE subscription.id = $1
                   AND subscription.organization_id = $2
                   AND subscription.principal_id = $3
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $2
                              AND membership.mailbox_id = subscription.mailbox_id
                              AND membership.principal_id = $3
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= $4
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $4
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $2
                              AND delegation.mailbox_id = subscription.mailbox_id
                              AND delegation.grantee_principal_id = $3
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= $4
                              AND delegation.valid_until > $4
                       )
                   )
            )
            "#,
        )
        .bind(query.subscription_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.observed_at)
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !authorized {
            return Err(StoreError::MissingReference);
        }

        let anchor_failed_at = if let Some(before_message_id) = query.before_message_id {
            sqlx::query_scalar::<_, DateTime<Utc>>(
                r#"
                SELECT state.failed_at
                  FROM agent_message_states AS state
                  JOIN agent_subscriptions AS subscription
                    ON subscription.id = state.subscription_id
                   AND subscription.organization_id = state.organization_id
                  JOIN principals AS principal
                    ON principal.id = subscription.principal_id
                   AND principal.organization_id = subscription.organization_id
                 WHERE state.organization_id = $1
                   AND state.principal_id = $2
                   AND state.subscription_id = $3
                   AND state.message_id = $4
                   AND state.state = 'dead_lettered'
                   AND subscription.principal_id = $2
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = subscription.mailbox_id
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= $5
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $5
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = subscription.mailbox_id
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= $5
                              AND delegation.valid_until > $5
                       )
                   )
                "#,
            )
            .bind(query.organization_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(query.subscription_id.as_uuid())
            .bind(before_message_id)
            .bind(query.observed_at)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            None
        };
        if query.before_message_id.is_some() && anchor_failed_at.is_none() {
            return Ok(AgentProcessingDeadLetterBatch {
                items: Vec::new(),
                cursor_valid: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            subscription_id: Uuid,
            message_id: Uuid,
            principal_id: Uuid,
            attempt_count: i32,
            reason_code: String,
            failed_at: DateTime<Utc>,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT state.organization_id, state.subscription_id, state.message_id,
                   state.principal_id, state.attempt_count,
                   state.last_error AS reason_code, state.failed_at
              FROM agent_message_states AS state
              JOIN agent_subscriptions AS subscription
                ON subscription.id = state.subscription_id
               AND subscription.organization_id = state.organization_id
              JOIN principals AS principal
                ON principal.id = subscription.principal_id
               AND principal.organization_id = subscription.organization_id
             WHERE state.organization_id = $1
               AND state.principal_id = $2
               AND state.subscription_id = $3
               AND state.state = 'dead_lettered'
               AND subscription.principal_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
               AND (
                   EXISTS (
                       SELECT 1
                         FROM mailbox_memberships AS membership
                        WHERE membership.organization_id = $1
                          AND membership.mailbox_id = subscription.mailbox_id
                          AND membership.principal_id = $2
                          AND membership.role IN ('owner', 'member')
                          AND membership.valid_from <= $4
                          AND (
                              membership.valid_until IS NULL
                              OR membership.valid_until > $4
                          )
                   )
                   OR EXISTS (
                       SELECT 1
                         FROM delegations AS delegation
                        WHERE delegation.organization_id = $1
                          AND delegation.mailbox_id = subscription.mailbox_id
                          AND delegation.grantee_principal_id = $2
                          AND 'mailbox.read' = ANY(delegation.scopes)
                          AND delegation.revoked_at IS NULL
                          AND delegation.valid_from <= $4
                          AND delegation.valid_until > $4
                   )
               )
               AND (
                   $5::timestamptz IS NULL
                   OR (state.failed_at, state.message_id) < ($5, $6)
               )
             ORDER BY state.failed_at DESC, state.message_id DESC
             LIMIT $7
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.subscription_id.as_uuid())
        .bind(query.observed_at)
        .bind(anchor_failed_at)
        .bind(query.before_message_id)
        .bind(query.limit)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let items = rows
            .into_iter()
            .map(|row| {
                Ok(AgentProcessingDeadLetterRecord {
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    subscription_id: SubscriptionId::from_uuid(row.subscription_id),
                    message_id: row.message_id,
                    principal_id: PrincipalId::from_uuid(row.principal_id),
                    attempt_count: u32::try_from(row.attempt_count)
                        .map_err(|_| StoreError::Conflict)?,
                    reason_code: row.reason_code,
                    failed_at: row.failed_at,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(AgentProcessingDeadLetterBatch {
            items,
            cursor_valid: true,
        })
    }
}

#[async_trait]
impl MailboxQueryStore for PgControlPlaneStore {
    async fn get_mailbox(
        &self,
        mailbox_id: MailboxId,
    ) -> Result<Option<MailboxSummary>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            kind: String,
            primary_address: String,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, kind, primary_address, created_at
              FROM mailboxes
             WHERE id = $1
            "#,
        )
        .bind(mailbox_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let kind = mailbox_kind_from_str(&row.kind).ok_or(StoreError::Unavailable)?;
            let primary_address = copilotmail_domain::MailboxAddress::parse(row.primary_address)
                .map_err(|_| StoreError::Unavailable)?;
            Ok(MailboxSummary {
                mailbox_id: MailboxId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                kind,
                primary_address,
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    async fn get_mail_engine_mailbox_binding(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            version: i64,
            updated_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT organization_id, mailbox_id, mail_data_plane_id, account_ref,
                   version, updated_at
              FROM mail_engine_mailbox_bindings
             WHERE organization_id = $1
               AND mailbox_id = $2
               AND mail_data_plane_id = $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailEngineMailboxBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            account_ref: row.account_ref,
            version: row.version,
            updated_at: row.updated_at,
        }))
    }
}

#[async_trait]
impl MailDraftMetadataStore for PgControlPlaneStore {
    async fn get_mail_draft(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        created_by: PrincipalId,
        draft_id: DraftId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDraftRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailDraftRow>(
            r#"
            SELECT id, organization_id, mailbox_id, created_by, mail_data_plane_id,
                   engine_account_ref, engine_email_ref, engine_thread_ref,
                   reply_to_message_id, reply_to_automatic, body_ref, content_sha256,
                   recipient_refs, recipient_domains,
                   version, status, created_at, updated_at
              FROM mail_drafts
             WHERE id = $1
               AND organization_id = $2
               AND mailbox_id = $3
               AND created_by = $4
               AND mail_data_plane_id = $5
            "#,
        )
        .bind(draft_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(created_by.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_draft_from_row).transpose()
    }

    async fn record_mail_draft(
        &self,
        write: MailDraftWrite,
    ) -> Result<MailDraftRecord, StoreError> {
        let recipient_refs = write
            .recipients
            .iter()
            .map(|recipient| recipient.recipient_ref.to_vec())
            .collect::<Vec<_>>();
        let recipient_domains = write
            .recipients
            .iter()
            .map(|recipient| recipient.domain.clone())
            .collect::<Vec<_>>();
        let row = if let Some(expected_version) = write.expected_version {
            sqlx::query_as::<_, MailDraftRow>(
                r#"
                UPDATE mail_drafts
                   SET engine_email_ref = $5,
                       engine_thread_ref = $6,
                       content_sha256 = $7,
                       body_ref = $8,
                       recipient_refs = $9,
                       recipient_domains = $10,
                       version = version + 1,
                       updated_at = $11
                 WHERE id = $1
                   AND organization_id = $2
                   AND mailbox_id = $3
                   AND created_by = $4
                   AND mail_data_plane_id = $12
                   AND engine_account_ref = $13
                   AND version = $14
                   AND status = 'active'
                   AND reply_to_message_id IS NOT DISTINCT FROM $15
                   AND reply_to_automatic = $16
                RETURNING id, organization_id, mailbox_id, created_by,
                          mail_data_plane_id, engine_account_ref, engine_email_ref,
                          engine_thread_ref, reply_to_message_id, reply_to_automatic,
                          body_ref, content_sha256, recipient_refs, recipient_domains,
                          version, status, created_at, updated_at
                "#,
            )
            .bind(write.draft_id.as_uuid())
            .bind(write.organization_id.as_uuid())
            .bind(write.mailbox_id.as_uuid())
            .bind(write.created_by.as_uuid())
            .bind(&write.engine_email_ref)
            .bind(&write.engine_thread_ref)
            .bind(write.content_sha256.as_slice())
            .bind(write.body_ref.as_slice())
            .bind(&recipient_refs)
            .bind(&recipient_domains)
            .bind(write.now)
            .bind(write.data_plane_id.as_uuid())
            .bind(&write.engine_account_ref)
            .bind(expected_version)
            .bind(write.reply_to_message_id)
            .bind(write.reply_to_automatic)
            .fetch_optional(&self.pool)
            .await
            .map_err(map_store_error)?
            .ok_or(StoreError::Conflict)?
        } else {
            sqlx::query_as::<_, MailDraftRow>(
                r#"
                INSERT INTO mail_drafts(
                    id, organization_id, mailbox_id, created_by, mail_data_plane_id,
                    engine_source, engine_account_ref, engine_email_ref, engine_thread_ref,
                    reply_to_message_id, reply_to_automatic, body_ref, content_sha256,
                    recipient_refs, recipient_domains,
                    version, status, created_at, updated_at
                )
                VALUES (
                    $1, $2, $3, $4, $5, 'stalwart', $6, $7, $8, $9, $10, $11, $12,
                    $13, $14, 1, 'active', $15, $15
                )
                RETURNING id, organization_id, mailbox_id, created_by,
                          mail_data_plane_id, engine_account_ref, engine_email_ref,
                          engine_thread_ref, reply_to_message_id, reply_to_automatic,
                          body_ref, content_sha256, recipient_refs, recipient_domains,
                          version, status, created_at, updated_at
                "#,
            )
            .bind(write.draft_id.as_uuid())
            .bind(write.organization_id.as_uuid())
            .bind(write.mailbox_id.as_uuid())
            .bind(write.created_by.as_uuid())
            .bind(write.data_plane_id.as_uuid())
            .bind(&write.engine_account_ref)
            .bind(&write.engine_email_ref)
            .bind(&write.engine_thread_ref)
            .bind(write.reply_to_message_id)
            .bind(write.reply_to_automatic)
            .bind(write.body_ref.as_slice())
            .bind(write.content_sha256.as_slice())
            .bind(&recipient_refs)
            .bind(&recipient_domains)
            .bind(write.now)
            .fetch_one(&self.pool)
            .await
            .map_err(map_store_error)?
        };
        mail_draft_from_row(row)
    }
}

#[async_trait]
impl HumanDraftQueryStore for PgControlPlaneStore {
    async fn list_human_drafts(
        &self,
        query: &HumanDraftListQuery,
    ) -> Result<HumanDraftListBatch, StoreError> {
        if !(1..=50).contains(&query.limit) {
            return Err(StoreError::Conflict);
        }

        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;

        let active_human = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(query.principal_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !active_human {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftListBatch {
                active_human: false,
                cursor_valid: false,
                authorization: None,
                binding: None,
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let Some((mailbox, binding, authorization)) = load_human_draft_access_snapshot(
            &mut transaction,
            query.organization_id,
            query.principal_id,
            query.mailbox_id,
            query.expected_data_plane_id,
            query.now,
        )
        .await?
        else {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftListBatch {
                active_human: true,
                cursor_valid: query.after_draft_id.is_none(),
                authorization: None,
                binding: None,
                candidates: Vec::new(),
                has_more: false,
            });
        };
        debug_assert_eq!(mailbox.mailbox_id, query.mailbox_id);

        if !human_draft_access_permits_read_and_send(
            &authorization,
            query.organization_id,
            query.mailbox_id,
        ) {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftListBatch {
                active_human: true,
                cursor_valid: query.after_draft_id.is_none(),
                authorization: Some(authorization),
                binding: Some(binding),
                candidates: Vec::new(),
                has_more: false,
            });
        }

        let cursor_valid = if let Some(after_draft_id) = query.after_draft_id {
            sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS (
                    SELECT 1
                      FROM mail_drafts AS draft
                     WHERE draft.id = $1
                       AND draft.organization_id = $2
                       AND draft.mailbox_id = $3
                       AND draft.created_by = $4
                       AND draft.mail_data_plane_id = $5
                       AND draft.engine_account_ref = $6
                       AND draft.status = 'active'
                       AND draft.recipient_refs IS NOT NULL
                       AND draft.recipient_domains IS NOT NULL
                       AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
                       AND cardinality(draft.recipient_refs)
                           = cardinality(draft.recipient_domains)
                )
                "#,
            )
            .bind(after_draft_id.as_uuid())
            .bind(query.organization_id.as_uuid())
            .bind(query.mailbox_id.as_uuid())
            .bind(query.principal_id.as_uuid())
            .bind(query.expected_data_plane_id.as_uuid())
            .bind(&binding.account_ref)
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?
        } else {
            true
        };
        if !cursor_valid {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftListBatch {
                active_human: true,
                cursor_valid: false,
                authorization: Some(authorization),
                binding: Some(binding),
                candidates: Vec::new(),
                has_more: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            mailbox_id: Uuid,
            created_by: Uuid,
            mail_data_plane_id: Uuid,
            version: i64,
            reply_to_message_id: Option<Uuid>,
            recipient_count: i32,
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
        }

        let mut rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT draft.id, draft.organization_id, draft.mailbox_id,
                   draft.created_by, draft.mail_data_plane_id, draft.version,
                   draft.reply_to_message_id,
                   cardinality(draft.recipient_refs) AS recipient_count,
                   draft.created_at, draft.updated_at
              FROM mail_drafts AS draft
             WHERE draft.organization_id = $1
               AND draft.mailbox_id = $2
               AND draft.created_by = $3
               AND draft.mail_data_plane_id = $4
               AND draft.engine_account_ref = $5
               AND draft.status = 'active'
               AND draft.recipient_refs IS NOT NULL
               AND draft.recipient_domains IS NOT NULL
               AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
               AND cardinality(draft.recipient_refs)
                   = cardinality(draft.recipient_domains)
               AND ($6::uuid IS NULL OR draft.id < $6)
             ORDER BY draft.id DESC
             LIMIT $7
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.mailbox_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.expected_data_plane_id.as_uuid())
        .bind(&binding.account_ref)
        .bind(query.after_draft_id.map(DraftId::as_uuid))
        .bind(i64::from(query.limit) + 1)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let has_more = rows.len() > usize::from(query.limit);
        rows.truncate(usize::from(query.limit));
        let candidates = rows
            .into_iter()
            .map(|row| {
                Ok(HumanDraftListCandidate {
                    draft_id: DraftId::from_uuid(row.id),
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    mailbox_id: MailboxId::from_uuid(row.mailbox_id),
                    created_by: PrincipalId::from_uuid(row.created_by),
                    data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
                    version: row.version,
                    status: MailDraftStatus::Active,
                    reply_to_message_id: row.reply_to_message_id,
                    recipient_count: u16::try_from(row.recipient_count)
                        .map_err(|_| StoreError::Unavailable)?,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanDraftListBatch {
            active_human: true,
            cursor_valid: true,
            authorization: Some(authorization),
            binding: Some(binding),
            candidates,
            has_more,
        })
    }

    async fn resolve_human_draft_restore_target(
        &self,
        query: &HumanDraftRestoreQuery,
    ) -> Result<HumanDraftRestoreResolution, StoreError> {
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;

        let active_human = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(query.principal_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .fetch_one(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if !active_human {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftRestoreResolution {
                active_human: false,
                target: None,
            });
        }

        let Some((mailbox, binding, authorization)) = load_human_draft_access_snapshot(
            &mut transaction,
            query.organization_id,
            query.principal_id,
            query.mailbox_id,
            query.expected_data_plane_id,
            query.now,
        )
        .await?
        else {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftRestoreResolution {
                active_human: true,
                target: None,
            });
        };
        if !human_draft_access_permits_read_and_send(
            &authorization,
            query.organization_id,
            query.mailbox_id,
        ) {
            transaction.commit().await.map_err(map_store_error)?;
            return Ok(HumanDraftRestoreResolution {
                active_human: true,
                target: None,
            });
        }

        let draft = sqlx::query_as::<_, MailDraftRow>(
            r#"
            SELECT draft.id, draft.organization_id, draft.mailbox_id,
                   draft.created_by, draft.mail_data_plane_id,
                   draft.engine_account_ref, draft.engine_email_ref,
                   draft.engine_thread_ref, draft.reply_to_message_id,
                   draft.reply_to_automatic, draft.body_ref,
                   draft.content_sha256, draft.recipient_refs,
                   draft.recipient_domains, draft.version, draft.status,
                   draft.created_at, draft.updated_at
              FROM mail_drafts AS draft
             WHERE draft.id = $1
               AND draft.organization_id = $2
               AND draft.mailbox_id = $3
               AND draft.created_by = $4
               AND draft.mail_data_plane_id = $5
               AND draft.engine_account_ref = $6
               AND draft.status = 'active'
               AND draft.recipient_refs IS NOT NULL
               AND draft.recipient_domains IS NOT NULL
               AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
               AND cardinality(draft.recipient_refs)
                   = cardinality(draft.recipient_domains)
            "#,
        )
        .bind(query.draft_id.as_uuid())
        .bind(query.organization_id.as_uuid())
        .bind(query.mailbox_id.as_uuid())
        .bind(query.principal_id.as_uuid())
        .bind(query.expected_data_plane_id.as_uuid())
        .bind(&binding.account_ref)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?
        .map(mail_draft_from_row)
        .transpose()?;

        transaction.commit().await.map_err(map_store_error)?;
        Ok(HumanDraftRestoreResolution {
            active_human: true,
            target: draft.map(|draft| HumanDraftRestoreTarget {
                authorization,
                mailbox,
                binding,
                draft,
            }),
        })
    }
}

#[async_trait]
impl MailSubmissionQueryStore for PgControlPlaneStore {
    async fn get_mail_submission_intent(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id, draft_id,
                   draft_version, mail_data_plane_id, request_sha256, status,
                   state_version, created_at, updated_at
              FROM mail_submission_intents
             WHERE id = $1
               AND organization_id = $2
               AND requested_by = $3
            "#,
        )
        .bind(submission_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(requested_by.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_submission_intent_from_row).transpose()
    }

    async fn get_mail_submission_intent_for_execution(
        &self,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id, draft_id,
                   draft_version, mail_data_plane_id, request_sha256, status,
                   state_version, created_at, updated_at
              FROM mail_submission_intents
             WHERE id = $1
            "#,
        )
        .bind(submission_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_submission_intent_from_row).transpose()
    }
}

#[async_trait]
impl MailDeliveryQueryStore for PgControlPlaneStore {
    async fn get_mail_delivery_reconciliation_target(
        &self,
        submission_id: MailSubmissionId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDeliveryReconciliationTarget>, StoreError> {
        let row = sqlx::query_as::<_, MailDeliveryTargetRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id,
                   mail_data_plane_id, engine_account_ref, engine_email_ref,
                   engine_identity_ref, engine_submission_ref
              FROM mail_submission_intents
             WHERE id = $1
               AND mail_data_plane_id = $2
               AND status = 'accepted'
               AND engine_identity_ref IS NOT NULL
               AND engine_submission_ref IS NOT NULL
            "#,
        )
        .bind(submission_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailDeliveryReconciliationTarget {
            submission_id: MailSubmissionId::from_uuid(row.id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            requested_by: PrincipalId::from_uuid(row.requested_by),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            engine_account_ref: row.engine_account_ref,
            engine_email_ref: row.engine_email_ref,
            engine_identity_ref: row.engine_identity_ref,
            engine_submission_ref: row.engine_submission_ref,
        }))
    }

    async fn get_latest_mail_delivery_snapshot(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailDeliverySnapshotRecord>, StoreError> {
        let row = sqlx::query_as::<_, MailDeliverySnapshotRow>(
            r#"
            SELECT snapshot.id, snapshot.organization_id,
                   snapshot.submission_id, snapshot.source_state,
                   snapshot.source_sha256, snapshot.terminal,
                   snapshot.observed_at
              FROM mail_delivery_snapshots AS snapshot
              JOIN mail_submission_intents AS submission
                ON submission.id = snapshot.submission_id
               AND submission.organization_id = snapshot.organization_id
             WHERE snapshot.organization_id = $1
               AND snapshot.submission_id = $2
               AND submission.requested_by = $3
             ORDER BY snapshot.observed_at DESC, snapshot.id DESC
             LIMIT 1
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(submission_id.as_uuid())
        .bind(requested_by.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let recipients = sqlx::query_as::<_, MailDeliveryRecipientRow>(
            r#"
            SELECT recipient_ref, state, smtp_reply
              FROM mail_delivery_recipient_evidence
             WHERE snapshot_id = $1
               AND organization_id = $2
               AND submission_id = $3
             ORDER BY recipient_ref
            "#,
        )
        .bind(row.id)
        .bind(row.organization_id)
        .bind(row.submission_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?
        .into_iter()
        .map(mail_delivery_recipient_from_row)
        .collect::<Result<Vec<_>, _>>()?;
        let source_sha256 = digest_from_bytes(row.source_sha256, "mail delivery source")?;
        Ok(Some(MailDeliverySnapshotRecord {
            snapshot_id: row.id,
            organization_id: OrganizationId::from_uuid(row.organization_id),
            submission_id: MailSubmissionId::from_uuid(row.submission_id),
            source_state: row.source_state,
            source_sha256,
            recipients,
            terminal: row.terminal,
            observed_at: row.observed_at,
        }))
    }
}

#[async_trait]
impl MailDomainQueryStore for PgControlPlaneStore {
    async fn get_mail_domain(
        &self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        now: DateTime<Utc>,
    ) -> Result<Option<MailDomainSummary>, StoreError> {
        let row = sqlx::query_as::<_, MailDomainSummaryRow>(
            r#"
            SELECT domain.id AS mail_domain_id,
                   domain.organization_id,
                   domain.name,
                   domain.status,
                   domain.route_mode,
                   domain.ownership_challenge_version,
                   route.version AS route_version,
                   route.inbound_enabled,
                   route.outbound_enabled,
                   route.smart_host_ref,
                   route.dkim_key_ref,
                   domain.mail_engine_domain_ref,
                   domain.mail_engine_projection_status,
                   domain.mail_engine_projected_enabled,
                   domain.mail_engine_projection_last_attempt_at,
                   domain.mail_engine_projection_succeeded_at,
                   domain.mail_engine_projection_error_code,
                   observation.observed_at AS latest_dns_observed_at,
                   observation.valid_until AS latest_dns_valid_until,
                   COALESCE(
                       observation.ownership_txt
                       AND observation.mx_present
                       AND observation.mx_hosts_public
                       AND observation.spf_present
                       AND observation.dkim_present
                       AND observation.dmarc_present
                       AND observation.observed_at <= $3
                       AND observation.valid_until > $3,
                       false
                   ) AS latest_dns_activation_ready,
                   COALESCE(observation.failure_codes, '{}'::text[])
                       AS latest_dns_failure_codes
              FROM mail_domains AS domain
              JOIN mail_domain_routes AS route
                ON route.mail_domain_id = domain.id
               AND route.organization_id = domain.organization_id
              LEFT JOIN LATERAL (
                    SELECT observed_at, valid_until, ownership_txt, mx_present,
                           mx_hosts_public, spf_present, dkim_present, dmarc_present,
                           failure_codes
                      FROM mail_domain_dns_observations
                     WHERE organization_id = domain.organization_id
                       AND mail_domain_id = domain.id
                       AND ownership_token_sha256 = domain.ownership_token_sha256
                     ORDER BY observed_at DESC, created_at DESC, id DESC
                     LIMIT 1
              ) AS observation ON true
             WHERE domain.id = $1 AND domain.organization_id = $2
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(mail_domain_summary_from_row).transpose()
    }

    async fn list_mail_domains(
        &self,
        organization_id: OrganizationId,
        after_name: Option<&MailDomainName>,
        limit: i64,
        now: DateTime<Utc>,
    ) -> Result<Vec<MailDomainSummary>, StoreError> {
        if !(1..=200).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, MailDomainSummaryRow>(
            r#"
            SELECT domain.id AS mail_domain_id,
                   domain.organization_id,
                   domain.name,
                   domain.status,
                   domain.route_mode,
                   domain.ownership_challenge_version,
                   route.version AS route_version,
                   route.inbound_enabled,
                   route.outbound_enabled,
                   route.smart_host_ref,
                   route.dkim_key_ref,
                   domain.mail_engine_domain_ref,
                   domain.mail_engine_projection_status,
                   domain.mail_engine_projected_enabled,
                   domain.mail_engine_projection_last_attempt_at,
                   domain.mail_engine_projection_succeeded_at,
                   domain.mail_engine_projection_error_code,
                   observation.observed_at AS latest_dns_observed_at,
                   observation.valid_until AS latest_dns_valid_until,
                   COALESCE(
                       observation.ownership_txt
                       AND observation.mx_present
                       AND observation.mx_hosts_public
                       AND observation.spf_present
                       AND observation.dkim_present
                       AND observation.dmarc_present
                       AND observation.observed_at <= $4
                       AND observation.valid_until > $4,
                       false
                   ) AS latest_dns_activation_ready,
                   COALESCE(observation.failure_codes, '{}'::text[])
                       AS latest_dns_failure_codes
              FROM mail_domains AS domain
              JOIN mail_domain_routes AS route
                ON route.mail_domain_id = domain.id
               AND route.organization_id = domain.organization_id
              LEFT JOIN LATERAL (
                    SELECT observed_at, valid_until, ownership_txt, mx_present,
                           mx_hosts_public, spf_present, dkim_present, dmarc_present,
                           failure_codes
                      FROM mail_domain_dns_observations
                     WHERE organization_id = domain.organization_id
                       AND mail_domain_id = domain.id
                       AND ownership_token_sha256 = domain.ownership_token_sha256
                     ORDER BY observed_at DESC, created_at DESC, id DESC
                     LIMIT 1
              ) AS observation ON true
             WHERE domain.organization_id = $1
               AND ($2::text IS NULL OR domain.name > $2)
             ORDER BY domain.name
             LIMIT $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(after_name.map(MailDomainName::as_str))
        .bind(limit)
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter().map(mail_domain_summary_from_row).collect()
    }
}

#[async_trait]
impl WebhookSubscriptionQueryStore for PgControlPlaneStore {
    async fn get_webhook_subscription(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscriptionSummary>, StoreError> {
        let row = sqlx::query_as::<_, WebhookSubscriptionRow>(
            r#"
            SELECT id AS subscription_id, organization_id, principal_id,
                   endpoint_url, event_types, signing_key_reference, enabled,
                   version, created_at, updated_at
              FROM webhook_subscriptions
             WHERE id = $1 AND organization_id = $2
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(webhook_subscription_summary_from_row).transpose()
    }

    async fn list_webhook_subscriptions(
        &self,
        organization_id: OrganizationId,
        principal_id: Option<PrincipalId>,
        after_id: Option<WebhookSubscriptionId>,
        limit: i64,
    ) -> Result<Vec<WebhookSubscriptionSummary>, StoreError> {
        if !(1..=200).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, WebhookSubscriptionRow>(
            r#"
            SELECT id AS subscription_id, organization_id, principal_id,
                   endpoint_url, event_types, signing_key_reference, enabled,
                   version, created_at, updated_at
              FROM webhook_subscriptions
             WHERE organization_id = $1
               AND ($2::uuid IS NULL OR principal_id = $2)
               AND ($3::uuid IS NULL OR id > $3)
             ORDER BY id
             LIMIT $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.map(PrincipalId::as_uuid))
        .bind(after_id.map(WebhookSubscriptionId::as_uuid))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(webhook_subscription_summary_from_row)
            .collect()
    }
}

#[async_trait]
impl AgentEventCursorQueryStore for PgControlPlaneStore {
    async fn webhook_cursor_event_exists(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        event_id: Uuid,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM webhook_subscriptions AS subscription
                  JOIN principals AS principal
                    ON principal.id = subscription.principal_id
                   AND principal.organization_id = subscription.organization_id
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                  JOIN outbox_events AS event
                    ON event.id = $4
                   AND event.payload ->> 'organization_id' =
                       subscription.organization_id::text
                   AND event.event_type = ANY(subscription.event_types)
                   AND event.event_type IN (
                       'message.received.v1',
                       'message.delivery_status_changed.v1',
                       'agent.message.dead_lettered.v1'
                   )
                   AND event.occurred_at >= subscription.created_at
                 WHERE subscription.organization_id = $1
                   AND subscription.id = $2
                   AND subscription.principal_id = $3
                   AND subscription.enabled
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(subscription_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(event_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_webhook_cursor_events(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        after_event_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<AgentCursorEvent>, StoreError> {
        if !(1..=101).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, AgentCursorEventRow>(
            r#"
            SELECT event.id AS event_id,
                   subscription.organization_id,
                   event.event_type,
                   event.aggregate_type,
                   event.aggregate_id,
                   event.schema_version,
                   event.payload,
                   event.occurred_at
              FROM webhook_subscriptions AS subscription
              JOIN principals AS principal
                ON principal.id = subscription.principal_id
               AND principal.organization_id = subscription.organization_id
               AND principal.kind = 'agent'
               AND principal.status = 'active'
              JOIN outbox_events AS event
                ON event.payload ->> 'organization_id' =
                   subscription.organization_id::text
               AND event.event_type = ANY(subscription.event_types)
               AND event.event_type IN (
                   'message.received.v1',
                   'message.delivery_status_changed.v1',
                   'agent.message.dead_lettered.v1'
               )
               AND event.occurred_at >= subscription.created_at
             WHERE subscription.organization_id = $1
               AND subscription.id = $2
               AND subscription.principal_id = $3
               AND subscription.enabled
               AND event.id > COALESCE(
                   $4,
                   '00000000-0000-0000-0000-000000000000'::uuid
               )
             ORDER BY event.id
             LIMIT $5
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(subscription_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(after_event_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(|row| {
                if !row.payload.is_object() {
                    return Err(StoreError::Conflict);
                }
                Ok(AgentCursorEvent {
                    event_id: row.event_id,
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    event_type: WebhookEventType::parse(&row.event_type)
                        .map_err(|_| StoreError::Conflict)?,
                    aggregate_type: row.aggregate_type,
                    aggregate_id: row.aggregate_id,
                    schema_version: row.schema_version,
                    payload: row.payload,
                    occurred_at: row.occurred_at,
                })
            })
            .collect()
    }

    async fn release_agent_event_stream_lease(
        &self,
        lease: &AgentEventStreamLease,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM agent_event_stream_leases
             WHERE id = $1
               AND organization_id = $2
               AND principal_id = $3
               AND subscription_id = $4
            "#,
        )
        .bind(lease.lease_id)
        .bind(lease.organization_id.as_uuid())
        .bind(lease.principal_id.as_uuid())
        .bind(lease.subscription_id.as_uuid())
        .execute(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }
}

#[async_trait]
impl AgentIngressRateLimitStore for PgControlPlaneStore {
    async fn consume_agent_ingress_capacity(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        tenant_requests_per_minute: u32,
        principal_requests_per_minute: u32,
    ) -> Result<AgentIngressRateLimitDecision, StoreError> {
        if tenant_requests_per_minute == 0
            || principal_requests_per_minute == 0
            || principal_requests_per_minute > tenant_requests_per_minute
        {
            return Err(StoreError::Conflict);
        }
        let tenant_limit =
            i32::try_from(tenant_requests_per_minute).map_err(|_| StoreError::Conflict)?;
        let principal_limit =
            i32::try_from(principal_requests_per_minute).map_err(|_| StoreError::Conflict)?;
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 30))")
            .bind(organization_id.as_uuid())
            .execute(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        let (window_started_at, window_ends_at, observed_at, retry_after_seconds) =
            sqlx::query_as::<_, (DateTime<Utc>, DateTime<Utc>, DateTime<Utc>, i32)>(
                r#"
                WITH snapshot AS (
                    SELECT clock_timestamp() AS observed_at
                )
                SELECT date_trunc('minute', observed_at),
                       date_trunc('minute', observed_at) + interval '1 minute',
                       observed_at,
                       GREATEST(
                           1,
                           CEIL(EXTRACT(
                               EPOCH FROM (
                                   date_trunc('minute', observed_at) + interval '1 minute'
                                   - observed_at
                               )
                           ))::integer
                       )
                  FROM snapshot
                "#,
            )
            .fetch_one(&mut *transaction)
            .await
            .map_err(map_store_error)?;
        sqlx::query(
            r#"
            DELETE FROM agent_ingress_rate_limit_windows
             WHERE organization_id = $1
               AND window_ends_at <= clock_timestamp()
            "#,
        )
        .bind(organization_id.as_uuid())
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;

        if !increment_agent_ingress_window(
            &mut transaction,
            organization_id,
            None,
            window_started_at,
            window_ends_at,
            observed_at,
            tenant_limit,
        )
        .await
        .map_err(map_store_error)?
        {
            transaction.rollback().await.map_err(map_store_error)?;
            return Ok(AgentIngressRateLimitDecision::TenantLimited {
                retry_after_seconds: u32::try_from(retry_after_seconds).unwrap_or(1),
            });
        }
        if !increment_agent_ingress_window(
            &mut transaction,
            organization_id,
            Some(principal_id),
            window_started_at,
            window_ends_at,
            observed_at,
            principal_limit,
        )
        .await
        .map_err(map_store_error)?
        {
            transaction.rollback().await.map_err(map_store_error)?;
            return Ok(AgentIngressRateLimitDecision::PrincipalLimited {
                retry_after_seconds: u32::try_from(retry_after_seconds).unwrap_or(1),
            });
        }
        transaction.commit().await.map_err(map_store_error)?;
        Ok(AgentIngressRateLimitDecision::Allowed)
    }
}

#[async_trait]
impl HumanMailContentQueryStore for PgControlPlaneStore {
    async fn resolve_human_mail_message_projections(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
        engine_account_ref: &str,
        document_refs: &[String],
    ) -> Result<Vec<HumanMailMessageProjection>, StoreError> {
        if document_refs.is_empty() {
            return Ok(Vec::new());
        }

        #[derive(sqlx::FromRow)]
        struct Row {
            message_id: Uuid,
            document_ref: String,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT event.message_id, event.document_ref
              FROM unnest($5::text[]) WITH ORDINALITY
                   AS requested(document_ref, ordinal)
              JOIN inbound_mail_engine_events AS event
                ON event.document_ref = requested.document_ref
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $2
               AND event.mail_data_plane_id = $3
               AND event.account_ref = $4
             ORDER BY requested.ordinal
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .bind(engine_account_ref)
        .bind(document_refs)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;

        Ok(rows
            .into_iter()
            .map(|row| HumanMailMessageProjection {
                message_id: row.message_id,
                engine_document_ref: row.document_ref,
            })
            .collect())
    }

    async fn resolve_human_mail_content_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<HumanMailContentTarget>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            message_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            document_ref: String,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT event.organization_id, event.mailbox_id, event.message_id,
                   event.mail_data_plane_id, event.account_ref, event.document_ref
              FROM inbound_mail_engine_events AS event
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $2
               AND event.message_id = $3
               AND event.mail_data_plane_id = $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(message_id)
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;

        Ok(row.map(|row| HumanMailContentTarget {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            message_id: row.message_id,
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            engine_account_ref: row.account_ref,
            engine_document_ref: row.document_ref,
        }))
    }
}

#[async_trait]
impl AgentMailContentQueryStore for PgControlPlaneStore {
    async fn resolve_agent_mail_content_target(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        message_id: Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<AgentMailContentTarget>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            message_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            document_ref: String,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT event.organization_id, event.mailbox_id, event.message_id,
                   event.mail_data_plane_id, event.account_ref, event.document_ref
              FROM inbound_mail_engine_events AS event
              JOIN principals AS principal
                ON principal.id = $2
               AND principal.organization_id = event.organization_id
               AND principal.kind = 'agent'
               AND principal.status = 'active'
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $3
               AND event.message_id = $4
               AND event.mail_data_plane_id = $5
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(message_id)
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| AgentMailContentTarget {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            message_id: row.message_id,
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            engine_account_ref: row.account_ref,
            engine_document_ref: row.document_ref,
        }))
    }
}

#[async_trait]
impl MailReplyTargetQueryStore for PgControlPlaneStore {
    async fn resolve_mail_reply_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailReplyTarget>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            message_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            document_ref: String,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT event.organization_id, event.mailbox_id, event.message_id,
                   event.mail_data_plane_id, event.account_ref, event.document_ref
              FROM inbound_mail_engine_events AS event
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $2
               AND event.message_id = $3
               AND event.mail_data_plane_id = $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(message_id)
        .bind(expected_data_plane_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailReplyTarget {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            message_id: row.message_id,
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            engine_account_ref: row.account_ref,
            engine_document_ref: row.document_ref,
        }))
    }

    async fn resolve_mail_reply_message_id_by_engine_document(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
        engine_document_ref: &str,
    ) -> Result<Option<Uuid>, StoreError> {
        let message_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT event.message_id
              FROM inbound_mail_engine_events AS event
              JOIN mail_engine_mailbox_bindings AS binding
                ON binding.organization_id = event.organization_id
               AND binding.mailbox_id = event.mailbox_id
               AND binding.mail_data_plane_id = event.mail_data_plane_id
               AND binding.account_ref = event.account_ref
             WHERE event.organization_id = $1
               AND event.mailbox_id = $2
               AND event.mail_data_plane_id = $3
               AND event.document_ref = $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .bind(engine_document_ref)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(message_id)
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

#[async_trait]
impl WebhookDeadLetterQueryStore for PgControlPlaneStore {
    async fn list_webhook_dead_letters(
        &self,
        organization_id: OrganizationId,
        after_delivery_id: Option<Uuid>,
        limit: i64,
    ) -> Result<Vec<WebhookDeadLetterSummary>, StoreError> {
        if !(1..=200).contains(&limit) {
            return Err(StoreError::Conflict);
        }
        let rows = sqlx::query_as::<_, WebhookDeadLetterRow>(
            r#"
            SELECT delivery.id AS delivery_id,
                   delivery.subscription_id,
                   subscription.organization_id,
                   subscription.principal_id,
                   delivery.event_id,
                   outbox.event_type,
                   delivery.attempt,
                   delivery.response_status,
                   delivery.error_code,
                   delivery.created_at AS failed_at
              FROM webhook_deliveries AS delivery
              JOIN webhook_subscriptions AS subscription
                ON subscription.id = delivery.subscription_id
              JOIN outbox_events AS outbox
                ON outbox.id = delivery.event_id
               AND outbox.payload ->> 'organization_id' =
                   subscription.organization_id::text
             WHERE subscription.organization_id = $1
               AND delivery.status = 'dead_lettered'
               AND delivery.id > COALESCE($2, '00000000-0000-0000-0000-000000000000'::uuid)
               AND NOT EXISTS (
                   SELECT 1
                     FROM webhook_deliveries AS later
                    WHERE later.subscription_id = delivery.subscription_id
                      AND later.event_id = delivery.event_id
                      AND later.attempt > delivery.attempt
               )
             ORDER BY delivery.id
             LIMIT $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(after_delivery_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(rows
            .into_iter()
            .map(|row| WebhookDeadLetterSummary {
                delivery_id: row.delivery_id,
                subscription_id: WebhookSubscriptionId::from_uuid(row.subscription_id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                principal_id: PrincipalId::from_uuid(row.principal_id),
                event_id: row.event_id,
                event_type: row.event_type,
                attempt: row.attempt,
                response_status: row.response_status,
                error_code: row.error_code,
                failed_at: row.failed_at,
            })
            .collect())
    }
}

#[async_trait]
impl ApprovalQueryStore for PgControlPlaneStore {
    async fn approval_permits(&self, proof: &ApprovalProof) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM approval_requests AS request
                  JOIN policy_versions AS version
                    ON version.id = request.policy_version_id
                   AND version.organization_id = request.organization_id
                 WHERE request.id = $1
                   AND request.organization_id = $2
                   AND request.action = $3
                   AND request.requested_by = $7
                   AND version.action = request.action
                   AND request.resource_kind = $4
                   AND request.resource_id = $5
                   AND request.status = 'approved'
                   AND request.expires_at > $6
                   AND (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.approval_request_id = request.id
                          AND step.organization_id = request.organization_id
                          AND step.decision = 'approved'
                   ) >= request.required_approvals
                   AND NOT EXISTS (
                       SELECT 1
                         FROM approval_steps AS step
                        WHERE step.approval_request_id = request.id
                          AND step.organization_id = request.organization_id
                          AND step.decision = 'rejected'
                   )
            )
            "#,
        )
        .bind(proof.request_id.as_uuid())
        .bind(proof.organization_id.as_uuid())
        .bind(proof.action.as_str())
        .bind(&proof.resource_kind)
        .bind(&proof.resource_id)
        .bind(proof.now)
        .bind(proof.requested_by.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn approved_request_evidence(
        &self,
        proof: &ApprovalProof,
    ) -> Result<Option<ApprovedRequestEvidence>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            expires_at: DateTime<Utc>,
            required_approvals: i16,
            approval_count: i64,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT request.expires_at,
                   request.required_approvals,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.approval_request_id = request.id
                          AND step.organization_id = request.organization_id
                          AND step.decision = 'approved'
                   ) AS approval_count
              FROM approval_requests AS request
              JOIN policy_versions AS version
                ON version.id = request.policy_version_id
               AND version.organization_id = request.organization_id
             WHERE request.id = $1
               AND request.organization_id = $2
               AND request.requested_by = $3
               AND request.action = $4
               AND version.action = request.action
               AND request.resource_kind = $5
               AND request.resource_id = $6
               AND request.status = 'approved'
               AND request.expires_at > $7
               AND NOT EXISTS (
                   SELECT 1
                     FROM approval_steps AS step
                    WHERE step.approval_request_id = request.id
                      AND step.organization_id = request.organization_id
                      AND step.decision = 'rejected'
               )
            "#,
        )
        .bind(proof.request_id.as_uuid())
        .bind(proof.organization_id.as_uuid())
        .bind(proof.requested_by.as_uuid())
        .bind(proof.action.as_str())
        .bind(&proof.resource_kind)
        .bind(&proof.resource_id)
        .bind(proof.now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(ApprovedRequestEvidence {
                expires_at: row.expires_at,
                required_approvals: u8::try_from(row.required_approvals)
                    .map_err(|_| StoreError::Unavailable)?,
                approval_count: u8::try_from(row.approval_count)
                    .map_err(|_| StoreError::Unavailable)?,
            })
        })
        .transpose()
    }

    async fn get_approval_request(
        &self,
        organization_id: OrganizationId,
        request_id: copilotmail_domain::ApprovalRequestId,
        now: DateTime<Utc>,
    ) -> Result<Option<ApprovalRequestDetail>, StoreError> {
        let row = sqlx::query_as::<_, ApprovalRequestSummaryRow>(
            r#"
            SELECT request.id, request.organization_id, request.policy_version_id,
                   request.action, request.resource_kind, request.resource_id,
                   request.requested_by, request.reason, request.ticket_reference,
                   request.required_approvals,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.organization_id = request.organization_id
                          AND step.approval_request_id = request.id
                          AND step.decision = 'approved'
                   ) AS approval_count,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.organization_id = request.organization_id
                          AND step.approval_request_id = request.id
                          AND step.decision = 'rejected'
                   ) AS rejection_count,
                   CASE
                       WHEN request.status IN ('pending', 'approved')
                            AND request.expires_at <= $3
                       THEN 'expired'
                       ELSE request.status
                   END AS effective_status,
                   request.created_at, request.expires_at,
                   request.completed_at, request.cancelled_at
              FROM approval_requests AS request
             WHERE request.organization_id = $1
               AND request.id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(request_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let request = approval_request_summary_from_row(row)?;
        let step_rows = sqlx::query_as::<_, ApprovalStepSummaryRow>(
            r#"
            SELECT id, approver_principal_id, decision, reason, decided_at
              FROM approval_steps
             WHERE organization_id = $1
               AND approval_request_id = $2
             ORDER BY decided_at, id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(request_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        let steps = step_rows
            .into_iter()
            .map(approval_step_summary_from_row)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(ApprovalRequestDetail { request, steps }))
    }

    async fn approval_request_cursor_exists(
        &self,
        organization_id: OrganizationId,
        view: ApprovalRequestView,
        actor: PrincipalId,
        status: Option<ApprovalStatus>,
        request_id: copilotmail_domain::ApprovalRequestId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM approval_requests AS request
                 WHERE request.organization_id = $1
                   AND request.id = $5
                   AND (
                       ($2 = 'requested' AND request.requested_by = $3)
                       OR (
                           $2 = 'actionable'
                           AND request.status = 'pending'
                           AND request.expires_at > $6
                           AND request.requested_by <> $3
                           AND NOT EXISTS (
                               SELECT 1
                                 FROM approval_steps AS own_step
                                WHERE own_step.organization_id =
                                      request.organization_id
                                  AND own_step.approval_request_id = request.id
                                  AND own_step.approver_principal_id = $3
                           )
                       )
                   )
                   AND (
                       $4::text IS NULL
                       OR CASE
                           WHEN request.status IN ('pending', 'approved')
                                AND request.expires_at <= $6
                           THEN 'expired'
                           ELSE request.status
                          END = $4
                   )
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_view(view))
        .bind(actor.as_uuid())
        .bind(status.map(approval_status))
        .bind(request_id.as_uuid())
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_approval_requests(
        &self,
        query: ApprovalRequestListQuery,
    ) -> Result<Vec<ApprovalRequestSummary>, StoreError> {
        let ApprovalRequestListQuery {
            organization_id,
            view,
            actor,
            status,
            after_request_id,
            limit,
            now,
        } = query;
        let rows = sqlx::query_as::<_, ApprovalRequestSummaryRow>(
            r#"
            WITH eligible AS (
                SELECT request.id, request.organization_id,
                       request.policy_version_id, request.action,
                       request.resource_kind, request.resource_id,
                       request.requested_by, request.reason,
                       request.ticket_reference, request.required_approvals,
                       (
                           SELECT count(*)
                             FROM approval_steps AS step
                            WHERE step.organization_id =
                                  request.organization_id
                              AND step.approval_request_id = request.id
                              AND step.decision = 'approved'
                       ) AS approval_count,
                       (
                           SELECT count(*)
                             FROM approval_steps AS step
                            WHERE step.organization_id =
                                  request.organization_id
                              AND step.approval_request_id = request.id
                              AND step.decision = 'rejected'
                       ) AS rejection_count,
                       CASE
                           WHEN request.status IN ('pending', 'approved')
                                AND request.expires_at <= $6
                           THEN 'expired'
                           ELSE request.status
                       END AS effective_status,
                       request.created_at, request.expires_at,
                       request.completed_at, request.cancelled_at
                  FROM approval_requests AS request
                 WHERE request.organization_id = $1
                   AND (
                       ($2 = 'requested' AND request.requested_by = $3)
                       OR (
                           $2 = 'actionable'
                           AND request.status = 'pending'
                           AND request.expires_at > $6
                           AND request.requested_by <> $3
                           AND NOT EXISTS (
                               SELECT 1
                                 FROM approval_steps AS own_step
                                WHERE own_step.organization_id =
                                      request.organization_id
                                  AND own_step.approval_request_id = request.id
                                  AND own_step.approver_principal_id = $3
                           )
                       )
                   )
            )
            SELECT *
              FROM eligible
             WHERE ($4::text IS NULL OR effective_status = $4)
               AND (
                   $5::uuid IS NULL
                   OR (created_at, id) < (
                       SELECT anchor.created_at, anchor.id
                         FROM eligible AS anchor
                        WHERE anchor.id = $5
                          AND ($4::text IS NULL OR anchor.effective_status = $4)
                   )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT $7
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_view(view))
        .bind(actor.as_uuid())
        .bind(status.map(approval_status))
        .bind(after_request_id.map(|id| id.as_uuid()))
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(approval_request_summary_from_row)
            .collect()
    }
}

#[async_trait]
impl RoleGovernanceQueryStore for PgControlPlaneStore {
    async fn list_roles(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<RoleSummary>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            system_key: String,
            display_name: String,
            permissions: Vec<String>,
            created_at: DateTime<Utc>,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT role.id, role.organization_id, role.system_key,
                   role.display_name,
                   array_agg(permission.permission ORDER BY permission.permission)
                       AS permissions,
                   role.created_at
              FROM roles AS role
              JOIN role_permissions AS permission
                ON permission.role_id = role.id
             WHERE role.organization_id = $1
               AND role.system_key IS NOT NULL
             GROUP BY role.id
             ORDER BY role.system_key
            "#,
        )
        .bind(organization_id.as_uuid())
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;

        rows.into_iter()
            .map(|row| {
                let permissions = row
                    .permissions
                    .iter()
                    .map(|permission| {
                        permission_from_str(permission).ok_or(StoreError::Unavailable)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(RoleSummary {
                    role_id: RoleId::from_uuid(row.id),
                    organization_id: OrganizationId::from_uuid(row.organization_id),
                    system_role: system_role_from_str(&row.system_key)?,
                    display_name: row.display_name,
                    permissions,
                    created_at: row.created_at,
                })
            })
            .collect()
    }

    async fn get_role_assignment(
        &self,
        organization_id: OrganizationId,
        assignment_id: RoleAssignmentId,
    ) -> Result<Option<RoleAssignmentSummary>, StoreError> {
        let row = sqlx::query_as::<_, RoleAssignmentSummaryRow>(
            r#"
            SELECT assignment.id, assignment.organization_id,
                   assignment.principal_id, principal.kind AS principal_kind,
                   principal.status AS principal_status,
                   principal.display_name AS principal_display_name,
                   assignment.role_id, role.system_key,
                   role.display_name AS role_display_name,
                   assignment.created_by, grant_record.approval_request_id,
                   assignment.valid_from, assignment.valid_until,
                   assignment.revoked_at, assignment.created_at
              FROM role_assignments AS assignment
              JOIN principals AS principal
                ON principal.id = assignment.principal_id
               AND principal.organization_id = assignment.organization_id
              JOIN roles AS role
                ON role.id = assignment.role_id
               AND role.organization_id = assignment.organization_id
         LEFT JOIN role_assignment_grants AS grant_record
                ON grant_record.assignment_id = assignment.id
               AND grant_record.organization_id = assignment.organization_id
             WHERE assignment.organization_id = $1
               AND assignment.id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(role_assignment_summary_from_row).transpose()
    }

    async fn role_assignment_cursor_exists(
        &self,
        query: RoleAssignmentListQuery,
        assignment_id: RoleAssignmentId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM role_assignments AS assignment
                  JOIN principals AS principal
                    ON principal.id = assignment.principal_id
                   AND principal.organization_id = assignment.organization_id
                 WHERE assignment.organization_id = $1
                   AND assignment.id = $2
                   AND ($3::uuid IS NULL OR assignment.principal_id = $3)
                   AND (
                        $4
                        OR (
                            assignment.revoked_at IS NULL
                            AND assignment.valid_from <= $5
                            AND (
                                assignment.valid_until IS NULL
                                OR assignment.valid_until > $5
                            )
                            AND principal.status = 'active'
                        )
                   )
            )
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .bind(query.principal_id.map(PrincipalId::as_uuid))
        .bind(query.include_inactive)
        .bind(query.now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_role_assignments(
        &self,
        query: RoleAssignmentListQuery,
    ) -> Result<Vec<RoleAssignmentSummary>, StoreError> {
        let rows = sqlx::query_as::<_, RoleAssignmentSummaryRow>(
            r#"
            WITH eligible AS (
                SELECT assignment.id, assignment.organization_id,
                       assignment.principal_id,
                       principal.kind AS principal_kind,
                       principal.status AS principal_status,
                       principal.display_name AS principal_display_name,
                       assignment.role_id, role.system_key,
                       role.display_name AS role_display_name,
                       assignment.created_by,
                       grant_record.approval_request_id,
                       assignment.valid_from, assignment.valid_until,
                       assignment.revoked_at, assignment.created_at
                  FROM role_assignments AS assignment
                  JOIN principals AS principal
                    ON principal.id = assignment.principal_id
                   AND principal.organization_id = assignment.organization_id
                  JOIN roles AS role
                    ON role.id = assignment.role_id
                   AND role.organization_id = assignment.organization_id
             LEFT JOIN role_assignment_grants AS grant_record
                    ON grant_record.assignment_id = assignment.id
                   AND grant_record.organization_id = assignment.organization_id
                 WHERE assignment.organization_id = $1
                   AND ($2::uuid IS NULL OR assignment.principal_id = $2)
                   AND (
                        $3
                        OR (
                            assignment.revoked_at IS NULL
                            AND assignment.valid_from <= $5
                            AND (
                                assignment.valid_until IS NULL
                                OR assignment.valid_until > $5
                            )
                            AND principal.status = 'active'
                        )
                   )
            )
            SELECT *
              FROM eligible
             WHERE (
                $4::uuid IS NULL
                OR (created_at, id) < (
                    SELECT anchor.created_at, anchor.id
                      FROM eligible AS anchor
                     WHERE anchor.id = $4
                )
             )
             ORDER BY created_at DESC, id DESC
             LIMIT $6
            "#,
        )
        .bind(query.organization_id.as_uuid())
        .bind(query.principal_id.map(PrincipalId::as_uuid))
        .bind(query.include_inactive)
        .bind(query.after_assignment_id.map(RoleAssignmentId::as_uuid))
        .bind(query.now)
        .bind(query.limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(role_assignment_summary_from_row)
            .collect()
    }
}

#[async_trait]
impl AuditExportQueryStore for PgControlPlaneStore {
    async fn get_audit_export_grant(
        &self,
        organization_id: OrganizationId,
        grant_id: AuditExportId,
    ) -> Result<Option<AuditExportGrant>, StoreError> {
        let row = sqlx::query_as::<_, AuditExportGrantRow>(
            r#"
            SELECT id, organization_id, archive_id, approval_request_id,
                   requested_by, created_at, expires_at
              FROM audit_export_grants
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(grant_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(audit_export_grant_from_row).transpose()
    }

    async fn get_audit_export_grant_by_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<AuditExportGrant>, StoreError> {
        let row = sqlx::query_as::<_, AuditExportGrantRow>(
            r#"
            SELECT id, organization_id, archive_id, approval_request_id,
                   requested_by, created_at, expires_at
              FROM audit_export_grants
             WHERE organization_id = $1 AND approval_request_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(audit_export_grant_from_row).transpose()
    }
}

#[async_trait]
impl RetentionPolicyQueryStore for PgControlPlaneStore {
    async fn get_retention_policy(
        &self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyDetailRow>(
            r#"
            SELECT
                   policy.id AS policy_id,
                   policy.organization_id,
                   policy.policy_key,
                   policy.display_name,
                   policy.resource_kind,
                   policy.created_by,
                   policy.created_at AS policy_created_at,
                   latest.id AS latest_id,
                   latest.version AS latest_version,
                   latest.minimum_retention_days AS latest_minimum_retention_days,
                   latest.archive_after_days AS latest_archive_after_days,
                   latest.delete_after_days AS latest_delete_after_days,
                   latest.rule AS latest_rule,
                   latest.created_by AS latest_created_by,
                   latest.created_at AS latest_created_at,
                   active.id AS active_id,
                   active.version AS active_version,
                   active.minimum_retention_days AS active_minimum_retention_days,
                   active.archive_after_days AS active_archive_after_days,
                   active.delete_after_days AS active_delete_after_days,
                   active.rule AS active_rule,
                   active.created_by AS active_created_by,
                   active.created_at AS active_created_at
              FROM retention_policies AS policy
              JOIN LATERAL (
                  SELECT version.*
                    FROM retention_policy_versions AS version
                   WHERE version.organization_id = policy.organization_id
                     AND version.policy_id = policy.id
                   ORDER BY version.version DESC
                   LIMIT 1
              ) AS latest ON true
              LEFT JOIN retention_policy_versions AS active
                ON active.organization_id = policy.organization_id
               AND active.policy_id = policy.id
               AND active.id = policy.active_version_id
             WHERE policy.organization_id = $1
               AND policy.id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        retention_policy_detail_from_row(row).map(Some)
    }
}

#[async_trait]
impl ComplianceQueryStore for PgControlPlaneStore {
    async fn active_human_compliance_actor_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn get_compliance_case(
        &self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            external_reference: String,
            title: String,
            purpose: String,
            status: String,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            closed_by: Option<Uuid>,
            closed_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, external_reference, title, purpose,
                   status, created_by, created_at, closed_by, closed_at
              FROM compliance_cases
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            ComplianceCase::from_persisted(
                ComplianceCaseId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                row.external_reference,
                row.title,
                row.purpose,
                parse_compliance_case_status(&row.status)?,
                PrincipalId::from_uuid(row.created_by),
                row.created_at,
                row.closed_by.map(PrincipalId::from_uuid),
                row.closed_at,
            )
            .map_err(|error| {
                tracing::error!(%error, "invalid compliance case persisted");
                StoreError::Unavailable
            })
        })
        .transpose()
    }

    async fn compliance_case_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<ComplianceCaseStatus>,
        case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM compliance_cases
                 WHERE organization_id = $1
                   AND id = $2
                   AND ($3::text IS NULL OR status = $3)
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .bind(status.map(compliance_case_status))
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_compliance_cases(
        &self,
        organization_id: OrganizationId,
        status: Option<ComplianceCaseStatus>,
        after_case_id: Option<ComplianceCaseId>,
        limit: i64,
    ) -> Result<Vec<ComplianceCase>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            external_reference: String,
            title: String,
            purpose: String,
            status: String,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            closed_by: Option<Uuid>,
            closed_at: Option<DateTime<Utc>>,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, external_reference, title, purpose,
                   status, created_by, created_at, closed_by, closed_at
              FROM compliance_cases
             WHERE organization_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND (
                   $3::uuid IS NULL
                   OR (created_at, id) < (
                       SELECT anchor.created_at, anchor.id
                         FROM compliance_cases AS anchor
                        WHERE anchor.organization_id = $1
                          AND anchor.id = $3
                          AND ($2::text IS NULL OR anchor.status = $2)
                   )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT $4
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(status.map(compliance_case_status))
        .bind(after_case_id.map(ComplianceCaseId::as_uuid))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(|row| {
                ComplianceCase::from_persisted(
                    ComplianceCaseId::from_uuid(row.id),
                    OrganizationId::from_uuid(row.organization_id),
                    row.external_reference,
                    row.title,
                    row.purpose,
                    parse_compliance_case_status(&row.status)?,
                    PrincipalId::from_uuid(row.created_by),
                    row.created_at,
                    row.closed_by.map(PrincipalId::from_uuid),
                    row.closed_at,
                )
                .map_err(|error| {
                    tracing::error!(%error, "invalid compliance case persisted");
                    StoreError::Unavailable
                })
            })
            .collect()
    }

    async fn get_audit_archive(
        &self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            checkpoint_id: Uuid,
            provider: String,
            bucket: String,
            events_object_key: String,
            events_version_id: String,
            events_sha256: String,
            events_size_bytes: i64,
            manifest_object_key: String,
            manifest_version_id: String,
            manifest_sha256: String,
            manifest_size_bytes: i64,
            retention_mode: String,
            retain_until: DateTime<Utc>,
            verified_at: DateTime<Utc>,
            created_by: Uuid,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, checkpoint_id, provider, bucket,
                   events_object_key, events_version_id, events_sha256,
                   events_size_bytes, manifest_object_key, manifest_version_id,
                   manifest_sha256, manifest_size_bytes, retention_mode,
                   retain_until, verified_at, created_by, created_at
              FROM audit_archives
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(archive_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(AuditArchiveRecord {
                archive_id: AuditArchiveId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                checkpoint_id: AuditCheckpointId::from_uuid(row.checkpoint_id),
                provider: row.provider,
                bucket: row.bucket,
                events: ArchivedObjectRecord {
                    key: row.events_object_key,
                    version_id: row.events_version_id,
                    sha256_hex: row.events_sha256,
                    size_bytes: u64::try_from(row.events_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                manifest: ArchivedObjectRecord {
                    key: row.manifest_object_key,
                    version_id: row.manifest_version_id,
                    sha256_hex: row.manifest_sha256,
                    size_bytes: u64::try_from(row.manifest_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                retention_mode: row.retention_mode,
                retain_until: row.retain_until,
                verified_at: row.verified_at,
                created_by: PrincipalId::from_uuid(row.created_by),
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    async fn get_legal_hold(
        &self,
        organization_id: OrganizationId,
        hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            case_id: Uuid,
            archive_id: Uuid,
            reason: String,
            status: String,
            apply_approval_request_id: Uuid,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            release_approval_request_id: Option<Uuid>,
            released_by: Option<Uuid>,
            released_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, case_id, archive_id, reason, status,
                   apply_approval_request_id, created_by, created_at,
                   release_approval_request_id, released_by, released_at
              FROM legal_holds
             WHERE organization_id = $1 AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(hold_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            LegalHold::from_persisted(
                LegalHoldId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                ComplianceCaseId::from_uuid(row.case_id),
                AuditArchiveId::from_uuid(row.archive_id),
                row.reason,
                parse_legal_hold_status(&row.status)?,
                copilotmail_domain::ApprovalRequestId::from_uuid(row.apply_approval_request_id),
                PrincipalId::from_uuid(row.created_by),
                row.created_at,
                row.release_approval_request_id
                    .map(copilotmail_domain::ApprovalRequestId::from_uuid),
                row.released_by.map(PrincipalId::from_uuid),
                row.released_at,
            )
            .map_err(|error| {
                tracing::error!(%error, "invalid legal hold persisted");
                StoreError::Unavailable
            })
        })
        .transpose()
    }

    async fn legal_hold_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        hold_id: LegalHoldId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM legal_holds
                 WHERE organization_id = $1
                   AND id = $2
                   AND ($3::text IS NULL OR status = $3)
                   AND ($4::uuid IS NULL OR case_id = $4)
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(hold_id.as_uuid())
        .bind(status.map(legal_hold_status))
        .bind(case_id.map(ComplianceCaseId::as_uuid))
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }

    async fn list_legal_holds(
        &self,
        organization_id: OrganizationId,
        status: Option<LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        after_hold_id: Option<LegalHoldId>,
        limit: i64,
    ) -> Result<Vec<LegalHold>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            case_id: Uuid,
            archive_id: Uuid,
            reason: String,
            status: String,
            apply_approval_request_id: Uuid,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            release_approval_request_id: Option<Uuid>,
            released_by: Option<Uuid>,
            released_at: Option<DateTime<Utc>>,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, case_id, archive_id, reason, status,
                   apply_approval_request_id, created_by, created_at,
                   release_approval_request_id, released_by, released_at
              FROM legal_holds
             WHERE organization_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::uuid IS NULL OR case_id = $3)
               AND (
                   $4::uuid IS NULL
                   OR (created_at, id) < (
                       SELECT anchor.created_at, anchor.id
                         FROM legal_holds AS anchor
                        WHERE anchor.organization_id = $1
                          AND anchor.id = $4
                          AND ($2::text IS NULL OR anchor.status = $2)
                          AND ($3::uuid IS NULL OR anchor.case_id = $3)
                   )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT $5
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(status.map(legal_hold_status))
        .bind(case_id.map(ComplianceCaseId::as_uuid))
        .bind(after_hold_id.map(LegalHoldId::as_uuid))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        rows.into_iter()
            .map(|row| {
                LegalHold::from_persisted(
                    LegalHoldId::from_uuid(row.id),
                    OrganizationId::from_uuid(row.organization_id),
                    ComplianceCaseId::from_uuid(row.case_id),
                    AuditArchiveId::from_uuid(row.archive_id),
                    row.reason,
                    parse_legal_hold_status(&row.status)?,
                    copilotmail_domain::ApprovalRequestId::from_uuid(row.apply_approval_request_id),
                    PrincipalId::from_uuid(row.created_by),
                    row.created_at,
                    row.release_approval_request_id
                        .map(copilotmail_domain::ApprovalRequestId::from_uuid),
                    row.released_by.map(PrincipalId::from_uuid),
                    row.released_at,
                )
                .map_err(|error| {
                    tracing::error!(%error, "invalid legal hold persisted");
                    StoreError::Unavailable
                })
            })
            .collect()
    }

    async fn get_legal_hold_by_apply_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError> {
        let hold_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
              FROM legal_holds
             WHERE organization_id = $1
               AND apply_approval_request_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        match hold_id {
            Some(hold_id) => {
                self.get_legal_hold(organization_id, LegalHoldId::from_uuid(hold_id))
                    .await
            }
            None => Ok(None),
        }
    }
}

#[async_trait]
impl OrganizationQueryStore for PgControlPlaneStore {
    async fn get_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<OrganizationSummary>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct OrganizationRow {
            id: Uuid,
            slug: String,
            display_name: String,
        }

        sqlx::query_as::<_, OrganizationRow>(
            "SELECT id, slug, display_name FROM organizations WHERE id = $1",
        )
        .bind(organization_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)
        .map(|row| {
            row.map(|row| OrganizationSummary {
                organization_id: OrganizationId::from_uuid(row.id),
                slug: row.slug,
                display_name: row.display_name,
            })
        })
    }
}

#[async_trait]
impl ExternalIdentityResolver for PgControlPlaneStore {
    async fn resolve_human_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<PrincipalId>, StoreError> {
        let principal_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT principal.id
              FROM human_profiles AS profile
              JOIN principals AS principal ON principal.id = profile.principal_id
             WHERE profile.identity_provider = $1
               AND profile.identity_subject = $2
               AND principal.kind = 'human'
               AND principal.status = 'active'
            "#,
        )
        .bind(issuer)
        .bind(subject)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(principal_id.map(PrincipalId::from_uuid))
    }
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

#[async_trait]
impl ScimDirectoryQueryStore for PgControlPlaneStore {
    async fn authenticate_scim_token(
        &self,
        token_hash: &[u8],
        now: DateTime<Utc>,
    ) -> Result<Option<ScimCredential>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct CredentialRow {
            id: Uuid,
            organization_id: Uuid,
            actor_principal_id: Uuid,
            label: String,
            expires_at: Option<DateTime<Utc>>,
        }
        if token_hash.len() != 32 {
            return Ok(None);
        }
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            UPDATE scim_tokens AS token
               SET last_used_at = $2
              FROM principals AS actor
             WHERE token.token_hash = $1
               AND token.status = 'active'
               AND (token.expires_at IS NULL OR token.expires_at > $2)
               AND actor.id = token.actor_principal_id
               AND actor.organization_id = token.organization_id
               AND actor.status = 'active'
            RETURNING token.id, token.organization_id, token.actor_principal_id,
                      token.label, token.expires_at
            "#,
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ScimCredential {
            token_id: ScimTokenId::from_uuid(row.id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            actor_principal_id: PrincipalId::from_uuid(row.actor_principal_id),
            label: row.label,
            expires_at: row.expires_at,
        }))
    }

    async fn get_scim_user(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<ExistingScimUser>, StoreError> {
        let row = sqlx::query_as::<_, ScimUserRow>(
            r#"
            SELECT directory.organization_id,
                   directory.principal_id,
                   directory.external_id,
                   directory.user_name,
                   principal.display_name,
                   principal.status = 'active' AS active,
                   directory.version,
                   directory.created_at,
                   directory.updated_at
              FROM scim_users AS directory
              JOIN principals AS principal ON principal.id = directory.principal_id
             WHERE directory.organization_id = $1
               AND directory.principal_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(principal_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_user_from_row))
    }

    async fn list_scim_users(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        user_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimUser>, i64), StoreError> {
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
              FROM scim_users
             WHERE organization_id = $1
               AND ($2::text IS NULL OR external_id = $2)
               AND ($3::text IS NULL OR lower(user_name) = lower($3))
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(user_name)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)?;
        let rows = sqlx::query_as::<_, ScimUserRow>(
            r#"
            SELECT directory.organization_id,
                   directory.principal_id,
                   directory.external_id,
                   directory.user_name,
                   principal.display_name,
                   principal.status = 'active' AS active,
                   directory.version,
                   directory.created_at,
                   directory.updated_at
              FROM scim_users AS directory
              JOIN principals AS principal ON principal.id = directory.principal_id
             WHERE directory.organization_id = $1
               AND ($2::text IS NULL OR directory.external_id = $2)
               AND ($3::text IS NULL OR lower(directory.user_name) = lower($3))
             ORDER BY directory.created_at, directory.principal_id
             OFFSET $4 LIMIT $5
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(user_name)
        .bind(offset)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok((rows.into_iter().map(scim_user_from_row).collect(), total))
    }

    async fn get_scim_group(
        &self,
        organization_id: OrganizationId,
        group_id: ScimGroupId,
    ) -> Result<Option<ExistingScimGroup>, StoreError> {
        let row = sqlx::query_as::<_, ScimGroupRow>(
            r#"
            SELECT directory.organization_id,
                   directory.id AS group_id,
                   directory.external_id,
                   directory.display_name,
                   COALESCE(
                     array_agg(member.principal_id ORDER BY member.principal_id)
                       FILTER (WHERE member.principal_id IS NOT NULL),
                     ARRAY[]::uuid[]
                   ) AS members,
                   directory.version,
                   directory.created_at,
                   directory.updated_at
              FROM scim_groups AS directory
              LEFT JOIN scim_group_members AS member ON member.group_id = directory.id
             WHERE directory.organization_id = $1
               AND directory.id = $2
             GROUP BY directory.id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(group_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_group_from_row))
    }

    async fn list_scim_groups(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        display_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimGroup>, i64), StoreError> {
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
              FROM scim_groups
             WHERE organization_id = $1
               AND ($2::text IS NULL OR external_id = $2)
               AND ($3::text IS NULL OR lower(display_name) = lower($3))
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(display_name)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)?;
        let rows = sqlx::query_as::<_, ScimGroupRow>(
            r#"
            WITH page AS (
                SELECT *
                  FROM scim_groups
                 WHERE organization_id = $1
                   AND ($2::text IS NULL OR external_id = $2)
                   AND ($3::text IS NULL OR lower(display_name) = lower($3))
                 ORDER BY created_at, id
                 OFFSET $4 LIMIT $5
            )
            SELECT page.organization_id,
                   page.id AS group_id,
                   page.external_id,
                   page.display_name,
                   COALESCE(
                     array_agg(member.principal_id ORDER BY member.principal_id)
                       FILTER (WHERE member.principal_id IS NOT NULL),
                     ARRAY[]::uuid[]
                   ) AS members,
                   page.version,
                   page.created_at,
                   page.updated_at
              FROM page
              LEFT JOIN scim_group_members AS member ON member.group_id = page.id
             GROUP BY page.id, page.organization_id, page.external_id,
                      page.display_name, page.version, page.created_at, page.updated_at
             ORDER BY page.created_at, page.id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .bind(display_name)
        .bind(offset)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok((rows.into_iter().map(scim_group_from_row).collect(), total))
    }
}

#[async_trait]
impl AgentCredentialStore for PgControlPlaneStore {
    async fn get_agent_credential(
        &self,
        client_id: PrincipalId,
        credential_id: CredentialId,
        now: DateTime<Utc>,
    ) -> Result<Option<ExistingAgentCredential>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct CredentialRow {
            credential_id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            public_key_pem: String,
            allowed_scopes: Vec<String>,
            expires_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            SELECT credential.id AS credential_id,
                   principal.organization_id,
                   principal.id AS principal_id,
                   key.public_key_pem,
                   key.allowed_scopes,
                   credential.expires_at
              FROM credentials AS credential
              JOIN agent_private_key_credentials AS key
                ON key.credential_id = credential.id
              JOIN principals AS principal ON principal.id = credential.principal_id
             WHERE credential.id = $1
               AND credential.principal_id = $2
               AND credential.kind = 'private_key_jwt'
               AND credential.status = 'active'
               AND credential.expires_at > $3
               AND principal.kind = 'agent'
               AND principal.status = 'active'
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(client_id.as_uuid())
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingAgentCredential {
            credential_id: CredentialId::from_uuid(row.credential_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            public_key_pem: row.public_key_pem,
            allowed_scopes: row
                .allowed_scopes
                .into_iter()
                .filter_map(|scope| permission_from_str(&scope))
                .collect(),
            expires_at: row.expires_at,
        }))
    }

    async fn consume_agent_assertion(
        &self,
        credential_id: CredentialId,
        jti_hash: &[u8],
        assertion_expires_at: DateTime<Utc>,
        source: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        if jti_hash.len() != 32
            || source.is_some_and(|source| {
                source.is_empty() || source.len() > 200 || source.chars().any(char::is_control)
            })
            || assertion_expires_at <= now
        {
            return Ok(false);
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        let active: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT true
              FROM credentials
             WHERE id = $1
               AND status = 'active'
               AND expires_at > $2
             FOR UPDATE
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if active.is_none() {
            return Ok(false);
        }
        let inserted = sqlx::query(
            r#"
            INSERT INTO oauth_client_assertion_jtis(
                credential_id, jti_hash, expires_at, created_at
            )
            VALUES ($1, $2, $3, $4)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(jti_hash)
        .bind(assertion_expires_at)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        if inserted.rows_affected() != 1 {
            return Ok(false);
        }
        sqlx::query(
            r#"
            UPDATE credentials
               SET last_used_at = $2,
                   last_used_source = $3
             WHERE id = $1
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(now)
        .bind(source)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(true)
    }

    async fn authenticate_agent_mtls(
        &self,
        client_id: PrincipalId,
        certificate_fingerprint: &str,
        source: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Option<ExistingAgentMtlsCredential>, StoreError> {
        if certificate_fingerprint.len() != 64
            || !certificate_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || source.is_some_and(|source| {
                source.is_empty() || source.len() > 200 || source.chars().any(char::is_control)
            })
        {
            return Ok(None);
        }
        #[derive(sqlx::FromRow)]
        struct CredentialRow {
            credential_id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            certificate_fingerprint: String,
            allowed_scopes: Vec<String>,
            expires_at: DateTime<Utc>,
        }
        let mut transaction = self.pool.begin().await.map_err(map_store_error)?;
        let row = sqlx::query_as::<_, CredentialRow>(
            r#"
            SELECT credential.id AS credential_id,
                   principal.organization_id,
                   principal.id AS principal_id,
                   credential.public_fingerprint AS certificate_fingerprint,
                   mtls.allowed_scopes,
                   credential.expires_at
              FROM credentials AS credential
              JOIN agent_mtls_credentials AS mtls
                ON mtls.credential_id = credential.id
              JOIN principals AS principal ON principal.id = credential.principal_id
             WHERE credential.principal_id = $1
               AND credential.public_fingerprint = $2
               AND credential.kind = 'mtls'
               AND credential.status = 'active'
               AND credential.expires_at > $3
               AND principal.kind = 'agent'
               AND principal.status = 'active'
             FOR UPDATE OF credential
            "#,
        )
        .bind(client_id.as_uuid())
        .bind(certificate_fingerprint.to_ascii_lowercase())
        .bind(now)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        sqlx::query(
            r#"
            UPDATE credentials
               SET last_used_at = $2,
                   last_used_source = $3
             WHERE id = $1
            "#,
        )
        .bind(row.credential_id)
        .bind(now)
        .bind(source)
        .execute(&mut *transaction)
        .await
        .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(Some(ExistingAgentMtlsCredential {
            credential_id: CredentialId::from_uuid(row.credential_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            certificate_fingerprint: row.certificate_fingerprint,
            allowed_scopes: row
                .allowed_scopes
                .into_iter()
                .filter_map(|scope| permission_from_str(&scope))
                .collect(),
            expires_at: row.expires_at,
        }))
    }

    async fn validate_agent_token_binding(
        &self,
        principal_id: PrincipalId,
        credential_id: CredentialId,
        scopes: &BTreeSet<Permission>,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        if scopes.is_empty()
            || !scopes
                .iter()
                .all(|scope| Permission::AGENT_TOKEN.contains(scope))
        {
            return Ok(false);
        }
        let requested = scopes
            .iter()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>();
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM credentials AS credential
                  JOIN principals AS principal ON principal.id = credential.principal_id
                 WHERE credential.id = $1
                   AND credential.principal_id = $2
                   AND credential.kind IN ('private_key_jwt', 'mtls')
                   AND credential.status = 'active'
                   AND credential.expires_at > $4
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM agent_private_key_credentials AS key
                            WHERE key.credential_id = credential.id
                              AND key.allowed_scopes @> $3::text[]
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM agent_mtls_credentials AS mtls
                            WHERE mtls.credential_id = credential.id
                              AND mtls.allowed_scopes @> $3::text[]
                       )
                   )
            )
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(requested)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_store_error)
    }
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
}

impl PgControlPlaneTransaction {
    fn transaction(&mut self) -> Result<&mut Transaction<'static, Postgres>, StoreError> {
        self.transaction.as_mut().ok_or(StoreError::Unavailable)
    }
}

#[async_trait]
impl ControlPlaneStore for PgControlPlaneStore {
    async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
        let transaction = self.pool.begin().await.map_err(map_store_error)?;
        Ok(Box::new(PgControlPlaneTransaction {
            transaction: Some(transaction),
        }))
    }
}

#[async_trait]
impl MailEventIngestionStore for PgControlPlaneStore {
    async fn begin_mail_event_ingestion(
        &self,
    ) -> Result<Box<dyn MailEventIngestionTransaction>, StoreError> {
        let transaction = self.pool.begin().await.map_err(map_store_error)?;
        Ok(Box::new(PgControlPlaneTransaction {
            transaction: Some(transaction),
        }))
    }
}

#[async_trait]
impl ControlPlaneTransaction for PgControlPlaneTransaction {
    async fn find_organization_bootstrap(
        &mut self,
        slug: &OrganizationSlug,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(slug.as_str())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct ExistingRow {
            organization_id: Uuid,
            root_principal_id: Uuid,
            slug: String,
            display_name: String,
        }

        let existing = sqlx::query_as::<_, ExistingRow>(
            r#"
            SELECT organization.id AS organization_id,
                   bootstrap.root_principal_id,
                   organization.slug,
                   organization.display_name
              FROM organizations AS organization
              JOIN organization_bootstraps AS bootstrap
                ON bootstrap.organization_id = organization.id
             WHERE organization.slug = $1
            "#,
        )
        .bind(slug.as_str())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        Ok(existing.map(|row| ExistingOrganizationBootstrap {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            root_principal_id: PrincipalId::from_uuid(row.root_principal_id),
            slug: row.slug,
            display_name: row.display_name,
        }))
    }

    async fn insert_organization(&mut self, organization: &Organization) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO organizations(id, slug, display_name, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(organization.id().as_uuid())
        .bind(organization.slug().as_str())
        .bind(organization.display_name())
        .bind(organization.created_at())
        .bind(organization.updated_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_principal(&mut self, principal: &Principal) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO principals(
                id, organization_id, kind, status, display_name, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(principal.id().as_uuid())
        .bind(principal.organization_id().as_uuid())
        .bind(principal_kind(principal.kind()))
        .bind(principal_status(principal.status()))
        .bind(principal.display_name())
        .bind(principal.created_at())
        .bind(principal.updated_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_role(&mut self, role: &Role) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO roles(id, organization_id, system_key, display_name, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(role.id().as_uuid())
        .bind(role.organization_id().as_uuid())
        .bind(role.system_role().as_str())
        .bind(role.display_name())
        .bind(role.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        for permission in role.permissions() {
            sqlx::query("INSERT INTO role_permissions(role_id, permission) VALUES ($1, $2)")
                .bind(role.id().as_uuid())
                .bind(permission.as_str())
                .execute(&mut **self.transaction()?)
                .await
                .map_err(map_store_error)?;
        }
        Ok(())
    }

    async fn insert_role_assignment(
        &mut self,
        assignment: &RoleAssignment,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO role_assignments(
                id, organization_id, principal_id, role_id, created_by,
                valid_from, valid_until, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(assignment.id().as_uuid())
        .bind(assignment.organization_id().as_uuid())
        .bind(assignment.principal_id().as_uuid())
        .bind(assignment.role_id().as_uuid())
        .bind(assignment.created_by().as_uuid())
        .bind(assignment.valid_from())
        .bind(assignment.valid_until())
        .bind(assignment.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_role_assignment_grant(
        &mut self,
        assignment: &RoleAssignment,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO role_assignment_grants(
                assignment_id, organization_id, approval_request_id,
                granted_by, granted_at
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(assignment.id().as_uuid())
        .bind(assignment.organization_id().as_uuid())
        .bind(approval_request_id.as_uuid())
        .bind(assignment.created_by().as_uuid())
        .bind(assignment.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_role_assignment_grant_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<RoleAssignmentGrantRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            assignment_id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            role_id: Uuid,
            approval_request_id: Uuid,
            granted_by: Uuid,
            valid_from: DateTime<Utc>,
            valid_until: DateTime<Utc>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT grant_record.assignment_id, grant_record.organization_id,
                   assignment.principal_id, assignment.role_id,
                   grant_record.approval_request_id, grant_record.granted_by,
                   assignment.valid_from, assignment.valid_until
              FROM role_assignment_grants AS grant_record
              JOIN role_assignments AS assignment
                ON assignment.id = grant_record.assignment_id
               AND assignment.organization_id = grant_record.organization_id
             WHERE grant_record.organization_id = $1
               AND grant_record.approval_request_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| RoleAssignmentGrantRecord {
            assignment_id: RoleAssignmentId::from_uuid(row.assignment_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            role_id: RoleId::from_uuid(row.role_id),
            approval_request_id: copilotmail_domain::ApprovalRequestId::from_uuid(
                row.approval_request_id,
            ),
            granted_by: PrincipalId::from_uuid(row.granted_by),
            valid_from: row.valid_from,
            valid_until: row.valid_until,
        }))
    }

    async fn revoke_governed_role_assignment(
        &mut self,
        organization_id: OrganizationId,
        assignment_id: RoleAssignmentId,
        revoked_at: DateTime<Utc>,
    ) -> Result<RoleAssignmentRevocationResult, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 7))")
            .bind(organization_id.to_string())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct Row {
            system_key: String,
            valid_from: DateTime<Utc>,
            valid_until: Option<DateTime<Utc>>,
            revoked_at: Option<DateTime<Utc>>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT role.system_key, assignment.valid_from,
                   assignment.valid_until, assignment.revoked_at
              FROM role_assignments AS assignment
              JOIN roles AS role
                ON role.id = assignment.role_id
               AND role.organization_id = assignment.organization_id
             WHERE assignment.organization_id = $1
               AND assignment.id = $2
             FOR UPDATE OF assignment
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(RoleAssignmentRevocationResult::NotFound);
        };
        if row.revoked_at.is_some() {
            return Ok(RoleAssignmentRevocationResult::AlreadyRevoked);
        }
        if row.system_key == "org_owner"
            && row.valid_from <= revoked_at
            && row.valid_until.is_none_or(|until| until > revoked_at)
        {
            let other_owner_count = sqlx::query_scalar::<_, i64>(
                r#"
                SELECT count(*)
                  FROM role_assignments AS assignment
                  JOIN roles AS role
                    ON role.id = assignment.role_id
                   AND role.organization_id = assignment.organization_id
                  JOIN principals AS principal
                    ON principal.id = assignment.principal_id
                   AND principal.organization_id = assignment.organization_id
                 WHERE assignment.organization_id = $1
                   AND assignment.id <> $2
                   AND role.system_key = 'org_owner'
                   AND assignment.revoked_at IS NULL
                   AND assignment.valid_from <= $3
                   AND (
                        assignment.valid_until IS NULL
                        OR assignment.valid_until > $3
                   )
                   AND principal.status = 'active'
                "#,
            )
            .bind(organization_id.as_uuid())
            .bind(assignment_id.as_uuid())
            .bind(revoked_at)
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
            if other_owner_count == 0 {
                return Ok(RoleAssignmentRevocationResult::LastActiveOwner);
            }
        }

        let changed = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE role_assignments
               SET revoked_at = $3
             WHERE organization_id = $1
               AND id = $2
               AND revoked_at IS NULL
            RETURNING 1
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(assignment_id.as_uuid())
        .bind(revoked_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(if changed.is_some() {
            RoleAssignmentRevocationResult::Revoked
        } else {
            RoleAssignmentRevocationResult::AlreadyRevoked
        })
    }

    async fn record_organization_bootstrap(
        &mut self,
        organization_id: OrganizationId,
        root_principal_id: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO organization_bootstraps(
                organization_id, root_principal_id, created_at
            )
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(root_principal_id.as_uuid())
        .bind(created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_oidc_owner_binding(
        &mut self,
        organization_id: OrganizationId,
    ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 1))")
            .bind(organization_id.to_string())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct BindingRow {
            organization_id: Uuid,
            principal_id: Uuid,
            identity_provider: String,
            identity_subject: String,
            display_name: String,
        }
        let row = sqlx::query_as::<_, BindingRow>(
            r#"
            SELECT binding.organization_id,
                   binding.principal_id,
                   binding.identity_provider,
                   binding.identity_subject,
                   principal.display_name
              FROM oidc_owner_bindings AS binding
              JOIN principals AS principal ON principal.id = binding.principal_id
             WHERE binding.organization_id = $1
            "#,
        )
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingOidcOwnerBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            issuer: row.identity_provider,
            subject: row.identity_subject,
            display_name: row.display_name,
        }))
    }

    async fn find_organization_bootstrap_by_id(
        &mut self,
        organization_id: OrganizationId,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct BootstrapRow {
            organization_id: Uuid,
            root_principal_id: Uuid,
            slug: String,
            display_name: String,
        }
        let row = sqlx::query_as::<_, BootstrapRow>(
            r#"
            SELECT organization.id AS organization_id,
                   bootstrap.root_principal_id,
                   organization.slug,
                   organization.display_name
              FROM organization_bootstraps AS bootstrap
              JOIN organizations AS organization
                ON organization.id = bootstrap.organization_id
             WHERE organization.id = $1
            "#,
        )
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingOrganizationBootstrap {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            root_principal_id: PrincipalId::from_uuid(row.root_principal_id),
            slug: row.slug,
            display_name: row.display_name,
        }))
    }

    async fn find_system_role(
        &mut self,
        organization_id: OrganizationId,
        system_role: SystemRole,
    ) -> Result<Option<RoleId>, StoreError> {
        let role_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM roles WHERE organization_id = $1 AND system_key = $2",
        )
        .bind(organization_id.as_uuid())
        .bind(system_role.as_str())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(role_id.map(RoleId::from_uuid))
    }

    async fn insert_human_profile(&mut self, profile: &HumanProfile) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO human_profiles(
                principal_id, identity_provider, identity_subject
            )
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(profile.principal_id().as_uuid())
        .bind(profile.identity_provider())
        .bind(profile.identity_subject())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn record_oidc_owner_binding(
        &mut self,
        binding: &ExistingOidcOwnerBinding,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO oidc_owner_bindings(
                organization_id, principal_id, identity_provider,
                identity_subject, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(binding.organization_id.as_uuid())
        .bind(binding.principal_id.as_uuid())
        .bind(&binding.issuer)
        .bind(&binding.subject)
        .bind(created_by.as_uuid())
        .bind(created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn retire_principal(
        &mut self,
        principal_id: PrincipalId,
        retired_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE principals
               SET status = 'retired',
                   updated_at = $2
             WHERE id = $1
               AND kind = 'service'
               AND status <> 'retired'
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(retired_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(StoreError::MissingReference)
        }
    }

    async fn revoke_active_role_assignments(
        &mut self,
        principal_id: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            UPDATE role_assignments
               SET revoked_at = $2
             WHERE principal_id = $1
               AND revoked_at IS NULL
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_scim_token(
        &mut self,
        credential: &ScimCredential,
        token_hash: &[u8],
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO scim_tokens(
                id, organization_id, actor_principal_id, token_hash,
                label, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(credential.token_id.as_uuid())
        .bind(credential.organization_id.as_uuid())
        .bind(credential.actor_principal_id.as_uuid())
        .bind(token_hash)
        .bind(&credential.label)
        .bind(created_at)
        .bind(credential.expires_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn revoke_scim_token(
        &mut self,
        organization_id: OrganizationId,
        token_id: ScimTokenId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE scim_tokens
               SET status = 'revoked',
                   revoked_at = $3
             WHERE id = $1
               AND organization_id = $2
               AND status = 'active'
            "#,
        )
        .bind(token_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn find_scim_user_by_external_id(
        &mut self,
        organization_id: OrganizationId,
        external_id: &str,
    ) -> Result<Option<ExistingScimUser>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 2))")
            .bind(format!("{organization_id}:{external_id}"))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        let row = sqlx::query_as::<_, ScimUserRow>(
            r#"
            SELECT directory.organization_id,
                   directory.principal_id,
                   directory.external_id,
                   directory.user_name,
                   principal.display_name,
                   principal.status = 'active' AS active,
                   directory.version,
                   directory.created_at,
                   directory.updated_at
              FROM scim_users AS directory
              JOIN principals AS principal ON principal.id = directory.principal_id
             WHERE directory.organization_id = $1
               AND directory.external_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_user_from_row))
    }

    async fn insert_scim_user(&mut self, user: &ExistingScimUser) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO scim_users(
                principal_id, organization_id, external_id, user_name,
                version, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.external_id)
        .bind(&user.user_name)
        .bind(user.version)
        .bind(user.created_at)
        .bind(user.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_human_identity(
        &mut self,
        organization_id: OrganizationId,
        identity_provider: &str,
        external_id: &str,
    ) -> Result<Option<ExistingHumanIdentity>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct HumanRow {
            organization_id: Uuid,
            principal_id: Uuid,
            display_name: String,
            active: bool,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, HumanRow>(
            r#"
            SELECT principal.organization_id,
                   principal.id AS principal_id,
                   principal.display_name,
                   principal.status = 'active' AS active,
                   principal.created_at
              FROM human_profiles AS profile
              JOIN principals AS principal ON principal.id = profile.principal_id
             WHERE principal.organization_id = $1
               AND principal.kind = 'human'
               AND principal.status <> 'retired'
               AND profile.identity_provider = $2
               AND profile.identity_subject = $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(identity_provider)
        .bind(external_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| ExistingHumanIdentity {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            principal_id: PrincipalId::from_uuid(row.principal_id),
            display_name: row.display_name,
            active: row.active,
            created_at: row.created_at,
        }))
    }

    async fn adopt_scim_user(&mut self, user: &ExistingScimUser) -> Result<(), StoreError> {
        let status = if user.active { "active" } else { "suspended" };
        let result = sqlx::query(
            r#"
            UPDATE principals
               SET display_name = $3,
                   status = $4,
                   updated_at = $5
             WHERE id = $1
               AND organization_id = $2
               AND kind = 'human'
               AND status <> 'retired'
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.display_name)
        .bind(status)
        .bind(user.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if result.rows_affected() != 1 {
            return Err(StoreError::MissingReference);
        }
        self.insert_scim_user(user).await
    }

    async fn replace_scim_user(
        &mut self,
        user: &ExistingScimUser,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        let status = if user.active { "active" } else { "suspended" };
        let updated = sqlx::query(
            r#"
            UPDATE scim_users
               SET user_name = $3,
                   version = $4,
                   updated_at = $5
             WHERE principal_id = $1
               AND organization_id = $2
               AND version = $6
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.user_name)
        .bind(user.version)
        .bind(user.updated_at)
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if updated.rows_affected() != 1 {
            return Ok(false);
        }
        let principal = sqlx::query(
            r#"
            UPDATE principals
               SET display_name = $3,
                   status = $4,
                   updated_at = $5
             WHERE id = $1
               AND organization_id = $2
               AND kind = 'human'
               AND status <> 'retired'
            "#,
        )
        .bind(user.principal_id.as_uuid())
        .bind(user.organization_id.as_uuid())
        .bind(&user.display_name)
        .bind(status)
        .bind(user.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if principal.rows_affected() != 1 {
            return Err(StoreError::MissingReference);
        }
        Ok(true)
    }

    async fn find_scim_group_by_external_id(
        &mut self,
        organization_id: OrganizationId,
        external_id: &str,
    ) -> Result<Option<ExistingScimGroup>, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 3))")
            .bind(format!("{organization_id}:{external_id}"))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        let row = sqlx::query_as::<_, ScimGroupRow>(
            r#"
            SELECT directory.organization_id,
                   directory.id AS group_id,
                   directory.external_id,
                   directory.display_name,
                   COALESCE(
                     array_agg(member.principal_id ORDER BY member.principal_id)
                       FILTER (WHERE member.principal_id IS NOT NULL),
                     ARRAY[]::uuid[]
                   ) AS members,
                   directory.version,
                   directory.created_at,
                   directory.updated_at
              FROM scim_groups AS directory
              LEFT JOIN scim_group_members AS member ON member.group_id = directory.id
             WHERE directory.organization_id = $1
               AND directory.external_id = $2
             GROUP BY directory.id
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(external_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(scim_group_from_row))
    }

    async fn insert_scim_group(&mut self, group: &ExistingScimGroup) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO scim_groups(
                id, organization_id, external_id, display_name,
                version, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(group.group_id.as_uuid())
        .bind(group.organization_id.as_uuid())
        .bind(&group.external_id)
        .bind(&group.display_name)
        .bind(group.version)
        .bind(group.created_at)
        .bind(group.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        insert_scim_group_members(self.transaction()?, group).await
    }

    async fn replace_scim_group(
        &mut self,
        group: &ExistingScimGroup,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE scim_groups
               SET display_name = $3,
                   version = $4,
                   updated_at = $5
             WHERE id = $1
               AND organization_id = $2
               AND version = $6
            "#,
        )
        .bind(group.group_id.as_uuid())
        .bind(group.organization_id.as_uuid())
        .bind(&group.display_name)
        .bind(group.version)
        .bind(group.updated_at)
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if result.rows_affected() != 1 {
            return Ok(false);
        }
        sqlx::query("DELETE FROM scim_group_members WHERE group_id = $1")
            .bind(group.group_id.as_uuid())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        insert_scim_group_members(self.transaction()?, group).await?;
        Ok(true)
    }

    async fn delete_scim_group(
        &mut self,
        organization_id: OrganizationId,
        group_id: ScimGroupId,
        expected_version: Option<i64>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            DELETE FROM scim_groups
             WHERE id = $1
               AND organization_id = $2
               AND ($3::bigint IS NULL OR version = $3)
            "#,
        )
        .bind(group_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn insert_agent_profile(&mut self, profile: &AgentProfile) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO agent_profiles(
                principal_id, owner_principal_id, environment, risk_level,
                purpose, review_due_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(profile.principal_id().as_uuid())
        .bind(profile.owner_principal_id().as_uuid())
        .bind(agent_environment(profile.environment()))
        .bind(agent_risk_level(profile.risk_level()))
        .bind(profile.purpose())
        .bind(profile.review_due_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_agent_credential(
        &mut self,
        credential: &AgentCredential,
        public_key_pem: &str,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO credentials(
                id, principal_id, kind, public_fingerprint, status,
                created_at, expires_at
            )
            VALUES ($1, $2, 'private_key_jwt', $3, 'active', $4, $5)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(credential.principal_id().as_uuid())
        .bind(credential.fingerprint())
        .bind(credential.created_at())
        .bind(credential.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let scopes = credential
            .allowed_scopes()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>();
        sqlx::query(
            r#"
            INSERT INTO agent_private_key_credentials(
                credential_id, public_key_pem, allowed_scopes
            )
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(public_key_pem)
        .bind(scopes)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_agent_mtls_credential(
        &mut self,
        credential: &AgentMtlsCredential,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO credentials(
                id, principal_id, kind, public_fingerprint, status,
                created_at, expires_at
            )
            VALUES ($1, $2, 'mtls', $3, 'active', $4, $5)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(credential.principal_id().as_uuid())
        .bind(credential.certificate_fingerprint())
        .bind(credential.created_at())
        .bind(credential.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let scopes = credential
            .allowed_scopes()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>();
        sqlx::query(
            r#"
            INSERT INTO agent_mtls_credentials(credential_id, allowed_scopes)
            VALUES ($1, $2)
            "#,
        )
        .bind(credential.id().as_uuid())
        .bind(scopes)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn active_agent_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'agent'
                   AND status = 'active'
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn revoke_agent_credential(
        &mut self,
        organization_id: OrganizationId,
        credential_id: CredentialId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE credentials AS credential
               SET status = 'revoked',
                   revoked_at = $3
              FROM principals AS principal
             WHERE credential.id = $1
               AND credential.status = 'active'
               AND principal.id = credential.principal_id
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
            "#,
        )
        .bind(credential_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn active_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND status = 'active'
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn active_agent_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'agent'
                   AND status = 'active'
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn acquire_agent_event_stream_lease(
        &mut self,
        lease: &AgentEventStreamLease,
        tenant_limit: u32,
        principal_limit: u32,
    ) -> Result<AgentEventStreamLeaseAcquisition, StoreError> {
        if tenant_limit == 0 || principal_limit == 0 || principal_limit > tenant_limit {
            return Err(StoreError::Conflict);
        }
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::uuid::text, 29))")
            .bind(lease.organization_id.as_uuid())
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        sqlx::query(
            r#"
            DELETE FROM agent_event_stream_leases
             WHERE organization_id = $1
               AND expires_at <= $2
            "#,
        )
        .bind(lease.organization_id.as_uuid())
        .bind(lease.opened_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let (tenant_count, principal_count) = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT count(*)::bigint,
                   count(*) FILTER (WHERE principal_id = $2)::bigint
              FROM agent_event_stream_leases
             WHERE organization_id = $1
               AND expires_at > $3
            "#,
        )
        .bind(lease.organization_id.as_uuid())
        .bind(lease.principal_id.as_uuid())
        .bind(lease.opened_at)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if tenant_count >= i64::from(tenant_limit) {
            return Ok(AgentEventStreamLeaseAcquisition::TenantCapacityReached);
        }
        if principal_count >= i64::from(principal_limit) {
            return Ok(AgentEventStreamLeaseAcquisition::PrincipalCapacityReached);
        }
        sqlx::query(
            r#"
            INSERT INTO agent_event_stream_leases(
                id, organization_id, principal_id, subscription_id,
                opened_at, expires_at
            )
            VALUES($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(lease.lease_id)
        .bind(lease.organization_id.as_uuid())
        .bind(lease.principal_id.as_uuid())
        .bind(lease.subscription_id.as_uuid())
        .bind(lease.opened_at)
        .bind(lease.expires_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(AgentEventStreamLeaseAcquisition::Acquired(lease.clone()))
    }

    async fn active_human_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM principals
                 WHERE id = $1
                   AND organization_id = $2
                   AND kind = 'human'
                   AND status = 'active'
            )
            "#,
        )
        .bind(principal_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn human_draft_list_still_authorized(
        &mut self,
        proof: &HumanDraftListRecheck,
    ) -> Result<bool, StoreError> {
        if proof.expected_drafts.len() > 50
            || proof
                .expected_drafts
                .iter()
                .any(|draft| draft.draft_id.as_uuid().is_nil() || draft.expected_version <= 0)
        {
            return Ok(false);
        }
        let mut unique_draft_ids = BTreeSet::new();
        if proof
            .expected_drafts
            .iter()
            .any(|draft| !unique_draft_ids.insert(draft.draft_id))
        {
            return Ok(false);
        }
        let expected_draft_ids = proof
            .expected_drafts
            .iter()
            .map(|draft| draft.draft_id.as_uuid())
            .collect::<Vec<_>>();
        let expected_versions = proof
            .expected_drafts
            .iter()
            .map(|draft| draft.expected_version)
            .collect::<Vec<_>>();

        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals AS principal
                  JOIN mailboxes AS mailbox
                    ON mailbox.organization_id = principal.organization_id
                   AND mailbox.id = $3
                  JOIN mail_engine_mailbox_bindings AS binding
                    ON binding.organization_id = mailbox.organization_id
                   AND binding.mailbox_id = mailbox.id
                   AND binding.mail_data_plane_id = $4
                   AND binding.account_ref = $5
                 WHERE principal.id = $2
                   AND principal.organization_id = $1
                   AND principal.kind = 'human'
                   AND principal.status = 'active'
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
                          AND assignment.valid_from <= CURRENT_TIMESTAMP
                          AND (
                              assignment.valid_until IS NULL
                              OR assignment.valid_until > CURRENT_TIMESTAMP
                          )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.send' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND NOT EXISTS (
                       SELECT 1
                         FROM unnest($6::uuid[], $7::bigint[])
                              AS expected(draft_id, expected_version)
                         LEFT JOIN mail_drafts AS draft
                           ON draft.id = expected.draft_id
                          AND draft.organization_id = $1
                          AND draft.mailbox_id = $3
                          AND draft.created_by = $2
                          AND draft.mail_data_plane_id = $4
                          AND draft.engine_account_ref = $5
                          AND draft.status = 'active'
                          AND draft.version = expected.expected_version
                        WHERE draft.id IS NULL
                   )
            )
            "#,
        )
        .bind(proof.organization_id.as_uuid())
        .bind(proof.principal_id.as_uuid())
        .bind(proof.mailbox_id.as_uuid())
        .bind(proof.expected_data_plane_id.as_uuid())
        .bind(&proof.expected_engine_account_ref)
        .bind(expected_draft_ids.as_slice())
        .bind(expected_versions.as_slice())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn human_draft_read_still_authorized(
        &mut self,
        proof: &HumanDraftReadRecheck,
    ) -> Result<bool, StoreError> {
        if proof.expected_version <= 0 || !(1..=100).contains(&proof.expected_recipients.len()) {
            return Ok(false);
        }
        let recipient_refs = proof
            .expected_recipients
            .iter()
            .map(|recipient| recipient.recipient_ref.to_vec())
            .collect::<Vec<_>>();
        let recipient_domains = proof
            .expected_recipients
            .iter()
            .map(|recipient| recipient.domain.clone())
            .collect::<Vec<_>>();
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM principals AS principal
                  JOIN mailboxes AS mailbox
                    ON mailbox.organization_id = principal.organization_id
                   AND mailbox.id = $3
                   AND mailbox.primary_address = $10
                  JOIN mail_engine_mailbox_bindings AS binding
                    ON binding.organization_id = mailbox.organization_id
                   AND binding.mailbox_id = mailbox.id
                   AND binding.mail_data_plane_id = $6
                   AND binding.account_ref = $7
                  JOIN mail_drafts AS draft
                    ON draft.organization_id = mailbox.organization_id
                   AND draft.mailbox_id = mailbox.id
                   AND draft.id = $4
                   AND draft.created_by = principal.id
                   AND draft.mail_data_plane_id = binding.mail_data_plane_id
                   AND draft.engine_account_ref = binding.account_ref
                 WHERE principal.id = $2
                   AND principal.organization_id = $1
                   AND principal.kind = 'human'
                   AND principal.status = 'active'
                   AND draft.status = 'active'
                   AND draft.version = $5
                   AND draft.engine_email_ref = $8
                   AND draft.engine_thread_ref = $9
                   AND draft.body_ref = $11
                   AND draft.content_sha256 = $12
                   AND draft.recipient_refs = $13
                   AND draft.recipient_domains = $14
                   AND cardinality(draft.recipient_refs) BETWEEN 1 AND 100
                   AND cardinality(draft.recipient_refs)
                       = cardinality(draft.recipient_domains)
                   AND draft.reply_to_message_id IS NOT DISTINCT FROM $15
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
                          AND assignment.valid_from <= CURRENT_TIMESTAMP
                          AND (
                              assignment.valid_until IS NULL
                              OR assignment.valid_until > CURRENT_TIMESTAMP
                          )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = $3
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= CURRENT_TIMESTAMP
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > CURRENT_TIMESTAMP
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = $3
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.send' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= CURRENT_TIMESTAMP
                              AND delegation.valid_until > CURRENT_TIMESTAMP
                       )
                   )
            )
            "#,
        )
        .bind(proof.organization_id.as_uuid())
        .bind(proof.principal_id.as_uuid())
        .bind(proof.mailbox_id.as_uuid())
        .bind(proof.draft_id.as_uuid())
        .bind(proof.expected_version)
        .bind(proof.expected_data_plane_id.as_uuid())
        .bind(&proof.expected_engine_account_ref)
        .bind(&proof.expected_engine_email_ref)
        .bind(&proof.expected_engine_thread_ref)
        .bind(proof.expected_primary_address.as_str())
        .bind(proof.expected_body_ref.as_slice())
        .bind(proof.expected_content_sha256.as_slice())
        .bind(&recipient_refs)
        .bind(&recipient_domains)
        .bind(proof.expected_reply_to_message_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn insert_human_session(&mut self, write: &HumanSessionWrite) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO human_browser_sessions(
                id, organization_id, principal_id, login_transaction_id,
                client_key, token_sha256, oidc_token_sha256, oidc_jti,
                upstream_expires_at, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(write.session_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(write.login_transaction_id.as_uuid())
        .bind(human_session_client(write.client_key))
        .bind(write.token_sha256.as_slice())
        .bind(write.oidc_token_sha256.as_slice())
        .bind(write.oidc_jti.as_deref())
        .bind(write.upstream_expires_at)
        .bind(write.created_at)
        .bind(write.expires_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn revoke_human_session_by_token_sha256(
        &mut self,
        token_sha256: &[u8; 32],
        revocation_id: HumanSessionRevocationId,
        cause: HumanSessionRevocationCause,
        revoked_at: DateTime<Utc>,
    ) -> Result<Option<HumanSessionRevocationResult>, StoreError> {
        let session = sqlx::query_as::<_, HumanSessionRow>(
            r#"
            SELECT id, organization_id, principal_id, login_transaction_id,
                   client_key, oidc_jti, upstream_expires_at,
                   created_at, expires_at
              FROM human_browser_sessions
             WHERE token_sha256 = $1
             FOR UPDATE
            "#,
        )
        .bind(token_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(session) = session else {
            return Ok(None);
        };
        let session = human_session_from_row(session)?;

        #[derive(sqlx::FromRow)]
        struct RevocationRow {
            id: Uuid,
            cause: String,
            revoked_at: DateTime<Utc>,
        }

        if let Some(existing) = sqlx::query_as::<_, RevocationRow>(
            r#"
            SELECT id, cause, revoked_at
              FROM human_browser_session_revocations
             WHERE session_id = $1
            "#,
        )
        .bind(session.session_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        {
            return Ok(Some(HumanSessionRevocationResult {
                session,
                revocation_id: HumanSessionRevocationId::from_uuid(existing.id),
                cause: human_session_revocation_cause_from_str(&existing.cause)?,
                revoked_at: existing.revoked_at,
                newly_revoked: false,
            }));
        }

        sqlx::query(
            r#"
            INSERT INTO human_browser_session_revocations(
                id, session_id, organization_id, principal_id, cause, revoked_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(revocation_id.as_uuid())
        .bind(session.session_id.as_uuid())
        .bind(session.organization_id.as_uuid())
        .bind(session.principal_id.as_uuid())
        .bind(human_session_revocation_cause(cause))
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(Some(HumanSessionRevocationResult {
            session,
            revocation_id,
            cause,
            revoked_at,
            newly_revoked: true,
        }))
    }

    async fn insert_webhook_subscription(
        &mut self,
        subscription: &WebhookSubscription,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO webhook_subscriptions(
                id, organization_id, principal_id, endpoint_url, event_types,
                signing_key_reference, enabled, version, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
            "#,
        )
        .bind(subscription.id.as_uuid())
        .bind(subscription.organization_id.as_uuid())
        .bind(subscription.principal_id.as_uuid())
        .bind(&subscription.endpoint_url)
        .bind(webhook_event_type_names(&subscription.event_types))
        .bind(&subscription.signing_key_reference)
        .bind(subscription.enabled)
        .bind(subscription.version)
        .bind(subscription.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_webhook_subscription_for_update(
        &mut self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscription>, StoreError> {
        let row = sqlx::query_as::<_, WebhookSubscriptionRow>(
            r#"
            SELECT id AS subscription_id, organization_id, principal_id,
                   endpoint_url, event_types, signing_key_reference, enabled,
                   version, created_at, updated_at
              FROM webhook_subscriptions
             WHERE id = $1 AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(webhook_subscription_from_row).transpose()
    }

    async fn update_webhook_subscription(
        &mut self,
        subscription: &WebhookSubscription,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE webhook_subscriptions
               SET endpoint_url = $3,
                   event_types = $4,
                   signing_key_reference = $5,
                   enabled = $6,
                   version = $7,
                   updated_at = $8
             WHERE id = $1
               AND organization_id = $2
               AND version = $9
            "#,
        )
        .bind(subscription.id.as_uuid())
        .bind(subscription.organization_id.as_uuid())
        .bind(&subscription.endpoint_url)
        .bind(webhook_event_type_names(&subscription.event_types))
        .bind(&subscription.signing_key_reference)
        .bind(subscription.enabled)
        .bind(subscription.version)
        .bind(subscription.updated_at)
        .bind(expected_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn replay_dead_lettered_webhook_delivery(
        &mut self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        event_id: Uuid,
        requested_at: DateTime<Utc>,
    ) -> Result<Option<i32>, StoreError> {
        let transaction = self.transaction()?;
        let latest_attempt = sqlx::query_scalar::<_, i32>(
            r#"
            SELECT delivery.attempt
              FROM webhook_subscriptions AS subscription
              JOIN outbox_events AS outbox
                ON outbox.id = $3
               AND outbox.payload ->> 'organization_id' =
                   subscription.organization_id::text
              JOIN LATERAL (
                    SELECT attempt, status
                      FROM webhook_deliveries
                     WHERE subscription_id = subscription.id
                       AND event_id = $3
                     ORDER BY attempt DESC
                     LIMIT 1
              ) AS delivery ON true
             WHERE subscription.id = $1
               AND subscription.organization_id = $2
               AND subscription.enabled
               AND delivery.status = 'dead_lettered'
             FOR UPDATE OF subscription, outbox
            "#,
        )
        .bind(subscription_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(event_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(map_store_error)?;
        let Some(latest_attempt) = latest_attempt else {
            return Ok(None);
        };
        let replay_attempt = latest_attempt.checked_add(1).ok_or(StoreError::Conflict)?;
        sqlx::query(
            r#"
            INSERT INTO webhook_deliveries(
                id, subscription_id, event_id, attempt, status,
                response_status, next_attempt_at, error_code, delivered_at,
                created_at, updated_at
            )
            VALUES(
                $1, $2, $3, $4, 'retrying',
                NULL, $5, 'manual_replay_requested', NULL, $5, $5
            )
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(subscription_id.as_uuid())
        .bind(event_id)
        .bind(replay_attempt)
        .bind(requested_at)
        .execute(&mut **transaction)
        .await
        .map_err(map_store_error)?;
        let outbox = sqlx::query(
            r#"
            UPDATE outbox_events
               SET published_at = NULL,
                   locked_at = NULL,
                   locked_by = NULL,
                   next_attempt_at = $2,
                   last_error = NULL
             WHERE id = $1
            "#,
        )
        .bind(event_id)
        .bind(requested_at)
        .execute(&mut **transaction)
        .await
        .map_err(map_store_error)?;
        if outbox.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(Some(replay_attempt))
    }

    async fn insert_mail_domain(
        &mut self,
        domain: &MailDomain,
        created_by: PrincipalId,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mail_domains(
                id, organization_id, name, status, route_mode,
                ownership_token_sha256, ownership_challenge_version,
                mail_engine_domain_ref, created_by,
                created_at, updated_at, verified_at, activated_at, suspended_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, $8, $9, $9, $10, $11, $12)
            "#,
        )
        .bind(domain.id.as_uuid())
        .bind(domain.organization_id.as_uuid())
        .bind(domain.name.as_str())
        .bind(mail_domain_status(domain.status))
        .bind(mail_domain_route_mode(domain.route_mode))
        .bind(domain.ownership_challenge_sha256.as_slice())
        .bind(domain.ownership_challenge_version)
        .bind(created_by.as_uuid())
        .bind(domain.created_at)
        .bind(domain.verified_at)
        .bind(domain.activated_at)
        .bind(domain.suspended_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_mail_domain_route(
        &mut self,
        route: &MailDomainRoute,
        updated_by: PrincipalId,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mail_domain_routes(
                mail_domain_id, organization_id, version,
                inbound_enabled, outbound_enabled, smart_host_ref,
                dkim_key_ref, updated_by, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(route.mail_domain_id.as_uuid())
        .bind(route.organization_id.as_uuid())
        .bind(route.version)
        .bind(route.inbound_enabled)
        .bind(route.outbound_enabled)
        .bind(&route.smart_host_ref)
        .bind(&route.dkim_key_ref)
        .bind(updated_by.as_uuid())
        .bind(route.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_mail_domain_for_update(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomain>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            name: String,
            status: String,
            route_mode: String,
            ownership_token_sha256: Vec<u8>,
            ownership_challenge_version: i64,
            created_at: DateTime<Utc>,
            verified_at: Option<DateTime<Utc>>,
            activated_at: Option<DateTime<Utc>>,
            suspended_at: Option<DateTime<Utc>>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, name, status, route_mode,
                   ownership_token_sha256, ownership_challenge_version,
                   created_at, verified_at,
                   activated_at, suspended_at
              FROM mail_domains
             WHERE id = $1 AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        row.map(|row| {
            Ok(MailDomain {
                id: MailDomainId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                name: MailDomainName::parse(row.name).map_err(|error| {
                    tracing::error!(%error, "invalid mail domain name persisted");
                    StoreError::Unavailable
                })?,
                status: mail_domain_status_from_str(&row.status)?,
                route_mode: mail_domain_route_mode_from_str(&row.route_mode)?,
                ownership_challenge_sha256: sha256_array(
                    &row.ownership_token_sha256,
                    "mail domain ownership digest",
                )?,
                ownership_challenge_version: row.ownership_challenge_version,
                created_at: row.created_at,
                verified_at: row.verified_at,
                activated_at: row.activated_at,
                suspended_at: row.suspended_at,
            })
        })
        .transpose()
    }

    async fn update_mail_domain(
        &mut self,
        domain: &MailDomain,
        updated_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mail_domains
               SET status = $3,
                   updated_at = $4,
                   verified_at = $5,
                   activated_at = $6,
                   suspended_at = $7,
                   ownership_token_sha256 = $8,
                   ownership_challenge_version = $9,
                   mail_engine_projection_status = CASE
                       WHEN status <> $3 AND $3 IN ('active', 'suspended')
                           THEN 'pending'
                       ELSE mail_engine_projection_status
                   END,
                   mail_engine_projection_error_code = CASE
                       WHEN status <> $3 AND $3 IN ('active', 'suspended')
                           THEN NULL
                       ELSE mail_engine_projection_error_code
                   END
             WHERE id = $1 AND organization_id = $2
            "#,
        )
        .bind(domain.id.as_uuid())
        .bind(domain.organization_id.as_uuid())
        .bind(mail_domain_status(domain.status))
        .bind(updated_at)
        .bind(domain.verified_at)
        .bind(domain.activated_at)
        .bind(domain.suspended_at)
        .bind(domain.ownership_challenge_sha256.as_slice())
        .bind(domain.ownership_challenge_version)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn insert_mail_domain_dns_observation(
        &mut self,
        observation: &MailDomainDnsObservation,
    ) -> Result<(), StoreError> {
        let evidence = &observation.evidence;
        sqlx::query(
            r#"
            INSERT INTO mail_domain_dns_observations(
                id, organization_id, mail_domain_id, ownership_token_sha256,
                observed_at, valid_until, ownership_txt, mx_present,
                mx_hosts_public, spf_present, dkim_present, dmarc_present,
                raw_answers, failure_codes, created_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                $13, $14, $15
            )
            "#,
        )
        .bind(evidence.id.as_uuid())
        .bind(observation.organization_id.as_uuid())
        .bind(observation.mail_domain_id.as_uuid())
        .bind(evidence.ownership_challenge_sha256.as_slice())
        .bind(evidence.observed_at)
        .bind(evidence.valid_until)
        .bind(evidence.ownership_txt)
        .bind(evidence.mx_present)
        .bind(evidence.mx_hosts_public)
        .bind(evidence.spf_present)
        .bind(evidence.dkim_present)
        .bind(evidence.dmarc_present)
        .bind(&observation.raw_answers)
        .bind(&observation.failure_codes)
        .bind(observation.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn latest_mail_domain_dns_is_activation_ready(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        ownership_challenge_sha256: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let ready = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT ownership_txt
               AND mx_present
               AND mx_hosts_public
               AND spf_present
               AND dkim_present
               AND dmarc_present
               AND observed_at <= $4
               AND valid_until > $4
              FROM mail_domain_dns_observations
             WHERE organization_id = $1
               AND mail_domain_id = $2
               AND ownership_token_sha256 = $3
             ORDER BY observed_at DESC, created_at DESC, id DESC
             LIMIT 1
             FOR SHARE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mail_domain_id.as_uuid())
        .bind(ownership_challenge_sha256.as_slice())
        .bind(now)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(ready.unwrap_or(false))
    }

    async fn assign_mail_domain_data_plane(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        data_plane_id: MailDataPlaneId,
        assigned_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mail_domains AS domain
               SET mail_data_plane_id = COALESCE(domain.mail_data_plane_id, $3),
                   mail_data_plane_assigned_at = COALESCE(
                       domain.mail_data_plane_assigned_at,
                       $4
                   )
             WHERE domain.id = $1
               AND domain.organization_id = $2
               AND domain.status IN ('verified', 'active', 'suspended')
               AND (
                   (
                       domain.mail_data_plane_id IS NULL
                       AND domain.mail_data_plane_assigned_at IS NULL
                   )
                   OR (
                       domain.mail_data_plane_id = $3
                       AND domain.mail_data_plane_assigned_at IS NOT NULL
                   )
               )
               AND $4 >= domain.created_at
               AND EXISTS (
                   SELECT 1
                     FROM mail_data_planes AS plane
                    WHERE plane.id = $3
               )
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(data_plane_id.as_uuid())
        .bind(assigned_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn assigned_mail_domain_data_plane(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDataPlaneId>, StoreError> {
        sqlx::query_scalar::<_, Option<Uuid>>(
            r#"
            SELECT mail_data_plane_id
              FROM mail_domains
             WHERE id = $1
               AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
        .map(Option::flatten)
        .map(|value| value.map(MailDataPlaneId::from_uuid))
    }
    async fn find_mail_domain_route_for_update(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomainRoute>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            mail_domain_id: Uuid,
            organization_id: Uuid,
            version: i64,
            inbound_enabled: bool,
            outbound_enabled: bool,
            smart_host_ref: Option<String>,
            dkim_key_ref: Option<String>,
            updated_at: DateTime<Utc>,
        }

        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT mail_domain_id, organization_id, version,
                   inbound_enabled, outbound_enabled, smart_host_ref,
                   dkim_key_ref, updated_at
              FROM mail_domain_routes
             WHERE mail_domain_id = $1 AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(mail_domain_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        Ok(row.map(|row| MailDomainRoute {
            mail_domain_id: MailDomainId::from_uuid(row.mail_domain_id),
            organization_id: OrganizationId::from_uuid(row.organization_id),
            version: row.version,
            inbound_enabled: row.inbound_enabled,
            outbound_enabled: row.outbound_enabled,
            smart_host_ref: row.smart_host_ref,
            dkim_key_ref: row.dkim_key_ref,
            updated_at: row.updated_at,
        }))
    }

    async fn update_mail_domain_route(
        &mut self,
        route: &MailDomainRoute,
        expected_version: i64,
        updated_by: PrincipalId,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE mail_domain_routes
               SET version = $4,
                   inbound_enabled = $5,
                   outbound_enabled = $6,
                   smart_host_ref = $7,
                   dkim_key_ref = $8,
                   updated_by = $9,
                   updated_at = $10
             WHERE mail_domain_id = $1
               AND organization_id = $2
               AND version = $3
            "#,
        )
        .bind(route.mail_domain_id.as_uuid())
        .bind(route.organization_id.as_uuid())
        .bind(expected_version)
        .bind(route.version)
        .bind(route.inbound_enabled)
        .bind(route.outbound_enabled)
        .bind(&route.smart_host_ref)
        .bind(&route.dkim_key_ref)
        .bind(updated_by.as_uuid())
        .bind(route.updated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn insert_mailbox(&mut self, mailbox: &Mailbox) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mailboxes(id, organization_id, kind, primary_address, created_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(mailbox.id.as_uuid())
        .bind(mailbox.organization_id.as_uuid())
        .bind(mailbox_kind(mailbox.kind))
        .bind(mailbox.primary_address.as_str())
        .bind(mailbox.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn upsert_mail_engine_mailbox_binding(
        &mut self,
        write: &MailEngineMailboxBindingWrite,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            version: i64,
            updated_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            WITH updated AS (
                UPDATE mail_engine_mailbox_bindings
                   SET account_ref = $4,
                       version = version + 1,
                       updated_by = $6,
                       updated_at = $7
                 WHERE mailbox_id = $1
                   AND organization_id = $2
                   AND mail_data_plane_id = $3
                   AND version = $5
                   AND $5 > 0
                   AND $7 >= updated_at
                RETURNING organization_id, mailbox_id, mail_data_plane_id,
                          account_ref, version, updated_at
            ), inserted AS (
                INSERT INTO mail_engine_mailbox_bindings(
                    mailbox_id, organization_id, mail_data_plane_id, source, account_ref,
                    version, updated_by, created_at, updated_at
                )
                SELECT $1, $2, $3, 'stalwart', $4, 1, $6, $7, $7
                 WHERE $5 = 0
                   AND EXISTS (
                       SELECT 1
                         FROM mailboxes
                        WHERE id = $1
                          AND organization_id = $2
                   )
                   AND NOT EXISTS (SELECT 1 FROM updated)
                ON CONFLICT(mailbox_id) DO NOTHING
                RETURNING organization_id, mailbox_id, mail_data_plane_id,
                          account_ref, version, updated_at
            )
            SELECT organization_id, mailbox_id, mail_data_plane_id,
                   account_ref, version, updated_at
              FROM updated
            UNION ALL
            SELECT organization_id, mailbox_id, mail_data_plane_id,
                   account_ref, version, updated_at
              FROM inserted
            "#,
        )
        .bind(write.mailbox_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.data_plane_id.as_uuid())
        .bind(&write.account_ref)
        .bind(write.expected_version)
        .bind(write.updated_by.as_uuid())
        .bind(write.updated_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailEngineMailboxBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            account_ref: row.account_ref,
            version: row.version,
            updated_at: row.updated_at,
        }))
    }

    async fn tenant_mailbox_exists(
        &mut self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM mailboxes
                 WHERE id = $1
                   AND organization_id = $2
            )
            "#,
        )
        .bind(mailbox_id.as_uuid())
        .bind(organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn insert_mailbox_membership(
        &mut self,
        membership: &MailboxMembership,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO mailbox_memberships(
                organization_id, mailbox_id, principal_id, role,
                valid_from, valid_until, created_by
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(membership.organization_id().as_uuid())
        .bind(membership.mailbox_id().as_uuid())
        .bind(membership.principal_id().as_uuid())
        .bind(mailbox_membership_role(membership.role()))
        .bind(membership.valid_from())
        .bind(membership.valid_until())
        .bind(membership.created_by().as_uuid())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn mailbox_owned_by(
        &mut self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM mailbox_memberships AS membership
                  JOIN principals AS principal
                    ON principal.id = membership.principal_id
                   AND principal.organization_id = membership.organization_id
                   AND principal.status = 'active'
                 WHERE membership.organization_id = $1
                   AND membership.mailbox_id = $2
                   AND membership.principal_id = $3
                   AND membership.role = 'owner'
                   AND membership.valid_from <= $4
                   AND (
                       membership.valid_until IS NULL
                       OR membership.valid_until > $4
                   )
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(mailbox_id.as_uuid())
        .bind(principal_id.as_uuid())
        .bind(now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn insert_mailbox_delegation(
        &mut self,
        delegation: &MailboxDelegation,
    ) -> Result<(), StoreError> {
        let scopes = delegation
            .capabilities()
            .map(|capability| capability.as_str())
            .collect::<Vec<_>>();
        sqlx::query(
            r#"
            INSERT INTO delegations(
                id, organization_id, grantor_principal_id, grantee_principal_id,
                mailbox_id, scopes, reason, valid_from, valid_until, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(delegation.id().as_uuid())
        .bind(delegation.organization_id().as_uuid())
        .bind(delegation.grantor_principal_id().as_uuid())
        .bind(delegation.grantee_principal_id().as_uuid())
        .bind(delegation.mailbox_id().as_uuid())
        .bind(scopes)
        .bind(delegation.reason())
        .bind(delegation.valid_from())
        .bind(delegation.valid_until())
        .bind(delegation.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn revoke_mailbox_delegation(
        &mut self,
        organization_id: OrganizationId,
        delegation_id: DelegationId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE delegations
               SET revoked_at = $3
             WHERE id = $1
               AND organization_id = $2
               AND revoked_at IS NULL
            "#,
        )
        .bind(delegation_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(revoked_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mailbox_delegation_managed_by(
        &mut self,
        organization_id: OrganizationId,
        delegation_id: DelegationId,
        actor: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1
                  FROM delegations AS delegation
                  JOIN mailbox_memberships AS membership
                    ON membership.organization_id = delegation.organization_id
                   AND membership.mailbox_id = delegation.mailbox_id
                   AND membership.principal_id = $3
                   AND membership.role = 'owner'
                   AND membership.valid_from <= $4
                   AND (
                       membership.valid_until IS NULL
                       OR membership.valid_until > $4
                   )
                  JOIN principals AS principal
                    ON principal.id = membership.principal_id
                   AND principal.organization_id = membership.organization_id
                   AND principal.status = 'active'
                 WHERE delegation.id = $1
                   AND delegation.organization_id = $2
                   AND delegation.revoked_at IS NULL
            )
            "#,
        )
        .bind(delegation_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(actor.as_uuid())
        .bind(now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn insert_approval_policy(
        &mut self,
        policy_id: PolicyId,
        organization_id: OrganizationId,
        policy_key: &str,
        display_name: &str,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO policies(
                id, organization_id, policy_key, display_name, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(policy_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(policy_key)
        .bind(display_name)
        .bind(created_by.as_uuid())
        .bind(created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_approval_policy_version(
        &mut self,
        version: &ApprovalPolicyVersion,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO policy_versions(
                id, organization_id, policy_id, version, action,
                required_approvals, rule, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(version.id().as_uuid())
        .bind(version.organization_id().as_uuid())
        .bind(version.policy_id().as_uuid())
        .bind(i64::from(version.version()))
        .bind(version.action().as_str())
        .bind(i16::from(version.required_approvals()))
        .bind(version.rule())
        .bind(version.created_by().as_uuid())
        .bind(version.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn activate_approval_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: PolicyId,
        policy_version_id: PolicyVersionId,
        expected_active_version_id: Option<PolicyVersionId>,
    ) -> Result<bool, StoreError> {
        let result = sqlx::query(
            r#"
            UPDATE policies
               SET active_version_id = $3
             WHERE id = $1
               AND organization_id = $2
               AND retired_at IS NULL
               AND active_version_id IS NOT DISTINCT FROM $4
            "#,
        )
        .bind(policy_id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(policy_version_id.as_uuid())
        .bind(expected_active_version_id.map(PolicyVersionId::as_uuid))
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn find_active_approval_policy(
        &mut self,
        organization_id: OrganizationId,
        policy_key: &str,
    ) -> Result<Option<ActiveApprovalPolicy>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            policy_id: Uuid,
            policy_version_id: Uuid,
            version: i32,
            action: String,
            required_approvals: i16,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT policy.id AS policy_id,
                   version.id AS policy_version_id,
                   version.version,
                   version.action,
                   version.required_approvals
              FROM policies AS policy
              JOIN policy_versions AS version
                ON version.id = policy.active_version_id
               AND version.organization_id = policy.organization_id
               AND version.policy_id = policy.id
             WHERE policy.organization_id = $1
               AND policy.policy_key = $2
               AND policy.retired_at IS NULL
             FOR UPDATE OF policy
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_key)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let action = approval_action_from_str(&row.action).ok_or(StoreError::Unavailable)?;
            let required_approvals =
                u8::try_from(row.required_approvals).map_err(|_| StoreError::Unavailable)?;
            let version = u32::try_from(row.version).map_err(|_| StoreError::Unavailable)?;
            Ok(ActiveApprovalPolicy {
                policy_id: PolicyId::from_uuid(row.policy_id),
                policy_version_id: PolicyVersionId::from_uuid(row.policy_version_id),
                version,
                action,
                required_approvals,
            })
        })
        .transpose()
    }

    async fn insert_approval_request(
        &mut self,
        request: &ApprovalRequest,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO approval_requests(
                id, organization_id, policy_version_id, action,
                resource_kind, resource_id, requested_by, reason,
                ticket_reference, required_approvals, status, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            "#,
        )
        .bind(request.id().as_uuid())
        .bind(request.organization_id().as_uuid())
        .bind(request.policy_version_id().as_uuid())
        .bind(request.action().as_str())
        .bind(request.resource_kind())
        .bind(request.resource_id())
        .bind(request.requested_by().as_uuid())
        .bind(request.reason())
        .bind(request.ticket_reference())
        .bind(i16::from(request.required_approvals()))
        .bind(approval_status(request.status()))
        .bind(request.created_at())
        .bind(request.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn record_approval_decision(
        &mut self,
        decision_write: &ApprovalDecisionWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct RequestRow {
            requested_by: Uuid,
            required_approvals: i16,
            status: String,
            expires_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, RequestRow>(
            r#"
            SELECT requested_by, required_approvals, status, expires_at
              FROM approval_requests
             WHERE id = $1
               AND organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.requested_by == decision_write.approver.as_uuid()
            || row.status != "pending"
            || row.expires_at <= decision_write.decided_at
        {
            return Err(StoreError::Conflict);
        }
        sqlx::query(
            r#"
            INSERT INTO approval_steps(
                id, organization_id, approval_request_id,
                approver_principal_id, decision, reason, decided_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(decision_write.step_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.approver.as_uuid())
        .bind(approval_decision(decision_write.decision))
        .bind(&decision_write.reason)
        .bind(decision_write.decided_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let approval_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*)
              FROM approval_steps
             WHERE approval_request_id = $1
               AND organization_id = $2
               AND decision = 'approved'
            "#,
        )
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let status = if decision_write.decision == ApprovalDecision::Rejected {
            ApprovalStatus::Rejected
        } else if approval_count >= i64::from(row.required_approvals) {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Pending
        };
        sqlx::query(
            r#"
            UPDATE approval_requests
               SET status = $3,
                   completed_at = CASE WHEN $3 = 'pending' THEN NULL ELSE $4 END
             WHERE id = $1
               AND organization_id = $2
            "#,
        )
        .bind(decision_write.request_id.as_uuid())
        .bind(decision_write.organization_id.as_uuid())
        .bind(approval_status(status))
        .bind(decision_write.decided_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(Some(ApprovalDecisionRecord {
            status,
            approval_count: u8::try_from(approval_count).map_err(|_| StoreError::Unavailable)?,
        }))
    }

    async fn cancel_approval_request(
        &mut self,
        cancellation: &ApprovalCancellationWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct RequestRow {
            requested_by: Uuid,
            status: String,
            expires_at: DateTime<Utc>,
            approval_count: i64,
        }
        let row = sqlx::query_as::<_, RequestRow>(
            r#"
            SELECT request.requested_by, request.status, request.expires_at,
                   (
                       SELECT count(*)
                         FROM approval_steps AS step
                        WHERE step.organization_id = request.organization_id
                          AND step.approval_request_id = request.id
                          AND step.decision = 'approved'
                   ) AS approval_count
              FROM approval_requests AS request
             WHERE request.id = $1
               AND request.organization_id = $2
             FOR UPDATE
            "#,
        )
        .bind(cancellation.request_id.as_uuid())
        .bind(cancellation.organization_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.requested_by != cancellation.requested_by.as_uuid()
            || row.status != "pending"
            || row.expires_at <= cancellation.cancelled_at
        {
            return Err(StoreError::Conflict);
        }
        let updated = sqlx::query(
            r#"
            UPDATE approval_requests
               SET status = 'cancelled',
                   cancelled_at = $3
             WHERE id = $1
               AND organization_id = $2
               AND status = 'pending'
            "#,
        )
        .bind(cancellation.request_id.as_uuid())
        .bind(cancellation.organization_id.as_uuid())
        .bind(cancellation.cancelled_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if updated.rows_affected() != 1 {
            return Err(StoreError::Conflict);
        }
        Ok(Some(ApprovalDecisionRecord {
            status: ApprovalStatus::Cancelled,
            approval_count: u8::try_from(row.approval_count)
                .map_err(|_| StoreError::Unavailable)?,
        }))
    }

    async fn insert_audit_checkpoint(
        &mut self,
        checkpoint: &SignedAuditCheckpoint,
        archive_object_key: &str,
    ) -> Result<bool, StoreError> {
        let payload = &checkpoint.payload;
        let signed_payload = String::from_utf8(payload.canonical_bytes().map_err(|error| {
            tracing::error!(%error, "invalid signed audit checkpoint payload");
            StoreError::Unavailable
        })?)
        .map_err(|_| StoreError::Unavailable)?;
        let root_hash = decode_sha256(&payload.root_hash).map_err(|error| {
            tracing::error!(%error, "invalid audit checkpoint root hash");
            StoreError::Unavailable
        })?;
        let signature = checkpoint.signature_bytes().map_err(|error| {
            tracing::error!(%error, "invalid audit checkpoint signature encoding");
            StoreError::Unavailable
        })?;
        let inserted = sqlx::query_scalar::<_, i32>(
            r#"
            INSERT INTO audit_checkpoints(
                id, organization_id, period_start, period_end, root_hash,
                signature, key_id, object_key, created_at,
                chain_sequence_start, chain_sequence_end, event_count,
                first_event_id, last_event_id, hash_version,
                signature_algorithm, signed_payload, public_key_fingerprint
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9,
                $10, $11, $12, $13, $14, $15, $16, $17, $18
            )
            ON CONFLICT DO NOTHING
            RETURNING 1
            "#,
        )
        .bind(payload.checkpoint_id.as_uuid())
        .bind(payload.organization_id.as_uuid())
        .bind(payload.period_start)
        .bind(payload.period_end)
        .bind(root_hash)
        .bind(signature)
        .bind(&payload.key_id)
        .bind(archive_object_key)
        .bind(payload.created_at)
        .bind(payload.chain_sequence_start)
        .bind(payload.chain_sequence_end)
        .bind(i64::try_from(payload.event_count).map_err(|_| StoreError::Conflict)?)
        .bind(payload.first_event_id)
        .bind(payload.last_event_id)
        .bind(payload.hash_version)
        .bind(&checkpoint.signature_algorithm)
        .bind(signed_payload)
        .bind(&payload.public_key_sha256)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(inserted.is_some())
    }

    async fn find_audit_checkpoint(
        &mut self,
        organization_id: OrganizationId,
        checkpoint_id: AuditCheckpointId,
    ) -> Result<Option<ExistingAuditCheckpoint>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            signed_payload: String,
            signature_algorithm: String,
            signature: Vec<u8>,
            object_key: String,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT signed_payload, signature_algorithm, signature, object_key
              FROM audit_checkpoints
             WHERE organization_id = $1
               AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(checkpoint_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let checkpoint = SignedAuditCheckpoint::from_storage(
                &row.signed_payload,
                row.signature_algorithm,
                &row.signature,
            )
            .map_err(|error| {
                tracing::error!(%error, "stored audit checkpoint is invalid");
                StoreError::Unavailable
            })?;
            Ok(ExistingAuditCheckpoint {
                checkpoint,
                archive_object_key: row.object_key,
            })
        })
        .transpose()
    }

    async fn insert_audit_archive(
        &mut self,
        archive: &AuditArchiveRecord,
    ) -> Result<bool, StoreError> {
        let events_size =
            i64::try_from(archive.events.size_bytes).map_err(|_| StoreError::Conflict)?;
        let manifest_size =
            i64::try_from(archive.manifest.size_bytes).map_err(|_| StoreError::Conflict)?;
        let inserted = sqlx::query_scalar::<_, i32>(
            r#"
            INSERT INTO audit_archives(
                id, organization_id, checkpoint_id, provider, bucket,
                events_object_key, events_version_id, events_sha256,
                events_size_bytes, manifest_object_key, manifest_version_id,
                manifest_sha256, manifest_size_bytes, retention_mode,
                retain_until, verified_at, created_by, created_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9,
                $10, $11, $12, $13, $14, $15, $16, $17, $18
            )
            ON CONFLICT DO NOTHING
            RETURNING 1
            "#,
        )
        .bind(archive.archive_id.as_uuid())
        .bind(archive.organization_id.as_uuid())
        .bind(archive.checkpoint_id.as_uuid())
        .bind(&archive.provider)
        .bind(&archive.bucket)
        .bind(&archive.events.key)
        .bind(&archive.events.version_id)
        .bind(&archive.events.sha256_hex)
        .bind(events_size)
        .bind(&archive.manifest.key)
        .bind(&archive.manifest.version_id)
        .bind(&archive.manifest.sha256_hex)
        .bind(manifest_size)
        .bind(&archive.retention_mode)
        .bind(archive.retain_until)
        .bind(archive.verified_at)
        .bind(archive.created_by.as_uuid())
        .bind(archive.created_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(inserted.is_some())
    }

    async fn find_audit_archive(
        &mut self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            checkpoint_id: Uuid,
            provider: String,
            bucket: String,
            events_object_key: String,
            events_version_id: String,
            events_sha256: String,
            events_size_bytes: i64,
            manifest_object_key: String,
            manifest_version_id: String,
            manifest_sha256: String,
            manifest_size_bytes: i64,
            retention_mode: String,
            retain_until: DateTime<Utc>,
            verified_at: DateTime<Utc>,
            created_by: Uuid,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, checkpoint_id, provider, bucket,
                   events_object_key, events_version_id, events_sha256,
                   events_size_bytes, manifest_object_key, manifest_version_id,
                   manifest_sha256, manifest_size_bytes, retention_mode,
                   retain_until, verified_at, created_by, created_at
              FROM audit_archives
             WHERE organization_id = $1
               AND id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(archive_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            Ok(AuditArchiveRecord {
                archive_id: AuditArchiveId::from_uuid(row.id),
                organization_id: OrganizationId::from_uuid(row.organization_id),
                checkpoint_id: AuditCheckpointId::from_uuid(row.checkpoint_id),
                provider: row.provider,
                bucket: row.bucket,
                events: ArchivedObjectRecord {
                    key: row.events_object_key,
                    version_id: row.events_version_id,
                    sha256_hex: row.events_sha256,
                    size_bytes: u64::try_from(row.events_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                manifest: ArchivedObjectRecord {
                    key: row.manifest_object_key,
                    version_id: row.manifest_version_id,
                    sha256_hex: row.manifest_sha256,
                    size_bytes: u64::try_from(row.manifest_size_bytes)
                        .map_err(|_| StoreError::Unavailable)?,
                },
                retention_mode: row.retention_mode,
                retain_until: row.retain_until,
                verified_at: row.verified_at,
                created_by: PrincipalId::from_uuid(row.created_by),
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    async fn authorize_audit_export_grant(
        &mut self,
        grant: &AuditExportGrant,
    ) -> Result<AuditExportGrant, StoreError> {
        sqlx::query(
            r#"
            INSERT INTO audit_export_grants(
                id, organization_id, archive_id, approval_request_id,
                requested_by, created_at, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (approval_request_id) DO NOTHING
            "#,
        )
        .bind(grant.id().as_uuid())
        .bind(grant.organization_id().as_uuid())
        .bind(grant.archive_id().as_uuid())
        .bind(grant.approval_request_id().as_uuid())
        .bind(grant.requested_by().as_uuid())
        .bind(grant.created_at())
        .bind(grant.expires_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let row = sqlx::query_as::<_, AuditExportGrantRow>(
            r#"
            SELECT id, organization_id, archive_id, approval_request_id,
                   requested_by, created_at, expires_at
              FROM audit_export_grants
             WHERE organization_id = $1 AND approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(grant.organization_id().as_uuid())
        .bind(grant.approval_request_id().as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;
        audit_export_grant_from_row(row)
    }

    async fn record_audit_export_download(
        &mut self,
        download: &AuditExportDownloadRecord,
    ) -> Result<(), StoreError> {
        let size_bytes =
            i64::try_from(download.object.size_bytes).map_err(|_| StoreError::Conflict)?;
        sqlx::query(
            r#"
            INSERT INTO audit_export_downloads(
                id, organization_id, grant_id, archive_id, downloaded_by,
                artifact, object_key, object_version_id, sha256, size_bytes,
                downloaded_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(download.id.as_uuid())
        .bind(download.organization_id.as_uuid())
        .bind(download.grant_id.as_uuid())
        .bind(download.archive_id.as_uuid())
        .bind(download.downloaded_by.as_uuid())
        .bind(download.artifact.as_str())
        .bind(&download.object.key)
        .bind(&download.object.version_id)
        .bind(&download.object.sha256_hex)
        .bind(size_bytes)
        .bind(download.downloaded_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_retention_policy(
        &mut self,
        policy: &RetentionPolicy,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO retention_policies(
                id, organization_id, policy_key, display_name, resource_kind,
                active_version_id, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, NULL, $6, $7)
            "#,
        )
        .bind(policy.id().as_uuid())
        .bind(policy.organization_id().as_uuid())
        .bind(policy.policy_key())
        .bind(policy.display_name())
        .bind(policy.resource_kind().as_str())
        .bind(policy.created_by().as_uuid())
        .bind(policy.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn insert_retention_policy_version(
        &mut self,
        version: &RetentionPolicyVersion,
    ) -> Result<(), StoreError> {
        let version_number = i32::try_from(version.version()).map_err(|_| StoreError::Conflict)?;
        let minimum_retention_days =
            i32::try_from(version.minimum_retention_days()).map_err(|_| StoreError::Conflict)?;
        let archive_after_days = version
            .archive_after_days()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| StoreError::Conflict)?;
        let delete_after_days = version
            .delete_after_days()
            .map(i32::try_from)
            .transpose()
            .map_err(|_| StoreError::Conflict)?;
        sqlx::query(
            r#"
            INSERT INTO retention_policy_versions(
                id, organization_id, policy_id, version,
                minimum_retention_days, archive_after_days, delete_after_days,
                rule, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(version.id().as_uuid())
        .bind(version.organization_id().as_uuid())
        .bind(version.policy_id().as_uuid())
        .bind(version_number)
        .bind(minimum_retention_days)
        .bind(archive_after_days)
        .bind(delete_after_days)
        .bind(version.rule())
        .bind(version.created_by().as_uuid())
        .bind(version.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_retention_policy_for_update(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyRecord>, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyRow>(
            r#"
            SELECT policy.id, policy.organization_id, policy.policy_key,
                   policy.display_name, policy.resource_kind,
                   policy.active_version_id, policy.created_by,
                   policy.created_at,
                   COALESCE((
                       SELECT MAX(version.version)
                         FROM retention_policy_versions AS version
                        WHERE version.organization_id = policy.organization_id
                          AND version.policy_id = policy.id
                   ), 0)::integer AS latest_version
              FROM retention_policies AS policy
             WHERE policy.organization_id = $1
               AND policy.id = $2
             FOR UPDATE OF policy
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(retention_policy_record_from_row).transpose()
    }

    async fn find_retention_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
        version_id: RetentionPolicyVersionId,
    ) -> Result<Option<RetentionPolicyVersion>, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyVersionRow>(
            r#"
            SELECT id, policy_id, organization_id, version,
                   minimum_retention_days, archive_after_days,
                   delete_after_days, rule, created_by, created_at
              FROM retention_policy_versions
             WHERE organization_id = $1
               AND policy_id = $2
               AND id = $3
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_id.as_uuid())
        .bind(version_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(retention_policy_version_from_row).transpose()
    }

    async fn insert_retention_policy_activation(
        &mut self,
        activation: &RetentionPolicyActivationRecord,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO retention_policy_activations(
                id, organization_id, policy_id, policy_version_id,
                approval_request_id, activated_by, activated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(activation.id.as_uuid())
        .bind(activation.organization_id.as_uuid())
        .bind(activation.policy_id.as_uuid())
        .bind(activation.policy_version_id.as_uuid())
        .bind(activation.approval_request_id.as_uuid())
        .bind(activation.activated_by.as_uuid())
        .bind(activation.activated_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_retention_policy_activation_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<RetentionPolicyActivationRecord>, StoreError> {
        let row = sqlx::query_as::<_, RetentionPolicyActivationRow>(
            r#"
            SELECT id, organization_id, policy_id, policy_version_id,
                   approval_request_id, activated_by, activated_at
              FROM retention_policy_activations
             WHERE organization_id = $1
               AND approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(retention_policy_activation_from_row))
    }

    async fn activate_retention_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
        version_id: RetentionPolicyVersionId,
        expected_active_version_id: Option<RetentionPolicyVersionId>,
    ) -> Result<bool, StoreError> {
        let changed = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE retention_policies
               SET active_version_id = $3
             WHERE organization_id = $1
               AND id = $2
               AND active_version_id IS NOT DISTINCT FROM $4
            RETURNING 1
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(policy_id.as_uuid())
        .bind(version_id.as_uuid())
        .bind(expected_active_version_id.map(RetentionPolicyVersionId::as_uuid))
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(changed.is_some())
    }

    async fn find_agent_mail_policy_version(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        policy_version_id: Uuid,
    ) -> Result<Option<AgentMailPolicyVersionRecord>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            agent_id: Uuid,
            version: i32,
            allowed_recipient_domains: Vec<String>,
            max_recipients_per_message: i32,
            max_messages_per_hour: i32,
            max_messages_per_recipient_per_day: i32,
            require_first_contact_approval: bool,
            max_auto_replies_per_thread_per_day: i32,
            created_by: Uuid,
            created_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, agent_id, version,
                   allowed_recipient_domains, max_recipients_per_message,
                   max_messages_per_hour, max_messages_per_recipient_per_day,
                   require_first_contact_approval,
                   max_auto_replies_per_thread_per_day, created_by, created_at
              FROM agent_mail_policy_versions
             WHERE organization_id = $1
               AND agent_id = $2
               AND id = $3
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(agent_id.as_uuid())
        .bind(policy_version_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            let organization_id = OrganizationId::from_uuid(row.organization_id);
            let agent_id = PrincipalId::from_uuid(row.agent_id);
            Ok(AgentMailPolicyVersionRecord {
                id: row.id,
                policy: AgentMailPolicyVersion::new(
                    organization_id,
                    agent_id,
                    u32::try_from(row.version).map_err(|_| StoreError::Conflict)?,
                    row.allowed_recipient_domains,
                    u16::try_from(row.max_recipients_per_message)
                        .map_err(|_| StoreError::Conflict)?,
                    u32::try_from(row.max_messages_per_hour).map_err(|_| StoreError::Conflict)?,
                    u16::try_from(row.max_messages_per_recipient_per_day)
                        .map_err(|_| StoreError::Conflict)?,
                    row.require_first_contact_approval,
                    u8::try_from(row.max_auto_replies_per_thread_per_day)
                        .map_err(|_| StoreError::Conflict)?,
                )
                .map_err(|_| StoreError::Conflict)?,
                created_by: PrincipalId::from_uuid(row.created_by),
                created_at: row.created_at,
            })
        })
        .transpose()
    }

    async fn next_agent_mail_policy_version(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<u32, StoreError> {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("{organization_id}:{agent_id}"))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        let next = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COALESCE(MAX(version), 0)::bigint + 1
              FROM agent_mail_policy_versions
             WHERE organization_id = $1
               AND agent_id = $2
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(agent_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        u32::try_from(next).map_err(|_| StoreError::Conflict)
    }

    async fn insert_agent_mail_policy_version(
        &mut self,
        record: &AgentMailPolicyVersionRecord,
    ) -> Result<(), StoreError> {
        let policy = &record.policy;
        sqlx::query(
            r#"
            INSERT INTO agent_mail_policy_versions(
                id, organization_id, agent_id, version,
                allowed_recipient_domains, max_recipients_per_message,
                max_messages_per_hour, max_messages_per_recipient_per_day,
                require_first_contact_approval,
                max_auto_replies_per_thread_per_day, created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(record.id)
        .bind(policy.organization_id().as_uuid())
        .bind(policy.agent_id().as_uuid())
        .bind(i32::try_from(policy.version()).map_err(|_| StoreError::Conflict)?)
        .bind(
            policy
                .allowed_recipient_domains()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
        )
        .bind(i32::from(policy.max_recipients_per_message()))
        .bind(i32::try_from(policy.max_messages_per_hour()).map_err(|_| StoreError::Conflict)?)
        .bind(i32::from(policy.max_messages_per_recipient_per_day()))
        .bind(policy.require_first_contact_approval())
        .bind(i32::from(policy.max_auto_replies_per_thread_per_day()))
        .bind(record.created_by.as_uuid())
        .bind(record.created_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_agent_mail_policy_activation_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<AgentMailPolicyActivation>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            agent_id: Uuid,
            policy_version: i32,
            approval_request_id: Uuid,
            activated_by: Uuid,
            activated_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, agent_id, policy_version,
                   approval_request_id, activated_by, activated_at
              FROM agent_mail_policy_activations
             WHERE organization_id = $1
               AND approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            AgentMailPolicyActivation::restore(
                AgentMailPolicyActivationId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                PrincipalId::from_uuid(row.agent_id),
                u32::try_from(row.policy_version).map_err(|_| StoreError::Conflict)?,
                copilotmail_domain::ApprovalRequestId::from_uuid(row.approval_request_id),
                PrincipalId::from_uuid(row.activated_by),
                row.activated_at,
            )
            .map_err(|_| StoreError::Conflict)
        })
        .transpose()
    }

    async fn insert_agent_mail_policy_activation(
        &mut self,
        activation: &AgentMailPolicyActivation,
        policy_version_id: Uuid,
    ) -> Result<(), StoreError> {
        let policy_version =
            i32::try_from(activation.policy_version()).map_err(|_| StoreError::Conflict)?;
        sqlx::query(
            r#"
            INSERT INTO agent_mail_policy_activations(
                id, organization_id, agent_id, policy_version_id,
                policy_version, approval_request_id, activated_by, activated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(activation.id().as_uuid())
        .bind(activation.organization_id().as_uuid())
        .bind(activation.agent_id().as_uuid())
        .bind(policy_version_id)
        .bind(policy_version)
        .bind(activation.approval_request_id().as_uuid())
        .bind(activation.activated_by().as_uuid())
        .bind(activation.activated_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn ensure_agent_processing_subscription(
        &mut self,
        record: &AgentProcessingSubscriptionRecord,
    ) -> Result<(AgentProcessingSubscriptionRecord, bool), StoreError> {
        let inserted = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO agent_subscriptions(
                id, organization_id, principal_id, mailbox_id, name,
                status, cursor_value, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, 'default', 'active', NULL, $5, $5)
            ON CONFLICT (principal_id, mailbox_id, name) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(record.subscription_id.as_uuid())
        .bind(record.organization_id.as_uuid())
        .bind(record.principal_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.created_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            principal_id: Uuid,
            mailbox_id: Uuid,
            status: String,
            created_at: DateTime<Utc>,
            updated_at: DateTime<Utc>,
        }

        let stored = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, principal_id, mailbox_id,
                   status, created_at, updated_at
              FROM agent_subscriptions
             WHERE organization_id = $1
               AND principal_id = $2
               AND mailbox_id = $3
               AND name = 'default'
             FOR UPDATE
            "#,
        )
        .bind(record.organization_id.as_uuid())
        .bind(record.principal_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;

        Ok((
            AgentProcessingSubscriptionRecord {
                subscription_id: SubscriptionId::from_uuid(stored.id),
                organization_id: OrganizationId::from_uuid(stored.organization_id),
                principal_id: PrincipalId::from_uuid(stored.principal_id),
                mailbox_id: MailboxId::from_uuid(stored.mailbox_id),
                status: agent_processing_subscription_status(&stored.status)?,
                created_at: stored.created_at,
                updated_at: stored.updated_at,
            },
            inserted.is_some(),
        ))
    }

    async fn change_agent_processing_subscription(
        &mut self,
        write: &AgentProcessingSubscriptionChangeWrite,
    ) -> Result<Option<AgentProcessingSubscriptionChangeResult>, StoreError> {
        let operation = match write.target_status {
            AgentProcessingSubscriptionStatus::Active => "resumed",
            AgentProcessingSubscriptionStatus::Paused => "paused",
            AgentProcessingSubscriptionStatus::Retired => "retired",
        };
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 45))")
            .bind(format!(
                "{}:{}:{}:{}",
                write.organization_id,
                write.principal_id,
                operation,
                format_args!("{:?}", write.idempotency_key_sha256)
            ))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        #[derive(sqlx::FromRow)]
        struct Row {
            operation_id: Uuid,
            organization_id: Uuid,
            subscription_id: Uuid,
            principal_id: Uuid,
            mailbox_id: Uuid,
            previous_status: String,
            result_status: String,
            request_sha256: Vec<u8>,
            created_at: DateTime<Utc>,
            occurred_at: DateTime<Utc>,
        }

        let existing = sqlx::query_as::<_, Row>(
            r#"
            SELECT operation.id AS operation_id,
                   operation.organization_id,
                   operation.subscription_id,
                   operation.principal_id,
                   subscription.mailbox_id,
                   operation.previous_status,
                   operation.result_status,
                   operation.request_sha256,
                   subscription.created_at,
                   operation.occurred_at
              FROM agent_processing_subscription_operations AS operation
              JOIN agent_subscriptions AS subscription
                ON subscription.id = operation.subscription_id
               AND subscription.organization_id = operation.organization_id
             WHERE operation.organization_id = $1
               AND operation.principal_id = $2
               AND operation.operation = $3
               AND operation.idempotency_key_sha256 = $4
             FOR UPDATE OF operation
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(operation)
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            if existing.request_sha256 != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            return Ok(Some(agent_processing_subscription_change_result(
                existing.operation_id,
                existing.organization_id,
                existing.subscription_id,
                existing.principal_id,
                existing.mailbox_id,
                &existing.previous_status,
                &existing.result_status,
                existing.created_at,
                existing.occurred_at,
                true,
            )?));
        }

        let transitioned = sqlx::query_as::<_, Row>(
            r#"
            WITH changed AS (
                UPDATE agent_subscriptions AS subscription
                   SET status = $5,
                       updated_at = $6
                 WHERE subscription.id = $1
                   AND subscription.organization_id = $2
                   AND subscription.principal_id = $3
                   AND subscription.status = $4
                   AND EXISTS (
                       SELECT 1
                         FROM principals AS principal
                        WHERE principal.id = $3
                          AND principal.organization_id = $2
                          AND principal.kind = 'agent'
                          AND principal.status = 'active'
                   )
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $2
                              AND membership.mailbox_id = subscription.mailbox_id
                              AND membership.principal_id = $3
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= $6
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $6
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $2
                              AND delegation.mailbox_id = subscription.mailbox_id
                              AND delegation.grantee_principal_id = $3
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= $6
                              AND delegation.valid_until > $6
                       )
                   )
                   AND (
                       $5 <> 'retired'
                       OR NOT EXISTS (
                           SELECT 1
                             FROM agent_message_states AS state
                            WHERE state.organization_id = $2
                              AND state.subscription_id = $1
                              AND state.state = 'leased'
                              AND state.lease_expires_at > $6
                       )
                   )
                RETURNING subscription.organization_id,
                          subscription.id AS subscription_id,
                          subscription.principal_id,
                          subscription.mailbox_id,
                          subscription.created_at
            )
            INSERT INTO agent_processing_subscription_operations(
                id, organization_id, subscription_id, principal_id,
                operation, previous_status, result_status,
                idempotency_key_sha256, request_sha256, occurred_at
            )
            SELECT $7, changed.organization_id, changed.subscription_id,
                   changed.principal_id, $8, $4, $5, $9, $10, $6
              FROM changed
            RETURNING id AS operation_id, organization_id, subscription_id,
                      principal_id,
                      (SELECT mailbox_id FROM changed) AS mailbox_id,
                      previous_status, result_status, request_sha256,
                      (SELECT created_at FROM changed) AS created_at,
                      occurred_at
            "#,
        )
        .bind(write.subscription_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(write.expected_status.as_str())
        .bind(write.target_status.as_str())
        .bind(write.now)
        .bind(write.operation_id)
        .bind(operation)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        transitioned
            .map(|row| {
                agent_processing_subscription_change_result(
                    row.operation_id,
                    row.organization_id,
                    row.subscription_id,
                    row.principal_id,
                    row.mailbox_id,
                    &row.previous_status,
                    &row.result_status,
                    row.created_at,
                    row.occurred_at,
                    false,
                )
            })
            .transpose()
    }

    async fn claim_next_agent_message(
        &mut self,
        write: &AgentMessageClaimWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        if write.lease_expires_at <= write.now {
            return Err(StoreError::Conflict);
        }
        let operation = AgentMessageProcessingOperation::Claimed;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 43))")
            .bind(format!(
                "{}:{}:{}:{}",
                write.organization_id,
                write.principal_id,
                operation.as_str(),
                format_args!("{:?}", write.idempotency_key_sha256)
            ))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        let existing = sqlx::query_as::<_, AgentMessageProcessingOperationRow>(
            r#"
            SELECT id, organization_id, subscription_id, message_id,
                   principal_id, lease_id, operation, request_sha256,
                   result_state, attempt_count, lease_expires_at,
                   reason_code, occurred_at
              FROM agent_message_processing_operations
             WHERE organization_id = $1
               AND principal_id = $2
               AND operation = $3
               AND idempotency_key_sha256 = $4
             FOR UPDATE
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(operation.as_str())
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            if existing.request_sha256 != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            return agent_message_processing_result_from_row(existing, true).map(Some);
        }

        let row = sqlx::query_as::<_, AgentMessageProcessingOperationRow>(
            r#"
            WITH candidate AS (
                SELECT state.subscription_id, state.message_id
                  FROM agent_message_states AS state
                  JOIN agent_subscriptions AS subscription
                    ON subscription.id = state.subscription_id
                   AND subscription.organization_id = state.organization_id
                  JOIN principals AS principal
                    ON principal.id = subscription.principal_id
                   AND principal.organization_id = subscription.organization_id
                 WHERE state.organization_id = $1
                   AND state.principal_id = $2
                   AND state.subscription_id = $3
                   AND subscription.principal_id = $2
                   AND subscription.status = 'active'
                   AND principal.kind = 'agent'
                   AND principal.status = 'active'
                   AND (
                       EXISTS (
                           SELECT 1
                             FROM mailbox_memberships AS membership
                            WHERE membership.organization_id = $1
                              AND membership.mailbox_id = subscription.mailbox_id
                              AND membership.principal_id = $2
                              AND membership.role IN ('owner', 'member')
                              AND membership.valid_from <= $8
                              AND (
                                  membership.valid_until IS NULL
                                  OR membership.valid_until > $8
                              )
                       )
                       OR EXISTS (
                           SELECT 1
                             FROM delegations AS delegation
                            WHERE delegation.organization_id = $1
                              AND delegation.mailbox_id = subscription.mailbox_id
                              AND delegation.grantee_principal_id = $2
                              AND 'mailbox.read' = ANY(delegation.scopes)
                              AND delegation.revoked_at IS NULL
                              AND delegation.valid_from <= $8
                              AND delegation.valid_until > $8
                       )
                   )
                   AND (
                       state.state = 'pending'
                       OR (
                           state.state = 'leased'
                           AND state.lease_expires_at <= $8
                       )
                   )
                 ORDER BY state.updated_at, state.message_id
                 LIMIT 1
                 FOR UPDATE OF state SKIP LOCKED
            ),
            updated AS (
                UPDATE agent_message_states AS state
                   SET state = 'leased',
                       lease_id = $4,
                       last_lease_id = $4,
                       lease_expires_at = $5,
                       attempt_count = state.attempt_count + 1,
                       state_version = state.state_version + 1,
                       acknowledged_at = NULL,
                       failed_at = NULL,
                       updated_at = $8
                  FROM candidate
                 WHERE state.subscription_id = candidate.subscription_id
                   AND state.message_id = candidate.message_id
                RETURNING state.message_id, state.attempt_count
            )
            INSERT INTO agent_message_processing_operations(
                id, organization_id, subscription_id, message_id,
                principal_id, lease_id, operation, idempotency_key_sha256,
                request_sha256, result_state, attempt_count,
                lease_expires_at, reason_code, occurred_at
            )
            SELECT $9, $1, $3, updated.message_id, $2, $4, 'claimed',
                   $6, $7, 'leased', updated.attempt_count, $5, NULL, $8
              FROM updated
            RETURNING id, organization_id, subscription_id, message_id,
                      principal_id, lease_id, operation, request_sha256,
                      result_state, attempt_count, lease_expires_at,
                      reason_code, occurred_at
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(write.subscription_id.as_uuid())
        .bind(write.lease_id.as_uuid())
        .bind(write.lease_expires_at)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(write.now)
        .bind(Uuid::now_v7())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        row.map(|row| agent_message_processing_result_from_row(row, false))
            .transpose()
    }

    async fn transition_agent_message(
        &mut self,
        write: &AgentMessageTransitionWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        if matches!(
            write.operation,
            AgentMessageProcessingOperation::Claimed
                | AgentMessageProcessingOperation::DeadLettered
        ) || (write.operation == AgentMessageProcessingOperation::LeaseExtended
            && write
                .lease_expires_at
                .is_none_or(|expires_at| expires_at <= write.now))
            || (write.operation == AgentMessageProcessingOperation::Nacked
                && (write.max_attempts == 0 || write.reason_code.is_none()))
        {
            return Err(StoreError::Conflict);
        }
        let operation = write.operation.as_str();
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 43))")
            .bind(format!(
                "{}:{}:{}:{}",
                write.organization_id,
                write.principal_id,
                operation,
                format_args!("{:?}", write.idempotency_key_sha256)
            ))
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;

        let existing = sqlx::query_as::<_, AgentMessageProcessingOperationRow>(
            r#"
            SELECT id, organization_id, subscription_id, message_id,
                   principal_id, lease_id, operation, request_sha256,
                   result_state, attempt_count, lease_expires_at,
                   reason_code, occurred_at
              FROM agent_message_processing_operations
             WHERE organization_id = $1
               AND principal_id = $2
               AND operation = $3
               AND idempotency_key_sha256 = $4
             FOR UPDATE
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.principal_id.as_uuid())
        .bind(operation)
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            if existing.request_sha256 != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            return agent_message_processing_result_from_row(existing, true).map(Some);
        }

        #[derive(sqlx::FromRow)]
        struct TransitionRow {
            message_id: Uuid,
            attempt_count: i32,
            result_state: String,
            lease_expires_at: Option<DateTime<Utc>>,
            reason_code: Option<String>,
        }

        let transitioned = match write.operation {
            AgentMessageProcessingOperation::LeaseExtended => sqlx::query_as::<_, TransitionRow>(
                r#"
                    UPDATE agent_message_states AS state
                       SET lease_expires_at = $6,
                           state_version = state.state_version + 1,
                           updated_at = $7
                      FROM agent_subscriptions AS subscription
                     WHERE state.subscription_id = subscription.id
                       AND state.organization_id = subscription.organization_id
                       AND state.organization_id = $1
                       AND state.principal_id = $2
                       AND state.subscription_id = $3
                       AND state.message_id = $4
                       AND state.state = 'leased'
                       AND state.lease_id = $5
                       AND state.lease_expires_at > $7
                       AND subscription.principal_id = $2
                       AND subscription.status IN ('active', 'paused')
                       AND EXISTS (
                           SELECT 1 FROM principals AS principal
                            WHERE principal.id = $2
                              AND principal.organization_id = $1
                              AND principal.kind = 'agent'
                              AND principal.status = 'active'
                       )
                       AND (
                           EXISTS (
                               SELECT 1 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = $1
                                  AND membership.mailbox_id = subscription.mailbox_id
                                  AND membership.principal_id = $2
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $7
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $7
                                  )
                           )
                           OR EXISTS (
                               SELECT 1 FROM delegations AS delegation
                                WHERE delegation.organization_id = $1
                                  AND delegation.mailbox_id = subscription.mailbox_id
                                  AND delegation.grantee_principal_id = $2
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $7
                                  AND delegation.valid_until > $7
                           )
                       )
                    RETURNING state.message_id, state.attempt_count,
                              'leased'::text AS result_state,
                              state.lease_expires_at, NULL::text AS reason_code
                    "#,
            )
            .bind(write.organization_id.as_uuid())
            .bind(write.principal_id.as_uuid())
            .bind(write.subscription_id.as_uuid())
            .bind(write.message_id)
            .bind(write.lease_id.as_uuid())
            .bind(write.lease_expires_at)
            .bind(write.now)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?,
            AgentMessageProcessingOperation::Acknowledged => sqlx::query_as::<_, TransitionRow>(
                r#"
                    UPDATE agent_message_states AS state
                       SET state = 'acknowledged',
                           lease_id = NULL,
                           lease_expires_at = NULL,
                           last_error = NULL,
                           state_version = state.state_version + 1,
                           acknowledged_at = $6,
                           failed_at = NULL,
                           updated_at = $6
                      FROM agent_subscriptions AS subscription
                     WHERE state.subscription_id = subscription.id
                       AND state.organization_id = subscription.organization_id
                       AND state.organization_id = $1
                       AND state.principal_id = $2
                       AND state.subscription_id = $3
                       AND state.message_id = $4
                       AND state.state = 'leased'
                       AND state.lease_id = $5
                       AND state.lease_expires_at > $6
                       AND subscription.principal_id = $2
                       AND subscription.status IN ('active', 'paused')
                       AND EXISTS (
                           SELECT 1 FROM principals AS principal
                            WHERE principal.id = $2
                              AND principal.organization_id = $1
                              AND principal.kind = 'agent'
                              AND principal.status = 'active'
                       )
                       AND (
                           EXISTS (
                               SELECT 1 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = $1
                                  AND membership.mailbox_id = subscription.mailbox_id
                                  AND membership.principal_id = $2
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $6
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $6
                                  )
                           )
                           OR EXISTS (
                               SELECT 1 FROM delegations AS delegation
                                WHERE delegation.organization_id = $1
                                  AND delegation.mailbox_id = subscription.mailbox_id
                                  AND delegation.grantee_principal_id = $2
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $6
                                  AND delegation.valid_until > $6
                           )
                       )
                    RETURNING state.message_id, state.attempt_count,
                              'acknowledged'::text AS result_state,
                              NULL::timestamptz AS lease_expires_at,
                              NULL::text AS reason_code
                    "#,
            )
            .bind(write.organization_id.as_uuid())
            .bind(write.principal_id.as_uuid())
            .bind(write.subscription_id.as_uuid())
            .bind(write.message_id)
            .bind(write.lease_id.as_uuid())
            .bind(write.now)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?,
            AgentMessageProcessingOperation::Nacked => sqlx::query_as::<_, TransitionRow>(
                r#"
                    UPDATE agent_message_states AS state
                       SET state = CASE
                               WHEN state.attempt_count >= $6
                               THEN 'dead_lettered'
                               ELSE 'pending'
                           END,
                           lease_id = NULL,
                           lease_expires_at = NULL,
                           last_error = $8,
                           state_version = state.state_version + 1,
                           acknowledged_at = NULL,
                           failed_at = CASE
                               WHEN state.attempt_count >= $6 THEN $7
                               ELSE NULL
                           END,
                           updated_at = $7
                      FROM agent_subscriptions AS subscription
                     WHERE state.subscription_id = subscription.id
                       AND state.organization_id = subscription.organization_id
                       AND state.organization_id = $1
                       AND state.principal_id = $2
                       AND state.subscription_id = $3
                       AND state.message_id = $4
                       AND state.state = 'leased'
                       AND state.lease_id = $5
                       AND state.lease_expires_at > $7
                       AND subscription.principal_id = $2
                       AND subscription.status IN ('active', 'paused')
                       AND EXISTS (
                           SELECT 1 FROM principals AS principal
                            WHERE principal.id = $2
                              AND principal.organization_id = $1
                              AND principal.kind = 'agent'
                              AND principal.status = 'active'
                       )
                       AND (
                           EXISTS (
                               SELECT 1 FROM mailbox_memberships AS membership
                                WHERE membership.organization_id = $1
                                  AND membership.mailbox_id = subscription.mailbox_id
                                  AND membership.principal_id = $2
                                  AND membership.role IN ('owner', 'member')
                                  AND membership.valid_from <= $7
                                  AND (
                                      membership.valid_until IS NULL
                                      OR membership.valid_until > $7
                                  )
                           )
                           OR EXISTS (
                               SELECT 1 FROM delegations AS delegation
                                WHERE delegation.organization_id = $1
                                  AND delegation.mailbox_id = subscription.mailbox_id
                                  AND delegation.grantee_principal_id = $2
                                  AND 'mailbox.read' = ANY(delegation.scopes)
                                  AND delegation.revoked_at IS NULL
                                  AND delegation.valid_from <= $7
                                  AND delegation.valid_until > $7
                           )
                       )
                    RETURNING state.message_id, state.attempt_count,
                              state.state AS result_state,
                              NULL::timestamptz AS lease_expires_at,
                              state.last_error AS reason_code
                    "#,
            )
            .bind(write.organization_id.as_uuid())
            .bind(write.principal_id.as_uuid())
            .bind(write.subscription_id.as_uuid())
            .bind(write.message_id)
            .bind(write.lease_id.as_uuid())
            .bind(i64::from(write.max_attempts))
            .bind(write.now)
            .bind(write.reason_code.as_deref())
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?,
            AgentMessageProcessingOperation::Claimed
            | AgentMessageProcessingOperation::DeadLettered => unreachable!(),
        };

        let Some(transitioned) = transitioned else {
            return Ok(None);
        };
        let row = sqlx::query_as::<_, AgentMessageProcessingOperationRow>(
            r#"
            INSERT INTO agent_message_processing_operations(
                id, organization_id, subscription_id, message_id,
                principal_id, lease_id, operation, idempotency_key_sha256,
                request_sha256, result_state, attempt_count,
                lease_expires_at, reason_code, occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                    $10, $11, $12, $13, $14)
            RETURNING id, organization_id, subscription_id, message_id,
                      principal_id, lease_id, operation, request_sha256,
                      result_state, attempt_count, lease_expires_at,
                      reason_code, occurred_at
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(write.organization_id.as_uuid())
        .bind(write.subscription_id.as_uuid())
        .bind(transitioned.message_id)
        .bind(write.principal_id.as_uuid())
        .bind(write.lease_id.as_uuid())
        .bind(operation)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(&transitioned.result_state)
        .bind(transitioned.attempt_count)
        .bind(transitioned.lease_expires_at)
        .bind(transitioned.reason_code.as_deref())
        .bind(write.now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        agent_message_processing_result_from_row(row, false).map(Some)
    }

    async fn insert_compliance_case(&mut self, case: &ComplianceCase) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO compliance_cases(
                id, organization_id, external_reference, title, purpose,
                status, created_by, created_at, closed_by, closed_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(case.id().as_uuid())
        .bind(case.organization_id().as_uuid())
        .bind(case.external_reference())
        .bind(case.title())
        .bind(case.purpose())
        .bind(compliance_case_status(case.status()))
        .bind(case.created_by().as_uuid())
        .bind(case.created_at())
        .bind(case.closed_by().map(PrincipalId::as_uuid))
        .bind(case.closed_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_compliance_case_for_update(
        &mut self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            external_reference: String,
            title: String,
            purpose: String,
            status: String,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            closed_by: Option<Uuid>,
            closed_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, external_reference, title, purpose,
                   status, created_by, created_at, closed_by, closed_at
              FROM compliance_cases
             WHERE organization_id = $1 AND id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            ComplianceCase::from_persisted(
                ComplianceCaseId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                row.external_reference,
                row.title,
                row.purpose,
                parse_compliance_case_status(&row.status)?,
                PrincipalId::from_uuid(row.created_by),
                row.created_at,
                row.closed_by.map(PrincipalId::from_uuid),
                row.closed_at,
            )
            .map_err(|error| {
                tracing::error!(%error, "invalid compliance case persisted");
                StoreError::Unavailable
            })
        })
        .transpose()
    }

    async fn update_compliance_case(&mut self, case: &ComplianceCase) -> Result<bool, StoreError> {
        let updated = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE compliance_cases
               SET status = $3, closed_by = $4, closed_at = $5
             WHERE id = $1 AND organization_id = $2 AND status = 'open'
            RETURNING 1
            "#,
        )
        .bind(case.id().as_uuid())
        .bind(case.organization_id().as_uuid())
        .bind(compliance_case_status(case.status()))
        .bind(case.closed_by().map(PrincipalId::as_uuid))
        .bind(case.closed_at())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(updated.is_some())
    }

    async fn active_legal_hold_exists_for_case(
        &mut self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM legal_holds
                 WHERE organization_id = $1
                   AND case_id = $2
                   AND status = 'active'
            )
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(case_id.as_uuid())
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)
    }

    async fn insert_legal_hold(
        &mut self,
        hold: &LegalHold,
        evidence: &LegalHoldObjectEvidence,
    ) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO legal_holds(
                id, organization_id, case_id, archive_id, reason, status,
                apply_approval_request_id, applied_events_version_id,
                applied_manifest_version_id, applied_verified_at,
                created_by, created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            "#,
        )
        .bind(hold.id().as_uuid())
        .bind(hold.organization_id().as_uuid())
        .bind(hold.case_id().as_uuid())
        .bind(hold.archive_id().as_uuid())
        .bind(hold.reason())
        .bind(legal_hold_status(hold.status()))
        .bind(hold.apply_approval_request_id().as_uuid())
        .bind(&evidence.events_version_id)
        .bind(&evidence.manifest_version_id)
        .bind(evidence.verified_at)
        .bind(hold.created_by().as_uuid())
        .bind(hold.created_at())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn find_legal_hold_by_apply_approval_for_update(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            case_id: Uuid,
            archive_id: Uuid,
            reason: String,
            status: String,
            apply_approval_request_id: Uuid,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            release_approval_request_id: Option<Uuid>,
            released_by: Option<Uuid>,
            released_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, case_id, archive_id, reason, status,
                   apply_approval_request_id, created_by, created_at,
                   release_approval_request_id, released_by, released_at
              FROM legal_holds
             WHERE organization_id = $1
               AND apply_approval_request_id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(approval_request_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            LegalHold::from_persisted(
                LegalHoldId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                ComplianceCaseId::from_uuid(row.case_id),
                AuditArchiveId::from_uuid(row.archive_id),
                row.reason,
                parse_legal_hold_status(&row.status)?,
                copilotmail_domain::ApprovalRequestId::from_uuid(row.apply_approval_request_id),
                PrincipalId::from_uuid(row.created_by),
                row.created_at,
                row.release_approval_request_id
                    .map(copilotmail_domain::ApprovalRequestId::from_uuid),
                row.released_by.map(PrincipalId::from_uuid),
                row.released_at,
            )
            .map_err(|error| {
                tracing::error!(%error, "invalid legal hold persisted");
                StoreError::Unavailable
            })
        })
        .transpose()
    }

    async fn find_legal_hold_for_update(
        &mut self,
        organization_id: OrganizationId,
        hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: Uuid,
            organization_id: Uuid,
            case_id: Uuid,
            archive_id: Uuid,
            reason: String,
            status: String,
            apply_approval_request_id: Uuid,
            created_by: Uuid,
            created_at: DateTime<Utc>,
            release_approval_request_id: Option<Uuid>,
            released_by: Option<Uuid>,
            released_at: Option<DateTime<Utc>>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT id, organization_id, case_id, archive_id, reason, status,
                   apply_approval_request_id, created_by, created_at,
                   release_approval_request_id, released_by, released_at
              FROM legal_holds
             WHERE organization_id = $1 AND id = $2
             FOR UPDATE
            "#,
        )
        .bind(organization_id.as_uuid())
        .bind(hold_id.as_uuid())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(|row| {
            LegalHold::from_persisted(
                LegalHoldId::from_uuid(row.id),
                OrganizationId::from_uuid(row.organization_id),
                ComplianceCaseId::from_uuid(row.case_id),
                AuditArchiveId::from_uuid(row.archive_id),
                row.reason,
                parse_legal_hold_status(&row.status)?,
                copilotmail_domain::ApprovalRequestId::from_uuid(row.apply_approval_request_id),
                PrincipalId::from_uuid(row.created_by),
                row.created_at,
                row.release_approval_request_id
                    .map(copilotmail_domain::ApprovalRequestId::from_uuid),
                row.released_by.map(PrincipalId::from_uuid),
                row.released_at,
            )
            .map_err(|error| {
                tracing::error!(%error, "invalid legal hold persisted");
                StoreError::Unavailable
            })
        })
        .transpose()
    }

    async fn update_legal_hold(
        &mut self,
        hold: &LegalHold,
        evidence: &LegalHoldObjectEvidence,
    ) -> Result<bool, StoreError> {
        let updated = sqlx::query_scalar::<_, i32>(
            r#"
            UPDATE legal_holds
               SET status = $3,
                   release_approval_request_id = $4,
                   released_events_version_id = $5,
                   released_manifest_version_id = $6,
                   released_verified_at = $7,
                   released_by = $8,
                   released_at = $9
             WHERE id = $1 AND organization_id = $2 AND status = 'active'
            RETURNING 1
            "#,
        )
        .bind(hold.id().as_uuid())
        .bind(hold.organization_id().as_uuid())
        .bind(legal_hold_status(hold.status()))
        .bind(
            hold.release_approval_request_id()
                .map(copilotmail_domain::ApprovalRequestId::as_uuid),
        )
        .bind(&evidence.events_version_id)
        .bind(&evidence.manifest_version_id)
        .bind(evidence.verified_at)
        .bind(hold.released_by().map(PrincipalId::as_uuid))
        .bind(hold.released_at())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(updated.is_some())
    }

    async fn create_mail_submission_intent(
        &mut self,
        write: &MailSubmissionIntentWrite,
    ) -> Result<MailSubmissionIntentCreation, StoreError> {
        sqlx::query(
            r#"
            SELECT pg_advisory_xact_lock(
                hashtextextended(
                    $1::text || ':' || $2::text || ':' || encode($3::bytea, 'hex'),
                    0
                )
            )
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.idempotency_key_sha256.as_slice())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let existing = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            SELECT id, organization_id, requested_by, mailbox_id, draft_id,
                   draft_version, mail_data_plane_id, request_sha256, status,
                   state_version, created_at, updated_at
              FROM mail_submission_intents
             WHERE organization_id = $1
               AND requested_by = $2
               AND idempotency_key_sha256 = $3
             FOR UPDATE
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            let intent = mail_submission_intent_from_row(existing)?;
            if intent.request_sha256 != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            let production_admission = sqlx::query_as::<_, AgentMailAdmissionRow>(
                r#"
                SELECT id, organization_id, agent_id, submission_id,
                       policy_version_id, policy_version, recipient_count,
                       first_contact_count, admitted_at
                  FROM agent_mail_admissions
                 WHERE submission_id = $1
                "#,
            )
            .bind(intent.submission_id.as_uuid())
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?
            .map(agent_mail_admission_from_row)
            .transpose()?;
            return Ok(MailSubmissionIntentCreation {
                intent,
                created: false,
                production_admission,
            });
        }

        #[derive(sqlx::FromRow)]
        struct FrozenDraftRow {
            mail_data_plane_id: Uuid,
            engine_account_ref: String,
            engine_email_ref: String,
            draft_version: i64,
        }
        let frozen = sqlx::query_as::<_, FrozenDraftRow>(
            r#"
            UPDATE mail_drafts
               SET status = 'frozen',
                   version = version + 1,
                   updated_at = $6
             WHERE id = $1
               AND organization_id = $2
               AND mailbox_id = $3
               AND created_by = $4
               AND version = $5
               AND status = 'active'
            RETURNING mail_data_plane_id, engine_account_ref, engine_email_ref,
                      version AS draft_version
            "#,
        )
        .bind(write.draft_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.expected_draft_version)
        .bind(write.now)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;

        let row = sqlx::query_as::<_, MailSubmissionIntentRow>(
            r#"
            INSERT INTO mail_submission_intents(
                id, organization_id, requested_by, mailbox_id, draft_id,
                draft_version, idempotency_key_sha256, request_sha256,
                mail_data_plane_id, engine_source, engine_account_ref,
                engine_email_ref, status,
                state_version, attempt_count, created_at, updated_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                $9, 'stalwart', $10, $11, 'pending', 1, 0, $12, $12
            )
            RETURNING id, organization_id, requested_by, mailbox_id, draft_id,
                      draft_version, mail_data_plane_id, request_sha256, status,
                      state_version, created_at, updated_at
            "#,
        )
        .bind(write.submission_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.draft_id.as_uuid())
        .bind(frozen.draft_version)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(frozen.mail_data_plane_id)
        .bind(&frozen.engine_account_ref)
        .bind(&frozen.engine_email_ref)
        .bind(write.now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_agent_mail_admission_error)?;
        let production_admission = sqlx::query_as::<_, AgentMailAdmissionRow>(
            r#"
            INSERT INTO agent_mail_admissions(
                id, organization_id, agent_id, submission_id,
                policy_activation_id, policy_version_id, policy_version,
                draft_id, draft_version, thread_ref, recipient_count,
                first_contact_count, first_contact_approval_request_id,
                admitted_at, mail_data_plane_id
            )
            SELECT $1, $2, $3, $4,
                   '00000000-0000-0000-0000-000000000000'::uuid,
                   '00000000-0000-0000-0000-000000000000'::uuid,
                   1, $5, $6, decode(repeat('00', 32), 'hex'), 1, 0, $7, $8,
                   $9
              FROM principals AS principal
              JOIN agent_profiles AS profile
                ON profile.principal_id = principal.id
             WHERE principal.id = $3
               AND principal.organization_id = $2
               AND principal.kind = 'agent'
               AND principal.status = 'active'
               AND profile.environment = 'production'
            RETURNING id, organization_id, agent_id, submission_id,
                      policy_version_id, policy_version, recipient_count,
                      first_contact_count, admitted_at
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(write.organization_id.as_uuid())
        .bind(write.requested_by.as_uuid())
        .bind(write.submission_id.as_uuid())
        .bind(write.draft_id.as_uuid())
        .bind(frozen.draft_version)
        .bind(
            write
                .first_contact_approval_request_id
                .map(copilotmail_domain::ApprovalRequestId::as_uuid),
        )
        .bind(write.now)
        .bind(frozen.mail_data_plane_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_agent_mail_admission_error)?
        .map(agent_mail_admission_from_row)
        .transpose()?;
        Ok(MailSubmissionIntentCreation {
            intent: mail_submission_intent_from_row(row)?,
            created: true,
            production_admission,
        })
    }

    async fn create_agent_mail_sandbox_submission(
        &mut self,
        write: &AgentMailSandboxSubmissionWrite,
    ) -> Result<AgentMailSandboxSubmissionCreation, StoreError> {
        sqlx::query(
            r#"
            SELECT pg_advisory_xact_lock(
                hashtextextended($1::text || ':' || $2::text, 0)
            )
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;

        let existing = sqlx::query_as::<_, AgentMailSandboxSubmissionRow>(
            r#"
            SELECT id, organization_id, agent_id, mailbox_id, draft_id,
                   draft_version, request_sha256, recipient_count, captured_at
              FROM agent_mail_sandbox_submissions
             WHERE organization_id = $1
               AND agent_id = $2
               AND idempotency_key_sha256 = $3
             FOR UPDATE
            "#,
        )
        .bind(write.organization_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .bind(write.idempotency_key_sha256.as_slice())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if let Some(existing) = existing {
            if existing.request_sha256.as_slice() != write.request_sha256 {
                return Err(StoreError::Conflict);
            }
            return Ok(AgentMailSandboxSubmissionCreation {
                record: agent_mail_sandbox_submission_from_row(existing)?,
                created: false,
            });
        }

        #[derive(sqlx::FromRow)]
        struct FrozenSandboxDraftRow {
            draft_version: i64,
            body_ref: Vec<u8>,
            recipient_count: i32,
        }
        let frozen = sqlx::query_as::<_, FrozenSandboxDraftRow>(
            r#"
            UPDATE mail_drafts
               SET status = 'frozen',
                   version = version + 1,
                   updated_at = $6
             WHERE id = $1
               AND organization_id = $2
               AND mailbox_id = $3
               AND created_by = $4
               AND version = $5
               AND status = 'active'
            RETURNING version AS draft_version,
                      body_ref,
                      cardinality(recipient_refs)::integer AS recipient_count
            "#,
        )
        .bind(write.draft_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .bind(write.expected_draft_version)
        .bind(write.now)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;

        let row = sqlx::query_as::<_, AgentMailSandboxSubmissionRow>(
            r#"
            INSERT INTO agent_mail_sandbox_submissions(
                id, organization_id, agent_id, mailbox_id, draft_id,
                draft_version, idempotency_key_sha256, request_sha256,
                body_ref, recipient_count, captured_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            RETURNING id, organization_id, agent_id, mailbox_id, draft_id,
                      draft_version, request_sha256, recipient_count, captured_at
            "#,
        )
        .bind(write.submission_id)
        .bind(write.organization_id.as_uuid())
        .bind(write.agent_id.as_uuid())
        .bind(write.mailbox_id.as_uuid())
        .bind(write.draft_id.as_uuid())
        .bind(frozen.draft_version)
        .bind(write.idempotency_key_sha256.as_slice())
        .bind(write.request_sha256.as_slice())
        .bind(frozen.body_ref)
        .bind(frozen.recipient_count)
        .bind(write.now)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(AgentMailSandboxSubmissionCreation {
            record: agent_mail_sandbox_submission_from_row(row)?,
            created: true,
        })
    }

    async fn acquire_mail_submission_lease(
        &mut self,
        submission_id: MailSubmissionId,
        expected_data_plane_id: MailDataPlaneId,
        lease_id: Uuid,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Option<MailSubmissionLease>, StoreError> {
        if lease_expires_at <= now || lease_expires_at > now + chrono::Duration::minutes(5) {
            return Err(StoreError::Conflict);
        }
        let row = sqlx::query_as::<_, MailSubmissionLeaseRow>(
            r#"
            UPDATE mail_submission_intents AS intent
               SET status = 'reconciling',
                   state_version = intent.state_version + 1,
                   lease_id = $3,
                   lease_expires_at = clock_timestamp() + ($5 - $4),
                   attempt_count = intent.attempt_count + 1,
                   updated_at = clock_timestamp()
              FROM mailboxes AS mailbox,
                   principals AS requester
             WHERE intent.id = $1
               AND intent.mail_data_plane_id = $2
               AND intent.status IN ('pending', 'reconciling')
               AND (
                    intent.lease_id IS NULL
                    OR intent.lease_expires_at <= clock_timestamp()
               )
               AND mailbox.id = intent.mailbox_id
               AND mailbox.organization_id = intent.organization_id
               AND requester.id = intent.requested_by
               AND requester.organization_id = intent.organization_id
               AND (
                   requester.kind = 'human'
                   OR (
                       requester.kind = 'agent'
                       AND EXISTS (
                           SELECT 1
                             FROM agent_profiles AS agent
                            WHERE agent.principal_id = requester.id
                              AND (
                                  agent.environment IN ('development', 'testing')
                                  OR (
                                      agent.environment = 'production'
                                      AND EXISTS (
                                          SELECT 1
                                            FROM agent_mail_admissions AS admission
                                           WHERE admission.organization_id = intent.organization_id
                                             AND admission.agent_id = intent.requested_by
                                             AND admission.submission_id = intent.id
                                      )
                                  )
                              )
                       )
                   )
               )
               AND requester.status = 'active'
            RETURNING intent.id, intent.organization_id, intent.requested_by,
                      intent.mailbox_id, intent.draft_id, intent.draft_version,
                      intent.mail_data_plane_id, intent.engine_account_ref,
                      intent.engine_email_ref, mailbox.primary_address,
                      intent.lease_id, intent.lease_expires_at,
                      intent.attempt_count
            "#,
        )
        .bind(submission_id.as_uuid())
        .bind(expected_data_plane_id.as_uuid())
        .bind(lease_id)
        .bind(now)
        .bind(lease_expires_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        row.map(mail_submission_lease_from_row).transpose()
    }

    async fn finalize_mail_submission_intent(
        &mut self,
        write: &MailSubmissionTerminalWrite,
    ) -> Result<bool, StoreError> {
        let (status, identity_ref, submission_ref, error_code) = match write.status {
            MailSubmissionStatus::Accepted
                if write.engine_identity_ref.is_some()
                    && write.engine_submission_ref.is_some()
                    && write.error_code.is_none() =>
            {
                (
                    "accepted",
                    write.engine_identity_ref.as_deref(),
                    write.engine_submission_ref.as_deref(),
                    None,
                )
            }
            MailSubmissionStatus::Rejected
                if write.engine_submission_ref.is_none() && write.error_code.is_some() =>
            {
                (
                    "rejected",
                    write.engine_identity_ref.as_deref(),
                    None,
                    write.error_code.as_deref(),
                )
            }
            MailSubmissionStatus::ManualReview
                if write.engine_identity_ref.is_none()
                    && write.engine_submission_ref.is_none()
                    && write.error_code.is_some() =>
            {
                ("manual_review", None, None, write.error_code.as_deref())
            }
            _ => return Err(StoreError::Conflict),
        };

        #[derive(sqlx::FromRow)]
        struct FinalizedIntentRow {
            draft_id: Uuid,
            draft_version: i64,
        }
        let finalized = sqlx::query_as::<_, FinalizedIntentRow>(
            r#"
            UPDATE mail_submission_intents
               SET status = $4,
                   state_version = state_version + 1,
                   engine_identity_ref = $5,
                   engine_submission_ref = $6,
                   error_code = $7,
                   lease_id = NULL,
                   lease_expires_at = NULL,
                   updated_at = GREATEST($8, clock_timestamp()),
                   accepted_at = CASE
                       WHEN $4 = 'accepted'
                       THEN GREATEST($8, clock_timestamp())
                       ELSE NULL
                   END
             WHERE id = $1
               AND organization_id = $2
               AND status = 'reconciling'
               AND lease_id = $3
               AND lease_expires_at > GREATEST($8, clock_timestamp())
            RETURNING draft_id, draft_version
            "#,
        )
        .bind(write.submission_id.as_uuid())
        .bind(write.organization_id.as_uuid())
        .bind(write.lease_id)
        .bind(status)
        .bind(identity_ref)
        .bind(submission_ref)
        .bind(error_code)
        .bind(write.now)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(finalized) = finalized else {
            return Ok(false);
        };

        if write.status == MailSubmissionStatus::Accepted {
            let updated = sqlx::query_scalar::<_, i32>(
                r#"
                UPDATE mail_drafts
                   SET status = 'sent',
                       version = version + 1,
                       updated_at = GREATEST($4, clock_timestamp())
                 WHERE id = $1
                   AND organization_id = $2
                   AND version = $3
                   AND status = 'frozen'
                RETURNING 1
                "#,
            )
            .bind(finalized.draft_id)
            .bind(write.organization_id.as_uuid())
            .bind(finalized.draft_version)
            .bind(write.now)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
            if updated.is_none() {
                return Err(StoreError::Conflict);
            }
        }
        Ok(true)
    }

    async fn append_mail_delivery_snapshot(
        &mut self,
        write: &MailDeliverySnapshotWrite,
    ) -> Result<MailDeliverySnapshotInsert, StoreError> {
        if write.recipients.is_empty() || write.recipients.len() > 100 {
            return Err(StoreError::Conflict);
        }
        let mut delivered = 0_i32;
        let mut failed = 0_i32;
        let mut queued = 0_i32;
        let mut unknown = 0_i32;
        for recipient in &write.recipients {
            match recipient.state {
                MailDeliveryRecipientState::Delivered => delivered += 1,
                MailDeliveryRecipientState::PermanentFailure => failed += 1,
                MailDeliveryRecipientState::Queued => queued += 1,
                MailDeliveryRecipientState::Unknown => unknown += 1,
            }
        }
        let recipient_count =
            i32::try_from(write.recipients.len()).map_err(|_| StoreError::Conflict)?;
        let terminal = queued == 0 && unknown == 0;
        let inserted = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO mail_delivery_snapshots(
                id, organization_id, submission_id, mail_data_plane_id, source,
                source_state, source_sha256, recipient_count, delivered_count,
                failed_count, queued_count, unknown_count, terminal,
                observed_at, recorded_at
            )
            VALUES (
                $1, $2, $3, $4, 'stalwart_jmap', $5, $6, $7, $8, $9, $10, $11, $12,
                GREATEST(
                    $13,
                    (
                        SELECT accepted_at
                          FROM mail_submission_intents
                         WHERE id = $3
                           AND organization_id = $2
                           AND mail_data_plane_id = $4
                    )
                ),
                GREATEST($13, clock_timestamp())
            )
            ON CONFLICT (
                organization_id, submission_id, mail_data_plane_id, source_sha256
            )
            DO NOTHING
            RETURNING id
            "#,
        )
        .bind(write.snapshot_id)
        .bind(write.organization_id.as_uuid())
        .bind(write.submission_id.as_uuid())
        .bind(write.data_plane_id.as_uuid())
        .bind(&write.source_state)
        .bind(write.source_sha256.as_slice())
        .bind(recipient_count)
        .bind(delivered)
        .bind(failed)
        .bind(queued)
        .bind(unknown)
        .bind(terminal)
        .bind(write.observed_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        let Some(snapshot_id) = inserted else {
            return Ok(MailDeliverySnapshotInsert::Duplicate);
        };
        for recipient in &write.recipients {
            sqlx::query(
                r#"
                INSERT INTO mail_delivery_recipient_evidence(
                    snapshot_id, organization_id, submission_id,
                    recipient_ref, state, smtp_reply
                )
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(snapshot_id)
            .bind(write.organization_id.as_uuid())
            .bind(write.submission_id.as_uuid())
            .bind(recipient.recipient_ref.as_slice())
            .bind(mail_delivery_recipient_state(recipient.state))
            .bind(&recipient.smtp_reply)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(map_store_error)?;
        }
        Ok(MailDeliverySnapshotInsert::Inserted)
    }

    async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO audit_events(
                id, organization_id, actor_principal_id, on_behalf_of_principal_id,
                credential_id, action, resource_type, resource_id, outcome,
                trace_id, details, occurred_at, received_at, request_id,
                subject_principal_id, source_ip, client_id, session_id, token_jti,
                policy_id, policy_version_id, approval_request_id, ticket_reference,
                environment, region
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                $14, $15, $16::inet, $17, $18, $19, $20, $21, $22, $23, $24, $25
            )
            "#,
        )
        .bind(event.id.as_uuid())
        .bind(event.organization_id.as_uuid())
        .bind(event.actor.principal_id.as_uuid())
        .bind(event.actor.on_behalf_of.map(|id| id.as_uuid()))
        .bind(event.actor.credential_id.as_deref())
        .bind(&event.action)
        .bind(&event.resource.kind)
        .bind(&event.resource.id)
        .bind(audit_outcome(event.outcome))
        .bind(&event.trace_id)
        .bind(&event.details)
        .bind(event.occurred_at)
        .bind(event.received_at)
        .bind(&event.context.request_id)
        .bind(event.context.subject_principal_id.map(PrincipalId::as_uuid))
        .bind(&event.context.source_ip)
        .bind(&event.context.client_id)
        .bind(&event.context.session_id)
        .bind(&event.context.token_jti)
        .bind(event.context.policy_id.map(PolicyId::as_uuid))
        .bind(
            event
                .context
                .policy_version_id
                .map(PolicyVersionId::as_uuid),
        )
        .bind(
            event
                .context
                .approval_request_id
                .map(copilotmail_domain::ApprovalRequestId::as_uuid),
        )
        .bind(&event.context.ticket_reference)
        .bind(&event.context.environment)
        .bind(&event.context.region)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO outbox_events(
                id, event_type, aggregate_type, aggregate_id,
                schema_version, payload, mail_data_plane_id, occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(event.id)
        .bind(&event.event_type)
        .bind(&event.aggregate_type)
        .bind(&event.aggregate_id)
        .bind(i32::try_from(event.schema_version).map_err(|_| StoreError::Conflict)?)
        .bind(&event.payload)
        .bind(event.data_plane_id.map(MailDataPlaneId::as_uuid))
        .bind(event.occurred_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn commit(mut self: Box<Self>) -> Result<(), StoreError> {
        self.transaction
            .take()
            .ok_or(StoreError::Unavailable)?
            .commit()
            .await
            .map_err(map_store_error)
    }
}

#[async_trait]
impl MailEventIngestionTransaction for PgControlPlaneTransaction {
    async fn resolve_mail_engine_mailbox_binding(
        &mut self,
        data_plane_id: MailDataPlaneId,
        account_ref: &str,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            version: i64,
            updated_at: DateTime<Utc>,
        }
        let row = sqlx::query_as::<_, Row>(
            r#"
            SELECT organization_id, mailbox_id, mail_data_plane_id, account_ref,
                   version, updated_at
              FROM mail_engine_mailbox_bindings
             WHERE mail_data_plane_id = $1
               AND account_ref = $2
             FOR SHARE
            "#,
        )
        .bind(data_plane_id.as_uuid())
        .bind(account_ref)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(row.map(|row| MailEngineMailboxBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            account_ref: row.account_ref,
            version: row.version,
            updated_at: row.updated_at,
        }))
    }

    async fn insert_inbound_mail_event(
        &mut self,
        record: &InboundMailEventRecord,
    ) -> Result<InboundMailEventInsert, StoreError> {
        let inserted = sqlx::query_scalar::<_, bool>(
            r#"
            INSERT INTO inbound_mail_engine_events(
                event_id, message_id, organization_id, mailbox_id,
                mail_data_plane_id, source, source_event_id, source_event_sha256,
                account_ref, document_ref, classification,
                source_occurred_at, received_at
            )
            VALUES (
                $1, $2, $3, $4, $5, 'stalwart', $6, $7, $8, $9, $10, $11, $12
            )
            ON CONFLICT DO NOTHING
            RETURNING true
            "#,
        )
        .bind(record.event_id)
        .bind(record.message_id)
        .bind(record.organization_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .bind(record.source_event_sha256.as_slice())
        .bind(&record.account_ref)
        .bind(&record.document_ref)
        .bind(&record.classification)
        .bind(record.source_occurred_at)
        .bind(record.received_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .unwrap_or(false);
        if inserted {
            return Ok(InboundMailEventInsert::Inserted);
        }

        #[derive(sqlx::FromRow)]
        struct ExistingRow {
            organization_id: Uuid,
            mailbox_id: Uuid,
            source_event_id: String,
            source_event_sha256: Vec<u8>,
            account_ref: String,
            document_ref: String,
            classification: String,
        }
        let existing = sqlx::query_as::<_, ExistingRow>(
            r#"
            SELECT organization_id, mailbox_id, source_event_id,
                   source_event_sha256, account_ref, document_ref, classification
              FROM inbound_mail_engine_events
             WHERE mail_data_plane_id = $1
               AND (
                   source_event_id = $2
                   OR (account_ref = $3 AND document_ref = $4)
               )
             FOR SHARE
            "#,
        )
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .bind(&record.account_ref)
        .bind(&record.document_ref)
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if existing.len() != 1 {
            return Err(StoreError::Conflict);
        }
        let existing = &existing[0];
        if existing.source_event_id == record.source_event_id {
            if existing.source_event_sha256.as_slice() == record.source_event_sha256 {
                return Ok(InboundMailEventInsert::Duplicate);
            }
            return Err(StoreError::Conflict);
        }
        let same_message = existing.organization_id == record.organization_id.as_uuid()
            && existing.mailbox_id == record.mailbox_id.as_uuid()
            && existing.account_ref == record.account_ref
            && existing.document_ref == record.document_ref
            && existing.classification == record.classification;
        if same_message {
            Ok(InboundMailEventInsert::Duplicate)
        } else {
            Err(StoreError::Conflict)
        }
    }

    async fn fanout_agent_message_states(
        &mut self,
        record: &InboundMailEventRecord,
    ) -> Result<u64, StoreError> {
        let result = sqlx::query(
            r#"
            INSERT INTO agent_message_states(
                subscription_id, message_id, organization_id, principal_id,
                state, attempt_count, state_version, updated_at
            )
            SELECT subscription.id, $1, subscription.organization_id,
                   subscription.principal_id, 'pending', 0, 0, $4
              FROM agent_subscriptions AS subscription
             WHERE subscription.organization_id = $2
               AND subscription.mailbox_id = $3
               AND subscription.status IN ('active', 'paused')
            ON CONFLICT(subscription_id, message_id) DO NOTHING
            "#,
        )
        .bind(record.message_id)
        .bind(record.organization_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.received_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(result.rows_affected())
    }

    async fn resolve_mail_engine_mailbox_by_primary_address(
        &mut self,
        data_plane_id: MailDataPlaneId,
        primary_address: &copilotmail_domain::MailboxAddress,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            organization_id: Uuid,
            mailbox_id: Uuid,
            mail_data_plane_id: Uuid,
            account_ref: String,
            version: i64,
            updated_at: DateTime<Utc>,
        }
        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT binding.organization_id, binding.mailbox_id,
                   binding.mail_data_plane_id, binding.account_ref,
                   binding.version, binding.updated_at
              FROM mail_engine_mailbox_bindings AS binding
              JOIN mailboxes AS mailbox
                ON mailbox.id = binding.mailbox_id
               AND mailbox.organization_id = binding.organization_id
             WHERE binding.mail_data_plane_id = $1
               AND mailbox.primary_address = $2
             ORDER BY binding.organization_id, binding.mailbox_id
             LIMIT 2
             FOR SHARE OF binding, mailbox
            "#,
        )
        .bind(data_plane_id.as_uuid())
        .bind(primary_address.as_str())
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        if rows.len() > 1 {
            return Err(StoreError::Conflict);
        }
        Ok(rows.into_iter().next().map(|row| MailEngineMailboxBinding {
            organization_id: OrganizationId::from_uuid(row.organization_id),
            mailbox_id: MailboxId::from_uuid(row.mailbox_id),
            data_plane_id: MailDataPlaneId::from_uuid(row.mail_data_plane_id),
            account_ref: row.account_ref,
            version: row.version,
            updated_at: row.updated_at,
        }))
    }

    async fn insert_outbound_mail_event(
        &mut self,
        record: &OutboundMailEventRecord,
    ) -> Result<OutboundMailEventInsert, StoreError> {
        let inserted = sqlx::query_scalar::<_, bool>(
            r#"
            INSERT INTO outbound_mail_engine_events(
                event_id, organization_id, mailbox_id, mail_data_plane_id, source,
                source_event_id, source_event_sha256, queue_ref, event_kind,
                recipient_ref, smtp_code, next_retry_at,
                source_occurred_at, received_at
            )
            VALUES (
                $1, $2, $3, $4, 'stalwart', $5, $6, $7, $8,
                $9, $10, $11, $12, $13
            )
            ON CONFLICT (mail_data_plane_id, source_event_id) DO NOTHING
            RETURNING true
            "#,
        )
        .bind(record.event_id)
        .bind(record.organization_id.as_uuid())
        .bind(record.mailbox_id.as_uuid())
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .bind(record.source_event_sha256.as_slice())
        .bind(&record.queue_ref)
        .bind(outbound_mail_event_kind(record.kind))
        .bind(record.recipient_ref.as_ref().map(<[u8; 32]>::as_slice))
        .bind(
            record
                .smtp_code
                .map(|value| i16::try_from(value).map_err(|_| StoreError::Conflict))
                .transpose()?,
        )
        .bind(record.next_retry_at)
        .bind(record.source_occurred_at)
        .bind(record.received_at)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .unwrap_or(false);
        if inserted {
            return Ok(OutboundMailEventInsert::Inserted);
        }
        let existing = sqlx::query_scalar::<_, Vec<u8>>(
            r#"
            SELECT source_event_sha256
              FROM outbound_mail_engine_events
             WHERE mail_data_plane_id = $1
               AND source_event_id = $2
             FOR SHARE
            "#,
        )
        .bind(record.data_plane_id.as_uuid())
        .bind(&record.source_event_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?
        .ok_or(StoreError::Conflict)?;
        if existing.as_slice() == record.source_event_sha256 {
            Ok(OutboundMailEventInsert::Duplicate)
        } else {
            Err(StoreError::Conflict)
        }
    }

    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
        sqlx::query(
            r#"
            INSERT INTO outbox_events(
                id, event_type, aggregate_type, aggregate_id,
                schema_version, payload, mail_data_plane_id, occurred_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(event.id)
        .bind(&event.event_type)
        .bind(&event.aggregate_type)
        .bind(&event.aggregate_id)
        .bind(i32::try_from(event.schema_version).map_err(|_| StoreError::Conflict)?)
        .bind(&event.payload)
        .bind(event.data_plane_id.map(MailDataPlaneId::as_uuid))
        .bind(event.occurred_at)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(map_store_error)?;
        Ok(())
    }

    async fn commit(mut self: Box<Self>) -> Result<(), StoreError> {
        self.transaction
            .take()
            .ok_or(StoreError::Unavailable)?
            .commit()
            .await
            .map_err(map_store_error)
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
}
