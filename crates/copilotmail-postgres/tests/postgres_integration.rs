mod support;

use async_trait::async_trait;
use chrono::Utc;
use copilotmail_agent_auth::rsa_public_key_fingerprint;
use copilotmail_application::mail_ports::{
    CreatedMailSubmission, InboundMailEngineEvent, InboundMessageClassification, MailDataPlanePort,
    MailDeliverySnapshot, MailDraft, MailDraftContent, MailDraftContentReader, MailDraftStore,
    MailEngineError, MailEngineWebhookEvent, MailSubmissionAdapter, MailSubmissionReconciliation,
    OutboundDeliveryEventKind, OutboundMailEngineEvent, RecipientDeliveryEvidence,
    RecipientDeliveryState, TextDraftInput,
};
use copilotmail_application::{
    AcknowledgeAgentMessageCommand, ActivateAgentMailPolicyCommand, ActivateMailDomainCommand,
    ActivateRetentionPolicyVersionCommand, AddAgentCredentialCommand, AddAgentCredentialService,
    AddAgentMtlsCredentialCommand, AddAgentMtlsCredentialService, AgentCredentialStore,
    AgentEventCursorError, AgentEventCursorQueryStore, AgentEventCursorService,
    AgentIngressRateLimitDecision, AgentIngressRateLimitStore, AgentInspectorError,
    AgentInspectorService, AgentMailPolicyQueryStore, AgentMailPolicyService,
    AgentMailSandboxService, AgentMailSubmissionEvidenceService, AgentMessageProcessingState,
    AgentProcessingService, AgentProcessingSubscriptionStatus, AppendRetentionPolicyVersionCommand,
    ApprovalGateService, ApprovalPolicyService, ApprovalProof, ApprovalQueryStore,
    ApprovalRequestView, ApprovalWorkflowService, ArchivedObjectRecord, AuditArchiveRecord,
    AuditEventSearchError, AuditEventSearchService, AuditExportArtifact,
    AuditExportAuthorizationService, AuditExportDownloadService, AuditExportQueryStore,
    AuthorizationStore, AuthorizeAuditExportCommand, BeginHumanLoginCommand,
    BindMailEngineMailboxCommand, BindOidcOwnerCommand, BindOidcOwnerService,
    BootstrapOrganizationCommand, BootstrapOrganizationService, CancelApprovalRequestCommand,
    CaptureAgentMailSandboxCommand, ChangeAgentProcessingSubscriptionCommand,
    ClaimAgentMessageCommand, ComplianceQueryStore, ConsumeHumanLoginCommand, ControlPlaneStore,
    CreateAgentMailPolicyVersionCommand, CreateApprovalPolicyCommand, CreateAuditCheckpointCommand,
    CreateAuditCheckpointService, CreateHumanSessionCommand, CreateMailboxCommand,
    CreateRetentionPolicyCommand, CreateWebhookSubscriptionCommand, DecideApprovalCommand,
    DeleteScimGroupCommand, DraftService, EnsureAgentProcessingSubscriptionCommand,
    ExecuteMailSubmissionCommand, ExternalIdentityResolver, GetAgentInspectorCommand,
    GetAgentMailSubmissionEvidenceCommand, GetApprovalRequestCommand,
    GrantMailboxDelegationCommand, GrantRoleAssignmentCommand, HumanDraftError,
    HumanDraftListItemRecheck, HumanDraftListQuery, HumanDraftListRecheck, HumanDraftQueryStore,
    HumanDraftRestoreQuery, HumanDraftService, HumanMailContentQueryStore,
    HumanMailboxDirectoryError, HumanMailboxDirectoryService, HumanSentError,
    HumanSentListItemRecheck, HumanSentListQuery, HumanSentListRecheck, HumanSentQueryStore,
    HumanSentService, HumanSessionClient, HumanSessionError, HumanSessionRevocationCause,
    HumanSessionService, InboundMailEventIngestionService, IngestInboundMailEventsCommand,
    IngestMailEngineEventsCommand, IssueScimTokenCommand, IssueScimTokenService,
    ListAgentProcessingDeadLettersCommand, ListApprovalRequestsCommand, ListHumanDraftsCommand,
    ListHumanMailboxDirectoryCommand, ListHumanSentSubmissionsCommand, ListRoleAssignmentsCommand,
    MailDataPlaneFingerprintScheme, MailDeliveryQueryStore, MailDeliveryReconciliationError,
    MailDeliveryReconciliationResult, MailDeliveryReconciliationService, MailDomainGovernanceError,
    MailDomainGovernanceService, MailDomainQueryStore, MailDraftMetadataStore,
    MailDraftRecipientEvidence, MailDraftWrite, MailEngineEventIngestionService,
    MailEngineMailboxBindingService, MailEventFingerprintScheme, MailSubmissionExecutionResult,
    MailSubmissionExecutor, MailSubmissionIntentRecord, MailSubmissionService,
    MailboxAuthorizationStore, MailboxDelegationService, MailboxManagementService,
    NackAgentMessageCommand, OpenAgentEventStreamCommand, PendingEvent, ProvisionScimGroupCommand,
    ProvisionScimUserCommand, PullAgentEventsCommand, ReconcileMailDeliveryCommand,
    RecordAuditArchiveCommand, RecordAuditArchiveService, RecordAuditExportDownloadCommand,
    RecordMailDomainDnsCommand, RegisterAgentCommand, RegisterAgentService,
    RegisterMailDomainCommand, ReplaceScimGroupCommand, ReplaceScimUserCommand,
    ReplayWebhookDeadLetterCommand, RequestApprovalCommand, RequestMailSubmissionCommand,
    ResolveAuditExportObjectCommand, ResolveHumanSessionCommand, RestoreHumanDraftCommand,
    RetentionPolicyQueryStore, RetentionPolicyService, RevokeAgentCredentialCommand,
    RevokeAgentCredentialService, RevokeHumanSessionCommand, RevokeMailboxDelegationCommand,
    RevokeRoleAssignmentCommand, RevokeScimTokenCommand, RevokeScimTokenService,
    RoleGovernanceError, RoleGovernanceQueryStore, RoleGovernanceService,
    RotateMailDomainChallengeCommand, SaveTextDraftCommand, SavedTextDraft,
    ScimDirectoryQueryStore, ScimGroupService, ScimUserService, SearchAuditEventsCommand,
    StoreError, SuspendMailDomainCommand, UpdateMailDomainRouteCommand,
    WebhookDeadLetterQueryStore, WebhookDeadLetterService, WebhookSubscriptionService,
    role_assignment_approval_resource_id,
};
use copilotmail_audit::RsaAuditCheckpointSigner;
use copilotmail_crypto::RsaPrivateKey;
use copilotmail_domain::{
    AUDIT_EVENT_CATALOG, AgentEnvironment, AgentRiskLevel, ApprovalAction, ApprovalDecision,
    ApprovalStatus, AuditArchiveId, AuditCheckpointId, AuditRisk, AuthenticationBinding,
    AuthorizationContext, ComplianceCaseId, ComplianceCaseStatus, DnsVerificationChecks,
    DnsVerificationEvidence, DraftId, HumanSessionId, LeaseId, MailDataPlaneId,
    MailDomainRouteMode, MailDomainStatus, MailEngineProjectionStatus, MailboxAddress,
    MailboxCapability, MailboxKind, OrganizationId, Permission, PrincipalId, RetentionResourceKind,
    ScimTokenId, SystemRole,
};
use copilotmail_postgres::{PgControlPlaneStore, WebhookDeliveryOutcome};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use uuid::Uuid;

const fn legacy_mail_data_plane_id() -> MailDataPlaneId {
    MailDataPlaneId::from_uuid(Uuid::from_u128(0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2101))
}

const fn other_mail_data_plane_id() -> MailDataPlaneId {
    MailDataPlaneId::from_uuid(Uuid::from_u128(0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2102))
}
fn legacy_mail_event_fingerprint_scheme() -> MailEventFingerprintScheme {
    MailEventFingerprintScheme::try_legacy_source("stalwart")
        .expect("static legacy mail event source is valid")
}
/// Serializes tests that share the cluster-wide login admission windows and
/// login/session evidence tables (begin_login consumes a shared window count
/// and sweeps expired login rows).
static LOGIN_GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn migrated_store() -> PgControlPlaneStore {
    static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    let database_url = support::database_url();
    let store = PgControlPlaneStore::connect(&database_url, 5)
        .await
        .expect("connect integration database");
    MIGRATED
        .get_or_try_init(|| async { store.migrate().await })
        .await
        .expect("apply migrations");
    store
}

/// PostgreSQL `timestamptz` stores microseconds; truncate fixture timestamps so
/// domain values round-trip exactly through storage for equality assertions.
fn pg_micros(value: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp_micros(value.timestamp_micros())
        .expect("microsecond timestamp is representable")
}

fn bootstrap_command(slug: String) -> BootstrapOrganizationCommand {
    BootstrapOrganizationCommand {
        slug,
        display_name: "PostgreSQL Integration Organization".into(),
        root_principal_name: "Integration Bootstrap".into(),
        trace_id: Uuid::now_v7().to_string(),
        now: Utc::now(),
    }
}

#[tokio::test]
#[ignore = "requires an explicitly opted-in disposable PostgreSQL database"]
async fn retention_permission_recheck_and_single_connection_read_are_enforced() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "retention-code-quality-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .unwrap();
    let now = Utc::now();
    let actor = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    // Preserve an independent owner while the first actor loses its grant.
    insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let context = store
        .resolve_authorization(actor, now)
        .await
        .unwrap()
        .unwrap();
    let policy = RetentionPolicyService::new(store.clone())
        .create(CreateRetentionPolicyCommand {
            context: context.clone(),
            policy_key: "audit.quality".into(),
            display_name: "Quality policy".into(),
            resource_kind: RetentionResourceKind::AuditEvidence,
            minimum_retention_days: 365,
            archive_after_days: None,
            delete_after_days: None,
            rule: json!({}),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .unwrap();

    let single = PgControlPlaneStore::connect(&support::database_url(), 1)
        .await
        .unwrap();
    let read = copilotmail_application::GetRetentionPolicyCommand {
        context: context.clone(),
        policy_id: policy.policy.id(),
        trace_id: Uuid::new_v4().to_string(),
        now,
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        RetentionPolicyService::new(single).get(read.clone()),
    )
    .await
    .expect("read must not acquire a second pool connection")
    .unwrap();
    assert_eq!(result.policy.id(), policy.policy.id());

    sqlx::query("UPDATE role_assignments SET revoked_at = clock_timestamp() WHERE organization_id = $1 AND principal_id = $2 AND revoked_at IS NULL")
        .bind(bootstrap.organization_id.as_uuid()).bind(actor.as_uuid()).execute(store.pool()).await.unwrap();
    assert!(matches!(
        RetentionPolicyService::new(store.clone()).get(read).await,
        Err(copilotmail_application::RetentionPolicyError::Forbidden)
    ));
    let mut transaction = store.begin().await.unwrap();
    assert!(
        !transaction
            .authorize_human_permission(
                bootstrap.organization_id,
                actor,
                Permission::RetentionManage
            )
            .await
            .unwrap()
    );
    assert!(
        !transaction
            .authorize_principal_permission(
                bootstrap.organization_id,
                actor,
                Permission::RetentionManage,
            )
            .await
            .unwrap()
    );
}

#[tokio::test]
#[ignore = "requires an explicitly opted-in disposable PostgreSQL database"]
async fn retention_activation_rejects_backdated_expired_approval() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "retention-expiry-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .unwrap();
    let now = Utc::now();
    let actor = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: "https://quality-owner.example.test".into(),
            subject: Uuid::new_v4().to_string(),
            display_name: "Retention Test Owner".into(),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .unwrap()
        .principal_id;
    let approver_a = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let approver_b = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let context = store
        .resolve_authorization(actor, now)
        .await
        .unwrap()
        .unwrap();
    let draft = RetentionPolicyService::new(store.clone())
        .create(CreateRetentionPolicyCommand {
            context: context.clone(),
            policy_key: "audit.expiry".into(),
            display_name: "Expiry boundary".into(),
            resource_kind: RetentionResourceKind::AuditEvidence,
            minimum_retention_days: 365,
            archive_after_days: None,
            delete_after_days: None,
            rule: json!({}),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .unwrap();
    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "retention.expiry.review".into(),
            display_name: "Retention expiry review".into(),
            action: ApprovalAction::RetentionChange,
            required_approvals: 2,
            rule: json!({}),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .unwrap();
    let database_now: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(store.pool())
        .await
        .unwrap();
    let expires_at = database_now + chrono::Duration::seconds(5);
    let workflow = ApprovalWorkflowService::new(store.clone());
    let approval_id = workflow
        .request(RequestApprovalCommand {
            context,
            policy_key: "retention.expiry.review".into(),
            action: ApprovalAction::RetentionChange,
            resource_kind: "compliance.retention_policy_version".into(),
            resource_id: draft.version.id().to_string(),
            reason: "Expiry must use execution time".into(),
            ticket_reference: None,
            expires_at,
            trace_id: Uuid::new_v4().to_string(),
            now: database_now,
        })
        .await
        .unwrap();
    for approver in [approver_a, approver_b] {
        let context = store
            .resolve_authorization(approver, database_now)
            .await
            .unwrap()
            .unwrap();
        workflow
            .decide(DecideApprovalCommand {
                context,
                request_id: approval_id,
                decision: ApprovalDecision::Approved,
                reason: "Reviewed exact version".into(),
                trace_id: Uuid::new_v4().to_string(),
                now: database_now,
            })
            .await
            .unwrap();
    }
    // Wait until the database clock confirms the approval has expired; timer
    // granularity on virtualized hosts cannot be trusted for the boundary.
    for _ in 0..120 {
        let observed: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(store.pool())
            .await
            .unwrap();
        if observed > expires_at {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    // Simulate a queued operation retaining its original, pre-expiry timestamp.
    // The trigger must overwrite it with DB time and reject the expired proof.
    let result = sqlx::query("INSERT INTO retention_policy_activations(id,organization_id,policy_id,policy_version_id,approval_request_id,activated_by,activated_at) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(Uuid::new_v4()).bind(bootstrap.organization_id.as_uuid())
        .bind(draft.policy.id().as_uuid()).bind(draft.version.id().as_uuid())
        .bind(approval_id.as_uuid()).bind(actor.as_uuid()).bind(database_now)
        .execute(store.pool()).await;
    assert_check_violation(result);
}

async fn database_now(store: &PgControlPlaneStore) -> chrono::DateTime<Utc> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(store.pool())
        .await
        .expect("read database clock")
}

struct RetentionClockTenant {
    store: PgControlPlaneStore,
    organization_id: OrganizationId,
    actor: PrincipalId,
    approvers: [PrincipalId; 2],
    policy_id: copilotmail_domain::RetentionPolicyId,
    first_version_id: copilotmail_domain::RetentionPolicyVersionId,
}

async fn bootstrap_retention_clock_tenant(slug_prefix: &str) -> RetentionClockTenant {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "{slug_prefix}-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap retention clock tenant");
    let now = Utc::now();
    let actor = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: "https://quality-owner.example.test".into(),
            subject: Uuid::new_v4().to_string(),
            display_name: "Retention Clock Owner".into(),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("bind retention clock owner")
        .principal_id;
    let approvers = [
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            now,
        )
        .await,
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            now,
        )
        .await,
    ];
    let context = store
        .resolve_authorization(actor, now)
        .await
        .unwrap()
        .expect("resolve retention clock owner");
    let draft = RetentionPolicyService::new(store.clone())
        .create(CreateRetentionPolicyCommand {
            context,
            policy_key: "audit.clock".into(),
            display_name: "Clock policy".into(),
            resource_kind: RetentionResourceKind::AuditEvidence,
            minimum_retention_days: 365,
            archive_after_days: None,
            delete_after_days: None,
            rule: json!({}),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("create retention clock policy");
    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "retention.clock.review".into(),
            display_name: "Retention clock review".into(),
            action: ApprovalAction::RetentionChange,
            required_approvals: 2,
            rule: json!({}),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("create retention clock approval policy");
    RetentionClockTenant {
        store,
        organization_id: bootstrap.organization_id,
        actor,
        approvers,
        policy_id: draft.policy.id(),
        first_version_id: draft.version.id(),
    }
}

async fn approve_retention_version(
    tenant: &RetentionClockTenant,
    resource_id: &str,
    lifetime: chrono::Duration,
) -> (Uuid, chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
    let now = database_now(&tenant.store).await;
    let context = tenant
        .store
        .resolve_authorization(tenant.actor, now)
        .await
        .unwrap()
        .expect("resolve retention requester");
    let workflow = ApprovalWorkflowService::new(tenant.store.clone());
    let approval_id = workflow
        .request(RequestApprovalCommand {
            context,
            policy_key: "retention.clock.review".into(),
            action: ApprovalAction::RetentionChange,
            resource_kind: "compliance.retention_policy_version".into(),
            resource_id: resource_id.to_string(),
            reason: "Database clock must authorize retention activation".into(),
            ticket_reference: None,
            expires_at: now + lifetime,
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("request retention approval");
    for approver in tenant.approvers {
        let context = tenant
            .store
            .resolve_authorization(approver, now)
            .await
            .unwrap()
            .expect("resolve retention approver");
        workflow
            .decide(DecideApprovalCommand {
                context,
                request_id: approval_id,
                decision: ApprovalDecision::Approved,
                reason: "Reviewed exact version".into(),
                trace_id: Uuid::new_v4().to_string(),
                now,
            })
            .await
            .expect("approve retention version");
    }
    let stored: (chrono::DateTime<Utc>, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT created_at, expires_at FROM approval_requests WHERE id = $1")
            .bind(approval_id.as_uuid())
            .fetch_one(tenant.store.pool())
            .await
            .expect("read stored approval window");
    assert!(stored.1 > stored.0);
    (approval_id.as_uuid(), stored.0, stored.1)
}

const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

async fn expect_no_lock_hang<T>(operation: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(LOCK_WAIT, operation)
        .await
        .expect("operation blocked on a database lock for more than 15s")
}

fn assert_constraint_rejection(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) {
    let error = result.expect_err("stored database time must reject the caller clock");
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("23514")
    );
}

async fn sleep_until_database_deadline(
    blocker: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deadline: chrono::DateTime<Utc>,
) {
    sqlx::query(
        "SELECT pg_sleep(GREATEST(EXTRACT(EPOCH FROM ($1::timestamptz - clock_timestamp())), 0) + 0.05)",
    )
    .bind(deadline)
    .execute(&mut **blocker)
    .await
    .expect("hold the row lock across the database deadline");
}

#[tokio::test]
#[ignore = "requires an explicitly opted-in disposable PostgreSQL database"]
async fn retention_activation_samples_database_time_after_approval_lock() {
    let tenant = bootstrap_retention_clock_tenant("ret-lock").await;
    let (approval_id, stored_created_at, expires_at) = approve_retention_version(
        &tenant,
        &tenant.first_version_id.to_string(),
        chrono::Duration::seconds(20),
    )
    .await;
    let mut blocker = tenant
        .store
        .pool()
        .begin()
        .await
        .expect("begin approval lock");
    let locked: Uuid =
        sqlx::query_scalar("SELECT id FROM approval_requests WHERE id = $1 FOR UPDATE")
            .bind(approval_id)
            .fetch_one(&mut *blocker)
            .await
            .expect("lock approval request");
    assert_eq!(locked, approval_id);

    // The supplied timestamp is still inside the approval window. Keeping it
    // would authorize the insert; only a clock read after this lock wait fails.
    let mut waiting = Box::pin(
        sqlx::query(
            r#"
            INSERT INTO retention_policy_activations(
                id, organization_id, policy_id, policy_version_id,
                approval_request_id, activated_by, activated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(tenant.organization_id.as_uuid())
        .bind(tenant.policy_id.as_uuid())
        .bind(tenant.first_version_id.as_uuid())
        .bind(approval_id)
        .bind(tenant.actor.as_uuid())
        .bind(stored_created_at)
        .execute(tenant.store.pool()),
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), &mut waiting)
            .await
            .is_err(),
        "activation insert must wait until the approval lock is released"
    );
    assert!(
        database_now(&tenant.store).await < expires_at,
        "the lock wait must begin before the approval expires"
    );
    sleep_until_database_deadline(&mut blocker, expires_at).await;
    blocker.commit().await.expect("release approval lock");
    assert_constraint_rejection(expect_no_lock_hang(waiting).await);
    let kept: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM retention_policy_activations WHERE approval_request_id = $1",
    )
    .bind(approval_id)
    .fetch_one(tenant.store.pool())
    .await
    .expect("count rejected activation");
    assert_eq!(kept, 0);
}

#[tokio::test]
#[ignore = "requires an explicitly opted-in disposable PostgreSQL database"]
async fn retention_activation_pointer_change_rechecks_approval() {
    let tenant = bootstrap_retention_clock_tenant("ret-pointer").await;
    let (first_approval, first_created_at, _) = approve_retention_version(
        &tenant,
        &tenant.first_version_id.to_string(),
        chrono::Duration::hours(1),
    )
    .await;
    let caller_time = first_created_at - chrono::Duration::days(1);
    let before = database_now(&tenant.store).await;
    let stored_at: chrono::DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO retention_policy_activations(
            id, organization_id, policy_id, policy_version_id,
            approval_request_id, activated_by, activated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING activated_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant.organization_id.as_uuid())
    .bind(tenant.policy_id.as_uuid())
    .bind(tenant.first_version_id.as_uuid())
    .bind(first_approval)
    .bind(tenant.actor.as_uuid())
    .bind(caller_time)
    .fetch_one(tenant.store.pool())
    .await
    .expect("insert fresh retention activation");
    let after = database_now(&tenant.store).await;
    assert!(stored_at > caller_time + chrono::Duration::hours(12));
    assert!(stored_at >= before && stored_at <= after);
    let moved = sqlx::query(
        r#"
        UPDATE retention_policies
           SET active_version_id = $3
         WHERE organization_id = $1
           AND id = $2
           AND active_version_id IS NOT DISTINCT FROM $4
        "#,
    )
    .bind(tenant.organization_id.as_uuid())
    .bind(tenant.policy_id.as_uuid())
    .bind(tenant.first_version_id.as_uuid())
    .bind(None::<Uuid>)
    .execute(tenant.store.pool())
    .await
    .expect("fresh approval lets the active pointer move");
    assert_eq!(moved.rows_affected(), 1);

    let now = database_now(&tenant.store).await;
    let context = tenant
        .store
        .resolve_authorization(tenant.actor, now)
        .await
        .unwrap()
        .expect("resolve version author");
    let second = RetentionPolicyService::new(tenant.store.clone())
        .append_version(AppendRetentionPolicyVersionCommand {
            context,
            policy_id: tenant.policy_id,
            minimum_retention_days: 400,
            archive_after_days: None,
            delete_after_days: None,
            rule: json!({}),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("append second retention version");
    let (second_approval, _, second_expires_at) = approve_retention_version(
        &tenant,
        &second.id().to_string(),
        chrono::Duration::seconds(20),
    )
    .await;
    sqlx::query(
        r#"
        INSERT INTO retention_policy_activations(
            id, organization_id, policy_id, policy_version_id,
            approval_request_id, activated_by, activated_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant.organization_id.as_uuid())
    .bind(tenant.policy_id.as_uuid())
    .bind(second.id().as_uuid())
    .bind(second_approval)
    .bind(tenant.actor.as_uuid())
    .bind(caller_time)
    .execute(tenant.store.pool())
    .await
    .expect("activation evidence exists before the pointer moves");

    let mut blocker = tenant
        .store
        .pool()
        .begin()
        .await
        .expect("begin pointer approval lock");
    let locked: Uuid =
        sqlx::query_scalar("SELECT id FROM approval_requests WHERE id = $1 FOR UPDATE")
            .bind(second_approval)
            .fetch_one(&mut *blocker)
            .await
            .expect("lock the second approval");
    assert_eq!(locked, second_approval);
    let mut waiting = Box::pin(
        sqlx::query(
            r#"
            UPDATE retention_policies
               SET active_version_id = $3
             WHERE organization_id = $1
               AND id = $2
               AND active_version_id IS NOT DISTINCT FROM $4
            "#,
        )
        .bind(tenant.organization_id.as_uuid())
        .bind(tenant.policy_id.as_uuid())
        .bind(second.id().as_uuid())
        .bind(tenant.first_version_id.as_uuid())
        .execute(tenant.store.pool()),
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), &mut waiting)
            .await
            .is_err(),
        "pointer update must wait until the approval lock is released"
    );
    assert!(
        database_now(&tenant.store).await < second_expires_at,
        "the pointer recheck must start before the approval expires"
    );
    sleep_until_database_deadline(&mut blocker, second_expires_at).await;
    blocker
        .commit()
        .await
        .expect("release the second approval lock");
    assert_constraint_rejection(expect_no_lock_hang(waiting).await);
    let active: Uuid =
        sqlx::query_scalar("SELECT active_version_id FROM retention_policies WHERE id = $1")
            .bind(tenant.policy_id.as_uuid())
            .fetch_one(tenant.store.pool())
            .await
            .expect("read unchanged active pointer");
    assert_eq!(active, tenant.first_version_id.as_uuid());
}

#[tokio::test]
#[ignore = "requires an explicitly opted-in disposable PostgreSQL database"]
async fn audit_export_download_samples_database_time_after_actor_lock() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "exp-lock-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap export clock tenant");
    let private = RsaPrivateKey::generate_for_testing().expect("generate export clock key");
    let public = private.public_key();
    let private_pem = private
        .pkcs8_pem_for_testing()
        .expect("encode export clock private key");
    let public_pem = public.spki_pem().expect("encode export clock public key");
    let signer = RsaAuditCheckpointSigner::new(
        "audit-clock-2026".into(),
        private_pem.as_bytes(),
        public_pem.as_bytes(),
    )
    .expect("configure export clock signer");
    let checkpoint_id = AuditCheckpointId::new();
    CreateAuditCheckpointService::new(store.clone(), signer)
        .execute(CreateAuditCheckpointCommand {
            checkpoint_id,
            organization_id: bootstrap.organization_id,
            actor_principal_id: bootstrap.root_principal_id,
            archive_object_key: format!(
                "audit/{}/{}.jsonl",
                bootstrap.organization_id, checkpoint_id
            ),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("create export clock checkpoint");
    let verified_at = pg_micros(Utc::now());
    let archive_id = AuditArchiveId::new();
    let events_key = format!("audit/{checkpoint_id}/events.jsonl");
    let events_version = "events-version-1".to_string();
    let events_sha = "a".repeat(64);
    let events_size = 1024_i64;
    let archive = AuditArchiveRecord {
        archive_id,
        organization_id: bootstrap.organization_id,
        checkpoint_id,
        provider: "s3".into(),
        bucket: "copilotmail-audit".into(),
        events: ArchivedObjectRecord {
            key: events_key.clone(),
            version_id: events_version.clone(),
            sha256_hex: events_sha.clone(),
            size_bytes: u64::try_from(events_size).unwrap(),
        },
        manifest: ArchivedObjectRecord {
            key: format!("audit/{checkpoint_id}/manifest.json"),
            version_id: "manifest-version-1".into(),
            sha256_hex: "b".repeat(64),
            size_bytes: 512,
        },
        retention_mode: "compliance".into(),
        retain_until: verified_at + chrono::Duration::days(365),
        verified_at,
        created_by: bootstrap.root_principal_id,
        created_at: verified_at + chrono::Duration::milliseconds(1),
    };
    RecordAuditArchiveService::new(store.clone())
        .execute(RecordAuditArchiveCommand {
            archive,
            actor_principal_id: bootstrap.root_principal_id,
            trace_id: Uuid::now_v7().to_string(),
        })
        .await
        .expect("record export clock archive");
    let now = database_now(&store).await;
    let requester = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: "https://quality-owner.example.test".into(),
            subject: Uuid::new_v4().to_string(),
            display_name: "Export Clock Owner".into(),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("bind export clock owner")
        .principal_id;
    let approvers = [
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            now,
        )
        .await,
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            now,
        )
        .await,
    ];
    let context = store
        .resolve_authorization(requester, now)
        .await
        .unwrap()
        .expect("resolve export requester");
    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "audit.clock.export".into(),
            display_name: "Export clock review".into(),
            action: ApprovalAction::AuditExport,
            required_approvals: 2,
            rule: json!({}),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("create export clock policy");
    let workflow = ApprovalWorkflowService::new(store.clone());
    let approval_id = workflow
        .request(RequestApprovalCommand {
            context: context.clone(),
            policy_key: "audit.clock.export".into(),
            action: ApprovalAction::AuditExport,
            resource_kind: "audit.archive".into(),
            resource_id: archive_id.to_string(),
            reason: "Database clock must authorize the download ledger".into(),
            ticket_reference: None,
            expires_at: now + chrono::Duration::seconds(20),
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("request export approval");
    for approver in approvers {
        let approver_context = store
            .resolve_authorization(approver, now)
            .await
            .unwrap()
            .expect("resolve export approver");
        workflow
            .decide(DecideApprovalCommand {
                context: approver_context,
                request_id: approval_id,
                decision: ApprovalDecision::Approved,
                reason: "Reviewed exact archive".into(),
                trace_id: Uuid::new_v4().to_string(),
                now,
            })
            .await
            .expect("approve export");
    }
    let grant = AuditExportAuthorizationService::new(store.clone())
        .authorize(AuthorizeAuditExportCommand {
            context,
            archive_id,
            approval: ApprovalProof {
                organization_id: bootstrap.organization_id,
                request_id: approval_id,
                requested_by: requester,
                action: ApprovalAction::AuditExport,
                resource_kind: "audit.archive".into(),
                resource_id: archive_id.to_string(),
                now,
            },
            trace_id: Uuid::new_v4().to_string(),
            now,
        })
        .await
        .expect("authorize short export grant");
    let (grant_created_at, grant_expires_at): (chrono::DateTime<Utc>, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT created_at, expires_at FROM audit_export_grants WHERE id = $1")
            .bind(grant.id().as_uuid())
            .fetch_one(store.pool())
            .await
            .expect("read stored grant window");
    assert!(database_now(&store).await < grant_expires_at);
    let caller_time = grant_created_at - chrono::Duration::days(1);
    let before = database_now(&store).await;
    let stored_download_at: chrono::DateTime<Utc> = sqlx::query_scalar(
        r#"
        INSERT INTO audit_export_downloads(
            id, organization_id, grant_id, archive_id, downloaded_by,
            artifact, object_key, object_version_id, sha256, size_bytes,
            downloaded_at
        )
        VALUES ($1, $2, $3, $4, $5, 'events', $6, $7, $8, $9, $10)
        RETURNING downloaded_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(grant.id().as_uuid())
    .bind(archive_id.as_uuid())
    .bind(requester.as_uuid())
    .bind(&events_key)
    .bind(&events_version)
    .bind(&events_sha)
    .bind(events_size)
    .bind(caller_time)
    .fetch_one(store.pool())
    .await
    .expect("record a download with a backdated caller clock");
    let after = database_now(&store).await;
    assert!(stored_download_at > caller_time + chrono::Duration::hours(12));
    assert!(stored_download_at >= before && stored_download_at <= after);
    assert!(stored_download_at >= grant_created_at && stored_download_at < grant_expires_at);

    let mut blocker = store.pool().begin().await.expect("begin actor lock");
    let locked: Uuid = sqlx::query_scalar("SELECT id FROM principals WHERE id = $1 FOR UPDATE")
        .bind(requester.as_uuid())
        .fetch_one(&mut *blocker)
        .await
        .expect("lock download actor");
    assert_eq!(locked, requester.as_uuid());
    let mut waiting = Box::pin(
        sqlx::query(
            r#"
            INSERT INTO audit_export_downloads(
                id, organization_id, grant_id, archive_id, downloaded_by,
                artifact, object_key, object_version_id, sha256, size_bytes,
                downloaded_at
            )
            VALUES ($1, $2, $3, $4, $5, 'events', $6, $7, $8, $9, $10)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(bootstrap.organization_id.as_uuid())
        .bind(grant.id().as_uuid())
        .bind(archive_id.as_uuid())
        .bind(requester.as_uuid())
        .bind(&events_key)
        .bind(&events_version)
        .bind(&events_sha)
        .bind(events_size)
        .bind(grant_created_at)
        .execute(store.pool()),
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), &mut waiting)
            .await
            .is_err(),
        "download insert must wait until the actor lock is released"
    );
    assert!(
        database_now(&store).await < grant_expires_at,
        "the download lock wait must begin before the grant expires"
    );
    sleep_until_database_deadline(&mut blocker, grant_expires_at).await;
    blocker.commit().await.expect("release actor lock");
    assert_constraint_rejection(expect_no_lock_hang(waiting).await);
}

fn random_numeric_engine_ref() -> String {
    const MAX_REF: u128 = 9_000_000_000_000_000_000;
    ((Uuid::new_v4().as_u128() % MAX_REF) + 1).to_string()
}

async fn create_bound_human_mailbox_fixture(
    store: &PgControlPlaneStore,
    slug_prefix: &str,
    account_ref: &str,
    now: chrono::DateTime<Utc>,
) -> (OrganizationId, PrincipalId, copilotmail_domain::MailboxId) {
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "{slug_prefix}-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Human mail projection tenant");
    let human_id = insert_approval_test_owner(
        store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let context = store
        .resolve_authorization(human_id, now)
        .await
        .expect("resolve Human mail projection owner")
        .expect("Human mail projection owner is active");
    let mailbox = MailboxManagementService::new(store.clone())
        .create_authorized(
            &context,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Human,
                primary_address: format!("human-{}@example.test", Uuid::new_v4().simple()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create Human mail projection mailbox");
    MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct legacy binding service")
    .bind_authorized(
        &context,
        BindMailEngineMailboxCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            account_ref: account_ref.into(),
            expected_version: 0,
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    )
    .await
    .expect("bind Human mail projection mailbox");
    (bootstrap.organization_id, human_id, mailbox.mailbox_id)
}

async fn insert_human_mail_projection_event(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    mailbox_id: copilotmail_domain::MailboxId,
    account_ref: &str,
    document_ref: &str,
    message_id: Uuid,
    now: chrono::DateTime<Utc>,
) {
    let source_event_sha256 = Sha256::digest(message_id.as_bytes());
    sqlx::query(
        r#"
        INSERT INTO inbound_mail_engine_events(
            event_id, message_id, organization_id, mailbox_id, source,
            source_event_id, source_event_sha256, account_ref, document_ref,
            classification, source_occurred_at, received_at
        )
        VALUES ($1, $2, $3, $4, 'stalwart', $5, $6, $7, $8,
                'ham', $9, $9)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(message_id)
    .bind(organization_id.as_uuid())
    .bind(mailbox_id.as_uuid())
    .bind(format!("human-projection-{}", Uuid::new_v4().simple()))
    .bind(source_event_sha256.as_slice())
    .bind(account_ref)
    .bind(document_ref)
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert immutable Human mail projection event");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn audit_maintenance_lock_is_tenant_scoped_exclusive_and_cancel_safe() {
    let store = migrated_store().await;
    let organization_id = OrganizationId::new();
    let another_organization_id = OrganizationId::new();

    let first = store
        .try_acquire_audit_maintenance_lock(organization_id)
        .await
        .expect("acquire first lock")
        .expect("first lock is available");
    assert!(
        store
            .try_acquire_audit_maintenance_lock(organization_id)
            .await
            .expect("contending lock query")
            .is_none(),
        "the same tenant must have exactly one active maintainer"
    );
    let other_tenant = store
        .try_acquire_audit_maintenance_lock(another_organization_id)
        .await
        .expect("acquire other tenant lock")
        .expect("another tenant has an independent lock");

    first.release().await.expect("release first lock");
    let replacement = store
        .try_acquire_audit_maintenance_lock(organization_id)
        .await
        .expect("reacquire released lock")
        .expect("released lock is immediately reusable");

    drop(replacement);
    let mut recovered = None;
    for _ in 0..20 {
        if let Some(lock) = store
            .try_acquire_audit_maintenance_lock(organization_id)
            .await
            .expect("reacquire cancelled lock")
        {
            recovered = Some(lock);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    recovered
        .expect("dropping the guard closes its session and releases the lock")
        .release()
        .await
        .expect("release recovered lock");
    other_tenant
        .release()
        .await
        .expect("release other tenant lock");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn audit_legal_hold_lock_is_archive_scoped_exclusive_and_cancel_safe() {
    let store = migrated_store().await;
    let organization_id = OrganizationId::new();
    let archive_id = AuditArchiveId::new();
    let another_archive_id = AuditArchiveId::new();

    let first = store
        .try_acquire_audit_legal_hold_lock(organization_id, archive_id)
        .await
        .expect("acquire first Legal Hold lock")
        .expect("first archive lock is available");
    assert!(
        store
            .try_acquire_audit_legal_hold_lock(organization_id, archive_id)
            .await
            .expect("contending Legal Hold lock query")
            .is_none(),
        "the same tenant archive must have one active Legal Hold operation"
    );
    let other_archive = store
        .try_acquire_audit_legal_hold_lock(organization_id, another_archive_id)
        .await
        .expect("acquire another archive lock")
        .expect("another archive has an independent lock");

    drop(first);
    let mut recovered = None;
    for _ in 0..20 {
        if let Some(lock) = store
            .try_acquire_audit_legal_hold_lock(organization_id, archive_id)
            .await
            .expect("reacquire cancelled Legal Hold lock")
        {
            recovered = Some(lock);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    recovered
        .expect("dropping the guard closes its session and releases the archive lock")
        .release()
        .await
        .expect("release recovered archive lock");
    other_archive
        .release()
        .await
        .expect("release independent archive lock");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn outbox_rollback_skip_locked_release_and_expired_lease_are_recoverable() {
    let store = migrated_store().await;
    let now = Utc::now();

    let rollback_event_id = Uuid::now_v7();
    let mut unit = store.begin().await.expect("begin rollback unit");
    unit.enqueue_event(&PendingEvent::with_id(
        rollback_event_id,
        "integration.rollback.v1",
        "integration_test",
        rollback_event_id.to_string(),
        json!({"organization_id": OrganizationId::new().to_string()}),
        now,
    ))
    .await
    .expect("stage rollback event");
    drop(unit);

    let rollback_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE id = $1")
            .bind(rollback_event_id)
            .fetch_one(store.pool())
            .await
            .expect("count rolled back event");
    assert_eq!(
        rollback_count, 0,
        "dropping an application unit of work must roll back its Outbox writes"
    );

    let first_id = Uuid::now_v7();
    let second_id = Uuid::now_v7();
    for (id, occurred_at) in [
        (first_id, now - chrono::Duration::days(36_500)),
        (
            second_id,
            now - chrono::Duration::days(36_500) + chrono::Duration::seconds(1),
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO outbox_events(
                id, event_type, aggregate_type, aggregate_id,
                schema_version, payload, occurred_at, next_attempt_at
            )
            VALUES ($1, 'integration.outbox.v1', 'integration_test', $2, 1, $3, $4, $5)
            "#,
        )
        .bind(id)
        .bind(id.to_string())
        .bind(json!({"organization_id": OrganizationId::new().to_string()}))
        .bind(occurred_at)
        .bind(now - chrono::Duration::minutes(1))
        .execute(store.pool())
        .await
        .expect("insert deterministic Outbox fixture");
    }

    let mut held = store.pool().begin().await.expect("begin row lock");
    sqlx::query("SELECT id FROM outbox_events WHERE id = $1 FOR UPDATE")
        .bind(first_id)
        .fetch_one(&mut *held)
        .await
        .expect("hold the oldest Outbox row");

    let skip_worker = Uuid::now_v7();
    let skipped = expect_no_lock_hang(store.claim_outbox_batch(skip_worker, None, 1))
        .await
        .expect("claim around held row");
    assert_eq!(skipped.len(), 1);
    assert_eq!(
        skipped[0].id, second_id,
        "SKIP LOCKED must allow another worker to make progress"
    );
    held.rollback().await.expect("release held row");

    let recovery_worker = Uuid::now_v7();
    let claimed = store
        .claim_outbox_batch(recovery_worker, None, 1)
        .await
        .expect("claim previously held row");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, first_id);
    assert_eq!(claimed[0].attempts, 1);

    assert!(
        !store
            .release_outbox_event(second_id, recovery_worker, "wrong_worker")
            .await
            .expect("reject release by wrong worker")
    );
    assert!(
        store
            .release_outbox_event(second_id, skip_worker, "receiver_unavailable")
            .await
            .expect("release failed delivery")
    );
    let (locked_by, last_error, retry_is_delayed): (Option<Uuid>, Option<String>, bool) =
        sqlx::query_as(
            r#"
            SELECT locked_by, last_error, next_attempt_at > now()
              FROM outbox_events
             WHERE id = $1
            "#,
        )
        .bind(second_id)
        .fetch_one(store.pool())
        .await
        .expect("read released delivery");
    assert_eq!(locked_by, None);
    assert_eq!(last_error.as_deref(), Some("receiver_unavailable"));
    assert!(retry_is_delayed);

    sqlx::query(
        r#"
        UPDATE outbox_events
           SET locked_at = now() - interval '6 minutes'
         WHERE id = $1
           AND locked_by = $2
        "#,
    )
    .bind(first_id)
    .bind(recovery_worker)
    .execute(store.pool())
    .await
    .expect("expire worker lease");

    let replacement_worker = Uuid::now_v7();
    let reclaimed = store
        .claim_outbox_batch(replacement_worker, None, 1)
        .await
        .expect("reclaim expired lease");
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].id, first_id);
    assert_eq!(reclaimed[0].attempts, 2);
    assert!(
        !store
            .mark_outbox_published(first_id, recovery_worker)
            .await
            .expect("reject stale worker completion")
    );
    assert!(
        store
            .mark_outbox_published(first_id, replacement_worker)
            .await
            .expect("complete reclaimed delivery")
    );

    sqlx::query("DELETE FROM outbox_events WHERE id = ANY($1)")
        .bind([first_id, second_id])
        .execute(store.pool())
        .await
        .expect("clean Outbox fixtures");
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn compliance_case_pages_are_bounded_filter_and_cursor_scoped() {
    let store = migrated_store().await;
    let organization_id = OrganizationId::new();
    let actor_id = PrincipalId::new();
    let now = Utc::now();
    sqlx::query(
        r#"
        INSERT INTO organizations(id, slug, display_name, created_at, updated_at)
        VALUES ($1, $2, 'Compliance Query Integration', $3, $3)
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(format!("compliance-{}", Uuid::new_v4().simple()))
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert compliance query organization");
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        )
        VALUES ($1, $2, 'human', 'active', 'Compliance Actor', $3, $3)
        "#,
    )
    .bind(actor_id.as_uuid())
    .bind(organization_id.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert compliance query actor");

    let newest = ComplianceCaseId::new();
    let middle = ComplianceCaseId::new();
    let oldest = ComplianceCaseId::new();
    let closed = ComplianceCaseId::new();
    for (id, reference, created_at, status) in [
        (newest, "CASE-NEW", now, "open"),
        (
            middle,
            "CASE-MIDDLE",
            now - chrono::Duration::seconds(1),
            "open",
        ),
        (
            oldest,
            "CASE-OLD",
            now - chrono::Duration::seconds(2),
            "open",
        ),
        (
            closed,
            "CASE-CLOSED",
            now - chrono::Duration::seconds(3),
            "closed",
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO compliance_cases(
                id, organization_id, external_reference, title, purpose, status,
                created_by, created_at, closed_by, closed_at
            )
            VALUES (
                $1, $2, $3, 'Evidence case', 'Integration verification', $4,
                $5, $6,
                CASE WHEN $4 = 'closed' THEN $5 ELSE NULL END,
                CASE WHEN $4 = 'closed' THEN $6 ELSE NULL END
            )
            "#,
        )
        .bind(id.as_uuid())
        .bind(organization_id.as_uuid())
        .bind(reference)
        .bind(status)
        .bind(actor_id.as_uuid())
        .bind(created_at)
        .execute(store.pool())
        .await
        .expect("insert compliance case");
    }

    let first = store
        .list_compliance_cases(organization_id, Some(ComplianceCaseStatus::Open), None, 2)
        .await
        .expect("list first compliance page");
    assert_eq!(
        first.iter().map(|case| case.id()).collect::<Vec<_>>(),
        vec![newest, middle]
    );
    assert!(
        store
            .compliance_case_cursor_exists(
                organization_id,
                Some(ComplianceCaseStatus::Open),
                middle,
            )
            .await
            .expect("validate matching cursor")
    );
    assert!(
        !store
            .compliance_case_cursor_exists(
                organization_id,
                Some(ComplianceCaseStatus::Closed),
                middle,
            )
            .await
            .expect("reject cursor after filter change")
    );
    let second = store
        .list_compliance_cases(
            organization_id,
            Some(ComplianceCaseStatus::Open),
            Some(middle),
            2,
        )
        .await
        .expect("list second compliance page");
    assert_eq!(
        second.iter().map(|case| case.id()).collect::<Vec<_>>(),
        vec![oldest]
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn concurrent_bootstrap_is_idempotent_and_atomic() {
    let store = migrated_store().await;
    assert!(
        store
            .schema_ready()
            .await
            .expect("inspect migrated schema readiness")
    );
    sqlx::query(
        "ALTER TABLE agent_mail_admissions
         DISABLE TRIGGER agent_mail_admissions_validate_insert",
    )
    .execute(store.pool())
    .await
    .expect("disable authoritative Agent admission trigger for readiness probe");
    assert!(
        !store
            .schema_ready()
            .await
            .expect("reject incomplete schema readiness")
    );
    sqlx::query(
        "ALTER TABLE agent_mail_admissions
         ENABLE TRIGGER agent_mail_admissions_validate_insert",
    )
    .execute(store.pool())
    .await
    .expect("restore authoritative Agent admission trigger after readiness probe");
    assert!(
        store
            .schema_ready()
            .await
            .expect("accept restored schema readiness")
    );

    let slug = format!("integration-{}", Uuid::new_v4().simple());
    let first = BootstrapOrganizationService::new(store.clone());
    let second = BootstrapOrganizationService::new(store.clone());

    let (first_result, second_result) = expect_no_lock_hang(async {
        tokio::join!(
            first.execute(bootstrap_command(slug.clone())),
            second.execute(bootstrap_command(slug.clone()))
        )
    })
    .await;
    let first_result = first_result.expect("first bootstrap");
    let second_result = second_result.expect("second bootstrap");

    assert_eq!(first_result.organization_id, second_result.organization_id);
    assert_eq!(
        first_result.root_principal_id,
        second_result.root_principal_id
    );
    assert_ne!(first_result.created, second_result.created);

    let counts: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM organizations WHERE slug = $1),
          (SELECT count(*) FROM organization_bootstraps WHERE organization_id = $2),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2 AND action = 'organization.bootstrapped'),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $3 AND event_type = 'organization.bootstrapped.v1'),
          (SELECT count(*) FROM roles
            WHERE organization_id = $2 AND system_key = 'org_owner'),
          (SELECT count(*) FROM role_assignments
            WHERE organization_id = $2 AND principal_id = $4 AND revoked_at IS NULL)
        "#,
    )
    .bind(&slug)
    .bind(first_result.organization_id.as_uuid())
    .bind(first_result.organization_id.to_string())
    .bind(first_result.root_principal_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("query bootstrap evidence");
    assert_eq!(counts, (1, 1, 1, 1, 1, 1));

    let authorization = store
        .resolve_authorization(first_result.root_principal_id, Utc::now())
        .await
        .expect("resolve authorization")
        .expect("active bootstrap identity");
    assert!(authorization.permits(first_result.organization_id, Permission::OrganizationManage));
    assert!(!authorization.permits(first_result.organization_id, Permission::ContentReview));
    assert!(!authorization.permits(
        copilotmail_domain::OrganizationId::new(),
        Permission::OrganizationRead
    ));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn mail_domain_lifecycle_is_atomic_versioned_and_append_only() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "mail-domain-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap mail-domain tenant");
    let context = store
        .resolve_authorization(bootstrap.root_principal_id, Utc::now())
        .await
        .expect("resolve bootstrap authorization")
        .expect("active bootstrap principal");
    assert!(context.permits(bootstrap.organization_id, Permission::MailAdmin));

    let now = Utc::now();
    let original_challenge_digest = [9; 32];
    let service = MailDomainGovernanceService::new(store.clone());
    let registered = service
        .register(
            &context,
            RegisterMailDomainCommand {
                organization_id: bootstrap.organization_id,
                name: format!("{}.example.test", Uuid::new_v4().simple()),
                route_mode: MailDomainRouteMode::DirectMx,
                ownership_challenge_sha256: original_challenge_digest,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("register mail domain");
    assert_eq!(registered.status, MailDomainStatus::PendingDns);
    assert_eq!(registered.ownership_challenge_version, 1);
    assert_eq!(registered.route_version, 1);

    let first_service = MailDomainGovernanceService::new(store.clone());
    let second_service = MailDomainGovernanceService::new(store.clone());
    let first_rotation = first_service.rotate_ownership_challenge(
        &context,
        RotateMailDomainChallengeCommand {
            organization_id: bootstrap.organization_id,
            mail_domain_id: registered.mail_domain_id,
            expected_version: 1,
            ownership_challenge_sha256: [11; 32],
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    );
    let second_rotation = second_service.rotate_ownership_challenge(
        &context,
        RotateMailDomainChallengeCommand {
            organization_id: bootstrap.organization_id,
            mail_domain_id: registered.mail_domain_id,
            expected_version: 1,
            ownership_challenge_sha256: [12; 32],
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    );
    let (first_result, second_result) =
        expect_no_lock_hang(async { tokio::join!(first_rotation, second_rotation) }).await;
    assert_ne!(first_result.is_ok(), second_result.is_ok());
    assert!(
        matches!(&first_result, Err(MailDomainGovernanceError::Conflict))
            || matches!(&second_result, Err(MailDomainGovernanceError::Conflict))
    );
    let (rotated, challenge_digest) = match (first_result, second_result) {
        (Ok(result), Err(MailDomainGovernanceError::Conflict)) => (result, [11; 32]),
        (Err(MailDomainGovernanceError::Conflict), Ok(result)) => (result, [12; 32]),
        results => panic!("unexpected concurrent rotation results: {results:?}"),
    };
    assert_eq!(rotated.ownership_challenge_version, 2);
    assert_eq!(rotated.status, MailDomainStatus::PendingDns);

    let old_evidence = DnsVerificationEvidence::new(
        original_challenge_digest,
        now,
        now + chrono::Duration::minutes(10),
        DnsVerificationChecks {
            ownership_txt: true,
            mx_present: true,
            mx_hosts_public: true,
            spf_present: true,
            dkim_present: true,
            dmarc_present: true,
        },
    )
    .expect("valid old evidence");
    let old_evidence_result = service
        .record_dns(
            &context,
            RecordMailDomainDnsCommand {
                organization_id: bootstrap.organization_id,
                mail_domain_id: registered.mail_domain_id,
                observed_domain: registered.name.parse().expect("registered domain name"),
                evidence: old_evidence,
                raw_answers: json!({}),
                failure_codes: vec![],
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await;
    assert!(matches!(
        old_evidence_result,
        Err(MailDomainGovernanceError::Domain(_))
    ));

    let evidence = DnsVerificationEvidence::new(
        challenge_digest,
        now,
        now + chrono::Duration::minutes(10),
        DnsVerificationChecks {
            ownership_txt: true,
            mx_present: true,
            mx_hosts_public: true,
            spf_present: true,
            dkim_present: true,
            dmarc_present: true,
        },
    )
    .expect("valid complete evidence");
    assert_eq!(
        service
            .record_dns(
                &context,
                RecordMailDomainDnsCommand {
                    organization_id: bootstrap.organization_id,
                    mail_domain_id: registered.mail_domain_id,
                    observed_domain: registered.name.parse().expect("registered domain name"),
                    evidence,
                    raw_answers: json!({
                        "ownership_txt": ["sha256:redacted"],
                        "mx": ["10 mx.example.test."]
                    }),
                    failure_codes: vec![],
                    trace_id: Uuid::now_v7().to_string(),
                    now,
                },
            )
            .await
            .expect("record complete DNS evidence"),
        MailDomainStatus::Verified
    );
    service
        .activate(
            &context,
            ActivateMailDomainCommand {
                organization_id: bootstrap.organization_id,
                mail_domain_id: registered.mail_domain_id,
                data_plane_id: legacy_mail_data_plane_id(),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("activate verified domain");
    let route = service
        .update_route(
            &context,
            UpdateMailDomainRouteCommand {
                organization_id: bootstrap.organization_id,
                mail_domain_id: registered.mail_domain_id,
                expected_version: 1,
                inbound_enabled: true,
                outbound_enabled: true,
                smart_host_ref: None,
                dkim_key_ref: Some("kms://integration/dkim".into()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("enable active routes");
    assert_eq!(route.version, 2);

    let stale = service
        .update_route(
            &context,
            UpdateMailDomainRouteCommand {
                organization_id: bootstrap.organization_id,
                mail_domain_id: registered.mail_domain_id,
                expected_version: 1,
                inbound_enabled: false,
                outbound_enabled: false,
                smart_host_ref: None,
                dkim_key_ref: None,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await;
    assert!(matches!(stale, Err(MailDomainGovernanceError::Conflict)));
    service
        .suspend(
            &context,
            SuspendMailDomainCommand {
                organization_id: bootstrap.organization_id,
                mail_domain_id: registered.mail_domain_id,
                reason_code: "integration_test".into(),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("suspend active domain");

    let evidence: (String, i64, bool, bool, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT domain.status,
               route.version,
               route.inbound_enabled,
               route.outbound_enabled,
               domain.ownership_challenge_version,
               (SELECT count(*) FROM mail_domain_dns_observations
                 WHERE organization_id = domain.organization_id
                   AND mail_domain_id = domain.id),
               (SELECT count(*) FROM audit_events
                 WHERE organization_id = domain.organization_id
                   AND resource_id = domain.id::text
                   AND resource_type = 'mail_domain'),
               (SELECT count(*) FROM outbox_events
                 WHERE aggregate_id = domain.id::text
                   AND aggregate_type = 'mail_domain')
          FROM mail_domains AS domain
          JOIN mail_domain_routes AS route
            ON route.mail_domain_id = domain.id
           AND route.organization_id = domain.organization_id
         WHERE domain.id = $1 AND domain.organization_id = $2
        "#,
    )
    .bind(registered.mail_domain_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("query mail-domain lifecycle evidence");
    assert_eq!(evidence, ("suspended".into(), 3, false, false, 2, 1, 6, 6));
    let projection = store
        .get_mail_domain(bootstrap.organization_id, registered.mail_domain_id, now)
        .await
        .expect("query domain projection")
        .expect("tenant domain projection");
    assert_eq!(projection.status, MailDomainStatus::Suspended);
    assert_eq!(projection.ownership_challenge_version, 2);
    assert_eq!(projection.route_version, 3);
    assert_eq!(projection.latest_dns_failure_codes, Vec::<String>::new());
    assert_eq!(
        projection.mail_engine_projection_status,
        MailEngineProjectionStatus::Pending
    );
    let projection_target = store
        .load_mail_domain_projection_target(registered.mail_domain_id)
        .await
        .expect("load engine projection target")
        .expect("engine projection target");
    assert_eq!(projection_target.status, MailDomainStatus::Suspended);
    assert!(projection_target.mail_engine_domain_ref.is_none());
    let projection_plane = projection_target
        .mail_data_plane_id
        .expect("verified domain has an assigned data plane");
    assert!(
        store
            .mark_mail_domain_projection_attempt(
                registered.mail_domain_id,
                MailDomainStatus::Suspended,
                projection_plane,
                projection_target.projection_last_attempt_at,
                now,
            )
            .await
            .expect("mark engine projection attempt")
    );
    assert!(
        store
            .fail_mail_domain_projection(
                registered.mail_domain_id,
                MailDomainStatus::Suspended,
                projection_plane,
                now,
                "mail_engine_unavailable",
                now,
            )
            .await
            .expect("record engine projection failure")
    );
    assert_eq!(
        store
            .get_mail_domain(bootstrap.organization_id, registered.mail_domain_id, now,)
            .await
            .expect("query failed projection")
            .expect("failed projection")
            .mail_engine_projection_status,
        MailEngineProjectionStatus::Failed
    );
    let retry_at = now + chrono::Duration::microseconds(1);
    assert!(
        store
            .mark_mail_domain_projection_attempt(
                registered.mail_domain_id,
                MailDomainStatus::Suspended,
                projection_plane,
                Some(now),
                retry_at,
            )
            .await
            .expect("mark projection retry")
    );
    assert!(
        store
            .complete_mail_domain_projection(
                registered.mail_domain_id,
                MailDomainStatus::Suspended,
                projection_plane,
                retry_at,
                "stalwart-domain-1",
                false,
                retry_at,
            )
            .await
            .expect("complete engine projection")
    );
    let ready_projection = store
        .get_mail_domain(bootstrap.organization_id, registered.mail_domain_id, now)
        .await
        .expect("query ready projection")
        .expect("ready projection");
    assert_eq!(
        ready_projection.mail_engine_projection_status,
        MailEngineProjectionStatus::Ready
    );
    assert_eq!(ready_projection.mail_engine_projected_enabled, Some(false));
    assert_eq!(
        ready_projection.mail_engine_domain_ref.as_deref(),
        Some("stalwart-domain-1")
    );
    assert!(
        !store
            .complete_mail_domain_projection(
                registered.mail_domain_id,
                MailDomainStatus::Suspended,
                projection_plane,
                retry_at,
                "different-domain",
                false,
                retry_at,
            )
            .await
            .expect("reject conflicting engine domain reference")
    );
    let listed = store
        .list_mail_domains(bootstrap.organization_id, None, 10, now)
        .await
        .expect("list tenant domains");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].mail_domain_id, registered.mail_domain_id);
    let second = service
        .register(
            &context,
            RegisterMailDomainCommand {
                organization_id: bootstrap.organization_id,
                name: format!("zz-{}.example.test", Uuid::new_v4().simple()),
                route_mode: MailDomainRouteMode::SmartHost,
                ownership_challenge_sha256: [10; 32],
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("register second domain for cursor pagination");
    let first_page = store
        .list_mail_domains(bootstrap.organization_id, None, 1, now)
        .await
        .expect("first domain page");
    assert_eq!(first_page.len(), 1);
    let second_page = store
        .list_mail_domains(bootstrap.organization_id, Some(&first_page[0].name), 1, now)
        .await
        .expect("second domain page");
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].mail_domain_id, second.mail_domain_id);
    assert!(
        store
            .get_mail_domain(
                copilotmail_domain::OrganizationId::new(),
                registered.mail_domain_id,
                now,
            )
            .await
            .expect("cross-tenant domain lookup")
            .is_none()
    );

    assert_append_only_error(
        sqlx::query(
            "UPDATE mail_domain_dns_observations SET failure_codes = '{}' WHERE mail_domain_id = $1",
        )
        .bind(registered.mail_domain_id.as_uuid())
        .execute(store.pool())
        .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn webhook_delivery_ledger_skips_terminal_subscriptions() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "webhook-ledger-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap webhook tenant");
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Webhook Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind webhook owner");
    let public_key = test_public_key();
    let now = Utc::now();
    let agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Webhook Agent".into(),
            environment: AgentEnvironment::Testing,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Validate append-only webhook delivery evidence".into(),
            review_due_at: now + chrono::Duration::days(90),
            public_key_fingerprint: rsa_public_key_fingerprint(public_key.as_bytes()).unwrap(),
            public_key_pem: public_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register webhook Agent");
    let agent_context = store
        .resolve_authorization(agent.principal_id, Utc::now())
        .await
        .expect("resolve Agent authorization")
        .expect("active Agent authorization");
    assert!(
        store
            .principal_can_submit_mail(bootstrap.organization_id, agent.principal_id)
            .await
            .expect("evaluate testing Agent mail policy")
    );
    let production_key = test_public_key();
    let production_agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Production Mail Policy Agent".into(),
            environment: AgentEnvironment::Production,
            risk_level: AgentRiskLevel::High,
            purpose: "Prove production Agent mail submission remains fail closed".into(),
            review_due_at: now + chrono::Duration::days(30),
            public_key_fingerprint: rsa_public_key_fingerprint(production_key.as_bytes()).unwrap(),
            public_key_pem: production_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register production policy Agent");
    assert!(
        !store
            .principal_can_submit_mail(bootstrap.organization_id, production_agent.principal_id)
            .await
            .expect("deny production Agent mail policy")
    );
    assert_eq!(
        store
            .consume_agent_ingress_capacity(bootstrap.organization_id, agent.principal_id, 2, 1,)
            .await
            .expect("admit first Agent ingress request"),
        AgentIngressRateLimitDecision::Allowed
    );
    assert!(matches!(
        store
            .consume_agent_ingress_capacity(bootstrap.organization_id, agent.principal_id, 2, 1,)
            .await
            .expect("enforce Agent ingress principal limit"),
        AgentIngressRateLimitDecision::PrincipalLimited { .. }
    ));
    let tenant_window_count: i32 = sqlx::query_scalar(
        r#"
        SELECT request_count
          FROM agent_ingress_rate_limit_windows
         WHERE organization_id = $1
           AND principal_id IS NULL
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("read tenant ingress window");
    assert_eq!(
        tenant_window_count, 1,
        "Agent rejection must roll back its tentative tenant increment"
    );
    assert!(matches!(
        store
            .consume_agent_ingress_capacity(bootstrap.organization_id, agent.principal_id, 1, 1,)
            .await
            .expect("enforce Agent ingress tenant limit"),
        AgentIngressRateLimitDecision::TenantLimited { .. }
    ));
    let subscription = WebhookSubscriptionService::new(store.clone())
        .create(
            &agent_context,
            CreateWebhookSubscriptionCommand {
                organization_id: bootstrap.organization_id,
                principal_id: agent.principal_id,
                endpoint_url: "https://hooks.example.test/copilotmail".into(),
                event_types: vec!["agent.message.dead_lettered.v1".into()],
                signing_key_reference: "agent-hook-2026-01".into(),
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
            },
        )
        .await
        .expect("create governed webhook subscription");
    let event_id = Uuid::now_v7();
    let subscription_id = subscription.subscription_id.as_uuid();

    let targets = store
        .pending_webhook_delivery_targets(
            bootstrap.organization_id,
            event_id,
            "agent.message.dead_lettered.v1",
            Utc::now(),
        )
        .await
        .expect("load first delivery target");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].previous_attempts, 0);

    let attempted_at = Utc::now();
    assert_eq!(
        store
            .append_webhook_delivery(
                subscription_id,
                event_id,
                WebhookDeliveryOutcome::Retrying {
                    response_status: Some(503),
                    error_code: "webhook_receiver_retryable".into(),
                    next_attempt_at: attempted_at + chrono::Duration::seconds(10),
                },
                attempted_at,
            )
            .await
            .expect("append retryable delivery"),
        Some(1)
    );
    let targets = store
        .pending_webhook_delivery_targets(
            bootstrap.organization_id,
            event_id,
            "agent.message.dead_lettered.v1",
            Utc::now(),
        )
        .await
        .expect("load retry delivery target");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].previous_attempts, 1);

    assert_eq!(
        store
            .append_webhook_delivery(
                subscription_id,
                event_id,
                WebhookDeliveryOutcome::Delivered {
                    response_status: 204,
                },
                Utc::now(),
            )
            .await
            .expect("append delivered result"),
        Some(2)
    );
    assert!(
        store
            .pending_webhook_delivery_targets(
                bootstrap.organization_id,
                event_id,
                "agent.message.dead_lettered.v1",
                Utc::now(),
            )
            .await
            .expect("terminal subscription query")
            .is_empty()
    );
    let rows: Vec<(i32, String, Option<i32>, Option<String>)> = sqlx::query_as(
        r#"
        SELECT attempt, status, response_status, error_code
          FROM webhook_deliveries
         WHERE subscription_id = $1 AND event_id = $2
         ORDER BY attempt
        "#,
    )
    .bind(subscription_id)
    .bind(event_id)
    .fetch_all(store.pool())
    .await
    .expect("query delivery evidence");
    assert_eq!(
        rows,
        vec![
            (
                1,
                "retrying".into(),
                Some(503),
                Some("webhook_receiver_retryable".into())
            ),
            (2, "delivered".into(), Some(204), None),
        ]
    );

    let replay_event_id = Uuid::now_v7();
    let replay_occurred_at = Utc::now();
    sqlx::query(
        r#"
        INSERT INTO outbox_events(
            id, event_type, aggregate_type, aggregate_id,
            schema_version, payload, occurred_at
        )
        VALUES(
            $1, 'agent.message.dead_lettered.v1', 'message', $2,
            1, $3, $4
        )
        "#,
    )
    .bind(replay_event_id)
    .bind(replay_event_id.to_string())
    .bind(json!({
        "organization_id": bootstrap.organization_id,
        "message_id": replay_event_id,
    }))
    .bind(replay_occurred_at)
    .execute(store.pool())
    .await
    .expect("insert replayable Outbox fixture");
    let cursor_page = AgentEventCursorService::new(store.clone())
        .pull(
            &agent_context,
            PullAgentEventsCommand {
                organization_id: bootstrap.organization_id,
                subscription_id: subscription.subscription_id,
                after_event_id: None,
                limit: 10,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
            },
        )
        .await
        .expect("pull subscription cursor page");
    assert_eq!(cursor_page.items.len(), 1);
    assert_eq!(cursor_page.items[0].event_id, replay_event_id);
    assert_eq!(cursor_page.next_event_id, Some(replay_event_id));
    assert!(!cursor_page.has_more);
    let stream_lease = AgentEventCursorService::new(store.clone())
        .open_stream(
            &agent_context,
            OpenAgentEventStreamCommand {
                organization_id: bootstrap.organization_id,
                subscription_id: subscription.subscription_id,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
                lease_id: Uuid::now_v7(),
                tenant_connection_limit: 100,
                principal_connection_limit: 5,
            },
        )
        .await
        .expect("record Agent SSE stream establishment");
    let capacity_error = AgentEventCursorService::new(store.clone())
        .open_stream(
            &agent_context,
            OpenAgentEventStreamCommand {
                organization_id: bootstrap.organization_id,
                subscription_id: subscription.subscription_id,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
                lease_id: Uuid::now_v7(),
                tenant_connection_limit: 100,
                principal_connection_limit: 1,
            },
        )
        .await
        .expect_err("one Agent lease must consume its configured capacity");
    assert!(matches!(
        capacity_error,
        AgentEventCursorError::PrincipalCapacityReached
    ));
    assert!(
        store
            .release_agent_event_stream_lease(&stream_lease)
            .await
            .expect("release exact Agent SSE lease")
    );
    let active_lease_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM agent_event_stream_leases WHERE organization_id = $1",
    )
    .bind(bootstrap.organization_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(active_lease_count, 0);
    let stream_evidence: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT count(*) FROM audit_events
              WHERE organization_id = $1
                AND action = 'agent.event_stream.opened'
                AND resource_id = $2),
            (SELECT count(*) FROM outbox_events
              WHERE event_type = 'agent.event_stream.opened.v1'
                AND aggregate_id = $2)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(subscription.subscription_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query Agent SSE audit and Outbox evidence");
    assert_eq!(stream_evidence, (1, 1));
    let empty_page = AgentEventCursorService::new(store.clone())
        .pull(
            &agent_context,
            PullAgentEventsCommand {
                organization_id: bootstrap.organization_id,
                subscription_id: subscription.subscription_id,
                after_event_id: cursor_page.next_event_id,
                limit: 10,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
            },
        )
        .await
        .expect("resume subscription cursor");
    assert!(empty_page.items.is_empty());
    assert_eq!(empty_page.next_event_id, Some(replay_event_id));
    assert_eq!(
        store
            .append_webhook_delivery(
                subscription_id,
                replay_event_id,
                WebhookDeliveryOutcome::DeadLettered {
                    response_status: Some(410),
                    error_code: "webhook_receiver_rejected".into(),
                },
                Utc::now(),
            )
            .await
            .expect("append dead letter"),
        Some(1)
    );
    let dead_letters = store
        .list_webhook_dead_letters(bootstrap.organization_id, None, 10)
        .await
        .expect("list dead letters");
    assert_eq!(dead_letters.len(), 1);
    assert_eq!(dead_letters[0].event_id, replay_event_id);
    assert_eq!(dead_letters[0].error_code, "webhook_receiver_rejected");

    let owner_context = store
        .resolve_authorization(owner.principal_id, Utc::now())
        .await
        .expect("resolve owner authorization")
        .expect("active owner authorization");
    let replay = WebhookDeadLetterService::new(store.clone())
        .replay(
            &owner_context,
            ReplayWebhookDeadLetterCommand {
                organization_id: bootstrap.organization_id,
                subscription_id: subscription.subscription_id,
                event_id: replay_event_id,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
            },
        )
        .await
        .expect("schedule controlled dead-letter replay");
    assert_eq!(replay.attempt, 2);
    assert!(
        store
            .list_webhook_dead_letters(bootstrap.organization_id, None, 10)
            .await
            .expect("list replayed dead letters")
            .is_empty()
    );
    let replay_targets = store
        .pending_webhook_delivery_targets(
            bootstrap.organization_id,
            replay_event_id,
            "agent.message.dead_lettered.v1",
            replay_occurred_at,
        )
        .await
        .expect("load manual replay target");
    assert_eq!(replay_targets.len(), 1);
    assert_eq!(replay_targets[0].previous_attempts, 0);
    let replay_rows: Vec<(i32, String, Option<String>)> = sqlx::query_as(
        r#"
        SELECT attempt, status, error_code
          FROM webhook_deliveries
         WHERE subscription_id = $1 AND event_id = $2
         ORDER BY attempt
        "#,
    )
    .bind(subscription_id)
    .bind(replay_event_id)
    .fetch_all(store.pool())
    .await
    .expect("query replay evidence");
    assert_eq!(
        replay_rows,
        vec![
            (
                1,
                "dead_lettered".into(),
                Some("webhook_receiver_rejected".into())
            ),
            (2, "retrying".into(), Some("manual_replay_requested".into())),
        ]
    );
    assert_append_only_error(
        sqlx::query(
            "UPDATE webhook_deliveries SET error_code = 'tampered' \
             WHERE subscription_id = $1 AND event_id = $2",
        )
        .bind(subscription_id)
        .bind(replay_event_id)
        .execute(store.pool())
        .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn audit_rows_reject_update_and_delete() {
    let store = migrated_store().await;
    let slug = format!("audit-{}", Uuid::new_v4().simple());
    let result = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(slug))
        .await
        .expect("bootstrap audit fixture");
    let event_id: Uuid =
        sqlx::query_scalar("SELECT id FROM audit_events WHERE organization_id = $1 LIMIT 1")
            .bind(result.organization_id.as_uuid())
            .fetch_one(store.pool())
            .await
            .expect("load audit event");

    assert_append_only_error(
        sqlx::query("UPDATE audit_events SET details = '{}'::jsonb WHERE id = $1")
            .bind(event_id)
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM audit_events WHERE id = $1")
            .bind(event_id)
            .execute(store.pool())
            .await,
    );

    let context_evidence: (bool, bool) = sqlx::query_as(
        r#"
        SELECT request_id = trace_id,
               received_at >= occurred_at
          FROM audit_events
         WHERE id = $1
        "#,
    )
    .bind(event_id)
    .fetch_one(store.pool())
    .await
    .expect("query explicit audit context");
    assert_eq!(context_evidence, (true, true));
    let chain_evidence: (i64, i16, bool, i32, bool) = sqlx::query_as(
        r#"
        SELECT chain_sequence,
               hash_version,
               previous_hash IS NULL,
               octet_length(event_hash),
               hash_payload = build_audit_event_hash_payload(audit_events)
          FROM audit_events
         WHERE id = $1
        "#,
    )
    .bind(event_id)
    .fetch_one(store.pool())
    .await
    .expect("query audit hash-chain evidence");
    assert_eq!(chain_evidence, (1, 1, true, 32, true));
    let report = store
        .verify_audit_chain(result.organization_id)
        .await
        .expect("verify bootstrap audit chain");
    assert_eq!(report.event_count, 1);
    assert_eq!(report.first_event_id, Some(event_id));
    assert_eq!(report.last_event_id, Some(event_id));
    assert_eq!(report.head_hash.as_deref().map(str::len), Some(64));

    let database_catalog = sqlx::query_as::<_, (String, String, String, bool)>(
        "SELECT action, resource_type, risk, privileged FROM audit_event_catalog",
    )
    .fetch_all(store.pool())
    .await
    .expect("query governed audit catalog")
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    let domain_catalog = AUDIT_EVENT_CATALOG
        .iter()
        .map(|definition| {
            (
                definition.action.to_owned(),
                definition.resource_kind.to_owned(),
                match definition.risk {
                    AuditRisk::Low => "low",
                    AuditRisk::Medium => "medium",
                    AuditRisk::High => "high",
                    AuditRisk::Critical => "critical",
                }
                .to_owned(),
                definition.privileged,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(database_catalog, domain_catalog);
    assert_append_only_error(
        sqlx::query(
            "UPDATE audit_event_catalog SET risk = 'low' WHERE action = 'organization.bootstrapped'",
        )
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM audit_event_catalog WHERE action = 'organization.bootstrapped'")
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE audit_events")
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE audit_event_catalog")
            .execute(store.pool())
            .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn audit_event_search_is_tenant_scoped_cursor_checked_and_self_auditing() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "audit-search-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap audit-search tenant");
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://audit-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("auditor-{}", Uuid::new_v4().simple()),
            display_name: "Compliance Auditor".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind active Human auditor");
    let authorization = store
        .resolve_authorization(owner.principal_id, Utc::now())
        .await
        .expect("resolve auditor authorization")
        .expect("active auditor");
    let now = Utc::now();
    let service = AuditEventSearchService::new(store.clone());
    let first = service
        .search(
            &authorization,
            SearchAuditEventsCommand {
                organization_id: bootstrap.organization_id,
                occurred_from: now - chrono::Duration::hours(1),
                occurred_to: now + chrono::Duration::seconds(1),
                action: None,
                actor_principal_id: None,
                resource_type: None,
                resource_id: None,
                outcome: None,
                request_id: None,
                before_chain_sequence: None,
                limit: 1,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("search first immutable audit page");
    assert_eq!(first.items.len(), 1);
    assert!(first.has_more);
    assert!(matches!(
        first.items[0].actor_kind.as_str(),
        "human" | "agent" | "service"
    ));
    assert_eq!(first.items[0].event_hash_hex.len(), 64);
    let first_cursor = first.next_chain_sequence.expect("first audit cursor");

    let second = service
        .search(
            &authorization,
            SearchAuditEventsCommand {
                organization_id: bootstrap.organization_id,
                occurred_from: now - chrono::Duration::hours(1),
                occurred_to: now + chrono::Duration::seconds(1),
                action: None,
                actor_principal_id: None,
                resource_type: None,
                resource_id: None,
                outcome: None,
                request_id: None,
                before_chain_sequence: Some(first_cursor),
                limit: 10,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("resume audit search from a proven anchor");
    assert!(
        second
            .items
            .iter()
            .all(|event| event.chain_sequence < first_cursor)
    );

    let fabricated = service
        .search(
            &authorization,
            SearchAuditEventsCommand {
                organization_id: bootstrap.organization_id,
                occurred_from: now - chrono::Duration::hours(1),
                occurred_to: now + chrono::Duration::seconds(1),
                action: None,
                actor_principal_id: None,
                resource_type: None,
                resource_id: None,
                outcome: None,
                request_id: None,
                before_chain_sequence: Some(i64::MAX - 1),
                limit: 10,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect_err("fabricated audit cursor must fail closed");
    assert!(matches!(fabricated, AuditEventSearchError::InvalidCursor));
    let evidence: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT count(*) FROM audit_events
              WHERE organization_id = $1
                AND action = 'audit.events.queried'),
            (SELECT count(*) FROM outbox_events
              WHERE event_type = 'audit.events.queried.v1'
                AND payload ->> 'organization_id' = $1::text)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("query audit-search evidence");
    assert_eq!(evidence, (2, 2));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn concurrent_audit_appends_form_one_verifiable_tenant_chain() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "audit-chain-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap audit-chain fixture");
    let first_id = Uuid::now_v7();
    let second_id = Uuid::now_v7();
    let first = insert_test_audit_event(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        first_id,
    );
    let second = insert_test_audit_event(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        second_id,
    );
    let (first_result, second_result) =
        expect_no_lock_hang(async { tokio::join!(first, second) }).await;
    first_result.expect("first concurrent audit append");
    second_result.expect("second concurrent audit append");

    let report = store
        .verify_audit_chain(bootstrap.organization_id)
        .await
        .expect("verify concurrent audit chain");
    assert_eq!(report.event_count, 3);

    let sequences: Vec<i64> = sqlx::query_scalar(
        "SELECT chain_sequence FROM audit_events WHERE organization_id = $1 ORDER BY chain_sequence",
    )
    .bind(bootstrap.organization_id.as_uuid())
    .fetch_all(store.pool())
    .await
    .expect("load tenant chain sequences");
    assert_eq!(sequences, vec![1, 2, 3]);

    let mut transaction = store.pool().begin().await.expect("begin tamper fixture");
    sqlx::query("ALTER TABLE audit_events DISABLE TRIGGER audit_events_append_only")
        .execute(&mut *transaction)
        .await
        .expect("disable append-only trigger in fixture transaction");
    sqlx::query("UPDATE audit_events SET hash_payload = hash_payload || ' ' WHERE id = $1")
        .bind(second_id)
        .execute(&mut *transaction)
        .await
        .expect("tamper fixture payload");
    sqlx::query("ALTER TABLE audit_events ENABLE TRIGGER audit_events_append_only")
        .execute(&mut *transaction)
        .await
        .expect("restore append-only trigger");
    transaction.commit().await.expect("commit tamper fixture");

    let error = store
        .verify_audit_chain(bootstrap.organization_id)
        .await
        .expect_err("tampered audit chain must fail verification");
    assert!(error.to_string().contains("event hash does not match"));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn signed_audit_checkpoint_is_atomic_idempotent_and_portable() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "checkpoint-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap checkpoint fixture");
    let private = RsaPrivateKey::generate_for_testing().expect("generate checkpoint key");
    let public = private.public_key();
    let private_pem = private
        .pkcs8_pem_for_testing()
        .expect("encode checkpoint private key");
    let public_pem = public.spki_pem().expect("encode checkpoint public key");
    let signer = RsaAuditCheckpointSigner::new(
        "audit-integration-2026".into(),
        private_pem.as_bytes(),
        public_pem.as_bytes(),
    )
    .expect("configure checkpoint signer");
    let checkpoint_id = AuditCheckpointId::new();
    let command = CreateAuditCheckpointCommand {
        checkpoint_id,
        organization_id: bootstrap.organization_id,
        actor_principal_id: bootstrap.root_principal_id,
        archive_object_key: format!(
            "audit/{}/{}.jsonl",
            bootstrap.organization_id, checkpoint_id
        ),
        trace_id: Uuid::now_v7().to_string(),
        now: Utc::now(),
    };
    let service = CreateAuditCheckpointService::new(store.clone(), signer);
    let first = service
        .execute(command.clone())
        .await
        .expect("create signed checkpoint");
    let replay = service
        .execute(command)
        .await
        .expect("replay signed checkpoint");
    assert_eq!(first, replay);
    assert_eq!(first.event_count, 1);

    let counts: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_checkpoints WHERE id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2 AND action = 'audit.checkpoint.created'),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $3 AND event_type = 'audit.checkpoint.created.v1')
        "#,
    )
    .bind(checkpoint_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(checkpoint_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query checkpoint atomicity evidence");
    assert_eq!(counts, (1, 1, 1));

    let stored = store
        .get_signed_audit_checkpoint(bootstrap.organization_id, checkpoint_id)
        .await
        .expect("load signed checkpoint")
        .expect("checkpoint exists");
    stored
        .checkpoint
        .verify(public_pem.as_bytes())
        .expect("verify stored checkpoint signature");
    assert_eq!(
        store
            .oldest_unarchived_audit_checkpoint(bootstrap.organization_id)
            .await
            .expect("load resumable unarchived checkpoint")
            .expect("checkpoint is pending archive")
            .checkpoint
            .payload
            .checkpoint_id,
        checkpoint_id
    );
    let records = store
        .load_audit_export_page(bootstrap.organization_id, 0, 1, 100)
        .await
        .expect("load portable audit records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].event_hash, first.root_hash);
    assert!(
        store
            .get_signed_audit_checkpoint(copilotmail_domain::OrganizationId::new(), checkpoint_id)
            .await
            .expect("cross-tenant checkpoint query")
            .is_none()
    );
    assert_append_only_error(
        sqlx::query("UPDATE audit_checkpoints SET object_key = object_key WHERE id = $1")
            .bind(checkpoint_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM audit_checkpoints WHERE id = $1")
            .bind(checkpoint_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE audit_checkpoints")
            .execute(store.pool())
            .await,
    );

    let verified_at = pg_micros(Utc::now());
    let archive_id = AuditArchiveId::new();
    let archive = AuditArchiveRecord {
        archive_id,
        organization_id: bootstrap.organization_id,
        checkpoint_id,
        provider: "s3".into(),
        bucket: "copilotmail-audit".into(),
        events: ArchivedObjectRecord {
            key: format!("audit/{checkpoint_id}/events.jsonl"),
            version_id: "events-version-1".into(),
            sha256_hex: "a".repeat(64),
            size_bytes: 1024,
        },
        manifest: ArchivedObjectRecord {
            key: format!("audit/{checkpoint_id}/manifest.json"),
            version_id: "manifest-version-1".into(),
            sha256_hex: "b".repeat(64),
            size_bytes: 512,
        },
        retention_mode: "compliance".into(),
        retain_until: verified_at + chrono::Duration::days(365),
        verified_at,
        created_by: bootstrap.root_principal_id,
        created_at: verified_at + chrono::Duration::milliseconds(1),
    };
    let archive_command = RecordAuditArchiveCommand {
        archive,
        actor_principal_id: bootstrap.root_principal_id,
        trace_id: Uuid::now_v7().to_string(),
    };
    let archive_service = RecordAuditArchiveService::new(store.clone());
    let first_archive = archive_service
        .execute(archive_command.clone())
        .await
        .expect("record verified WORM archive");
    let replayed_archive = archive_service
        .execute(archive_command)
        .await
        .expect("replay verified WORM archive");
    assert_eq!(first_archive, replayed_archive);
    assert!(
        store
            .oldest_unarchived_audit_checkpoint(bootstrap.organization_id)
            .await
            .expect("check completed checkpoint archive")
            .is_none()
    );

    let archive_counts: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_archives WHERE id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2 AND action = 'audit.archive.completed'),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $3 AND event_type = 'audit.archive.completed.v1')
        "#,
    )
    .bind(archive_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(archive_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query archive atomicity evidence");
    assert_eq!(archive_counts, (1, 1, 1));
    assert_append_only_error(
        sqlx::query("UPDATE audit_archives SET bucket = bucket WHERE id = $1")
            .bind(archive_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM audit_archives WHERE id = $1")
            .bind(archive_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE audit_archives")
            .execute(store.pool())
            .await,
    );

    let export_requester = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: "https://quality-owner.example.test".into(),
            subject: Uuid::new_v4().to_string(),
            display_name: "Export Test Owner".into(),
            trace_id: Uuid::new_v4().to_string(),
            now: verified_at,
        })
        .await
        .expect("bind the operator-only policy owner's identity")
        .principal_id;
    let export_context = store
        .resolve_authorization(export_requester, verified_at)
        .await
        .unwrap()
        .expect("resolve audit export requester");
    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "audit.archive.export".into(),
            display_name: "Immutable audit archive export".into(),
            action: ApprovalAction::AuditExport,
            required_approvals: 2,
            rule: json!({"archive_scope": "exact"}),
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect("create audit export policy");
    let workflow = ApprovalWorkflowService::new(store.clone());
    let export_approval_id = workflow
        .request(RequestApprovalCommand {
            context: export_context.clone(),
            policy_key: "audit.archive.export".into(),
            action: ApprovalAction::AuditExport,
            resource_kind: "audit.archive".into(),
            resource_id: archive_id.to_string(),
            reason: "Produce reviewed evidence package".into(),
            ticket_reference: Some("AUDIT-42".into()),
            expires_at: verified_at + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect("request audit export approval");
    for approver in [
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            verified_at,
        )
        .await,
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            verified_at,
        )
        .await,
    ] {
        let context = store
            .resolve_authorization(approver, verified_at)
            .await
            .unwrap()
            .expect("resolve audit export approver");
        workflow
            .decide(DecideApprovalCommand {
                context,
                request_id: export_approval_id,
                decision: ApprovalDecision::Approved,
                reason: "independent audit review complete".into(),
                trace_id: Uuid::now_v7().to_string(),
                now: verified_at,
            })
            .await
            .expect("approve audit export");
    }
    let proof = ApprovalProof {
        organization_id: bootstrap.organization_id,
        request_id: export_approval_id,
        requested_by: export_requester,
        action: ApprovalAction::AuditExport,
        resource_kind: "audit.archive".into(),
        resource_id: archive_id.to_string(),
        now: verified_at,
    };
    let export_service = AuditExportAuthorizationService::new(store.clone());
    let command = AuthorizeAuditExportCommand {
        context: export_context.clone(),
        archive_id,
        approval: proof,
        trace_id: Uuid::now_v7().to_string(),
        now: verified_at,
    };
    let grant = export_service
        .authorize(command.clone())
        .await
        .expect("authorize exact audit export");
    let replay = export_service
        .authorize(command)
        .await
        .expect("replay exact audit export authorization");
    assert_eq!(grant, replay);
    assert_eq!(
        grant.expires_at(),
        verified_at + chrono::Duration::minutes(15)
    );
    assert_eq!(
        store
            .get_audit_export_grant(bootstrap.organization_id, grant.id())
            .await
            .unwrap(),
        Some(grant.clone())
    );

    let download_service = AuditExportDownloadService::new(store.clone());
    let authorized = download_service
        .resolve(ResolveAuditExportObjectCommand {
            context: export_context.clone(),
            grant_id: grant.id(),
            artifact: AuditExportArtifact::Events,
            now: Utc::now(),
        })
        .await
        .expect("resolve exact audit export object");
    let download = download_service
        .record(RecordAuditExportDownloadCommand {
            context: export_context.clone(),
            retrieved: authorized.object.clone(),
            authorized,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("record exact audit export download");
    assert!(download.downloaded_at >= grant.created_at());
    assert!(download.downloaded_at < grant.expires_at());
    let download_counts: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_export_downloads WHERE id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2
              AND action = 'audit.export.downloaded'
              AND resource_id = $3),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $3
              AND event_type = 'audit.export.downloaded.v1')
        "#,
    )
    .bind(download.id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(grant.id().to_string())
    .fetch_one(store.pool())
    .await
    .expect("query export download atomicity evidence");
    assert_eq!(download_counts, (1, 1, 1));
    assert_append_only_error(
        sqlx::query("UPDATE audit_export_downloads SET sha256 = sha256 WHERE id = $1")
            .bind(download.id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM audit_export_downloads WHERE id = $1")
            .bind(download.id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE audit_export_downloads")
            .execute(store.pool())
            .await,
    );
    let mismatched_download = sqlx::query(
        r#"
        INSERT INTO audit_export_downloads(
            id, organization_id, grant_id, archive_id, downloaded_by,
            artifact, object_key, object_version_id, sha256, size_bytes,
            downloaded_at
        )
        VALUES ($1, $2, $3, $4, $5, 'events', $6, $7, $8, $9, $10)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(grant.id().as_uuid())
    .bind(archive_id.as_uuid())
    .bind(export_requester.as_uuid())
    .bind("substituted/events.jsonl")
    .bind("substituted-version")
    .bind("0".repeat(64))
    .bind(1_i64)
    .bind(verified_at)
    .execute(store.pool())
    .await
    .expect_err("reject substituted archive object evidence");
    assert_eq!(
        mismatched_download
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("23514")
    );

    assert_append_only_error(
        sqlx::query("UPDATE audit_export_grants SET expires_at = expires_at WHERE id = $1")
            .bind(grant.id().as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM audit_export_grants WHERE id = $1")
            .bind(grant.id().as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE audit_export_grants")
            .execute(store.pool())
            .await,
    );

    // These direct SQL probes intentionally bypass the service clock. All
    // fixture copies are rolled back: no unaudited grant is kept by the test.
    for expired_approval in [false, true] {
        let mut probe = store.pool().begin().await.unwrap();
        let copied_approval = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO approval_requests (
                id, organization_id, policy_version_id, action, resource_kind,
                resource_id, requested_by, reason, ticket_reference,
                required_approvals, status, created_at, expires_at, completed_at
            )
            SELECT $1, organization_id, policy_version_id, action, resource_kind,
                   resource_id, requested_by, reason, ticket_reference,
                   required_approvals, status,
                   CASE WHEN $3 THEN clock_timestamp() - interval '2 minutes' ELSE created_at END,
                   CASE WHEN $3 THEN clock_timestamp() - interval '1 minute' ELSE expires_at END,
                   completed_at
              FROM approval_requests WHERE id = $2
            "#,
        )
        .bind(copied_approval)
        .bind(export_approval_id.as_uuid())
        .bind(expired_approval)
        .execute(&mut *probe)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO approval_steps(id, organization_id, approval_request_id, approver_principal_id, decision, reason, decided_at) SELECT gen_random_uuid(), organization_id, $1, approver_principal_id, decision, reason, decided_at FROM approval_steps WHERE approval_request_id = $2",
        ).bind(copied_approval).bind(export_approval_id.as_uuid()).execute(&mut *probe).await.unwrap();
        let inserted = sqlx::query_as::<_, (Uuid, chrono::DateTime<Utc>, chrono::DateTime<Utc>)>(
            r#"
            INSERT INTO audit_export_grants (
                id, organization_id, archive_id, approval_request_id,
                requested_by, created_at, expires_at
            ) SELECT $1, organization_id, archive_id, $2, requested_by,
                     clock_timestamp() - interval '2 minutes',
                     CASE WHEN $4 THEN clock_timestamp() - interval '1 minute'
                          ELSE clock_timestamp() + interval '3 seconds' END
                FROM audit_export_grants WHERE id = $3
            RETURNING id, created_at, expires_at
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(copied_approval)
        .bind(grant.id().as_uuid())
        .bind(expired_approval)
        .fetch_one(&mut *probe)
        .await;
        let rejection = if expired_approval {
            inserted.expect_err("a backdated caller clock cannot revive expired approval")
        } else {
            let (probe_grant, created_at, expires_at) = inserted.unwrap();
            assert!(created_at >= grant.created_at());
            sqlx::query("SELECT pg_sleep(GREATEST(EXTRACT(EPOCH FROM ($1::timestamptz - clock_timestamp())), 0) + 0.02)")
                .bind(expires_at).execute(&mut *probe).await.unwrap();
            sqlx::query(
                r#"
                INSERT INTO audit_export_downloads (
                    id, organization_id, grant_id, archive_id, downloaded_by,
                    artifact, object_key, object_version_id, sha256, size_bytes, downloaded_at
                ) SELECT $1, organization_id, $2, archive_id, downloaded_by,
                         artifact, object_key, object_version_id, sha256, size_bytes, $3
                    FROM audit_export_downloads WHERE id = $4
                "#,
            )
            .bind(Uuid::now_v7())
            .bind(probe_grant)
            .bind(created_at)
            .bind(download.id.as_uuid())
            .execute(&mut *probe)
            .await
            .expect_err("a backdated download cannot reuse an expired grant")
        };
        assert_eq!(
            rejection
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::code)
                .as_deref(),
            Some("23514")
        );
        probe.rollback().await.unwrap();
    }

    let retention_service = RetentionPolicyService::new(store.clone());
    let retention_draft = retention_service
        .create(CreateRetentionPolicyCommand {
            context: export_context.clone(),
            policy_key: "audit.standard".into(),
            display_name: "Standard audit evidence retention".into(),
            resource_kind: RetentionResourceKind::AuditEvidence,
            minimum_retention_days: 2_555,
            archive_after_days: Some(30),
            delete_after_days: Some(3_650),
            rule: json!({"classification": ["standard"]}),
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect("create retention policy draft");
    assert!(
        store
            .get_retention_policy(bootstrap.organization_id, retention_draft.policy.id(),)
            .await
            .unwrap()
            .expect("read retention policy draft")
            .active_version
            .is_none()
    );

    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "compliance.retention.policychange".into(),
            display_name: "Retention policy activation".into(),
            action: ApprovalAction::RetentionChange,
            required_approvals: 2,
            rule: json!({"policy_scope": "exact"}),
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect("create retention activation policy");
    let retention_approval_id = workflow
        .request(RequestApprovalCommand {
            context: export_context.clone(),
            policy_key: "compliance.retention.policychange".into(),
            action: ApprovalAction::RetentionChange,
            resource_kind: "compliance.retention_policy_version".into(),
            resource_id: retention_draft.version.id().to_string(),
            reason: "Activate reviewed production retention".into(),
            ticket_reference: Some("RETENTION-42".into()),
            expires_at: verified_at + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect("request retention activation approval");
    for approver in [
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            verified_at,
        )
        .await,
        insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            verified_at,
        )
        .await,
    ] {
        let context = store
            .resolve_authorization(approver, verified_at)
            .await
            .unwrap()
            .expect("resolve retention approver");
        workflow
            .decide(DecideApprovalCommand {
                context,
                request_id: retention_approval_id,
                decision: ApprovalDecision::Approved,
                reason: "retention review complete".into(),
                trace_id: Uuid::now_v7().to_string(),
                now: verified_at,
            })
            .await
            .expect("approve retention activation");
    }
    let active_version = retention_service
        .activate_version(ActivateRetentionPolicyVersionCommand {
            context: export_context.clone(),
            policy_id: retention_draft.policy.id(),
            policy_version_id: retention_draft.version.id(),
            approval: ApprovalProof {
                organization_id: bootstrap.organization_id,
                request_id: retention_approval_id,
                requested_by: export_requester,
                action: ApprovalAction::RetentionChange,
                resource_kind: "compliance.retention_policy_version".into(),
                resource_id: retention_draft.version.id().to_string(),
                now: verified_at,
            },
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect("activate retention policy version");
    assert_eq!(active_version.id(), retention_draft.version.id());

    let next_version = retention_service
        .append_version(AppendRetentionPolicyVersionCommand {
            context: export_context.clone(),
            policy_id: retention_draft.policy.id(),
            minimum_retention_days: 3_650,
            archive_after_days: Some(30),
            delete_after_days: None,
            rule: json!({"classification": ["standard", "regulated"]}),
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect("append retention policy version");
    assert_eq!(next_version.version(), 2);
    let stale_approval = retention_service
        .activate_version(ActivateRetentionPolicyVersionCommand {
            context: export_context,
            policy_id: retention_draft.policy.id(),
            policy_version_id: next_version.id(),
            approval: ApprovalProof {
                organization_id: bootstrap.organization_id,
                request_id: retention_approval_id,
                requested_by: export_requester,
                action: ApprovalAction::RetentionChange,
                resource_kind: "compliance.retention_policy_version".into(),
                resource_id: next_version.id().to_string(),
                now: verified_at,
            },
            trace_id: Uuid::now_v7().to_string(),
            now: verified_at,
        })
        .await
        .expect_err("an approval for version one cannot activate version two");
    assert!(matches!(
        stale_approval,
        copilotmail_application::RetentionPolicyError::Forbidden
    ));
    let unauthorized_activation =
        sqlx::query("UPDATE retention_policies SET active_version_id = $1 WHERE id = $2")
            .bind(next_version.id().as_uuid())
            .bind(retention_draft.policy.id().as_uuid())
            .execute(store.pool())
            .await
            .expect_err("reject retention activation without evidence");
    assert_eq!(
        unauthorized_activation
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("23514")
    );
    assert_append_only_error(
        sqlx::query("UPDATE retention_policy_versions SET rule = rule WHERE id = $1")
            .bind(next_version.id().as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM retention_policy_activations WHERE policy_id = $1")
            .bind(retention_draft.policy.id().as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM retention_policies WHERE id = $1")
            .bind(retention_draft.policy.id().as_uuid())
            .execute(store.pool())
            .await,
    );
    let retention_counts: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM retention_policy_versions WHERE policy_id = $1),
          (SELECT count(*) FROM retention_policy_activations WHERE policy_id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2
              AND action IN (
                'compliance.retention_policy.created',
                'compliance.retention_policy.versioned',
                'compliance.retention_policy.activated'
              )
              AND resource_id = $3)
        "#,
    )
    .bind(retention_draft.policy.id().as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(retention_draft.policy.id().to_string())
    .fetch_one(store.pool())
    .await
    .expect("query retention policy evidence");
    assert_eq!(retention_counts, (2, 1, 3));
}

async fn insert_test_audit_event(
    store: &PgControlPlaneStore,
    organization_id: copilotmail_domain::OrganizationId,
    actor_principal_id: PrincipalId,
    event_id: Uuid,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(
            id, organization_id, actor_principal_id, action, resource_type,
            resource_id, outcome, trace_id, details, occurred_at, received_at, request_id
        )
        VALUES (
            $1, $2, $3, 'mailbox.created', 'mailbox',
            $4, 'success', $5, '{}'::jsonb, now(), now(), $5
        )
        "#,
    )
    .bind(event_id)
    .bind(organization_id.as_uuid())
    .bind(actor_principal_id.as_uuid())
    .bind(Uuid::now_v7().to_string())
    .bind(Uuid::now_v7().to_string())
    .execute(store.pool())
    .await
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_identity_mapping_requires_exact_active_human() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "identity-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap identity tenant");
    let principal_id = Uuid::now_v7();
    let issuer = format!("https://login-{}.example.test", Uuid::new_v4().simple());
    let subject = format!("employee-{}", Uuid::new_v4().simple());
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        )
        VALUES ($1, $2, 'human', 'active', 'Integration Human', now(), now())
        "#,
    )
    .bind(principal_id)
    .bind(bootstrap.organization_id.as_uuid())
    .execute(store.pool())
    .await
    .expect("insert human principal");
    sqlx::query(
        r#"
        INSERT INTO human_profiles(principal_id, identity_provider, identity_subject)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(principal_id)
    .bind(&issuer)
    .bind(&subject)
    .execute(store.pool())
    .await
    .expect("insert human profile");

    assert_eq!(
        store
            .resolve_human_identity(&issuer, &subject)
            .await
            .unwrap(),
        Some(copilotmail_domain::PrincipalId::from_uuid(principal_id))
    );
    assert!(
        store
            .resolve_human_identity(&issuer, "different-subject")
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("UPDATE principals SET status = 'suspended' WHERE id = $1")
        .bind(principal_id)
        .execute(store.pool())
        .await
        .expect("suspend human");
    assert!(
        store
            .resolve_human_identity(&issuer, &subject)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn concurrent_oidc_owner_handover_retires_bootstrap_privilege() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "handover-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap handover tenant");
    let issuer = format!("https://login-{}.example.test", Uuid::new_v4().simple());
    let subject = format!("owner-{}", Uuid::new_v4().simple());
    let command = BindOidcOwnerCommand {
        organization_id: bootstrap.organization_id,
        issuer: issuer.clone(),
        subject: subject.clone(),
        display_name: "Enterprise Owner".into(),
        trace_id: Uuid::now_v7().to_string(),
        now: Utc::now(),
    };
    let first = BindOidcOwnerService::new(store.clone());
    let second = BindOidcOwnerService::new(store.clone());
    let (first_result, second_result) = expect_no_lock_hang(async {
        tokio::join!(first.execute(command.clone()), second.execute(command))
    })
    .await;
    let first_result = first_result.expect("first owner handover");
    let second_result = second_result.expect("second owner handover");

    assert_eq!(first_result.principal_id, second_result.principal_id);
    assert_ne!(first_result.created, second_result.created);
    let evidence: (String, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT status FROM principals WHERE id = $1),
          (SELECT count(*) FROM role_assignments
            WHERE principal_id = $1 AND revoked_at IS NULL),
          (SELECT count(*) FROM role_assignments
            WHERE principal_id = $2 AND revoked_at IS NULL),
          (SELECT count(*) FROM oidc_owner_bindings
            WHERE organization_id = $3 AND principal_id = $2),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $3 AND action = 'identity.oidc_owner.bound'),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $4 AND event_type = 'identity.oidc_owner.bound.v1')
        "#,
    )
    .bind(bootstrap.root_principal_id.as_uuid())
    .bind(first_result.principal_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(first_result.principal_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query owner handover evidence");
    assert_eq!(evidence, ("retired".into(), 0, 1, 1, 1, 1));

    assert_eq!(
        store
            .resolve_human_identity(&issuer, &subject)
            .await
            .unwrap(),
        Some(first_result.principal_id)
    );
    let owner_authorization = store
        .resolve_authorization(first_result.principal_id, Utc::now())
        .await
        .unwrap()
        .expect("human owner authorization");
    assert!(owner_authorization.permits(bootstrap.organization_id, Permission::OrganizationManage));
    assert!(
        store
            .resolve_authorization(bootstrap.root_principal_id, Utc::now())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn scim_provisioning_adopts_owner_is_idempotent_and_deprovisions_login() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "scim-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap SCIM tenant");
    let issuer = format!("https://login-{}.example.test", Uuid::new_v4().simple());
    let subject = format!("owner-{}", Uuid::new_v4().simple());
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: issuer.clone(),
            subject: subject.clone(),
            display_name: "Enterprise Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("handover SCIM tenant");

    let plaintext = format!("integration-scim-{}", Uuid::new_v4());
    let token_hash = Sha256::digest(plaintext.as_bytes()).to_vec();
    let credential = IssueScimTokenService::new(store.clone())
        .execute(IssueScimTokenCommand {
            organization_id: bootstrap.organization_id,
            token_id: ScimTokenId::new(),
            token_hash: token_hash.clone(),
            label: "PostgreSQL integration".into(),
            expires_at: Some(pg_micros(Utc::now()) + chrono::Duration::hours(1)),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("issue SCIM token");
    assert_eq!(
        store
            .authenticate_scim_token(&token_hash, Utc::now())
            .await
            .expect("authenticate SCIM token"),
        Some(credential.clone())
    );

    let command = ProvisionScimUserCommand {
        credential: credential.clone(),
        identity_provider: issuer.clone(),
        external_id: subject.clone(),
        user_name: "owner@example.test".into(),
        display_name: "Enterprise Owner".into(),
        active: true,
        trace_id: Uuid::now_v7().to_string(),
        now: Utc::now(),
    };
    let first = ScimUserService::new(store.clone())
        .create(command.clone())
        .await
        .expect("adopt existing owner");
    let replay = ScimUserService::new(store.clone())
        .create(command)
        .await
        .expect("replay owner adoption");
    assert!(first.created);
    assert!(!replay.created);
    assert_eq!(first.user.principal_id, owner.principal_id);
    assert_eq!(first.user.principal_id, replay.user.principal_id);

    let group = ScimGroupService::new(store.clone())
        .create(ProvisionScimGroupCommand {
            credential: credential.clone(),
            external_id: format!("group-{}", Uuid::new_v4()),
            display_name: "Engineering".into(),
            members: vec![owner.principal_id, owner.principal_id],
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("create SCIM group")
        .group;
    assert_eq!(group.members, [owner.principal_id]);
    let group = ScimGroupService::new(store.clone())
        .replace(ReplaceScimGroupCommand {
            credential: credential.clone(),
            group_id: group.group_id,
            external_id: group.external_id,
            display_name: "Platform Engineering".into(),
            members: Vec::new(),
            expected_version: Some(group.version),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("replace SCIM group");
    assert_eq!(group.version, 2);
    assert!(group.members.is_empty());
    ScimGroupService::new(store.clone())
        .delete(DeleteScimGroupCommand {
            credential: credential.clone(),
            group_id: group.group_id,
            expected_version: Some(group.version),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("delete SCIM group");
    assert!(
        store
            .get_scim_group(bootstrap.organization_id, group.group_id)
            .await
            .expect("query deleted group")
            .is_none()
    );

    let deprovisioned = ScimUserService::new(store.clone())
        .replace(ReplaceScimUserCommand {
            credential: credential.clone(),
            principal_id: first.user.principal_id,
            external_id: first.user.external_id,
            user_name: first.user.user_name,
            display_name: first.user.display_name,
            active: false,
            expected_version: Some(first.user.version),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("deprovision SCIM user");
    assert!(!deprovisioned.active);
    assert_eq!(deprovisioned.version, 2);
    assert!(
        store
            .resolve_human_identity(&issuer, &subject)
            .await
            .expect("resolve deprovisioned human")
            .is_none()
    );
    RevokeScimTokenService::new(store.clone())
        .execute(RevokeScimTokenCommand {
            organization_id: bootstrap.organization_id,
            token_id: credential.token_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("revoke SCIM token");
    assert!(
        store
            .authenticate_scim_token(&token_hash, Utc::now())
            .await
            .expect("resolve revoked SCIM token")
            .is_none()
    );

    let evidence: (i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM scim_tokens WHERE organization_id = $1),
          (SELECT count(*) FROM scim_users
            WHERE organization_id = $1 AND principal_id = $2 AND version = 2),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action IN ('identity.scim_user.adopted', 'identity.scim_user.updated')),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $3
              AND event_type IN ('identity.scim_user.adopted.v1', 'identity.scim_user.updated.v1')),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action IN (
                'identity.scim_group.provisioned',
                'identity.scim_group.updated',
                'identity.scim_group.deleted'
              )),
          (SELECT count(*) FROM scim_groups WHERE organization_id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1 AND action = 'identity.scim_token.revoked')
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(owner.principal_id.as_uuid())
    .bind(owner.principal_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query SCIM evidence");
    assert_eq!(evidence, (1, 1, 2, 2, 3, 0, 1));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn agent_credentials_overlap_reject_replay_and_revoke_immediately() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "agent-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Agent tenant");
    BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Agent Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind Agent owner");

    let first_public = test_public_key();
    let now = Utc::now();
    let registered = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Integration Agent".into(),
            environment: AgentEnvironment::Testing,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Validate Agent credential lifecycle".into(),
            review_due_at: now + chrono::Duration::days(90),
            public_key_pem: first_public.clone(),
            public_key_fingerprint: rsa_public_key_fingerprint(first_public.as_bytes()).unwrap(),
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register Agent");
    let second_public = test_public_key();
    let rotated = AddAgentCredentialService::new(store.clone())
        .execute(AddAgentCredentialCommand {
            organization_id: bootstrap.organization_id,
            principal_id: registered.principal_id,
            public_key_pem: second_public.clone(),
            public_key_fingerprint: rsa_public_key_fingerprint(second_public.as_bytes()).unwrap(),
            allowed_scopes: [Permission::OrganizationRead, Permission::MailboxUse]
                .into_iter()
                .collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("add overlapping Agent credential");
    let mtls_fingerprint = "ab".repeat(32);
    let mtls = AddAgentMtlsCredentialService::new(store.clone())
        .execute(AddAgentMtlsCredentialCommand {
            organization_id: bootstrap.organization_id,
            principal_id: registered.principal_id,
            certificate_fingerprint: mtls_fingerprint.clone(),
            allowed_scopes: [Permission::OrganizationRead].into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("add mTLS Agent credential");
    assert_eq!(
        store
            .authenticate_agent_mtls(
                registered.principal_id,
                &mtls_fingerprint,
                Some("192.0.2.20"),
                Utc::now(),
            )
            .await
            .unwrap()
            .expect("mTLS credential must authenticate")
            .credential_id,
        mtls.credential_id
    );

    assert!(
        store
            .get_agent_credential(
                registered.principal_id,
                registered.credential_id,
                Utc::now()
            )
            .await
            .unwrap()
            .is_some()
    );
    let jti_hash = Sha256::digest(b"integration-assertion-jti");
    assert!(
        store
            .consume_agent_assertion(
                registered.credential_id,
                &jti_hash,
                Utc::now() + chrono::Duration::minutes(2),
                Some("192.0.2.10"),
                Utc::now(),
            )
            .await
            .unwrap()
    );
    RevokeAgentCredentialService::new(store.clone())
        .execute(RevokeAgentCredentialCommand {
            organization_id: bootstrap.organization_id,
            credential_id: mtls.credential_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("revoke mTLS Agent credential");
    assert!(
        store
            .authenticate_agent_mtls(registered.principal_id, &mtls_fingerprint, None, Utc::now(),)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !store
            .consume_agent_assertion(
                registered.credential_id,
                &jti_hash,
                Utc::now() + chrono::Duration::minutes(2),
                Some("192.0.2.10"),
                Utc::now(),
            )
            .await
            .unwrap()
    );
    RevokeAgentCredentialService::new(store.clone())
        .execute(RevokeAgentCredentialCommand {
            organization_id: bootstrap.organization_id,
            credential_id: registered.credential_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("revoke first Agent credential");
    assert!(
        !store
            .validate_agent_token_binding(
                registered.principal_id,
                registered.credential_id,
                &registered.allowed_scopes,
                Utc::now(),
            )
            .await
            .unwrap()
    );
    assert!(
        store
            .validate_agent_token_binding(
                registered.principal_id,
                rotated.credential_id,
                &rotated.allowed_scopes,
                Utc::now(),
            )
            .await
            .unwrap()
    );

    let evidence: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM credentials WHERE principal_id = $1),
          (SELECT count(*) FROM credentials
            WHERE principal_id = $1 AND status = 'active'),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2
              AND action IN (
                'identity.agent.registered',
                'identity.agent_credential.added',
                'identity.agent_mtls_credential.added',
                'identity.agent_credential.revoked'
              )),
          (SELECT count(*) FROM oauth_client_assertion_jtis
            WHERE credential_id = $3)
        "#,
    )
    .bind(registered.principal_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(registered.credential_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("query Agent credential evidence");
    assert_eq!(evidence, (3, 1, 5, 1));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn agent_inspector_is_scoped_by_live_credentials_rbac_and_owner_integrity() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "agent-inspector-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Agent Inspector tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Agent Inspector Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind Agent Inspector owner");
    let public_key = test_public_key();
    let agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Agent Inspector Target".into(),
            environment: AgentEnvironment::Testing,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Inspect the Agent control-plane projection".into(),
            review_due_at: now + chrono::Duration::days(90),
            public_key_fingerprint: rsa_public_key_fingerprint(public_key.as_bytes()).unwrap(),
            public_key_pem: public_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register Agent Inspector target");
    let owner_context = store
        .resolve_authorization(owner.principal_id, now)
        .await
        .expect("resolve Agent Inspector owner authorization")
        .expect("Agent Inspector owner is active");
    let command = || GetAgentInspectorCommand {
        context: owner_context.clone(),
        organization_id: bootstrap.organization_id,
        agent_id: agent.principal_id,
        trace_id: Uuid::now_v7().to_string(),
    };
    let mailbox = MailboxManagementService::new(store.clone())
        .create_authorized(
            &owner_context,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Agent,
                primary_address: format!("inspector-{}@example.test", Uuid::new_v4().simple()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create Agent Inspector processing mailbox");
    MailboxDelegationService::new(store.clone())
        .grant_authorized(
            &owner_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: agent.principal_id,
                capabilities: [MailboxCapability::Read].into_iter().collect(),
                reason: "Inspect only aggregate processing state".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::days(30),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("delegate Agent processing mailbox read");
    let agent_context = store
        .resolve_authorization(agent.principal_id, Utc::now())
        .await
        .expect("resolve Agent Inspector target authorization")
        .expect("Agent Inspector target is active");
    let processing = AgentProcessingService::new(store.clone());
    let subscription = processing
        .ensure_subscription(EnsureAgentProcessingSubscriptionCommand {
            context: agent_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("create Agent Inspector processing subscription");

    let initial = AgentInspectorService::new(store.clone())
        .get(command())
        .await
        .expect("read initial Agent Inspector snapshot");
    assert_eq!(initial.agent.principal_id, agent.principal_id);
    assert_eq!(initial.owner.principal_id, owner.principal_id);
    assert_eq!(initial.token_access.active_credential_count, 1);
    assert_eq!(
        initial.token_access.effective_scopes,
        Permission::AGENT_TOKEN.into_iter().collect()
    );
    assert_eq!(initial.token_access.state.as_str(), "effective");
    assert_eq!(
        initial.control_plane.mail_admission_state.as_str(),
        "not_required"
    );
    assert_eq!(
        initial.control_plane.active_processing_subscription_count,
        1
    );
    assert_eq!(
        initial.control_plane.paused_processing_subscription_count,
        0
    );
    assert_eq!(
        initial.control_plane.retired_processing_subscription_count,
        0
    );
    assert!(initial.control_plane.last_processing_activity_at.is_none());

    processing
        .change_subscription(ChangeAgentProcessingSubscriptionCommand {
            context: agent_context.clone(),
            subscription_id: subscription.subscription_id,
            expected_status: AgentProcessingSubscriptionStatus::Active,
            target_status: AgentProcessingSubscriptionStatus::Paused,
            idempotency_key: "agent-inspector-pause-subscription-000001".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("pause Agent Inspector processing subscription");
    let paused = AgentInspectorService::new(store.clone())
        .get(command())
        .await
        .expect("read Agent Inspector after subscription pause");
    assert_eq!(paused.control_plane.active_processing_subscription_count, 0);
    assert_eq!(paused.control_plane.paused_processing_subscription_count, 1);
    assert_eq!(
        paused.control_plane.retired_processing_subscription_count,
        0
    );
    assert!(paused.control_plane.last_processing_activity_at.is_some());

    processing
        .change_subscription(ChangeAgentProcessingSubscriptionCommand {
            context: agent_context,
            subscription_id: subscription.subscription_id,
            expected_status: AgentProcessingSubscriptionStatus::Paused,
            target_status: AgentProcessingSubscriptionStatus::Retired,
            idempotency_key: "agent-inspector-retire-subscription-00001".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("retire Agent Inspector processing subscription");
    let retired = AgentInspectorService::new(store.clone())
        .get(command())
        .await
        .expect("read Agent Inspector after subscription retirement");
    assert_eq!(
        retired.control_plane.active_processing_subscription_count,
        0
    );
    assert_eq!(
        retired.control_plane.paused_processing_subscription_count,
        0
    );
    assert_eq!(
        retired.control_plane.retired_processing_subscription_count,
        1
    );
    assert!(retired.control_plane.last_processing_activity_at.is_some());

    let revoked_assignment = sqlx::query(
        r#"
        UPDATE role_assignments
           SET revoked_at = $3
         WHERE organization_id = $1
           AND principal_id = $2
           AND revoked_at IS NULL
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(agent.principal_id.as_uuid())
    .bind(Utc::now())
    .execute(store.pool())
    .await
    .expect("revoke Agent runtime role assignment");
    assert_eq!(revoked_assignment.rows_affected(), 1);

    let rbac_restricted = AgentInspectorService::new(store.clone())
        .get(command())
        .await
        .expect("read Agent Inspector after Agent role revocation");
    assert_eq!(rbac_restricted.token_access.active_credential_count, 1);
    assert!(rbac_restricted.token_access.effective_scopes.is_empty());
    assert_eq!(
        rbac_restricted.token_access.state.as_str(),
        "no_effective_scope"
    );
    assert_eq!(
        rbac_restricted
            .control_plane
            .retired_processing_subscription_count,
        1
    );

    let expired_credential =
        sqlx::query("UPDATE credentials SET expires_at = $2 WHERE id = $1 AND status = 'active'")
            .bind(agent.credential_id.as_uuid())
            // Expiry is evaluated on the database clock, so keep the margin
            // wider than any realistic caller/database clock skew.
            .bind(Utc::now() - chrono::Duration::minutes(1))
            .execute(store.pool())
            .await
            .expect("expire Agent Inspector credential");
    assert_eq!(expired_credential.rows_affected(), 1);

    let expired = AgentInspectorService::new(store.clone())
        .get(command())
        .await
        .expect("read Agent Inspector after credential expiry");
    assert_eq!(expired.token_access.active_credential_count, 0);
    assert!(expired.token_access.effective_scopes.is_empty());
    assert_eq!(expired.token_access.state.as_str(), "no_active_credential");

    sqlx::query(
        r#"
        UPDATE agent_profiles
           SET owner_principal_id = $2
         WHERE principal_id = $1
        "#,
    )
    .bind(agent.principal_id.as_uuid())
    .bind(bootstrap.root_principal_id.as_uuid())
    .execute(store.pool())
    .await
    .expect("corrupt Agent profile owner to the bootstrap Service");
    let corrupted = AgentInspectorService::new(store.clone())
        .get(command())
        .await;
    assert!(
        matches!(
            corrupted,
            Err(AgentInspectorError::Unavailable)
                | Err(AgentInspectorError::Store(StoreError::Unavailable))
        ),
        "corrupted Agent owner must fail closed: {corrupted:?}"
    );

    let evidence: (i64, i64, bool, bool) = sqlx::query_as(
        r#"
        SELECT
            (SELECT count(*)
               FROM audit_events
              WHERE organization_id = $1
                AND action = 'identity.agent.queried'
                AND resource_id = $2),
            (SELECT count(*)
               FROM outbox_events
              WHERE event_type = 'identity.agent.queried.v1'
                AND aggregate_type = 'identity.agent'
                AND aggregate_id = $2),
            (SELECT bool_and(details::text NOT LIKE '%Inspect the Agent control-plane projection%')
               FROM audit_events
              WHERE organization_id = $1
                AND action = 'identity.agent.queried'),
            (SELECT bool_and(payload::text NOT LIKE '%agent.mail.read%')
               FROM outbox_events
              WHERE event_type = 'identity.agent.queried.v1'
                AND aggregate_id = $2)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(agent.principal_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query Agent Inspector evidence");
    assert_eq!(evidence.0, 5, "only successful reads are audited");
    assert_eq!(evidence.1, 5, "only successful reads emit Outbox evidence");
    assert!(evidence.2, "audit details must not disclose Agent purpose");
    assert!(
        evidence.3,
        "Outbox payload must not disclose effective scopes"
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn mailbox_delegation_is_tenant_resource_time_and_send_as_bound() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "mailbox-auth-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap mailbox tenant");
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Mailbox Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind mailbox owner");
    let now = Utc::now();
    let public_key = test_public_key();
    let agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Mailbox Agent".into(),
            environment: AgentEnvironment::Testing,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Validate mailbox resource authorization".into(),
            review_due_at: now + chrono::Duration::days(90),
            public_key_fingerprint: rsa_public_key_fingerprint(public_key.as_bytes()).unwrap(),
            public_key_pem: public_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register mailbox Agent");
    let mailbox_service = MailboxManagementService::new(store.clone());
    let mailbox = mailbox_service
        .create(CreateMailboxCommand {
            organization_id: bootstrap.organization_id,
            kind: MailboxKind::Shared,
            primary_address: format!("shared-{}@example.test", Uuid::new_v4().simple()),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create delegated mailbox");
    let other_mailbox = mailbox_service
        .create(CreateMailboxCommand {
            organization_id: bootstrap.organization_id,
            kind: MailboxKind::Shared,
            primary_address: format!("other-{}@example.test", Uuid::new_v4().simple()),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create non-delegated mailbox");
    let delegation_service = MailboxDelegationService::new(store.clone());
    let grant = delegation_service
        .grant(GrantMailboxDelegationCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            grantee_principal_id: agent.principal_id,
            capabilities: [MailboxCapability::Read, MailboxCapability::SendAs]
                .into_iter()
                .collect(),
            reason: "Process approved shared mailbox traffic".into(),
            valid_from: now,
            valid_until: now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("grant mailbox delegation");

    let access = store
        .resolve_mailbox_access(agent.principal_id, mailbox.mailbox_id, now)
        .await
        .unwrap()
        .expect("delegated mailbox snapshot");
    assert!(
        access
            .authorize(
                bootstrap.organization_id,
                mailbox.mailbox_id,
                MailboxCapability::Read,
                None,
            )
            .is_some()
    );
    assert!(
        access
            .authorize(
                bootstrap.organization_id,
                mailbox.mailbox_id,
                MailboxCapability::SendAs,
                None,
            )
            .is_none()
    );
    assert!(
        access
            .authorize(
                bootstrap.organization_id,
                mailbox.mailbox_id,
                MailboxCapability::SendAs,
                Some(owner.principal_id),
            )
            .is_some()
    );
    assert!(
        store
            .resolve_mailbox_access(agent.principal_id, other_mailbox.mailbox_id, now)
            .await
            .unwrap()
            .expect("same-tenant mailbox remains opaque")
            .authorize(
                bootstrap.organization_id,
                other_mailbox.mailbox_id,
                MailboxCapability::Read,
                None,
            )
            .is_none()
    );

    delegation_service
        .revoke(RevokeMailboxDelegationCommand {
            organization_id: bootstrap.organization_id,
            delegation_id: grant.delegation_id,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("revoke mailbox delegation");
    let revoked = store
        .resolve_mailbox_access(agent.principal_id, mailbox.mailbox_id, now)
        .await
        .unwrap()
        .expect("mailbox still exists");
    assert!(
        revoked
            .authorize(
                bootstrap.organization_id,
                mailbox.mailbox_id,
                MailboxCapability::Read,
                None,
            )
            .is_none()
    );

    delegation_service
        .grant(GrantMailboxDelegationCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            grantee_principal_id: agent.principal_id,
            capabilities: [MailboxCapability::Read].into_iter().collect(),
            reason: "Short-lived processing window".into(),
            valid_from: now,
            valid_until: now + chrono::Duration::minutes(1),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("grant expiring mailbox delegation");
    let expired = store
        .resolve_mailbox_access(
            agent.principal_id,
            mailbox.mailbox_id,
            now + chrono::Duration::minutes(2),
        )
        .await
        .unwrap()
        .expect("mailbox still exists after delegation expiry");
    assert!(
        expired
            .authorize(
                bootstrap.organization_id,
                mailbox.mailbox_id,
                MailboxCapability::Read,
                None,
            )
            .is_none()
    );

    let other_tenant = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "other-tenant-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap other tenant");
    BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: other_tenant.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Other Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind other owner");
    let foreign_mailbox = MailboxManagementService::new(store.clone())
        .create(CreateMailboxCommand {
            organization_id: other_tenant.organization_id,
            kind: MailboxKind::Shared,
            primary_address: format!("foreign-{}@example.test", Uuid::new_v4().simple()),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create foreign mailbox");
    assert!(
        store
            .resolve_mailbox_access(agent.principal_id, foreign_mailbox.mailbox_id, now)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_mailbox_directory_is_bound_authorized_cursor_safe_and_human_only() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "human-directory-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Human mailbox directory tenant");
    let now = Utc::now();
    let caller_id = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let owner_id = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let caller = store
        .resolve_authorization(caller_id, now)
        .await
        .expect("resolve directory caller")
        .expect("directory caller is active");
    let owner = store
        .resolve_authorization(owner_id, now)
        .await
        .expect("resolve directory mailbox owner")
        .expect("directory mailbox owner is active");
    let mailbox_service = MailboxManagementService::new(store.clone());
    let owned = mailbox_service
        .create_authorized(
            &caller,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Human,
                primary_address: format!(
                    "directory-owned-{}@example.test",
                    Uuid::new_v4().simple()
                ),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create caller-owned mailbox");
    let delegated_send_as = mailbox_service
        .create_authorized(
            &owner,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Shared,
                primary_address: format!(
                    "directory-send-as-{}@example.test",
                    Uuid::new_v4().simple()
                ),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create send-as delegated mailbox");
    let delegated_send = mailbox_service
        .create_authorized(
            &owner,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Shared,
                primary_address: format!("directory-send-{}@example.test", Uuid::new_v4().simple()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create send delegated mailbox");
    let unbound = mailbox_service
        .create_authorized(
            &owner,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Shared,
                primary_address: format!(
                    "directory-unbound-{}@example.test",
                    Uuid::new_v4().simple()
                ),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create unbound mailbox");
    let unreadable = mailbox_service
        .create_authorized(
            &owner,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Shared,
                primary_address: format!(
                    "directory-unreadable-{}@example.test",
                    Uuid::new_v4().simple()
                ),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create unreadable mailbox");

    let delegation_service = MailboxDelegationService::new(store.clone());
    delegation_service
        .grant_authorized(
            &owner,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: delegated_send_as.mailbox_id,
                grantee_principal_id: caller_id,
                capabilities: [MailboxCapability::Read, MailboxCapability::SendAs]
                    .into_iter()
                    .collect(),
                reason: "Human directory send-as-only fixture".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::hours(1),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("grant read and send-as delegation");
    delegation_service
        .grant_authorized(
            &owner,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: delegated_send.mailbox_id,
                grantee_principal_id: caller_id,
                capabilities: [MailboxCapability::Read, MailboxCapability::Send]
                    .into_iter()
                    .collect(),
                reason: "Human directory read-send fixture".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::hours(1),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("grant read and send delegation");
    delegation_service
        .grant_authorized(
            &owner,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: unbound.mailbox_id,
                grantee_principal_id: caller_id,
                capabilities: [MailboxCapability::Read].into_iter().collect(),
                reason: "Human directory unbound fixture".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::hours(1),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("grant read delegation on unbound mailbox");

    let binding_service = MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct directory mailbox binding service");
    for mailbox_id in [
        owned.mailbox_id,
        delegated_send_as.mailbox_id,
        delegated_send.mailbox_id,
    ] {
        binding_service
            .bind_authorized(
                if mailbox_id == owned.mailbox_id {
                    &caller
                } else {
                    &owner
                },
                BindMailEngineMailboxCommand {
                    organization_id: bootstrap.organization_id,
                    mailbox_id,
                    account_ref: random_numeric_engine_ref(),
                    expected_version: 0,
                    trace_id: Uuid::now_v7().to_string(),
                    now,
                },
            )
            .await
            .expect("bind visible directory mailbox");
    }
    binding_service
        .bind_authorized(
            &owner,
            BindMailEngineMailboxCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: unreadable.mailbox_id,
                account_ref: random_numeric_engine_ref(),
                expected_version: 0,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("bind unreadable directory mailbox");

    let directory = HumanMailboxDirectoryService::new(store.clone());
    let mut cursor = None;
    let mut observed = Vec::new();
    loop {
        let page = directory
            .list(ListHumanMailboxDirectoryCommand {
                context: caller.clone(),
                limit: 1,
                cursor,
                now,
            })
            .await
            .expect("list one Human mailbox directory page");
        assert!(page.items.len() <= 1);
        observed.extend(page.items);
        let Some(next_cursor) = page.next_cursor else {
            break;
        };
        assert!(next_cursor.starts_with("v1."));
        cursor = Some(next_cursor);
    }
    let mut expected_ids = vec![
        owned.mailbox_id,
        delegated_send_as.mailbox_id,
        delegated_send.mailbox_id,
    ];
    expected_ids.sort_unstable();
    assert_eq!(
        observed
            .iter()
            .map(|item| item.mailbox_id)
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert!(
        observed
            .iter()
            .find(|item| item.mailbox_id == owned.mailbox_id)
            .expect("owned mailbox is visible")
            .can_send
    );
    assert!(
        !observed
            .iter()
            .find(|item| item.mailbox_id == delegated_send_as.mailbox_id)
            .expect("send-as mailbox is visible")
            .can_send
    );
    assert!(
        observed
            .iter()
            .find(|item| item.mailbox_id == delegated_send.mailbox_id)
            .expect("send mailbox is visible")
            .can_send
    );
    assert!(
        !observed
            .iter()
            .any(|item| item.mailbox_id == unbound.mailbox_id)
    );
    assert!(
        !observed
            .iter()
            .any(|item| item.mailbox_id == unreadable.mailbox_id)
    );

    let hidden_cursor = format!("v1.{}", unbound.mailbox_id);
    assert!(matches!(
        directory
            .list(ListHumanMailboxDirectoryCommand {
                context: caller.clone(),
                limit: 25,
                cursor: Some(hidden_cursor),
                now,
            })
            .await,
        Err(HumanMailboxDirectoryError::InvalidCursor)
    ));

    let agent_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        ) VALUES ($1, $2, 'agent', 'active', 'Directory Agent', $3, $3)
        "#,
    )
    .bind(agent_id)
    .bind(bootstrap.organization_id.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert directory Agent fixture");
    let agent_context = copilotmail_domain::AuthorizationContext {
        organization_id: bootstrap.organization_id,
        principal_id: PrincipalId::from_uuid(agent_id),
        permissions: [Permission::MailboxUse].into_iter().collect(),
        authentication: None,
    };
    assert!(matches!(
        directory
            .list(ListHumanMailboxDirectoryCommand {
                context: agent_context,
                limit: 25,
                cursor: None,
                now,
            })
            .await,
        Err(HumanMailboxDirectoryError::Forbidden)
    ));
}

#[derive(Default)]
struct IntegrationHumanDraftAdapter {
    next_id: AtomicU64,
}

impl MailDataPlanePort for IntegrationHumanDraftAdapter {
    fn data_plane_id(&self) -> MailDataPlaneId {
        legacy_mail_data_plane_id()
    }
}

#[async_trait]
impl MailDraftStore for IntegrationHumanDraftAdapter {
    async fn probe_drafts(&self) -> Result<(), MailEngineError> {
        Ok(())
    }

    async fn save_text_draft(
        &self,
        account_ref: &str,
        previous_draft_id: Option<&str>,
        input: &TextDraftInput,
    ) -> Result<MailDraft, MailEngineError> {
        assert!(!account_ref.is_empty());
        assert!(input.from.ends_with("@example.test"));
        let sequence = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(MailDraft {
            id: format!("human-draft-{sequence}"),
            thread_id: format!("human-thread-{sequence}"),
            reply_source_automatic: false,
            previous_draft_destroyed: previous_draft_id.is_some(),
        })
    }
}

struct IntegrationHumanDraftReader {
    expected_account_ref: String,
    expected_email_ref: String,
    expected_thread_ref: String,
    expected_from: MailboxAddress,
    content: MailDraftContent,
    suspend_after_read: Option<(sqlx::PgPool, OrganizationId, PrincipalId)>,
}

impl MailDataPlanePort for IntegrationHumanDraftReader {
    fn data_plane_id(&self) -> MailDataPlaneId {
        legacy_mail_data_plane_id()
    }
}

#[async_trait]
impl MailDraftContentReader for IntegrationHumanDraftReader {
    async fn read_exact_text_draft(
        &self,
        account_ref: &str,
        email_ref: &str,
        expected_thread_ref: &str,
        expected_from: &MailboxAddress,
    ) -> Result<Option<MailDraftContent>, MailEngineError> {
        assert_eq!(account_ref, self.expected_account_ref);
        assert_eq!(email_ref, self.expected_email_ref);
        assert_eq!(expected_thread_ref, self.expected_thread_ref);
        assert_eq!(expected_from, &self.expected_from);
        if let Some((pool, organization_id, principal_id)) = &self.suspend_after_read {
            sqlx::query(
                r#"
                UPDATE principals
                   SET status = 'suspended', updated_at = clock_timestamp()
                 WHERE id = $1 AND organization_id = $2
                "#,
            )
            .bind(principal_id.as_uuid())
            .bind(organization_id.as_uuid())
            .execute(pool)
            .await
            .expect("suspend Human after remote draft read");
        }
        Ok(Some(self.content.clone()))
    }
}

/// A deterministic local data-plane fixture for the sent-directory tests.
/// Its first delivery observation leaves one recipient queued; the second
/// observation changes that recipient to delivered so the integration test can
/// prove the final recheck binds the exact latest snapshot identity.
#[derive(Default)]
struct IntegrationHumanSentSubmissionAdapter {
    delivery_reads: AtomicU64,
}

impl MailDataPlanePort for IntegrationHumanSentSubmissionAdapter {
    fn data_plane_id(&self) -> MailDataPlaneId {
        legacy_mail_data_plane_id()
    }
}

#[async_trait]
impl MailSubmissionAdapter for IntegrationHumanSentSubmissionAdapter {
    async fn probe_submissions(&self) -> Result<(), MailEngineError> {
        Ok(())
    }

    async fn reconcile_submission(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        expected_from: &str,
    ) -> Result<MailSubmissionReconciliation, MailEngineError> {
        assert!(!account_ref.is_empty());
        assert!(engine_email_ref.starts_with("human-draft-"));
        assert!(expected_from.ends_with("@example.test"));
        Ok(MailSubmissionReconciliation::Accepted {
            identity_ref: format!("human-sent-identity-{engine_email_ref}"),
            submission_ref: format!("human-sent-submission-{engine_email_ref}"),
        })
    }

    async fn create_submission(
        &self,
        _account_ref: &str,
        _engine_email_ref: &str,
        _identity_ref: &str,
    ) -> Result<CreatedMailSubmission, MailEngineError> {
        panic!("the integration fixture always reconciles an existing submission")
    }

    async fn get_delivery_snapshot(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        identity_ref: &str,
        submission_ref: &str,
    ) -> Result<MailDeliverySnapshot, MailEngineError> {
        assert!(!account_ref.is_empty());
        assert!(engine_email_ref.starts_with("human-draft-"));
        assert!(identity_ref.starts_with("human-sent-identity-human-draft-"));
        assert!(submission_ref.starts_with("human-sent-submission-human-draft-"));
        let observation = self.delivery_reads.fetch_add(1, Ordering::SeqCst);
        let second_state = if observation == 0 {
            RecipientDeliveryState::Queued
        } else {
            RecipientDeliveryState::Delivered
        };
        Ok(MailDeliverySnapshot {
            submission_ref: submission_ref.into(),
            source_state: format!("human-sent-delivery-state-{observation}"),
            recipients: vec![
                RecipientDeliveryEvidence {
                    recipient: MailboxAddress::parse("first@example.test")
                        .expect("static first recipient is valid"),
                    state: RecipientDeliveryState::Delivered,
                    smtp_reply: "250 2.0.0 Accepted".into(),
                },
                RecipientDeliveryEvidence {
                    recipient: MailboxAddress::parse("second@example.test")
                        .expect("static second recipient is valid"),
                    state: second_state,
                    smtp_reply: if observation == 0 {
                        "250 2.1.5 Queued"
                    } else {
                        "250 2.0.0 Accepted"
                    }
                    .into(),
                },
            ],
        })
    }
}

async fn create_human_sent_directory_submission(
    store: &PgControlPlaneStore,
    drafts: &DraftService<PgControlPlaneStore>,
    context: &AuthorizationContext,
    mailbox_id: copilotmail_domain::MailboxId,
    marker: &str,
    now: chrono::DateTime<Utc>,
) -> (SavedTextDraft, MailSubmissionIntentRecord) {
    let draft = drafts
        .save(
            context,
            SaveTextDraftCommand {
                mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec![
                    format!("first-{marker}@example.test"),
                    format!("second-{marker}@example.test"),
                ],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: format!("Human sent directory {marker}"),
                text_body: format!("Human sent directory fixture {marker}"),
                now,
            },
        )
        .await
        .expect("save Human sent-directory fixture draft");
    let intent = MailSubmissionService::new(store.clone())
        .request(
            context,
            RequestMailSubmissionCommand {
                mailbox_id,
                draft_id: draft.draft_id,
                expected_draft_version: draft.version,
                idempotency_key: format!(
                    "human-sent-directory-{marker}-{}",
                    Uuid::new_v4().simple()
                ),
                first_contact_approval_request_id: None,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("freeze Human sent-directory fixture draft");
    (draft, intent)
}

async fn count_human_sent_directory_evidence(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    mailbox_id: copilotmail_domain::MailboxId,
) -> (i64, i64) {
    sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action = 'mail.submissions.queried'
              AND resource_id = $2),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail.submissions.queried.v1'
              AND aggregate_id = $2
              AND payload ->> 'organization_id' = $1::text)
        "#,
    )
    .bind(organization_id.as_uuid())
    .bind(mailbox_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count sent-directory disclosure evidence")
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_sent_directory_is_private_cursor_safe_audited_and_finally_rechecked() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "human-sent-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Human sent-directory tenant");
    // Keep grant validity safely before the database-clock final rechecks.
    let now = Utc::now();
    let owner_id = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let owner_context = store
        .resolve_authorization(owner_id, now)
        .await
        .expect("resolve sent-directory owner")
        .expect("sent-directory owner is active");
    let other_human_id = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let other_context = store
        .resolve_authorization(other_human_id, now)
        .await
        .expect("resolve second sent-directory Human")
        .expect("second sent-directory Human is active");
    let mailbox = MailboxManagementService::new(store.clone())
        .create_authorized(
            &owner_context,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Shared,
                primary_address: format!("human-sent-{}@example.test", Uuid::new_v4().simple()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create shared sent-directory mailbox");
    let account_ref = random_numeric_engine_ref();
    MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct sent-directory binding service")
    .bind_authorized(
        &owner_context,
        BindMailEngineMailboxCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            account_ref,
            expected_version: 0,
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    )
    .await
    .expect("bind sent-directory mailbox");
    MailboxDelegationService::new(store.clone())
        .grant_authorized(
            &owner_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: other_human_id,
                capabilities: [MailboxCapability::Read, MailboxCapability::Send]
                    .into_iter()
                    .collect(),
                reason: "Sent directory private-history fixture".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::hours(1),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("grant second Human ordinary shared-mailbox access");

    let drafts = DraftService::new(
        store.clone(),
        Arc::new(IntegrationHumanDraftAdapter::default()),
    );
    let (_, older) = create_human_sent_directory_submission(
        &store,
        &drafts,
        &owner_context,
        mailbox.mailbox_id,
        "older",
        now + chrono::Duration::seconds(1),
    )
    .await;
    let (pending_draft, pending) = create_human_sent_directory_submission(
        &store,
        &drafts,
        &owner_context,
        mailbox.mailbox_id,
        "pending",
        now + chrono::Duration::seconds(2),
    )
    .await;
    let (_, newer) = create_human_sent_directory_submission(
        &store,
        &drafts,
        &owner_context,
        mailbox.mailbox_id,
        "newer",
        now + chrono::Duration::seconds(3),
    )
    .await;
    let (_, other) = create_human_sent_directory_submission(
        &store,
        &drafts,
        &other_context,
        mailbox.mailbox_id,
        "other",
        now + chrono::Duration::seconds(4),
    )
    .await;

    let adapter: Arc<dyn MailSubmissionAdapter> =
        Arc::new(IntegrationHumanSentSubmissionAdapter::default());
    let executor = MailSubmissionExecutor::new(store.clone(), Arc::clone(&adapter));
    // The finalization SQL stamps `accepted_at` with the database-owned
    // `clock_timestamp()`, and terminal intents are immutable afterwards. To
    // exercise the UUID tie-breaker deterministically, the two accepts are
    // normalized to one shared timestamp through a dedicated replication-role
    // connection; the shared pool never sees the relaxed session.
    let accepted_at = now + chrono::Duration::minutes(5);
    for (intent, executed_at) in [
        (&older, accepted_at),
        // The cursor must remain stable when multiple durable accepts share
        // exactly the same timestamp, so the UUID tie-breaker is exercised.
        (&newer, accepted_at),
        (&other, accepted_at + chrono::Duration::seconds(1)),
    ] {
        assert_eq!(
            executor
                .execute(ExecuteMailSubmissionCommand {
                    submission_id: intent.submission_id,
                    trace_id: Uuid::now_v7().to_string(),
                    now: executed_at,
                })
                .await
                .expect("accept sent-directory fixture submission"),
            MailSubmissionExecutionResult::Accepted
        );
    }
    {
        // A single-connection pool guarantees the relaxed session setting and
        // the fixture UPDATE share one session and are discarded together.
        let surgery_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&support::database_url())
            .await
            .expect("open fixture-surgery connection");
        sqlx::query("SET session_replication_role = 'replica'")
            .execute(&surgery_pool)
            .await
            .expect("enable fixture-surgery role");
        sqlx::query(
            r#"
            UPDATE mail_submission_intents
               SET accepted_at = (
                   SELECT min(accepted_at)
                     FROM mail_submission_intents
                    WHERE organization_id = $1 AND id IN ($2, $3)
               )
             WHERE organization_id = $1
               AND id IN ($2, $3)
            "#,
        )
        .bind(bootstrap.organization_id.as_uuid())
        .bind(older.submission_id.as_uuid())
        .bind(newer.submission_id.as_uuid())
        .execute(&surgery_pool)
        .await
        .expect("share one acceptance timestamp across the fixture");
        surgery_pool.close().await;
    }
    let pending_lifecycle: (String, String) = sqlx::query_as(
        r#"
        SELECT
          (SELECT status FROM mail_submission_intents WHERE id = $1),
          (SELECT status FROM mail_drafts WHERE id = $2)
        "#,
    )
    .bind(pending.submission_id.as_uuid())
    .bind(pending_draft.draft_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("read pending non-sent fixture lifecycle");
    assert_eq!(pending_lifecycle, ("pending".into(), "frozen".into()));

    let delivery_service = MailDeliveryReconciliationService::new(
        store.clone(),
        Arc::clone(&adapter),
        MailDataPlaneFingerprintScheme::LegacyV1,
    );
    assert_eq!(
        delivery_service
            .reconcile(ReconcileMailDeliveryCommand {
                submission_id: newer.submission_id,
                now: accepted_at + chrono::Duration::seconds(2),
            })
            .await
            .expect("record initial sent-directory delivery summary"),
        MailDeliveryReconciliationResult::Pending { inserted: true }
    );

    let initial_batch = store
        .list_human_sent_submissions(&HumanSentListQuery {
            organization_id: bootstrap.organization_id,
            principal_id: owner_id,
            mailbox_id: mailbox.mailbox_id,
            after_submission_id: None,
            limit: 50,
            now: accepted_at + chrono::Duration::seconds(3),
        })
        .await
        .expect("load owner sent-directory page directly");
    assert!(initial_batch.active_human);
    assert!(initial_batch.cursor_valid);
    assert!(initial_batch.authorization.is_some());
    assert!(!initial_batch.has_more);
    assert!(
        newer.submission_id > older.submission_id,
        "the later durable submission is the UUID tie-breaker"
    );
    assert_eq!(
        initial_batch
            .candidates
            .iter()
            .map(|candidate| candidate.submission_id)
            .collect::<Vec<_>>(),
        vec![newer.submission_id, older.submission_id],
        "only this Human's accepted, sent submissions are visible in acceptance order"
    );
    assert!(
        !initial_batch
            .candidates
            .iter()
            .any(|candidate| candidate.submission_id == pending.submission_id),
        "a pending intent and frozen non-sent Draft must not appear"
    );
    assert!(
        !initial_batch
            .candidates
            .iter()
            .any(|candidate| candidate.submission_id == other.submission_id),
        "shared mailbox access must not disclose another Human's history"
    );
    let initial_newer = initial_batch
        .candidates
        .iter()
        .find(|candidate| candidate.submission_id == newer.submission_id)
        .cloned()
        .expect("newer accepted submission is visible");
    let initial_older = initial_batch
        .candidates
        .iter()
        .find(|candidate| candidate.submission_id == older.submission_id)
        .expect("older accepted submission is visible");
    assert_eq!(
        initial_newer.accepted_at, initial_older.accepted_at,
        "the fixture intentionally shares one acceptance timestamp"
    );
    let initial_summary = initial_newer
        .delivery
        .as_ref()
        .expect("initial delivery counters are available");
    assert_eq!(initial_summary.recipient_count, 2);
    assert_eq!(initial_summary.delivered_count, 1);
    assert_eq!(initial_summary.failed_count, 0);
    assert_eq!(initial_summary.queued_count, 1);
    assert_eq!(initial_summary.unknown_count, 0);
    assert!(!initial_summary.terminal);

    let sent_directory = HumanSentService::new(store.clone());
    let evidence_before_first =
        count_human_sent_directory_evidence(&store, bootstrap.organization_id, mailbox.mailbox_id)
            .await;
    let first_page = sent_directory
        .list(ListHumanSentSubmissionsCommand {
            context: owner_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            limit: 1,
            cursor: None,
            trace_id: Uuid::now_v7().to_string(),
            now: accepted_at + chrono::Duration::seconds(4),
        })
        .await
        .expect("disclose first owner sent-directory page after final recheck");
    assert_eq!(first_page.items.len(), 1);
    assert_eq!(first_page.items[0].submission_id, newer.submission_id);
    assert_eq!(first_page.items[0].recipient_count, 2);
    assert_eq!(
        first_page
            .items
            .first()
            .and_then(|item| item.delivery.as_ref())
            .map(|summary| (
                summary.delivered_count,
                summary.queued_count,
                summary.terminal
            )),
        Some((1, 1, false))
    );
    let newest_cursor = first_page
        .next_cursor
        .clone()
        .expect("a one-item owner page has a cursor for the older item");
    let evidence_after_first =
        count_human_sent_directory_evidence(&store, bootstrap.organization_id, mailbox.mailbox_id)
            .await;
    assert_eq!(
        evidence_after_first,
        (evidence_before_first.0 + 1, evidence_before_first.1 + 1),
        "the final recheck, audit, and Outbox write commit atomically before disclosure"
    );

    let invalid_cursor_evidence =
        count_human_sent_directory_evidence(&store, bootstrap.organization_id, mailbox.mailbox_id)
            .await;
    assert!(matches!(
        sent_directory
            .list(ListHumanSentSubmissionsCommand {
                context: other_context.clone(),
                mailbox_id: mailbox.mailbox_id,
                limit: 1,
                cursor: Some(newest_cursor.clone()),
                trace_id: Uuid::now_v7().to_string(),
                now: accepted_at + chrono::Duration::seconds(4),
            })
            .await,
        Err(HumanSentError::InvalidCursor)
    ));
    assert_eq!(
        count_human_sent_directory_evidence(&store, bootstrap.organization_id, mailbox.mailbox_id,)
            .await,
        invalid_cursor_evidence,
        "a cursor for another Human's history must not write disclosure evidence"
    );

    let second_page = sent_directory
        .list(ListHumanSentSubmissionsCommand {
            context: owner_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            limit: 1,
            cursor: Some(newest_cursor),
            trace_id: Uuid::now_v7().to_string(),
            now: accepted_at + chrono::Duration::seconds(4),
        })
        .await
        .expect("disclose second owner sent-directory page");
    assert_eq!(
        second_page
            .items
            .iter()
            .map(|item| item.submission_id)
            .collect::<Vec<_>>(),
        vec![older.submission_id]
    );
    assert!(second_page.next_cursor.is_none());
    assert!(second_page.items[0].delivery.is_none());
    let other_page = sent_directory
        .list(ListHumanSentSubmissionsCommand {
            context: other_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            limit: 50,
            cursor: None,
            trace_id: Uuid::now_v7().to_string(),
            now: accepted_at + chrono::Duration::seconds(4),
        })
        .await
        .expect("list second Human's own sent history");
    assert_eq!(
        other_page
            .items
            .iter()
            .map(|item| item.submission_id)
            .collect::<Vec<_>>(),
        vec![other.submission_id]
    );

    assert_eq!(
        delivery_service
            .reconcile(ReconcileMailDeliveryCommand {
                submission_id: newer.submission_id,
                now: accepted_at + chrono::Duration::seconds(5),
            })
            .await
            .expect("record changed latest delivery snapshot"),
        MailDeliveryReconciliationResult::Terminal { inserted: true }
    );
    let mut snapshot_drift_transaction = store
        .begin()
        .await
        .expect("begin stale sent-directory snapshot recheck");
    assert!(
        !snapshot_drift_transaction
            .human_sent_list_still_authorized(&HumanSentListRecheck {
                organization_id: bootstrap.organization_id,
                principal_id: owner_id,
                mailbox_id: mailbox.mailbox_id,
                expected_submissions: vec![HumanSentListItemRecheck {
                    submission_id: newer.submission_id,
                    expected_accepted_at: initial_newer.accepted_at,
                    expected_recipient_count: initial_newer.recipient_count,
                    expected_delivery_snapshot_id: Some(initial_summary.snapshot_id),
                }],
            })
            .await
            .expect("evaluate stale sent-directory snapshot recheck"),
        "a changed latest delivery snapshot must invalidate a pre-disclosure proof"
    );
    drop(snapshot_drift_transaction);

    let refreshed_page = sent_directory
        .list(ListHumanSentSubmissionsCommand {
            context: owner_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            limit: 1,
            cursor: None,
            trace_id: Uuid::now_v7().to_string(),
            now: accepted_at + chrono::Duration::seconds(6),
        })
        .await
        .expect("disclose page after refreshing the delivery snapshot proof");
    assert_eq!(refreshed_page.items[0].submission_id, newer.submission_id);
    assert_eq!(
        refreshed_page.items[0].delivery.as_ref().map(|summary| {
            (
                summary.delivered_count,
                summary.failed_count,
                summary.queued_count,
                summary.unknown_count,
                summary.terminal,
            )
        }),
        Some((2, 0, 0, 0, true))
    );

    let replacement_account_ref = random_numeric_engine_ref();
    MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct sent-directory replacement binding service")
    .bind_authorized(
        &owner_context,
        BindMailEngineMailboxCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            account_ref: replacement_account_ref,
            expected_version: 1,
            trace_id: Uuid::now_v7().to_string(),
            now: accepted_at + chrono::Duration::seconds(7),
        },
    )
    .await
    .expect("replace current data-plane account binding");
    let rebound_page = sent_directory
        .list(ListHumanSentSubmissionsCommand {
            context: owner_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            limit: 50,
            cursor: None,
            trace_id: Uuid::now_v7().to_string(),
            now: accepted_at + chrono::Duration::seconds(8),
        })
        .await
        .expect("retain local sent history after a valid current-binding replacement");
    assert_eq!(
        rebound_page
            .items
            .iter()
            .map(|item| item.submission_id)
            .collect::<Vec<_>>(),
        vec![newer.submission_id, older.submission_id],
        "the content-free history must not depend on the current data-plane binding"
    );

    let empty_evidence_before =
        count_human_sent_directory_evidence(&store, bootstrap.organization_id, mailbox.mailbox_id)
            .await;
    let empty_page = sent_directory
        .list(ListHumanSentSubmissionsCommand {
            context: owner_context,
            mailbox_id: mailbox.mailbox_id,
            limit: 50,
            cursor: Some(format!("v1.{}", older.submission_id)),
            trace_id: Uuid::now_v7().to_string(),
            now: accepted_at + chrono::Duration::seconds(8),
        })
        .await
        .expect("authorize and audit an exact empty terminal sent-directory page");
    assert!(empty_page.items.is_empty());
    assert!(empty_page.next_cursor.is_none());
    assert_eq!(
        count_human_sent_directory_evidence(&store, bootstrap.organization_id, mailbox.mailbox_id,)
            .await,
        (empty_evidence_before.0 + 1, empty_evidence_before.1 + 1),
        "an empty page still receives the same final authorization proof and atomic evidence"
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_drafts_are_private_authorized_binding_safe_and_finally_rechecked() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "human-drafts-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Human drafts tenant");
    let now = Utc::now();
    let first_human_id = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let second_human_id = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let first_context = store
        .resolve_authorization(first_human_id, now)
        .await
        .expect("resolve first Human")
        .expect("first Human is active");
    let second_context = store
        .resolve_authorization(second_human_id, now)
        .await
        .expect("resolve second Human")
        .expect("second Human is active");
    let mailbox = MailboxManagementService::new(store.clone())
        .create_authorized(
            &first_context,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Shared,
                primary_address: format!("human-drafts-{}@example.test", Uuid::new_v4().simple()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create shared Human draft mailbox");
    let old_account_ref = random_numeric_engine_ref();
    let binding_service = MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct Human draft binding service");
    binding_service
        .bind_authorized(
            &first_context,
            BindMailEngineMailboxCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                account_ref: old_account_ref.clone(),
                expected_version: 0,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("bind initial shared mailbox account");

    let delegation_service = MailboxDelegationService::new(store.clone());
    let initial_second_grant = delegation_service
        .grant_authorized(
            &first_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: second_human_id,
                capabilities: [MailboxCapability::Read, MailboxCapability::Send]
                    .into_iter()
                    .collect(),
                reason: "Shared mailbox draft isolation fixture".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::hours(4),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("grant second Human read and send");

    let remote_drafts = Arc::new(IntegrationHumanDraftAdapter::default());
    let draft_service = DraftService::new(store.clone(), remote_drafts);
    let first_active = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec!["active@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Old active draft".into(),
                text_body: "old active body".into(),
                now: now + chrono::Duration::seconds(1),
            },
        )
        .await
        .expect("save first Human active draft");
    let first_frozen = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec!["frozen@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Frozen draft".into(),
                text_body: "frozen body".into(),
                now: now + chrono::Duration::seconds(2),
            },
        )
        .await
        .expect("save draft to freeze");
    let first_sent = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec!["sent@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Sent draft".into(),
                text_body: "sent body".into(),
                now: now + chrono::Duration::seconds(3),
            },
        )
        .await
        .expect("save draft to mark sent");
    let second_private = draft_service
        .save(
            &second_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec!["private@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Second Human private draft".into(),
                text_body: "private second Human body".into(),
                now: now + chrono::Duration::seconds(4),
            },
        )
        .await
        .expect("save second Human private draft");

    let query_for = |principal_id, after_draft_id, observed_at| HumanDraftListQuery {
        organization_id: bootstrap.organization_id,
        principal_id,
        mailbox_id: mailbox.mailbox_id,
        expected_data_plane_id: legacy_mail_data_plane_id(),
        after_draft_id,
        limit: 50,
        now: observed_at,
    };
    let first_page = store
        .list_human_drafts(&query_for(
            first_human_id,
            None,
            now + chrono::Duration::seconds(4),
        ))
        .await
        .expect("list first Human private drafts");
    assert_eq!(first_page.candidates.len(), 3);
    assert!(
        first_page
            .candidates
            .iter()
            .all(|draft| draft.created_by == first_human_id)
    );
    assert!(
        !first_page
            .candidates
            .iter()
            .any(|draft| draft.draft_id == second_private.draft_id)
    );
    let second_page = store
        .list_human_drafts(&query_for(
            second_human_id,
            None,
            now + chrono::Duration::seconds(4),
        ))
        .await
        .expect("list second Human private drafts");
    assert_eq!(
        second_page
            .candidates
            .iter()
            .map(|draft| draft.draft_id)
            .collect::<Vec<_>>(),
        vec![second_private.draft_id]
    );
    assert!(
        store
            .resolve_human_draft_restore_target(&HumanDraftRestoreQuery {
                organization_id: bootstrap.organization_id,
                principal_id: first_human_id,
                mailbox_id: mailbox.mailbox_id,
                draft_id: second_private.draft_id,
                expected_data_plane_id: legacy_mail_data_plane_id(),
                now: now + chrono::Duration::seconds(4),
            })
            .await
            .expect("resolve other creator's draft")
            .target
            .is_none()
    );

    let agent_id = PrincipalId::new();
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        ) VALUES ($1, $2, 'agent', 'active', 'Draft Query Agent', $3, $3)
        "#,
    )
    .bind(agent_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert draft-query Agent");
    let agent_batch = store
        .list_human_drafts(&query_for(
            agent_id,
            None,
            now + chrono::Duration::seconds(4),
        ))
        .await
        .expect("query drafts as Agent");
    assert!(!agent_batch.active_human);
    assert!(agent_batch.candidates.is_empty());

    delegation_service
        .revoke_authorized(
            &first_context,
            RevokeMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                delegation_id: initial_second_grant.delegation_id,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(5),
            },
        )
        .await
        .expect("revoke second Human draft authorization");
    assert!(
        store
            .list_human_drafts(&query_for(
                second_human_id,
                None,
                now + chrono::Duration::seconds(5),
            ))
            .await
            .expect("list after delegation revocation")
            .candidates
            .is_empty()
    );

    let read_only_grant = delegation_service
        .grant_authorized(
            &first_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: second_human_id,
                capabilities: [MailboxCapability::Read].into_iter().collect(),
                reason: "Read-only drafts must remain hidden".into(),
                valid_from: now + chrono::Duration::seconds(6),
                valid_until: now + chrono::Duration::hours(4),
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(6),
            },
        )
        .await
        .expect("grant read-only draft access");
    assert!(
        store
            .list_human_drafts(&query_for(
                second_human_id,
                None,
                now + chrono::Duration::seconds(6),
            ))
            .await
            .expect("list with read-only delegation")
            .candidates
            .is_empty()
    );
    delegation_service
        .revoke_authorized(
            &first_context,
            RevokeMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                delegation_id: read_only_grant.delegation_id,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(7),
            },
        )
        .await
        .expect("revoke read-only draft access");
    let send_as_grant = delegation_service
        .grant_authorized(
            &first_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: second_human_id,
                capabilities: [MailboxCapability::Read, MailboxCapability::SendAs]
                    .into_iter()
                    .collect(),
                reason: "Send-as is not ordinary draft send authority".into(),
                valid_from: now + chrono::Duration::seconds(8),
                valid_until: now + chrono::Duration::hours(4),
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(8),
            },
        )
        .await
        .expect("grant send-as-only draft access");
    assert!(
        store
            .list_human_drafts(&query_for(
                second_human_id,
                None,
                now + chrono::Duration::seconds(8),
            ))
            .await
            .expect("list with send-as-only delegation")
            .candidates
            .is_empty()
    );
    delegation_service
        .revoke_authorized(
            &first_context,
            RevokeMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                delegation_id: send_as_grant.delegation_id,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(9),
            },
        )
        .await
        .expect("revoke send-as-only draft access");
    let restored_second_grant = delegation_service
        .grant_authorized(
            &first_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: second_human_id,
                capabilities: [MailboxCapability::Read, MailboxCapability::Send]
                    .into_iter()
                    .collect(),
                reason: "Restore ordinary shared-mailbox draft access".into(),
                valid_from: now + chrono::Duration::seconds(10),
                valid_until: now + chrono::Duration::hours(4),
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(10),
            },
        )
        .await
        .expect("restore read and send draft access");

    sqlx::query(
        r#"
        UPDATE mail_drafts
           SET status = 'frozen', version = version + 1, updated_at = $2
         WHERE id = $1
        "#,
    )
    .bind(first_frozen.draft_id.as_uuid())
    .bind(now + chrono::Duration::seconds(11))
    .execute(store.pool())
    .await
    .expect("freeze draft fixture");
    sqlx::query(
        r#"
        UPDATE mail_drafts
           SET status = 'frozen', version = version + 1, updated_at = $2
         WHERE id = $1
        "#,
    )
    .bind(first_sent.draft_id.as_uuid())
    .bind(now + chrono::Duration::seconds(11))
    .execute(store.pool())
    .await
    .expect("freeze sent-draft fixture");
    sqlx::query(
        r#"
        UPDATE mail_drafts
           SET status = 'sent', version = version + 1, updated_at = $2
         WHERE id = $1
        "#,
    )
    .bind(first_sent.draft_id.as_uuid())
    .bind(now + chrono::Duration::seconds(12))
    .execute(store.pool())
    .await
    .expect("mark draft fixture sent");
    let lifecycle_page = store
        .list_human_drafts(&query_for(
            first_human_id,
            None,
            now + chrono::Duration::seconds(12),
        ))
        .await
        .expect("list after lifecycle transitions");
    assert_eq!(
        lifecycle_page
            .candidates
            .iter()
            .map(|draft| draft.draft_id)
            .collect::<Vec<_>>(),
        vec![first_active.draft_id]
    );
    for hidden_draft_id in [first_frozen.draft_id, first_sent.draft_id] {
        assert!(
            store
                .resolve_human_draft_restore_target(&HumanDraftRestoreQuery {
                    organization_id: bootstrap.organization_id,
                    principal_id: first_human_id,
                    mailbox_id: mailbox.mailbox_id,
                    draft_id: hidden_draft_id,
                    expected_data_plane_id: legacy_mail_data_plane_id(),
                    now: now + chrono::Duration::seconds(12),
                })
                .await
                .expect("resolve terminal draft")
                .target
                .is_none()
        );
    }

    let current_account_ref = random_numeric_engine_ref();
    binding_service
        .bind_authorized(
            &first_context,
            BindMailEngineMailboxCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                account_ref: current_account_ref.clone(),
                expected_version: 1,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(13),
            },
        )
        .await
        .expect("replace shared mailbox binding");
    let stale_binding_page = store
        .list_human_drafts(&query_for(
            first_human_id,
            None,
            now + chrono::Duration::seconds(13),
        ))
        .await
        .expect("list after binding replacement");
    assert!(stale_binding_page.candidates.is_empty());
    let stale_cursor = store
        .list_human_drafts(&query_for(
            first_human_id,
            Some(first_active.draft_id),
            now + chrono::Duration::seconds(13),
        ))
        .await
        .expect("validate stale-binding cursor");
    assert!(!stale_cursor.cursor_valid);
    assert!(
        store
            .resolve_human_draft_restore_target(&HumanDraftRestoreQuery {
                organization_id: bootstrap.organization_id,
                principal_id: first_human_id,
                mailbox_id: mailbox.mailbox_id,
                draft_id: first_active.draft_id,
                expected_data_plane_id: legacy_mail_data_plane_id(),
                now: now + chrono::Duration::seconds(13),
            })
            .await
            .expect("resolve old-binding draft")
            .target
            .is_none()
    );

    let recoverable_content = MailDraftContent {
        to: vec!["one@example.test".into()],
        cc: vec!["copy@example.test".into()],
        bcc: vec!["hidden@example.test".into()],
        subject: "Recover exact draft".into(),
        text_body: "recoverable exact body".into(),
    };
    let recoverable = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: recoverable_content.to.clone(),
                cc: recoverable_content.cc.clone(),
                bcc: recoverable_content.bcc.clone(),
                subject: recoverable_content.subject.clone(),
                text_body: recoverable_content.text_body.clone(),
                now: now + chrono::Duration::seconds(14),
            },
        )
        .await
        .expect("save recoverable current-binding draft");
    let other_current = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec!["two@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Second current draft".into(),
                text_body: "second current body".into(),
                now: now + chrono::Duration::seconds(15),
            },
        )
        .await
        .expect("save second current-binding draft");
    let recoverable_record = store
        .get_mail_draft(
            bootstrap.organization_id,
            mailbox.mailbox_id,
            first_human_id,
            recoverable.draft_id,
            legacy_mail_data_plane_id(),
        )
        .await
        .expect("load recoverable draft evidence")
        .expect("recoverable draft exists");
    let ordinary_reader = || IntegrationHumanDraftReader {
        expected_account_ref: current_account_ref.clone(),
        expected_email_ref: recoverable_record.engine_email_ref.clone(),
        expected_thread_ref: recoverable_record.engine_thread_ref.clone(),
        expected_from: MailboxAddress::parse(&mailbox.primary_address)
            .expect("created mailbox address remains valid"),
        content: recoverable_content.clone(),
        suspend_after_read: None,
    };
    let human_drafts = HumanDraftService::new(store.clone(), Arc::new(ordinary_reader()));
    let first_visible = human_drafts
        .list(ListHumanDraftsCommand {
            context: first_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            limit: 1,
            cursor: None,
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(15),
        })
        .await
        .expect("list first current draft page");
    assert_eq!(first_visible.items.len(), 1);
    let second_visible = human_drafts
        .list(ListHumanDraftsCommand {
            context: first_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            limit: 1,
            cursor: first_visible.next_cursor.clone(),
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(15),
        })
        .await
        .expect("list second current draft page");
    assert_eq!(second_visible.items.len(), 1);
    assert_ne!(
        first_visible.items[0].draft_id,
        second_visible.items[0].draft_id
    );
    assert!(
        [recoverable.draft_id, other_current.draft_id]
            .into_iter()
            .all(|draft_id| first_visible.items[0].draft_id == draft_id
                || second_visible.items[0].draft_id == draft_id)
    );
    assert!(matches!(
        human_drafts
            .list(ListHumanDraftsCommand {
                context: first_context.clone(),
                mailbox_id: mailbox.mailbox_id,
                limit: 25,
                cursor: Some(format!("v1.{}", first_active.draft_id)),
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(15),
            })
            .await,
        Err(HumanDraftError::InvalidCursor)
    ));

    let list_evidence_before_drift: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action = 'mail.drafts.queried'
              AND resource_id = $2),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail.drafts.queried.v1'
              AND aggregate_id = $2
              AND payload ->> 'organization_id' = $1::text)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(mailbox.mailbox_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count Human draft-list evidence before final-recheck drift");
    let list_recheck = |expected_drafts| HumanDraftListRecheck {
        organization_id: bootstrap.organization_id,
        principal_id: first_human_id,
        mailbox_id: mailbox.mailbox_id,
        expected_data_plane_id: legacy_mail_data_plane_id(),
        expected_engine_account_ref: current_account_ref.clone(),
        expected_drafts,
    };

    let mut proof_boundary_transaction = store
        .begin()
        .await
        .expect("begin Human draft-list proof-boundary transaction");
    assert!(
        proof_boundary_transaction
            .human_draft_list_still_authorized(&list_recheck(Vec::new()))
            .await
            .expect("an empty page remains a valid exact proof")
    );
    let duplicate_draft = HumanDraftListItemRecheck {
        draft_id: recoverable.draft_id,
        expected_version: recoverable.version,
    };
    assert!(
        !proof_boundary_transaction
            .human_draft_list_still_authorized(&list_recheck(vec![
                duplicate_draft,
                duplicate_draft,
            ]))
            .await
            .expect("reject duplicate Human draft-list proof")
    );
    assert!(
        !proof_boundary_transaction
            .human_draft_list_still_authorized(&list_recheck(vec![HumanDraftListItemRecheck {
                expected_version: 0,
                ..duplicate_draft
            },]))
            .await
            .expect("reject non-positive Human draft-list version")
    );
    assert!(
        !proof_boundary_transaction
            .human_draft_list_still_authorized(&list_recheck(
                (0..51)
                    .map(|_| HumanDraftListItemRecheck {
                        draft_id: DraftId::new(),
                        expected_version: 1,
                    })
                    .collect(),
            ))
            .await
            .expect("reject an over-limit Human draft-list proof")
    );
    drop(proof_boundary_transaction);

    let version_drift = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec!["version-drift@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Version drift before final disclosure".into(),
                text_body: "version one".into(),
                now: now + chrono::Duration::seconds(16),
            },
        )
        .await
        .expect("save draft for list-version drift");
    let version_snapshot = store
        .list_human_drafts(&query_for(
            first_human_id,
            None,
            now + chrono::Duration::seconds(17),
        ))
        .await
        .expect("read draft-list version snapshot")
        .candidates
        .into_iter()
        .find(|candidate| candidate.draft_id == version_drift.draft_id)
        .expect("version-drift draft appears in the initial page");
    let replaced_version = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: Some(version_drift.draft_id),
                expected_version: Some(version_snapshot.version),
                reply_to_message_id: None,
                to: vec!["version-drift@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Version drift before final disclosure".into(),
                text_body: "version two".into(),
                now: now + chrono::Duration::seconds(18),
            },
        )
        .await
        .expect("replace draft after initial list snapshot");
    assert_eq!(replaced_version.version, version_snapshot.version + 1);
    let mut version_drift_transaction = store
        .begin()
        .await
        .expect("begin list-version final recheck");
    assert!(
        !version_drift_transaction
            .human_draft_list_still_authorized(&list_recheck(vec![HumanDraftListItemRecheck {
                draft_id: version_snapshot.draft_id,
                expected_version: version_snapshot.version,
            },]))
            .await
            .expect("recheck exact disclosed draft version")
    );
    drop(version_drift_transaction);

    let status_drift = draft_service
        .save(
            &first_context,
            SaveTextDraftCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: None,
                expected_version: None,
                reply_to_message_id: None,
                to: vec!["status-drift@example.test".into()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: "Status drift before final disclosure".into(),
                text_body: "active before snapshot".into(),
                now: now + chrono::Duration::seconds(19),
            },
        )
        .await
        .expect("save draft for list-status drift");
    let status_snapshot = store
        .list_human_drafts(&query_for(
            first_human_id,
            None,
            now + chrono::Duration::seconds(20),
        ))
        .await
        .expect("read draft-list status snapshot")
        .candidates
        .into_iter()
        .find(|candidate| candidate.draft_id == status_drift.draft_id)
        .expect("status-drift draft appears in the initial page");
    sqlx::query(
        r#"
        UPDATE mail_drafts
           SET status = 'frozen', version = version + 1, updated_at = $2
         WHERE id = $1
        "#,
    )
    .bind(status_snapshot.draft_id.as_uuid())
    .bind(now + chrono::Duration::seconds(21))
    .execute(store.pool())
    .await
    .expect("freeze draft after initial list snapshot");
    let mut status_drift_transaction = store
        .begin()
        .await
        .expect("begin list-status final recheck");
    assert!(
        !status_drift_transaction
            .human_draft_list_still_authorized(&list_recheck(vec![HumanDraftListItemRecheck {
                draft_id: status_snapshot.draft_id,
                expected_version: status_snapshot.version,
            },]))
            .await
            .expect("recheck exact disclosed draft lifecycle")
    );
    drop(status_drift_transaction);
    let list_evidence_after_drift: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action = 'mail.drafts.queried'
              AND resource_id = $2),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail.drafts.queried.v1'
              AND aggregate_id = $2
              AND payload ->> 'organization_id' = $1::text)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(mailbox.mailbox_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count Human draft-list evidence after final-recheck drift");
    assert_eq!(list_evidence_after_drift, list_evidence_before_drift);

    delegation_service
        .revoke_authorized(
            &first_context,
            RevokeMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                delegation_id: restored_second_grant.delegation_id,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
            },
        )
        .await
        .expect("revoke long-lived delegation before expiry recheck");
    let expiry_grant_now = Utc::now();
    let expires_at = expiry_grant_now + chrono::Duration::seconds(2);
    delegation_service
        .grant_authorized(
            &first_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: second_human_id,
                capabilities: [MailboxCapability::Read, MailboxCapability::Send]
                    .into_iter()
                    .collect(),
                reason: "Final draft disclosure must use the database clock".into(),
                valid_from: expiry_grant_now,
                valid_until: expires_at,
                trace_id: Uuid::now_v7().to_string(),
                now: expiry_grant_now,
            },
        )
        .await
        .expect("grant short-lived draft authorization");
    tokio::time::sleep(std::time::Duration::from_millis(2_100)).await;
    let mut expiry_transaction = store.begin().await.expect("begin final expiry recheck");
    assert!(
        !expiry_transaction
            .human_draft_list_still_authorized(&HumanDraftListRecheck {
                organization_id: bootstrap.organization_id,
                principal_id: second_human_id,
                mailbox_id: mailbox.mailbox_id,
                expected_data_plane_id: legacy_mail_data_plane_id(),
                expected_engine_account_ref: current_account_ref.clone(),
                expected_drafts: Vec::new(),
            })
            .await
            .expect("recheck naturally expired draft authorization"),
        "an old request timestamp must not extend a naturally expired grant"
    );

    let drift_reader = IntegrationHumanDraftReader {
        suspend_after_read: Some((
            store.pool().clone(),
            bootstrap.organization_id,
            first_human_id,
        )),
        ..ordinary_reader()
    };
    let drift_result = HumanDraftService::new(store.clone(), Arc::new(drift_reader))
        .restore(RestoreHumanDraftCommand {
            context: first_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            draft_id: recoverable.draft_id,
            expected_version: recoverable.version,
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(15),
        })
        .await;
    assert_eq!(drift_result, Err(HumanDraftError::Conflict));
    let denied_evidence: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_events
            WHERE action = 'mail.draft.read' AND resource_id = $1),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail.draft.read.v1' AND aggregate_id = $1)
        "#,
    )
    .bind(recoverable.draft_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query denied draft-read evidence");
    assert_eq!(denied_evidence, (0, 0));

    sqlx::query(
        r#"
        UPDATE principals
           SET status = 'active', updated_at = clock_timestamp()
         WHERE id = $1 AND organization_id = $2
        "#,
    )
    .bind(first_human_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .execute(store.pool())
    .await
    .expect("reactivate Human after drift fixture");
    let restored = HumanDraftService::new(store.clone(), Arc::new(ordinary_reader()))
        .restore(RestoreHumanDraftCommand {
            context: first_context,
            mailbox_id: mailbox.mailbox_id,
            draft_id: recoverable.draft_id,
            expected_version: recoverable.version,
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(16),
        })
        .await
        .expect("restore exact current Human draft");
    assert_eq!(restored.to, recoverable_content.to);
    assert_eq!(restored.cc, recoverable_content.cc);
    assert_eq!(restored.bcc, recoverable_content.bcc);
    assert_eq!(restored.subject, recoverable_content.subject);
    assert_eq!(restored.text_body, recoverable_content.text_body);
    let successful_evidence: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_events
            WHERE action = 'mail.draft.read' AND resource_id = $1),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail.draft.read.v1' AND aggregate_id = $1)
        "#,
    )
    .bind(recoverable.draft_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query successful draft-read evidence");
    assert_eq!(successful_evidence, (1, 1));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn approval_workflow_enforces_dual_control_scope_and_expiry() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "approval-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap approval tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Approval Requester".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind approval requester");
    let requester_context = store
        .resolve_authorization(owner.principal_id, now)
        .await
        .unwrap()
        .expect("resolve requester authorization");
    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "mail.sendas.external".into(),
            display_name: "External Send As".into(),
            action: ApprovalAction::SendAs,
            required_approvals: 2,
            rule: json!({"recipient_scope": "external"}),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create approval policy");
    let workflow = ApprovalWorkflowService::new(store.clone());
    let request_id = workflow
        .request(RequestApprovalCommand {
            context: requester_context.clone(),
            policy_key: "mail.sendas.external".into(),
            action: ApprovalAction::SendAs,
            resource_kind: "mailbox".into(),
            resource_id: "mailbox-a".into(),
            reason: "Send a reviewed customer notice".into(),
            ticket_reference: Some("SEC-123".into()),
            expires_at: now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("request dual approval");
    let requested_page = workflow
        .list(ListApprovalRequestsCommand {
            context: requester_context.clone(),
            view: ApprovalRequestView::Requested,
            status: Some(ApprovalStatus::Pending),
            after_request_id: None,
            limit: 50,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("list requester-owned approval");
    assert_eq!(requested_page.items.len(), 1);
    assert_eq!(requested_page.items[0].request_id, request_id);
    let detail = workflow
        .get(GetApprovalRequestCommand {
            context: requester_context.clone(),
            request_id,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("read requester-owned approval");
    assert!(detail.steps.is_empty());
    assert!(
        workflow
            .decide(DecideApprovalCommand {
                context: requester_context.clone(),
                request_id,
                decision: ApprovalDecision::Approved,
                reason: "self approval must fail".into(),
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .is_err()
    );

    let first_id =
        insert_approval_test_owner(&store, bootstrap.organization_id, owner.principal_id, now)
            .await;
    let second_id =
        insert_approval_test_owner(&store, bootstrap.organization_id, owner.principal_id, now)
            .await;
    let first_context = store
        .resolve_authorization(first_id, now)
        .await
        .unwrap()
        .expect("resolve first approver");
    let second_context = store
        .resolve_authorization(second_id, now)
        .await
        .unwrap()
        .expect("resolve second approver");
    let actionable = workflow
        .list(ListApprovalRequestsCommand {
            context: first_context.clone(),
            view: ApprovalRequestView::Actionable,
            status: Some(ApprovalStatus::Pending),
            after_request_id: None,
            limit: 50,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("list independent approver inbox");
    assert_eq!(actionable.items.len(), 1);
    assert_eq!(actionable.items[0].request_id, request_id);
    let first = workflow
        .decide(DecideApprovalCommand {
            context: first_context.clone(),
            request_id,
            decision: ApprovalDecision::Approved,
            reason: "security review complete".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("first independent approval");
    assert_eq!(first.status, ApprovalStatus::Pending);
    assert!(
        workflow
            .list(ListApprovalRequestsCommand {
                context: first_context,
                view: ApprovalRequestView::Actionable,
                status: Some(ApprovalStatus::Pending),
                after_request_id: None,
                limit: 50,
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .expect("reload first approver inbox")
            .items
            .is_empty()
    );
    let proof = ApprovalProof {
        organization_id: bootstrap.organization_id,
        request_id,
        requested_by: owner.principal_id,
        action: ApprovalAction::SendAs,
        resource_kind: "mailbox".into(),
        resource_id: "mailbox-a".into(),
        now,
    };
    assert!(!store.approval_permits(&proof).await.unwrap());
    let second = workflow
        .decide(DecideApprovalCommand {
            context: second_context,
            request_id,
            decision: ApprovalDecision::Approved,
            reason: "independent compliance review complete".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("second independent approval");
    assert_eq!(second.status, ApprovalStatus::Approved);
    assert!(
        ApprovalGateService::new(store.clone())
            .permits(true, &proof)
            .await
            .unwrap()
    );
    assert!(
        !store
            .approval_permits(&ApprovalProof {
                requested_by: first_id,
                ..proof.clone()
            })
            .await
            .unwrap()
    );
    assert!(
        !store
            .approval_permits(&ApprovalProof {
                resource_id: "mailbox-b".into(),
                ..proof.clone()
            })
            .await
            .unwrap()
    );
    // Persistence uses its own clock: a fabricated caller timestamp must not
    // expire a live approval (or revive an expired one, checked below).
    assert!(
        store
            .approval_permits(&ApprovalProof {
                now: now + chrono::Duration::hours(2),
                ..proof
            })
            .await
            .unwrap()
    );

    let cancellation_id = workflow
        .request(RequestApprovalCommand {
            context: requester_context.clone(),
            policy_key: "mail.sendas.external".into(),
            action: ApprovalAction::SendAs,
            resource_kind: "mailbox".into(),
            resource_id: "mailbox-cancelled".into(),
            reason: "Withdraw this draft approval".into(),
            ticket_reference: Some("SEC-124".into()),
            expires_at: now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("request approval for cancellation");
    let cancelled = workflow
        .cancel(CancelApprovalRequestCommand {
            context: requester_context.clone(),
            request_id: cancellation_id,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("cancel own pending approval");
    assert_eq!(cancelled.status, ApprovalStatus::Cancelled);
    let cancelled_detail = store
        .get_approval_request(bootstrap.organization_id, cancellation_id, now)
        .await
        .expect("query cancelled approval")
        .expect("cancelled approval exists");
    assert_eq!(cancelled_detail.request.status, ApprovalStatus::Cancelled);
    assert!(cancelled_detail.request.cancelled_at.is_some());

    let evidence: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM approval_steps WHERE approval_request_id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2
              AND action = 'authorization.approval.decided'),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $3
              AND event_type = 'authorization.approval.decided.v1'),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2
              AND action = 'authorization.approval.cancelled'),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2
              AND action = 'authorization.approval.queried')
        "#,
    )
    .bind(request_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(request_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query approval evidence");
    assert_eq!(evidence, (2, 2, 2, 1, 4));

    let clock_start = Utc::now();
    let expiring = workflow
        .request(RequestApprovalCommand {
            context: requester_context.clone(),
            policy_key: "mail.sendas.external".into(),
            action: ApprovalAction::SendAs,
            resource_kind: "mailbox".into(),
            resource_id: "expiry-after-row-lock".into(),
            reason: "Check the post-lock clock".into(),
            ticket_reference: None,
            expires_at: clock_start + chrono::Duration::seconds(2),
            trace_id: Uuid::new_v4().to_string(),
            now: clock_start,
        })
        .await
        .unwrap();
    let mut blocker = store.pool().begin().await.unwrap();
    let expires_at: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT expires_at FROM approval_requests WHERE id = $1 FOR UPDATE")
            .bind(expiring.as_uuid())
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
    let mut decision_transaction = store.begin().await.unwrap();
    let decision = copilotmail_application::ApprovalDecisionWrite {
        organization_id: bootstrap.organization_id,
        request_id: expiring,
        step_id: copilotmail_domain::ApprovalStepId::new(),
        approver: first_id,
        decision: ApprovalDecision::Approved,
        reason: "Wait across expiry".into(),
        decided_at: clock_start,
    };
    let mut waiting = Box::pin(decision_transaction.record_approval_decision(&decision));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(25), &mut waiting)
            .await
            .is_err()
    );
    sqlx::query("SELECT pg_sleep(GREATEST(EXTRACT(EPOCH FROM ($1::timestamptz - clock_timestamp())), 0) + 0.02)")
        .bind(expires_at).execute(&mut *blocker).await.unwrap();
    blocker.commit().await.unwrap();
    assert!(matches!(
        expect_no_lock_hang(waiting).await,
        Err(copilotmail_application::StoreError::Conflict)
    ));
    drop(decision_transaction);
    assert!(matches!(
        workflow
            .cancel(CancelApprovalRequestCommand {
                context: requester_context,
                request_id: expiring,
                trace_id: Uuid::new_v4().to_string(),
                now: clock_start,
            })
            .await,
        Err(copilotmail_application::ApprovalWorkflowError::Store(
            copilotmail_application::StoreError::Conflict
        ))
    ));
    let denied_rows: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM approval_steps WHERE approval_request_id = $1), (SELECT count(*) FROM audit_events WHERE organization_id = $2 AND resource_id = $3 AND action IN ('authorization.approval.decided', 'authorization.approval.cancelled'))",
    ).bind(expiring.as_uuid()).bind(bootstrap.organization_id.as_uuid()).bind(expiring.to_string())
        .fetch_one(store.pool()).await.unwrap();
    assert_eq!(denied_rows, (0, 0));

    let grant_now = chrono::DateTime::from_timestamp_millis(
        (now + chrono::Duration::seconds(1)).timestamp_millis(),
    )
    .expect("millisecond-aligned grant time");
    let valid_until = grant_now + chrono::Duration::days(30);
    let employee_role = store
        .list_roles(bootstrap.organization_id)
        .await
        .expect("list built-in roles")
        .into_iter()
        .find(|role| role.system_role == SystemRole::Employee)
        .expect("employee role exists");
    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "authorization.role.employee".into(),
            display_name: "Employee role elevation".into(),
            action: ApprovalAction::PrivilegeElevation,
            required_approvals: 2,
            rule: json!({"maximum_days": 365}),
            trace_id: Uuid::now_v7().to_string(),
            now: grant_now,
        })
        .await
        .expect("create role elevation policy");
    let grant_resource_id =
        role_assignment_approval_resource_id(first_id, employee_role.role_id, valid_until)
            .expect("build exact role approval resource");
    let requester_context = store
        .resolve_authorization(owner.principal_id, grant_now)
        .await
        .unwrap()
        .expect("resolve role grant requester");
    let role_approval_id = workflow
        .request(RequestApprovalCommand {
            context: requester_context.clone(),
            policy_key: "authorization.role.employee".into(),
            action: ApprovalAction::PrivilegeElevation,
            resource_kind: "authorization.role_assignment".into(),
            resource_id: grant_resource_id.clone(),
            reason: "Time-bound employee mailbox access".into(),
            ticket_reference: Some("IAM-100".into()),
            expires_at: grant_now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now: grant_now,
        })
        .await
        .expect("request exact role elevation");
    for (approver, reason) in [
        (first_id, "security approval"),
        (second_id, "independent IAM approval"),
    ] {
        let approver_context = store
            .resolve_authorization(approver, grant_now)
            .await
            .unwrap()
            .expect("resolve role approver");
        workflow
            .decide(DecideApprovalCommand {
                context: approver_context,
                request_id: role_approval_id,
                decision: ApprovalDecision::Approved,
                reason: reason.into(),
                trace_id: Uuid::now_v7().to_string(),
                now: grant_now,
            })
            .await
            .expect("approve role elevation");
    }
    let role_governance = RoleGovernanceService::new(store.clone());
    let grant = role_governance
        .grant(GrantRoleAssignmentCommand {
            context: requester_context.clone(),
            principal_id: first_id,
            role_id: employee_role.role_id,
            valid_until,
            approval: ApprovalProof {
                organization_id: bootstrap.organization_id,
                request_id: role_approval_id,
                requested_by: owner.principal_id,
                action: ApprovalAction::PrivilegeElevation,
                resource_kind: "authorization.role_assignment".into(),
                resource_id: grant_resource_id,
                now: grant_now,
            },
            trace_id: Uuid::now_v7().to_string(),
            now: grant_now,
        })
        .await
        .expect("grant approved bounded role");
    let assignments = role_governance
        .list_assignments(ListRoleAssignmentsCommand {
            context: requester_context.clone(),
            principal_id: None,
            include_inactive: false,
            after_assignment_id: None,
            limit: 100,
            trace_id: Uuid::now_v7().to_string(),
            now: grant_now,
        })
        .await
        .expect("list active role assignments");
    assert!(assignments.items.iter().any(|assignment| {
        assignment.assignment_id == grant.assignment_id
            && assignment.approval_request_id == Some(role_approval_id)
            && assignment.is_active_at(grant_now)
    }));
    role_governance
        .revoke(RevokeRoleAssignmentCommand {
            context: requester_context.clone(),
            assignment_id: grant.assignment_id,
            trace_id: Uuid::now_v7().to_string(),
            now: grant_now,
        })
        .await
        .expect("revoke employee role");

    let owner_assignments = assignments
        .items
        .into_iter()
        .filter(|assignment| assignment.system_role == SystemRole::OrgOwner)
        .collect::<Vec<_>>();
    for principal_id in [first_id, second_id] {
        let assignment = owner_assignments
            .iter()
            .find(|assignment| assignment.principal_id == principal_id)
            .expect("approver owner assignment");
        role_governance
            .revoke(RevokeRoleAssignmentCommand {
                context: requester_context.clone(),
                assignment_id: assignment.assignment_id,
                trace_id: Uuid::now_v7().to_string(),
                now: grant_now,
            })
            .await
            .expect("revoke redundant owner");
    }
    let last_owner_assignment = owner_assignments
        .iter()
        .find(|assignment| assignment.principal_id == owner.principal_id)
        .expect("requester owner assignment");
    assert!(matches!(
        role_governance
            .revoke(RevokeRoleAssignmentCommand {
                context: requester_context,
                assignment_id: last_owner_assignment.assignment_id,
                trace_id: Uuid::now_v7().to_string(),
                now: grant_now,
            })
            .await,
        Err(RoleGovernanceError::LastActiveOwner)
    ));
    assert_append_only_error(
        sqlx::query("DELETE FROM role_assignment_grants WHERE assignment_id = $1")
            .bind(grant.assignment_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM approval_requests WHERE id = $1")
            .bind(cancellation_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE approval_steps")
            .execute(store.pool())
            .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_mail_uuid_projection_is_current_binding_scoped_and_tenant_private() {
    let store = migrated_store().await;
    let now = Utc::now();
    let account_a = random_numeric_engine_ref();
    let account_b = random_numeric_engine_ref();
    let (organization_a, human_a, mailbox_a) =
        create_bound_human_mailbox_fixture(&store, "human-projection-a", &account_a, now).await;
    let (organization_b, human_b, mailbox_b) =
        create_bound_human_mailbox_fixture(&store, "human-projection-b", &account_b, now).await;

    let document_a = random_numeric_engine_ref();
    let document_b = random_numeric_engine_ref();
    let message_a = Uuid::now_v7();
    let message_b = Uuid::now_v7();
    let other_tenant_message = Uuid::now_v7();
    insert_human_mail_projection_event(
        &store,
        organization_a,
        mailbox_a,
        &account_a,
        &document_a,
        message_a,
        now,
    )
    .await;
    insert_human_mail_projection_event(
        &store,
        organization_a,
        mailbox_a,
        &account_a,
        &document_b,
        message_b,
        now,
    )
    .await;
    insert_human_mail_projection_event(
        &store,
        organization_b,
        mailbox_b,
        &account_b,
        &document_a,
        other_tenant_message,
        now,
    )
    .await;

    let requested = vec![document_b.clone(), document_a.clone()];
    let projected = store
        .resolve_human_mail_message_projections(
            organization_a,
            mailbox_a,
            legacy_mail_data_plane_id(),
            &account_a,
            &requested,
        )
        .await
        .expect("project exact Human message page");
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].engine_document_ref, document_b);
    assert_eq!(projected[0].message_id, message_b);
    assert_eq!(projected[1].engine_document_ref, document_a);
    assert_eq!(projected[1].message_id, message_a);

    let duplicate_requested = vec![document_a.clone(), document_a.clone()];
    let duplicate_projection = store
        .resolve_human_mail_message_projections(
            organization_a,
            mailbox_a,
            legacy_mail_data_plane_id(),
            &account_a,
            &duplicate_requested,
        )
        .await
        .expect("preserve duplicate projection for application rejection");
    assert_eq!(duplicate_projection.len(), 2);
    assert!(
        duplicate_projection
            .iter()
            .all(|projection| projection.message_id == message_a
                && projection.engine_document_ref == document_a)
    );

    let unknown_document = random_numeric_engine_ref();
    let incomplete_requested = vec![document_a.clone(), unknown_document];
    let incomplete_projection = store
        .resolve_human_mail_message_projections(
            organization_a,
            mailbox_a,
            legacy_mail_data_plane_id(),
            &account_a,
            &incomplete_requested,
        )
        .await
        .expect("return only existing projection for application rejection");
    assert_eq!(incomplete_projection.len(), 1);
    assert_eq!(incomplete_projection[0].message_id, message_a);

    let target = store
        .resolve_human_mail_content_target(
            organization_a,
            mailbox_a,
            message_a,
            legacy_mail_data_plane_id(),
        )
        .await
        .expect("resolve exact Human content target")
        .expect("Human content target exists");
    assert_eq!(target.organization_id, organization_a);
    assert_eq!(target.mailbox_id, mailbox_a);
    assert_eq!(target.message_id, message_a);
    assert_eq!(target.data_plane_id, legacy_mail_data_plane_id());
    assert_eq!(target.engine_account_ref, account_a);
    assert_eq!(target.engine_document_ref, document_a);
    assert!(
        store
            .resolve_human_mail_content_target(
                organization_a,
                mailbox_a,
                message_a,
                other_mail_data_plane_id(),
            )
            .await
            .expect("wrong-plane safe UUID lookup")
            .is_none()
    );

    assert!(
        store
            .resolve_human_mail_message_projections(
                organization_b,
                mailbox_b,
                legacy_mail_data_plane_id(),
                &account_a,
                std::slice::from_ref(&document_a),
            )
            .await
            .expect("cross-tenant batch lookup")
            .is_empty()
    );
    assert!(
        store
            .resolve_human_mail_content_target(
                organization_b,
                mailbox_b,
                message_a,
                legacy_mail_data_plane_id()
            )
            .await
            .expect("cross-tenant safe UUID lookup")
            .is_none()
    );
    assert!(
        store
            .resolve_human_mail_content_target(
                organization_a,
                mailbox_a,
                other_tenant_message,
                legacy_mail_data_plane_id()
            )
            .await
            .expect("other-tenant safe UUID lookup")
            .is_none()
    );
    assert!(
        store
            .resolve_human_mail_content_target(
                organization_a,
                mailbox_a,
                Uuid::now_v7(),
                legacy_mail_data_plane_id()
            )
            .await
            .expect("unknown safe UUID lookup")
            .is_none()
    );

    let context_a = store
        .resolve_authorization(human_a, now)
        .await
        .expect("resolve Human owner for binding rotation")
        .expect("Human owner remains active");
    let replacement_account = random_numeric_engine_ref();
    MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct legacy binding service")
    .bind_authorized(
        &context_a,
        BindMailEngineMailboxCommand {
            organization_id: organization_a,
            mailbox_id: mailbox_a,
            account_ref: replacement_account.clone(),
            expected_version: 1,
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::milliseconds(1),
        },
    )
    .await
    .expect("rotate current Human mailbox binding");
    let retained_owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM mail_engine_account_owners
          WHERE mail_data_plane_id = $1
            AND organization_id = $2
            AND mailbox_id = $3
            AND account_ref IN ($4, $5)",
    )
    .bind(legacy_mail_data_plane_id().as_uuid())
    .bind(organization_a.as_uuid())
    .bind(mailbox_a.as_uuid())
    .bind(&account_a)
    .bind(&replacement_account)
    .fetch_one(store.pool())
    .await
    .expect("count permanent owners after binding rotation");
    assert_eq!(retained_owner_count, 2);

    let backwards_account = random_numeric_engine_ref();
    assert!(matches!(
        MailEngineMailboxBindingService::try_new(
            store.clone(),
            legacy_mail_data_plane_id(),
            "stalwart"
        )
        .expect("construct legacy binding service")
        .bind_authorized(
            &context_a,
            BindMailEngineMailboxCommand {
                organization_id: organization_a,
                mailbox_id: mailbox_a,
                account_ref: backwards_account.clone(),
                expected_version: 2,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await,
        Err(copilotmail_application::MailEventIngestionError::Store(
            copilotmail_application::StoreError::Conflict
        ))
    ));
    let backwards_owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM mail_engine_account_owners
          WHERE mail_data_plane_id = $1
            AND account_ref = $2",
    )
    .bind(legacy_mail_data_plane_id().as_uuid())
    .bind(&backwards_account)
    .fetch_one(store.pool())
    .await
    .expect("count owners after rejected backwards-time binding");
    assert_eq!(backwards_owner_count, 0);

    let duplicate_create_account = random_numeric_engine_ref();
    assert!(matches!(
        MailEngineMailboxBindingService::try_new(
            store.clone(),
            legacy_mail_data_plane_id(),
            "stalwart"
        )
        .expect("construct legacy binding service")
        .bind_authorized(
            &context_a,
            BindMailEngineMailboxCommand {
                organization_id: organization_a,
                mailbox_id: mailbox_a,
                account_ref: duplicate_create_account.clone(),
                expected_version: 0,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::milliseconds(2),
            },
        )
        .await,
        Err(copilotmail_application::MailEventIngestionError::Store(
            copilotmail_application::StoreError::Conflict
        ))
    ));
    let duplicate_create_owner_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM mail_engine_account_owners
          WHERE mail_data_plane_id = $1
            AND account_ref = $2",
    )
    .bind(legacy_mail_data_plane_id().as_uuid())
    .bind(&duplicate_create_account)
    .fetch_one(store.pool())
    .await
    .expect("count owners after rejected duplicate create");
    assert_eq!(duplicate_create_owner_count, 0);
    assert!(
        store
            .resolve_human_mail_message_projections(
                organization_a,
                mailbox_a,
                legacy_mail_data_plane_id(),
                &account_a,
                &[document_a],
            )
            .await
            .expect("stale binding batch lookup")
            .is_empty()
    );
    assert!(
        store
            .resolve_human_mail_content_target(
                organization_a,
                mailbox_a,
                message_a,
                legacy_mail_data_plane_id()
            )
            .await
            .expect("stale binding safe UUID lookup")
            .is_none()
    );

    let audit_count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
          WHERE organization_id = $1
            AND action = 'mail_engine.mailbox_binding.updated'
            AND resource_id = $2",
    )
    .bind(organization_b.as_uuid())
    .bind(mailbox_b.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count binding audit before rejected reassignment");
    let outbox_count_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM outbox_events
          WHERE event_type = 'mail_engine.mailbox_binding.updated.v1'
            AND aggregate_id = $1",
    )
    .bind(mailbox_b.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count binding outbox before rejected reassignment");
    let context_b = store
        .resolve_authorization(human_b, now)
        .await
        .expect("resolve second Human owner")
        .expect("second Human owner remains active");
    assert!(matches!(
        MailEngineMailboxBindingService::try_new(
            store.clone(),
            legacy_mail_data_plane_id(),
            "stalwart"
        )
        .expect("construct legacy binding service")
        .bind_authorized(
            &context_b,
            BindMailEngineMailboxCommand {
                organization_id: organization_b,
                mailbox_id: mailbox_b,
                account_ref: account_a.clone(),
                expected_version: 1,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::milliseconds(2),
            },
        )
        .await,
        Err(copilotmail_application::MailEventIngestionError::Store(
            copilotmail_application::StoreError::Conflict
        ))
    ));
    let audit_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
          WHERE organization_id = $1
            AND action = 'mail_engine.mailbox_binding.updated'
            AND resource_id = $2",
    )
    .bind(organization_b.as_uuid())
    .bind(mailbox_b.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count binding audit after rejected reassignment");
    let outbox_count_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM outbox_events
          WHERE event_type = 'mail_engine.mailbox_binding.updated.v1'
            AND aggregate_id = $1",
    )
    .bind(mailbox_b.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count binding outbox after rejected reassignment");
    assert_eq!(audit_count_after, audit_count_before);
    assert_eq!(outbox_count_after, outbox_count_before);

    assert_append_only_error(
        sqlx::query(
            "UPDATE mail_engine_account_owners
                SET first_bound_at = first_bound_at
              WHERE source = 'stalwart' AND account_ref = $1",
        )
        .bind(&account_a)
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query(
            "DELETE FROM mail_engine_account_owners
              WHERE source = 'stalwart' AND account_ref = $1",
        )
        .bind(&account_a)
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE mail_engine_account_owners")
            .execute(store.pool())
            .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn inbound_mail_event_is_bound_atomic_replay_safe_and_append_only() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "mail-ingress-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap mail-ingress tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Mail Ingress Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind mail-ingress OIDC owner");
    let context = store
        .resolve_authorization(owner.principal_id, now)
        .await
        .expect("resolve Human MailAdmin")
        .expect("Human MailAdmin is active");
    let mailbox = MailboxManagementService::new(store.clone())
        .create_authorized(
            &context,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Agent,
                primary_address: format!("agent-{}@example.test", Uuid::new_v4().simple()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create bound mailbox");
    let binding = MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct legacy binding service")
    .bind_authorized(
        &context,
        BindMailEngineMailboxCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            account_ref: "42".into(),
            expected_version: 0,
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    )
    .await
    .expect("bind Stalwart account");
    assert_eq!(binding.version, 1);

    let public_key = test_public_key();
    let agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Inbound Processing Agent".into(),
            environment: AgentEnvironment::Testing,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Exercise Claim Lease Ack and Nack processing".into(),
            review_due_at: now + chrono::Duration::days(30),
            public_key_fingerprint: rsa_public_key_fingerprint(public_key.as_bytes()).unwrap(),
            public_key_pem: public_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register inbound processing Agent");
    MailboxDelegationService::new(store.clone())
        .grant_authorized(
            &context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: agent.principal_id,
                capabilities: [MailboxCapability::Read].into_iter().collect(),
                reason: "Process inbound integration messages".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::hours(1),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("grant processing mailbox read access");
    let agent_context = store
        .resolve_authorization(agent.principal_id, now)
        .await
        .expect("resolve processing Agent authorization")
        .expect("processing Agent is active");
    let processing = AgentProcessingService::new(store.clone());
    let subscription = processing
        .ensure_subscription(EnsureAgentProcessingSubscriptionCommand {
            context: agent_context.clone(),
            mailbox_id: mailbox.mailbox_id,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("ensure self-bound processing subscription");

    let source_event = InboundMailEngineEvent {
        source_event_id: "17850123450001283".into(),
        source_occurred_at: now,
        account_ref: "42".into(),
        document_ref: "991".into(),
        classification: InboundMessageClassification::Ham,
    };
    let first_service = InboundMailEventIngestionService::new(
        store.clone(),
        legacy_mail_data_plane_id(),
        legacy_mail_event_fingerprint_scheme(),
    );
    let second_service = InboundMailEventIngestionService::new(
        store.clone(),
        legacy_mail_data_plane_id(),
        legacy_mail_event_fingerprint_scheme(),
    );
    let (first, second) = expect_no_lock_hang(async {
        tokio::join!(
            first_service.ingest(IngestInboundMailEventsCommand {
                events: vec![source_event.clone()],
                received_at: now,
            }),
            second_service.ingest(IngestInboundMailEventsCommand {
                events: vec![source_event.clone()],
                received_at: now,
            })
        )
    })
    .await;
    let first = first.expect("first concurrent ingress");
    let second = second.expect("second concurrent ingress");
    assert_eq!(first.inserted + second.inserted, 1);
    assert_eq!(first.duplicates + second.duplicates, 1);

    let evidence: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM mail_engine_mailbox_bindings
            WHERE organization_id = $1 AND mailbox_id = $2),
          (SELECT count(*) FROM inbound_mail_engine_events
            WHERE organization_id = $1 AND mailbox_id = $2),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'message.received.v1'
              AND payload->>'organization_id' = $3
              AND payload->>'mailbox_id' = $4),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action = 'mail_engine.mailbox_binding.updated'),
          (SELECT count(*) FROM agent_message_states
            WHERE organization_id = $1
              AND subscription_id = $5
              AND state = 'pending')
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(mailbox.mailbox_id.as_uuid())
    .bind(bootstrap.organization_id.to_string())
    .bind(mailbox.mailbox_id.to_string())
    .bind(subscription.subscription_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("query ingress evidence");
    assert_eq!(evidence, (1, 1, 1, 1, 1));

    let claim = processing
        .claim(ClaimAgentMessageCommand {
            context: agent_context.clone(),
            subscription_id: subscription.subscription_id,
            idempotency_key: "inbound-processing-claim-key-000001".into(),
            lease_seconds: 60,
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(1),
        })
        .await
        .expect("claim first inbound message")
        .expect("first inbound message is claimable");
    assert_eq!(claim.state, AgentMessageProcessingState::Leased);
    assert!(!claim.replayed);
    assert!(matches!(
        processing
            .acknowledge(AcknowledgeAgentMessageCommand {
                context: agent_context.clone(),
                subscription_id: subscription.subscription_id,
                message_id: claim.message_id,
                lease_id: LeaseId::new(),
                idempotency_key: "inbound-processing-wrong-ack-0001".into(),
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(2),
            })
            .await,
        Err(copilotmail_application::AgentProcessingError::Conflict)
    ));
    let acknowledgement = AcknowledgeAgentMessageCommand {
        context: agent_context.clone(),
        subscription_id: subscription.subscription_id,
        message_id: claim.message_id,
        lease_id: claim.lease_id,
        idempotency_key: "inbound-processing-ack-key-0000001".into(),
        trace_id: Uuid::now_v7().to_string(),
        now: now + chrono::Duration::seconds(2),
    };
    let acknowledged = processing
        .acknowledge(acknowledgement.clone())
        .await
        .expect("acknowledge first inbound message");
    assert_eq!(
        acknowledged.state,
        AgentMessageProcessingState::Acknowledged
    );
    let replayed = processing
        .acknowledge(acknowledgement)
        .await
        .expect("replay acknowledgement");
    assert!(replayed.replayed);

    let mutation = sqlx::query(
        "UPDATE inbound_mail_engine_events SET classification = 'spam' WHERE source_event_id = $1",
    )
    .bind(&source_event.source_event_id)
    .execute(store.pool())
    .await;
    assert_append_only_error(mutation);

    let mut tampered = source_event;
    tampered.document_ref = "992".into();
    let conflict = InboundMailEventIngestionService::new(
        store.clone(),
        legacy_mail_data_plane_id(),
        legacy_mail_event_fingerprint_scheme(),
    )
    .ingest(IngestInboundMailEventsCommand {
        events: vec![tampered],
        received_at: now,
    })
    .await;
    assert!(matches!(
        conflict,
        Err(copilotmail_application::MailEventIngestionError::Store(
            copilotmail_application::StoreError::Conflict
        ))
    ));

    InboundMailEventIngestionService::new(
        store.clone(),
        legacy_mail_data_plane_id(),
        legacy_mail_event_fingerprint_scheme(),
    )
    .ingest(IngestInboundMailEventsCommand {
        events: vec![InboundMailEngineEvent {
            source_event_id: "17850123450001284".into(),
            source_occurred_at: now + chrono::Duration::seconds(3),
            account_ref: "42".into(),
            document_ref: "993".into(),
            classification: InboundMessageClassification::Ham,
        }],
        received_at: now + chrono::Duration::seconds(3),
    })
    .await
    .expect("ingest message for retry and dead letter flow");
    let mut terminal = None;
    for attempt in 1..=5 {
        let step_now = now + chrono::Duration::seconds(3 + i64::from(attempt) * 2);
        let claimed = processing
            .claim(ClaimAgentMessageCommand {
                context: agent_context.clone(),
                subscription_id: subscription.subscription_id,
                idempotency_key: format!("inbound-retry-claim-{attempt:02}-stable-key"),
                lease_seconds: 60,
                trace_id: Uuid::now_v7().to_string(),
                now: step_now,
            })
            .await
            .expect("claim retry message")
            .expect("retry message remains claimable");
        assert_eq!(claimed.attempt_count, attempt);
        terminal = Some(
            processing
                .nack(NackAgentMessageCommand {
                    context: agent_context.clone(),
                    subscription_id: subscription.subscription_id,
                    message_id: claimed.message_id,
                    lease_id: claimed.lease_id,
                    idempotency_key: format!("inbound-retry-nack-{attempt:02}-stable-key"),
                    reason_code: "temporary_upstream_failure".into(),
                    trace_id: Uuid::now_v7().to_string(),
                    now: step_now + chrono::Duration::seconds(1),
                })
                .await
                .expect("nack retry message"),
        );
    }
    let terminal = terminal.expect("five attempts produce a terminal result");
    assert_eq!(terminal.state, AgentMessageProcessingState::DeadLettered);
    let dead_letters = processing
        .list_dead_letters(ListAgentProcessingDeadLettersCommand {
            context: agent_context.clone(),
            subscription_id: subscription.subscription_id,
            before_message_id: None,
            limit: 50,
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(20),
        })
        .await
        .expect("list self-owned processing dead letters");
    assert_eq!(dead_letters.items.len(), 1);
    assert_eq!(dead_letters.items[0].message_id, terminal.message_id);
    assert_eq!(
        dead_letters.items[0].reason_code,
        "temporary_upstream_failure"
    );
    assert!(!dead_letters.has_more);
    assert!(matches!(
        processing
            .list_dead_letters(ListAgentProcessingDeadLettersCommand {
                context: agent_context.clone(),
                subscription_id: subscription.subscription_id,
                before_message_id: Some(Uuid::now_v7()),
                limit: 50,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(21),
            })
            .await,
        Err(copilotmail_application::AgentProcessingError::InvalidRequest)
    ));
    let processing_evidence: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM agent_message_processing_operations
            WHERE organization_id = $1 AND subscription_id = $2),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action = 'agent.message.dead_lettered'),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'agent.message.dead_lettered.v1'
              AND payload->>'subscription_id' = $3)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(subscription.subscription_id.as_uuid())
    .bind(subscription.subscription_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("read processing operation evidence");
    assert_eq!(processing_evidence, (12, 1, 1));
    let query_evidence: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action = 'agent.message_dead_letters.queried'),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'agent.message_dead_letters.queried.v1'
              AND aggregate_id = $2)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(subscription.subscription_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("read dead-letter query evidence");
    assert_eq!(query_evidence, (1, 1));
    assert_append_only_error(
        sqlx::query("DELETE FROM agent_message_processing_operations WHERE organization_id = $1")
            .bind(bootstrap.organization_id.as_uuid())
            .execute(store.pool())
            .await,
    );

    let pause = ChangeAgentProcessingSubscriptionCommand {
        context: agent_context.clone(),
        subscription_id: subscription.subscription_id,
        expected_status: AgentProcessingSubscriptionStatus::Active,
        target_status: AgentProcessingSubscriptionStatus::Paused,
        idempotency_key: "processing-subscription-pause-000001".into(),
        trace_id: Uuid::now_v7().to_string(),
        now: now + chrono::Duration::seconds(22),
    };
    let paused = processing
        .change_subscription(pause.clone())
        .await
        .expect("pause processing subscription");
    assert_eq!(
        paused.subscription.status,
        AgentProcessingSubscriptionStatus::Paused
    );
    assert!(!paused.replayed);
    assert!(
        processing
            .change_subscription(pause)
            .await
            .expect("replay pause")
            .replayed
    );

    InboundMailEventIngestionService::new(
        store.clone(),
        legacy_mail_data_plane_id(),
        legacy_mail_event_fingerprint_scheme(),
    )
    .ingest(IngestInboundMailEventsCommand {
        events: vec![InboundMailEngineEvent {
            source_event_id: "17850123450001285".into(),
            source_occurred_at: now + chrono::Duration::seconds(23),
            account_ref: "42".into(),
            document_ref: "994".into(),
            classification: InboundMessageClassification::Ham,
        }],
        received_at: now + chrono::Duration::seconds(23),
    })
    .await
    .expect("fan out paused-subscription backlog");
    assert!(matches!(
        processing
            .claim(ClaimAgentMessageCommand {
                context: agent_context.clone(),
                subscription_id: subscription.subscription_id,
                idempotency_key: "processing-paused-claim-key-000001".into(),
                lease_seconds: 60,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(24),
            })
            .await,
        Err(copilotmail_application::AgentProcessingError::Conflict)
    ));

    let resumed = processing
        .change_subscription(ChangeAgentProcessingSubscriptionCommand {
            context: agent_context.clone(),
            subscription_id: subscription.subscription_id,
            expected_status: AgentProcessingSubscriptionStatus::Paused,
            target_status: AgentProcessingSubscriptionStatus::Active,
            idempotency_key: "processing-subscription-resume-00001".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(25),
        })
        .await
        .expect("resume processing subscription");
    assert_eq!(
        resumed.subscription.status,
        AgentProcessingSubscriptionStatus::Active
    );
    let backlog = processing
        .claim(ClaimAgentMessageCommand {
            context: agent_context.clone(),
            subscription_id: subscription.subscription_id,
            idempotency_key: "processing-backlog-claim-key-000001".into(),
            lease_seconds: 60,
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(26),
        })
        .await
        .expect("claim backlog after resume")
        .expect("paused ingress remains pending");
    assert!(matches!(
        processing
            .change_subscription(ChangeAgentProcessingSubscriptionCommand {
                context: agent_context.clone(),
                subscription_id: subscription.subscription_id,
                expected_status: AgentProcessingSubscriptionStatus::Active,
                target_status: AgentProcessingSubscriptionStatus::Retired,
                idempotency_key: "processing-live-lease-retire-0001".into(),
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(27),
            })
            .await,
        Err(copilotmail_application::AgentProcessingError::Conflict)
    ));
    processing
        .change_subscription(ChangeAgentProcessingSubscriptionCommand {
            context: agent_context.clone(),
            subscription_id: subscription.subscription_id,
            expected_status: AgentProcessingSubscriptionStatus::Active,
            target_status: AgentProcessingSubscriptionStatus::Paused,
            idempotency_key: "processing-subscription-pause-000002".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(28),
        })
        .await
        .expect("pause with live lease for drain");
    processing
        .acknowledge(AcknowledgeAgentMessageCommand {
            context: agent_context.clone(),
            subscription_id: subscription.subscription_id,
            message_id: backlog.message_id,
            lease_id: backlog.lease_id,
            idempotency_key: "processing-paused-drain-ack-000001".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: now + chrono::Duration::seconds(29),
        })
        .await
        .expect("drain lease while paused");
    let retire = ChangeAgentProcessingSubscriptionCommand {
        context: agent_context.clone(),
        subscription_id: subscription.subscription_id,
        expected_status: AgentProcessingSubscriptionStatus::Paused,
        target_status: AgentProcessingSubscriptionStatus::Retired,
        idempotency_key: "processing-subscription-retire-0001".into(),
        trace_id: Uuid::now_v7().to_string(),
        now: now + chrono::Duration::seconds(30),
    };
    let retired = processing
        .change_subscription(retire.clone())
        .await
        .expect("retire drained processing subscription");
    assert_eq!(
        retired.subscription.status,
        AgentProcessingSubscriptionStatus::Retired
    );
    assert!(
        processing
            .change_subscription(retire)
            .await
            .expect("replay retirement")
            .replayed
    );
    assert!(matches!(
        processing
            .claim(ClaimAgentMessageCommand {
                context: agent_context.clone(),
                subscription_id: subscription.subscription_id,
                idempotency_key: "processing-retired-claim-key-00001".into(),
                lease_seconds: 60,
                trace_id: Uuid::now_v7().to_string(),
                now: now + chrono::Duration::seconds(31),
            })
            .await,
        Err(copilotmail_application::AgentProcessingError::Conflict)
    ));
    let lifecycle_evidence: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM agent_processing_subscription_operations
            WHERE organization_id = $1 AND subscription_id = $2),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $1
              AND action IN (
                  'agent.processing_subscription.paused',
                  'agent.processing_subscription.resumed',
                  'agent.processing_subscription.retired'
              )),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $3
              AND event_type IN (
                  'agent.processing_subscription.paused.v1',
                  'agent.processing_subscription.resumed.v1',
                  'agent.processing_subscription.retired.v1'
              ))
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(subscription.subscription_id.as_uuid())
    .bind(subscription.subscription_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("read subscription lifecycle evidence");
    assert_eq!(lifecycle_evidence, (4, 4, 4));
    assert_append_only_error(
        sqlx::query(
            "DELETE FROM agent_processing_subscription_operations WHERE organization_id = $1",
        )
        .bind(bootstrap.organization_id.as_uuid())
        .execute(store.pool())
        .await,
    );

    let outbound = OutboundMailEngineEvent {
        source_event_id: "delivery-integration-1".into(),
        source_occurred_at: now,
        queue_ref: "771".into(),
        envelope_from: MailboxAddress::parse(&mailbox.primary_address).unwrap(),
        recipient: Some(MailboxAddress::parse("recipient@external.test").unwrap()),
        kind: OutboundDeliveryEventKind::Delivered,
        smtp_code: Some(250),
        next_retry_at: None,
    };
    let delivery_service = MailEngineEventIngestionService::new(
        store.clone(),
        legacy_mail_data_plane_id(),
        legacy_mail_event_fingerprint_scheme(),
    );
    let delivery = delivery_service
        .ingest(IngestMailEngineEventsCommand {
            events: vec![MailEngineWebhookEvent::Outbound(outbound.clone())],
            received_at: now,
        })
        .await
        .expect("ingest outbound delivery trace");
    assert_eq!(delivery.inserted, 1);
    let replay = delivery_service
        .ingest(IngestMailEngineEventsCommand {
            events: vec![MailEngineWebhookEvent::Outbound(outbound)],
            received_at: now,
        })
        .await
        .expect("deduplicate outbound delivery trace");
    assert_eq!(replay.duplicates, 1);
    let delivery_evidence: (i64, i64, bool) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM outbound_mail_engine_events
            WHERE organization_id = $1 AND mailbox_id = $2),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail_engine.delivery_trace.observed.v1'
              AND payload->>'organization_id' = $3),
          NOT EXISTS (
            SELECT 1 FROM outbox_events
             WHERE event_type = 'mail_engine.delivery_trace.observed.v1'
               AND payload::text LIKE '%@%'
          )
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(mailbox.mailbox_id.as_uuid())
    .bind(bootstrap.organization_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query outbound delivery evidence");
    assert_eq!(delivery_evidence, (1, 1, true));
    assert_append_only_error(
        sqlx::query(
            "UPDATE outbound_mail_engine_events SET event_kind = 'completed' WHERE source_event_id = $1",
        )
        .bind("delivery-integration-1")
        .execute(store.pool())
        .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn mail_draft_provenance_is_versioned_frozen_and_submission_bound() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "mail-draft-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap mail-draft tenant");
    let now = Utc::now();
    let human_id = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    let context = store
        .resolve_authorization(human_id, now)
        .await
        .expect("resolve Human owner")
        .expect("Human owner is active");
    let mailbox = MailboxManagementService::new(store.clone())
        .create_authorized(
            &context,
            CreateMailboxCommand {
                organization_id: bootstrap.organization_id,
                kind: MailboxKind::Human,
                primary_address: format!("draft-{}@example.test", Uuid::new_v4().simple()),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("create Human mailbox");
    MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct legacy binding service")
    .bind_authorized(
        &context,
        BindMailEngineMailboxCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            account_ref: "84".into(),
            expected_version: 0,
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    )
    .await
    .expect("bind draft account");

    let reply_to_message_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO inbound_mail_engine_events(
            event_id, message_id, organization_id, mailbox_id, source,
            source_event_id, source_event_sha256, account_ref, document_ref,
            classification, source_occurred_at, received_at
        )
        VALUES ($1, $2, $3, $4, 'stalwart', $5, $6, '84', '841',
                'ham', $7, $7)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(reply_to_message_id)
    .bind(bootstrap.organization_id.as_uuid())
    .bind(mailbox.mailbox_id.as_uuid())
    .bind(format!("reply-source-{}", Uuid::new_v4().simple()))
    .bind([31_u8; 32].as_slice())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert exact inbound reply target");

    let draft_id = DraftId::new();
    let first = store
        .record_mail_draft(MailDraftWrite {
            draft_id,
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: human_id,
            data_plane_id: legacy_mail_data_plane_id(),
            engine_account_ref: "84".into(),
            engine_email_ref: "draft-engine-1".into(),
            engine_thread_ref: "thread-engine-1".into(),
            reply_to_message_id: Some(reply_to_message_id),
            reply_to_automatic: false,
            body_ref: [21; 32],
            content_sha256: [1; 32],
            recipients: vec![copilotmail_application::MailDraftRecipientEvidence {
                recipient_ref: [11; 32],
                domain: "example.test".into(),
            }],
            expected_version: None,
            now,
        })
        .await
        .expect("record first draft");
    assert_eq!(first.version, 1);
    let updated_at = now + chrono::Duration::milliseconds(1);
    let second = store
        .record_mail_draft(MailDraftWrite {
            draft_id,
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: human_id,
            data_plane_id: legacy_mail_data_plane_id(),
            engine_account_ref: "84".into(),
            engine_email_ref: "draft-engine-2".into(),
            engine_thread_ref: "thread-engine-1".into(),
            reply_to_message_id: Some(reply_to_message_id),
            reply_to_automatic: false,
            body_ref: [22; 32],
            content_sha256: [2; 32],
            recipients: vec![copilotmail_application::MailDraftRecipientEvidence {
                recipient_ref: [12; 32],
                domain: "example.test".into(),
            }],
            expected_version: Some(1),
            now: updated_at,
        })
        .await
        .expect("replace draft with CAS");
    assert_eq!(second.version, 2);
    assert_eq!(second.reply_to_message_id, Some(reply_to_message_id));
    let changed_reply = store
        .record_mail_draft(MailDraftWrite {
            draft_id,
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: human_id,
            data_plane_id: legacy_mail_data_plane_id(),
            engine_account_ref: "84".into(),
            engine_email_ref: "draft-engine-forged-reply".into(),
            engine_thread_ref: "thread-engine-1".into(),
            reply_to_message_id: None,
            reply_to_automatic: false,
            body_ref: [23; 32],
            content_sha256: [3; 32],
            recipients: vec![copilotmail_application::MailDraftRecipientEvidence {
                recipient_ref: [13; 32],
                domain: "example.test".into(),
            }],
            expected_version: Some(2),
            now: updated_at + chrono::Duration::microseconds(1),
        })
        .await;
    assert!(matches!(
        changed_reply,
        Err(copilotmail_application::StoreError::Conflict)
    ));
    assert!(
        store
            .get_mail_draft(
                bootstrap.organization_id,
                mailbox.mailbox_id,
                PrincipalId::new(),
                draft_id,
                legacy_mail_data_plane_id(),
            )
            .await
            .expect("query wrong owner")
            .is_none()
    );

    let frozen_at = updated_at + chrono::Duration::milliseconds(1);
    let command = RequestMailSubmissionCommand {
        mailbox_id: mailbox.mailbox_id,
        draft_id,
        expected_draft_version: 2,
        idempotency_key: "integration-mail-submit-key-000001".into(),
        first_contact_approval_request_id: None,
        trace_id: Uuid::now_v7().to_string(),
        now: frozen_at,
    };
    let intent = MailSubmissionService::new(store.clone())
        .request(&context, command.clone())
        .await
        .expect("freeze draft and create submission intent");
    let replay = MailSubmissionService::new(store.clone())
        .request(&context, command.clone())
        .await
        .expect("exact idempotent replay");
    assert_eq!(replay.submission_id, intent.submission_id);
    let mut conflicting = command;
    conflicting.expected_draft_version = 3;
    assert!(matches!(
        MailSubmissionService::new(store.clone())
            .request(&context, conflicting)
            .await,
        Err(copilotmail_application::MailSubmissionError::Conflict)
    ));

    let evidence: (String, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT status FROM mail_drafts WHERE id = $1),
          (SELECT count(*) FROM mail_submission_intents WHERE id = $2),
          (SELECT count(*) FROM audit_events
            WHERE action = 'mail.submission.requested' AND resource_id = $3),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail.submission.requested.v1' AND aggregate_id = $3)
        "#,
    )
    .bind(draft_id.as_uuid())
    .bind(intent.submission_id.as_uuid())
    .bind(intent.submission_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query atomic mail-submission evidence");
    assert_eq!(evidence, ("frozen".into(), 1, 1, 1));

    let invalid_terminal_jump = sqlx::query(
        r#"
        UPDATE mail_submission_intents
           SET status = 'accepted',
               state_version = state_version + 1,
               engine_identity_ref = 'identity-1',
               engine_submission_ref = 'submission-1',
               accepted_at = $2,
               updated_at = $2
         WHERE id = $1
        "#,
    )
    .bind(intent.submission_id.as_uuid())
    .bind(frozen_at + chrono::Duration::milliseconds(1))
    .execute(store.pool())
    .await;
    assert_eq!(
        invalid_terminal_jump
            .expect_err("pending intent cannot skip reconciliation")
            .as_database_error()
            .and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("23514"))
    );

    let execution = MailSubmissionExecutor::new(store.clone(), Arc::new(ExistingSubmissionAdapter))
        .execute(ExecuteMailSubmissionCommand {
            submission_id: intent.submission_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("reconcile and accept submission");
    assert_eq!(execution, MailSubmissionExecutionResult::Accepted);
    let replay_execution =
        MailSubmissionExecutor::new(store.clone(), Arc::new(ExistingSubmissionAdapter))
            .execute(ExecuteMailSubmissionCommand {
                submission_id: intent.submission_id,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
            })
            .await
            .expect("terminal intent is not reacquired");
    assert_eq!(replay_execution, MailSubmissionExecutionResult::NotAcquired);
    let terminal_evidence: (String, String, String, String, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT status FROM mail_submission_intents WHERE id = $1),
          (SELECT engine_identity_ref FROM mail_submission_intents WHERE id = $1),
          (SELECT engine_submission_ref FROM mail_submission_intents WHERE id = $1),
          (SELECT status FROM mail_drafts WHERE id = $2),
          (SELECT count(*) FROM audit_events
            WHERE action = 'mail.submission.accepted' AND resource_id = $3),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'mail.submission.accepted.v1' AND aggregate_id = $3)
        "#,
    )
    .bind(intent.submission_id.as_uuid())
    .bind(draft_id.as_uuid())
    .bind(intent.submission_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query terminal submission evidence");
    assert_eq!(
        terminal_evidence,
        (
            "accepted".into(),
            "identity-integration".into(),
            "submission-integration".into(),
            "sent".into(),
            1,
            1,
        )
    );

    let delivery = MailDeliveryReconciliationService::new(
        store.clone(),
        Arc::new(ExistingSubmissionAdapter),
        MailDataPlaneFingerprintScheme::LegacyV1,
    )
    .reconcile(ReconcileMailDeliveryCommand {
        submission_id: intent.submission_id,
        // Exercise database/app clock skew: accepted_at is database-clamped
        // after this caller timestamp, while delivery remains logically later.
        now: frozen_at,
    })
    .await
    .expect("record exact recipient delivery evidence");
    assert_eq!(
        delivery,
        MailDeliveryReconciliationResult::Terminal { inserted: true }
    );
    let delivery_replay = MailDeliveryReconciliationService::new(
        store.clone(),
        Arc::new(ExistingSubmissionAdapter),
        MailDataPlaneFingerprintScheme::LegacyV1,
    )
    .reconcile(ReconcileMailDeliveryCommand {
        submission_id: intent.submission_id,
        now: frozen_at,
    })
    .await
    .expect("deduplicate exact delivery snapshot");
    assert_eq!(
        delivery_replay,
        MailDeliveryReconciliationResult::Terminal { inserted: false }
    );
    let latest = store
        .get_latest_mail_delivery_snapshot(
            bootstrap.organization_id,
            human_id,
            intent.submission_id,
        )
        .await
        .expect("query latest delivery snapshot")
        .expect("delivery snapshot exists");
    assert!(latest.terminal);
    assert_eq!(latest.recipients.len(), 2);
    let delivery_event_count: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*)
          FROM outbox_events
         WHERE event_type = 'message.delivery_status_changed.v1'
           AND aggregate_id = $1
        "#,
    )
    .bind(intent.submission_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("query delivery Outbox evidence");
    assert_eq!(delivery_event_count, 1);
    assert_append_only_error(
        sqlx::query("UPDATE mail_delivery_snapshots SET source_state = 'tampered' WHERE id = $1")
            .bind(latest.snapshot_id)
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM mail_delivery_recipient_evidence WHERE snapshot_id = $1")
            .bind(latest.snapshot_id)
            .execute(store.pool())
            .await,
    );

    assert_append_only_error(
        sqlx::query(
            r#"
            UPDATE mail_submission_intents
               SET state_version = state_version + 1,
                   updated_at = clock_timestamp()
             WHERE id = $1
            "#,
        )
        .bind(intent.submission_id.as_uuid())
        .execute(store.pool())
        .await,
    );

    assert_append_only_error(
        sqlx::query("DELETE FROM mail_submission_intents WHERE id = $1")
            .bind(intent.submission_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM mail_drafts WHERE id = $1")
            .bind(draft_id.as_uuid())
            .execute(store.pool())
            .await,
    );
}

struct ExistingSubmissionAdapter;

impl MailDataPlanePort for ExistingSubmissionAdapter {
    fn data_plane_id(&self) -> MailDataPlaneId {
        legacy_mail_data_plane_id()
    }
}

#[async_trait]
impl MailSubmissionAdapter for ExistingSubmissionAdapter {
    async fn probe_submissions(&self) -> Result<(), MailEngineError> {
        Ok(())
    }

    async fn reconcile_submission(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        expected_from: &str,
    ) -> Result<MailSubmissionReconciliation, MailEngineError> {
        assert_eq!(account_ref, "84");
        assert_eq!(engine_email_ref, "draft-engine-2");
        assert!(expected_from.ends_with("@example.test"));
        Ok(MailSubmissionReconciliation::Accepted {
            identity_ref: "identity-integration".into(),
            submission_ref: "submission-integration".into(),
        })
    }

    async fn create_submission(
        &self,
        _account_ref: &str,
        _engine_email_ref: &str,
        _identity_ref: &str,
    ) -> Result<CreatedMailSubmission, MailEngineError> {
        panic!("existing submission must never be recreated")
    }

    async fn get_delivery_snapshot(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        identity_ref: &str,
        submission_ref: &str,
    ) -> Result<MailDeliverySnapshot, MailEngineError> {
        assert_eq!(account_ref, "84");
        assert_eq!(engine_email_ref, "draft-engine-2");
        assert_eq!(identity_ref, "identity-integration");
        assert_eq!(submission_ref, "submission-integration");
        Ok(MailDeliverySnapshot {
            submission_ref: submission_ref.into(),
            source_state: "delivery-state-integration".into(),
            recipients: vec![
                RecipientDeliveryEvidence {
                    recipient: MailboxAddress::parse("done@example.test").unwrap(),
                    state: RecipientDeliveryState::Delivered,
                    smtp_reply: "250 2.0.0 Accepted".into(),
                },
                RecipientDeliveryEvidence {
                    recipient: MailboxAddress::parse("failed@example.test").unwrap(),
                    state: RecipientDeliveryState::PermanentFailure,
                    smtp_reply: "550 5.1.1 User unknown".into(),
                },
            ],
        })
    }
}

struct ProductionAgentAdmissionSubmissionAdapter {
    account_ref: String,
    engine_email_ref: String,
    delivery_reads: Arc<AtomicU64>,
}

impl MailDataPlanePort for ProductionAgentAdmissionSubmissionAdapter {
    fn data_plane_id(&self) -> MailDataPlaneId {
        legacy_mail_data_plane_id()
    }
}

#[async_trait]
impl MailSubmissionAdapter for ProductionAgentAdmissionSubmissionAdapter {
    async fn probe_submissions(&self) -> Result<(), MailEngineError> {
        Ok(())
    }

    async fn reconcile_submission(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        expected_from: &str,
    ) -> Result<MailSubmissionReconciliation, MailEngineError> {
        assert_eq!(account_ref, self.account_ref);
        assert_eq!(engine_email_ref, self.engine_email_ref);
        assert!(expected_from.ends_with("@example.test"));
        Ok(MailSubmissionReconciliation::Accepted {
            identity_ref: "production-agent-evidence-identity".into(),
            submission_ref: "production-agent-evidence-submission".into(),
        })
    }

    async fn create_submission(
        &self,
        _account_ref: &str,
        _engine_email_ref: &str,
        _identity_ref: &str,
    ) -> Result<CreatedMailSubmission, MailEngineError> {
        panic!("existing production Agent submission must never be recreated")
    }

    async fn get_delivery_snapshot(
        &self,
        account_ref: &str,
        engine_email_ref: &str,
        identity_ref: &str,
        submission_ref: &str,
    ) -> Result<MailDeliverySnapshot, MailEngineError> {
        self.delivery_reads.fetch_add(1, Ordering::SeqCst);
        assert_eq!(account_ref, self.account_ref);
        assert_eq!(engine_email_ref, self.engine_email_ref);
        assert_eq!(identity_ref, "production-agent-evidence-identity");
        assert_eq!(submission_ref, "production-agent-evidence-submission");
        Ok(MailDeliverySnapshot {
            submission_ref: submission_ref.into(),
            source_state: "production-agent-evidence-terminal".into(),
            recipients: vec![RecipientDeliveryEvidence {
                recipient: MailboxAddress::parse("production-agent@example.test").unwrap(),
                state: RecipientDeliveryState::Delivered,
                smtp_reply: "250 2.0.0 Accepted".into(),
            }],
        })
    }
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn production_agent_admission_is_approved_bounded_atomic_and_worker_visible() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "agent-admission-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Agent admission tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Agent Admission Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind Agent admission owner");
    let owner_context = store
        .resolve_authorization(owner.principal_id, now)
        .await
        .expect("resolve owner authorization")
        .expect("owner authorization");

    let public_key = test_public_key();
    let agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Production Admission Agent".into(),
            environment: AgentEnvironment::Production,
            risk_level: AgentRiskLevel::High,
            purpose: "Exercise atomic production outbound admission".into(),
            review_due_at: now + chrono::Duration::days(30),
            public_key_fingerprint: rsa_public_key_fingerprint(public_key.as_bytes()).unwrap(),
            public_key_pem: public_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register production admission Agent");
    let policy_version = AgentMailPolicyService::new(store.clone())
        .create_version(CreateAgentMailPolicyVersionCommand {
            context: owner_context.clone(),
            agent_id: agent.principal_id,
            allowed_recipient_domains: BTreeSet::from(["example.test".into()]),
            max_recipients_per_message: 2,
            max_messages_per_hour: 1,
            max_messages_per_recipient_per_day: 1,
            require_first_contact_approval: true,
            max_auto_replies_per_thread_per_day: 1,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create production Agent policy");

    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "agent.production.activation".into(),
            display_name: "Production Agent mail activation".into(),
            action: ApprovalAction::ProductionAgentMail,
            required_approvals: 2,
            rule: json!({"resource_scope": "exact_policy_version"}),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create production activation approval policy");
    let workflow = ApprovalWorkflowService::new(store.clone());
    let activation_approval_id = workflow
        .request(RequestApprovalCommand {
            context: owner_context.clone(),
            policy_key: "agent.production.activation".into(),
            action: ApprovalAction::ProductionAgentMail,
            resource_kind: "agent.mail_policy_version".into(),
            resource_id: policy_version.id.to_string(),
            reason: "Approve bounded production Agent mail".into(),
            ticket_reference: Some("AGENT-MAIL-1".into()),
            expires_at: now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("request production policy activation");
    for _ in 0..2 {
        let approver = insert_approval_test_owner(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            now,
        )
        .await;
        let approver_context = store
            .resolve_authorization(approver, now)
            .await
            .expect("resolve policy approver")
            .expect("policy approver authorization");
        workflow
            .decide(DecideApprovalCommand {
                context: approver_context,
                request_id: activation_approval_id,
                decision: ApprovalDecision::Approved,
                reason: "independent production policy review".into(),
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .expect("approve production policy activation");
    }
    AgentMailPolicyService::new(store.clone())
        .activate(ActivateAgentMailPolicyCommand {
            context: owner_context.clone(),
            agent_id: agent.principal_id,
            policy_version_id: policy_version.id,
            approval: ApprovalProof {
                organization_id: bootstrap.organization_id,
                request_id: activation_approval_id,
                requested_by: owner.principal_id,
                action: ApprovalAction::ProductionAgentMail,
                resource_kind: "agent.mail_policy_version".into(),
                resource_id: policy_version.id.to_string(),
                now,
            },
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("activate production Agent policy");
    assert!(
        store
            .principal_can_submit_mail(bootstrap.organization_id, agent.principal_id)
            .await
            .expect("evaluate activated production Agent")
    );

    let mailbox = MailboxManagementService::new(store.clone())
        .create(CreateMailboxCommand {
            organization_id: bootstrap.organization_id,
            kind: MailboxKind::Agent,
            primary_address: format!("agent-{}@example.test", Uuid::new_v4().simple()),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create production Agent mailbox");
    MailboxDelegationService::new(store.clone())
        .grant_authorized(
            &owner_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: agent.principal_id,
                capabilities: BTreeSet::from([MailboxCapability::Send]),
                reason: "Production Agent owns its send workflow".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::days(30),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("delegate production Agent Send");
    let engine_account_ref = random_numeric_engine_ref();
    MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct legacy binding service")
    .bind_authorized(
        &owner_context,
        BindMailEngineMailboxCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            account_ref: engine_account_ref.clone(),
            expected_version: 0,
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    )
    .await
    .expect("bind production Agent mail account");

    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id: bootstrap.organization_id,
            policy_key: "agent.first.contact".into(),
            display_name: "Production Agent first contact".into(),
            action: ApprovalAction::AgentFirstContact,
            required_approvals: 1,
            rule: json!({"resource_scope": "exact_draft_version"}),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create first-contact approval policy");
    let agent_context = store
        .resolve_authorization(agent.principal_id, now)
        .await
        .expect("resolve production Agent authorization")
        .expect("production Agent authorization");
    let recipient = MailDraftRecipientEvidence {
        recipient_ref: [41; 32],
        domain: "example.test".into(),
    };
    let first_draft_id = DraftId::new();
    store
        .record_mail_draft(MailDraftWrite {
            draft_id: first_draft_id,
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: agent.principal_id,
            data_plane_id: legacy_mail_data_plane_id(),
            engine_account_ref: engine_account_ref.clone(),
            engine_email_ref: "production-draft-1".into(),
            engine_thread_ref: "production-thread-1".into(),
            reply_to_message_id: None,
            reply_to_automatic: false,
            body_ref: [61; 32],
            content_sha256: [51; 32],
            recipients: vec![recipient.clone()],
            expected_version: None,
            now,
        })
        .await
        .expect("record production Agent draft evidence");
    let contact_approval_id = workflow
        .request(RequestApprovalCommand {
            context: agent_context.clone(),
            policy_key: "agent.first.contact".into(),
            action: ApprovalAction::AgentFirstContact,
            resource_kind: "agent.mail_draft".into(),
            resource_id: format!("{first_draft_id}:1"),
            reason: "First business contact is expected".into(),
            ticket_reference: Some("AGENT-CONTACT-1".into()),
            expires_at: now + chrono::Duration::minutes(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("request exact first-contact approval");
    let contact_approver = insert_approval_test_owner(
        &store,
        bootstrap.organization_id,
        bootstrap.root_principal_id,
        now,
    )
    .await;
    workflow
        .decide(DecideApprovalCommand {
            context: store
                .resolve_authorization(contact_approver, now)
                .await
                .expect("resolve contact approver")
                .expect("contact approver authorization"),
            request_id: contact_approval_id,
            decision: ApprovalDecision::Approved,
            reason: "first-contact purpose reviewed".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("approve first contact");
    let first_intent = MailSubmissionService::new(store.clone())
        .request(
            &agent_context,
            RequestMailSubmissionCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id: first_draft_id,
                expected_draft_version: 1,
                idempotency_key: "production-agent-admission-key-000001".into(),
                first_contact_approval_request_id: Some(contact_approval_id),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("commit approved production Agent admission");
    let evidence_counts: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM agent_mail_admissions WHERE submission_id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2 AND action = 'agent.mail.admitted'),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = (
                SELECT id::text FROM agent_mail_admissions WHERE submission_id = $1
            ) AND event_type = 'agent.mail.admitted.v1')
        "#,
    )
    .bind(first_intent.submission_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("read atomic production admission evidence");
    assert_eq!(evidence_counts, (1, 1, 1));
    let exact_evidence = AgentMailSubmissionEvidenceService::new(store.clone())
        .get(GetAgentMailSubmissionEvidenceCommand {
            context: owner_context.clone(),
            organization_id: bootstrap.organization_id,
            agent_id: agent.principal_id,
            submission_id: first_intent.submission_id,
            trace_id: Uuid::now_v7().to_string(),
        })
        .await
        .expect("read exact production Agent submission evidence");
    assert_eq!(exact_evidence.agent_id, agent.principal_id);
    assert_eq!(exact_evidence.submission_id, first_intent.submission_id);
    assert_eq!(exact_evidence.submission_status.as_str(), "pending");
    assert_eq!(
        exact_evidence.admission.policy_version,
        policy_version.policy.version()
    );
    assert_eq!(
        exact_evidence
            .admission
            .policy_activation_approval_request_id,
        activation_approval_id
    );
    assert_eq!(
        exact_evidence.admission.first_contact_approval_request_id,
        Some(contact_approval_id)
    );
    assert!(exact_evidence.delivery.is_none());
    assert_eq!(
        exact_evidence
            .audit_references
            .iter()
            .map(|reference| reference.action.as_str())
            .collect::<Vec<_>>(),
        vec!["mail.submission.requested", "agent.mail.admitted"]
    );

    let delivery_reads = Arc::new(AtomicU64::new(0));
    let production_submission_adapter: Arc<dyn MailSubmissionAdapter> =
        Arc::new(ProductionAgentAdmissionSubmissionAdapter {
            account_ref: engine_account_ref.clone(),
            engine_email_ref: "production-draft-1".into(),
            delivery_reads: Arc::clone(&delivery_reads),
        });
    let pre_accept_delivery = MailDeliveryReconciliationService::new(
        store.clone(),
        Arc::clone(&production_submission_adapter),
        MailDataPlaneFingerprintScheme::LegacyV1,
    )
    .reconcile(ReconcileMailDeliveryCommand {
        submission_id: first_intent.submission_id,
        now: Utc::now(),
    })
    .await;
    assert!(matches!(
        pre_accept_delivery,
        Err(MailDeliveryReconciliationError::NotEligible)
    ));
    assert_eq!(
        delivery_reads.load(Ordering::SeqCst),
        0,
        "a pending submission must not call the remote delivery reader"
    );

    let execution =
        MailSubmissionExecutor::new(store.clone(), Arc::clone(&production_submission_adapter))
            .execute(ExecuteMailSubmissionCommand {
                submission_id: first_intent.submission_id,
                trace_id: Uuid::now_v7().to_string(),
                now: Utc::now(),
            })
            .await
            .expect("reconcile and accept the approved production Agent submission");
    assert_eq!(execution, MailSubmissionExecutionResult::Accepted);
    let accepted_at: chrono::DateTime<Utc> = sqlx::query_scalar(
        "SELECT accepted_at FROM mail_submission_intents WHERE id = $1 AND status = 'accepted'",
    )
    .bind(first_intent.submission_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("read accepted production Agent timestamp");
    let delivery = MailDeliveryReconciliationService::new(
        store.clone(),
        Arc::clone(&production_submission_adapter),
        MailDataPlaneFingerprintScheme::LegacyV1,
    )
    .reconcile(ReconcileMailDeliveryCommand {
        submission_id: first_intent.submission_id,
        now: accepted_at + chrono::Duration::milliseconds(1),
    })
    .await
    .expect("record terminal production Agent delivery aggregate");
    assert_eq!(
        delivery,
        MailDeliveryReconciliationResult::Terminal { inserted: true }
    );
    assert_eq!(
        delivery_reads.load(Ordering::SeqCst),
        1,
        "only the accepted submission may read its exact delivery snapshot"
    );

    let p3_evidence_reads_before: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*)
             FROM audit_events
            WHERE organization_id = $1
              AND actor_principal_id = $2
              AND action = 'agent.mail_submission_evidence.queried'
              AND resource_type = 'agent.mail_submission_evidence'
              AND resource_id = $3
              AND outcome = 'success'),
          (SELECT count(*)
             FROM outbox_events
            WHERE event_type = 'agent.mail_submission_evidence.queried.v1'
              AND aggregate_type = 'agent.mail_submission_evidence'
              AND aggregate_id = $3
              AND payload ->> 'organization_id' = $1::text
              AND payload ->> 'agent_id' = $4)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(owner.principal_id.as_uuid())
    .bind(first_intent.submission_id.to_string())
    .bind(agent.principal_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("count pending P3 read evidence");
    assert_eq!(p3_evidence_reads_before, (1, 1));

    let terminal_evidence = AgentMailSubmissionEvidenceService::new(store.clone())
        .get(GetAgentMailSubmissionEvidenceCommand {
            context: owner_context.clone(),
            organization_id: bootstrap.organization_id,
            agent_id: agent.principal_id,
            submission_id: first_intent.submission_id,
            trace_id: Uuid::now_v7().to_string(),
        })
        .await
        .expect("read terminal production Agent submission evidence");
    assert_eq!(terminal_evidence.agent_id, agent.principal_id);
    assert_eq!(terminal_evidence.submission_id, first_intent.submission_id);
    assert_eq!(terminal_evidence.submission_status.as_str(), "accepted");
    let terminal_delivery = terminal_evidence
        .delivery
        .as_ref()
        .expect("accepted submission exposes its immutable aggregate delivery");
    assert!(terminal_delivery.terminal);
    assert_eq!(terminal_delivery.recipient_count, 1);
    assert_eq!(terminal_delivery.delivered_count, 1);
    assert_eq!(terminal_delivery.failed_count, 0);
    assert_eq!(terminal_delivery.queued_count, 0);
    assert_eq!(terminal_delivery.unknown_count, 0);
    assert_eq!(
        terminal_evidence
            .audit_references
            .iter()
            .map(|reference| reference.action.as_str())
            .collect::<Vec<_>>(),
        vec![
            "mail.submission.requested",
            "agent.mail.admitted",
            "mail.submission.accepted"
        ]
    );
    let terminal_audits: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT action, outcome
          FROM audit_events
         WHERE organization_id = $1
           AND actor_principal_id = $2
           AND on_behalf_of_principal_id IS NULL
           AND (
               (action = 'mail.submission.requested'
                AND resource_type = 'mail.submission'
                AND resource_id = $3)
               OR (action = 'agent.mail.admitted'
                   AND resource_type = 'agent.mail_admission'
                   AND resource_id = $4)
               OR (action = 'mail.submission.accepted'
                   AND resource_type = 'mail.submission'
                   AND resource_id = $3)
           )
         ORDER BY CASE action
                      WHEN 'mail.submission.requested' THEN 1
                      WHEN 'agent.mail.admitted' THEN 2
                      WHEN 'mail.submission.accepted' THEN 3
                  END
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(agent.principal_id.as_uuid())
    .bind(first_intent.submission_id.to_string())
    .bind(terminal_evidence.admission.admission_id.to_string())
    .fetch_all(store.pool())
    .await
    .expect("read exact accepted production Agent audit chain");
    assert_eq!(
        terminal_audits,
        vec![
            ("mail.submission.requested".into(), "success".into()),
            ("agent.mail.admitted".into(), "success".into()),
            ("mail.submission.accepted".into(), "success".into()),
        ]
    );
    let p3_evidence_reads_after: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*)
             FROM audit_events
            WHERE organization_id = $1
              AND actor_principal_id = $2
              AND action = 'agent.mail_submission_evidence.queried'
              AND resource_type = 'agent.mail_submission_evidence'
              AND resource_id = $3
              AND outcome = 'success'),
          (SELECT count(*)
             FROM outbox_events
            WHERE event_type = 'agent.mail_submission_evidence.queried.v1'
              AND aggregate_type = 'agent.mail_submission_evidence'
              AND aggregate_id = $3
              AND payload ->> 'organization_id' = $1::text
              AND payload ->> 'agent_id' = $4)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(owner.principal_id.as_uuid())
    .bind(first_intent.submission_id.to_string())
    .bind(agent.principal_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("read terminal P3 audit and Outbox evidence");
    assert_eq!(
        p3_evidence_reads_after,
        (
            p3_evidence_reads_before.0 + 1,
            p3_evidence_reads_before.1 + 1
        ),
        "a successful P3 evidence read commits one Audit event and one Outbox event"
    );
    let runtime = store
        .get_agent_mail_policy_runtime(bootstrap.organization_id, agent.principal_id)
        .await
        .expect("query Agent policy runtime")
        .expect("active Agent runtime exists");
    assert_eq!(runtime.environment, AgentEnvironment::Production);
    assert_eq!(
        runtime
            .active_policy
            .as_ref()
            .expect("active production policy")
            .version(),
        policy_version.policy.version()
    );
    assert_eq!(runtime.admitted_messages_in_hour, 1);
    assert!(runtime.observed_at >= runtime.hourly_window_started_at);

    let automatic_source_message_id = Uuid::now_v7();
    let automatic_source_document_ref = random_numeric_engine_ref();
    sqlx::query(
        r#"
        INSERT INTO inbound_mail_engine_events(
            event_id, message_id, organization_id, mailbox_id, source,
            source_event_id, source_event_sha256, account_ref, document_ref,
            classification, source_occurred_at, received_at
        )
        VALUES ($1, $2, $3, $4, 'stalwart', $5, $6,
                $7, $8,
                'ham', $9, $9)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(automatic_source_message_id)
    .bind(bootstrap.organization_id.as_uuid())
    .bind(mailbox.mailbox_id.as_uuid())
    .bind(format!("automatic-source-{}", Uuid::new_v4().simple()))
    .bind([63_u8; 32].as_slice())
    .bind(&engine_account_ref)
    .bind(automatic_source_document_ref)
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert automatic reply source");
    let automatic_reply_draft_id = DraftId::new();
    store
        .record_mail_draft(MailDraftWrite {
            draft_id: automatic_reply_draft_id,
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: agent.principal_id,
            data_plane_id: legacy_mail_data_plane_id(),
            engine_account_ref: engine_account_ref.clone(),
            engine_email_ref: "production-automatic-reply".into(),
            engine_thread_ref: "production-automatic-thread".into(),
            reply_to_message_id: Some(automatic_source_message_id),
            reply_to_automatic: true,
            body_ref: [64; 32],
            content_sha256: [65; 32],
            recipients: vec![recipient.clone()],
            expected_version: None,
            now,
        })
        .await
        .expect("record Agent reply to automatic source");
    assert!(matches!(
        MailSubmissionService::new(store.clone())
            .request(
                &agent_context,
                RequestMailSubmissionCommand {
                    mailbox_id: mailbox.mailbox_id,
                    draft_id: automatic_reply_draft_id,
                    expected_draft_version: 1,
                    idempotency_key: "production-agent-auto-reply-key-0001".into(),
                    first_contact_approval_request_id: None,
                    trace_id: Uuid::now_v7().to_string(),
                    now,
                },
            )
            .await,
        Err(
            copilotmail_application::MailSubmissionError::AgentAdmission(
                copilotmail_application::AgentMailAdmissionFailure::AutomaticReplyLoop
            )
        )
    ));
    let automatic_rollback: (String, i64) = sqlx::query_as(
        r#"
        SELECT status,
               (SELECT count(*) FROM mail_submission_intents WHERE draft_id = $1)
          FROM mail_drafts
         WHERE id = $1
        "#,
    )
    .bind(automatic_reply_draft_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("read automatic reply rollback");
    assert_eq!(automatic_rollback, ("active".into(), 0));

    let second_draft_id = DraftId::new();
    store
        .record_mail_draft(MailDraftWrite {
            draft_id: second_draft_id,
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: agent.principal_id,
            data_plane_id: legacy_mail_data_plane_id(),
            engine_account_ref,
            engine_email_ref: "production-draft-2".into(),
            engine_thread_ref: "production-thread-2".into(),
            reply_to_message_id: None,
            reply_to_automatic: false,
            body_ref: [62; 32],
            content_sha256: [52; 32],
            recipients: vec![recipient],
            expected_version: None,
            now,
        })
        .await
        .expect("record quota-rejected draft");
    assert!(matches!(
        MailSubmissionService::new(store.clone())
            .request(
                &agent_context,
                RequestMailSubmissionCommand {
                    mailbox_id: mailbox.mailbox_id,
                    draft_id: second_draft_id,
                    expected_draft_version: 1,
                    idempotency_key: "production-agent-admission-key-000002".into(),
                    first_contact_approval_request_id: None,
                    trace_id: Uuid::now_v7().to_string(),
                    now,
                },
            )
            .await,
        Err(
            copilotmail_application::MailSubmissionError::AgentAdmission(
                copilotmail_application::AgentMailAdmissionFailure::HourlyQuota
            )
        )
    ));
    let rejected_counts: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM mail_submission_intents WHERE draft_id = $1),
          (SELECT count(*) FROM agent_mail_admissions WHERE draft_id = $1)
        "#,
    )
    .bind(second_draft_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("read rejected admission rollback");
    assert_eq!(rejected_counts, (0, 0));
    assert_append_only_error(
        sqlx::query("DELETE FROM agent_mail_admissions WHERE submission_id = $1")
            .bind(first_intent.submission_id.as_uuid())
            .execute(store.pool())
            .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn testing_agent_sandbox_freezes_without_creating_a_deliverable_intent() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "agent-sandbox-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Agent sandbox tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Agent Sandbox Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind Agent sandbox owner");
    let owner_context = store
        .resolve_authorization(owner.principal_id, now)
        .await
        .expect("resolve sandbox owner")
        .expect("sandbox owner authorization");
    let public_key = test_public_key();
    let agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Testing Sandbox Agent".into(),
            environment: AgentEnvironment::Testing,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Prove test mail never reaches a delivery adapter".into(),
            review_due_at: now + chrono::Duration::days(30),
            public_key_fingerprint: rsa_public_key_fingerprint(public_key.as_bytes()).unwrap(),
            public_key_pem: public_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register testing Agent");
    let mailbox = MailboxManagementService::new(store.clone())
        .create(CreateMailboxCommand {
            organization_id: bootstrap.organization_id,
            kind: MailboxKind::Agent,
            primary_address: format!("sandbox-{}@example.test", Uuid::new_v4().simple()),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create testing Agent mailbox");
    // Account ownership is claimed permanently across runs, so each fixture
    // run needs a fresh account reference.
    let account_ref = random_numeric_engine_ref();
    MailEngineMailboxBindingService::try_new(
        store.clone(),
        legacy_mail_data_plane_id(),
        "stalwart",
    )
    .expect("construct sandbox binding service")
    .bind_authorized(
        &owner_context,
        BindMailEngineMailboxCommand {
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            account_ref: account_ref.clone(),
            expected_version: 0,
            trace_id: Uuid::now_v7().to_string(),
            now,
        },
    )
    .await
    .expect("bind testing Agent mailbox");
    MailboxDelegationService::new(store.clone())
        .grant_authorized(
            &owner_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: agent.principal_id,
                capabilities: BTreeSet::from([MailboxCapability::Send]),
                reason: "Testing Agent owns its sandbox workflow".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::days(30),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("delegate sandbox Send");
    let agent_context = store
        .resolve_authorization(agent.principal_id, now)
        .await
        .expect("resolve testing Agent")
        .expect("testing Agent authorization");
    let draft_id = DraftId::new();
    store
        .record_mail_draft(MailDraftWrite {
            draft_id,
            organization_id: bootstrap.organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: agent.principal_id,
            data_plane_id: legacy_mail_data_plane_id(),
            engine_account_ref: account_ref.clone(),
            engine_email_ref: format!("testing-agent-draft-{}", Uuid::new_v4().simple()),
            engine_thread_ref: format!("testing-agent-thread-{}", Uuid::new_v4().simple()),
            reply_to_message_id: None,
            reply_to_automatic: false,
            body_ref: [71; 32],
            content_sha256: [72; 32],
            recipients: vec![MailDraftRecipientEvidence {
                recipient_ref: [73; 32],
                domain: "example.test".into(),
            }],
            expected_version: None,
            now,
        })
        .await
        .expect("record testing Agent draft");

    let blocked_delivery = MailSubmissionService::new(store.clone())
        .request(
            &agent_context,
            RequestMailSubmissionCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id,
                expected_draft_version: 1,
                idempotency_key: "testing-agent-delivery-key-000001".into(),
                first_contact_approval_request_id: None,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await;
    assert!(matches!(
        blocked_delivery,
        Err(
            copilotmail_application::MailSubmissionError::AgentAdmission(
                copilotmail_application::AgentMailAdmissionFailure::SandboxRequired
            )
        )
    ));

    let command = CaptureAgentMailSandboxCommand {
        mailbox_id: mailbox.mailbox_id,
        draft_id,
        expected_draft_version: 1,
        idempotency_key: "testing-agent-sandbox-key-000001".into(),
        trace_id: Uuid::now_v7().to_string(),
        now,
    };
    let captured = AgentMailSandboxService::new(store.clone())
        .capture(&agent_context, command.clone())
        .await
        .expect("capture testing Agent draft");
    assert!(!captured.replayed);
    assert_eq!(captured.record.draft_version, 2);
    assert_eq!(captured.record.recipient_count, 1);
    let replay = AgentMailSandboxService::new(store.clone())
        .capture(&agent_context, command)
        .await
        .expect("replay exact testing Agent capture");
    assert!(replay.replayed);
    assert_eq!(replay.record.submission_id, captured.record.submission_id);

    let evidence: (String, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT status FROM mail_drafts WHERE id = $1),
          (SELECT count(*) FROM mail_submission_intents WHERE draft_id = $1),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $2
              AND action = 'agent.mail_sandbox.captured'
              AND resource_id = $3),
          (SELECT count(*) FROM outbox_events
            WHERE event_type = 'agent.mail_sandbox.captured.v1'
              AND aggregate_id = $3)
        "#,
    )
    .bind(draft_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(captured.record.submission_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("read atomic sandbox evidence");
    assert_eq!(evidence, ("frozen".into(), 0, 1, 1));
    assert_append_only_error(
        sqlx::query("UPDATE agent_mail_sandbox_submissions SET recipient_count = 2 WHERE id = $1")
            .bind(captured.record.submission_id)
            .execute(store.pool())
            .await,
    );

    let development_key = test_public_key();
    let development_agent = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Development Duplicate Guard Agent".into(),
            environment: AgentEnvironment::Development,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Exercise exact duplicate-body circuit breaker".into(),
            review_due_at: now + chrono::Duration::days(30),
            public_key_fingerprint: rsa_public_key_fingerprint(development_key.as_bytes()).unwrap(),
            public_key_pem: development_key,
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register development Agent");
    MailboxDelegationService::new(store.clone())
        .grant_authorized(
            &owner_context,
            GrantMailboxDelegationCommand {
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: development_agent.principal_id,
                capabilities: BTreeSet::from([MailboxCapability::Send]),
                reason: "Development Agent exercises duplicate guard".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::days(30),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("delegate development Agent Send");
    let development_context = store
        .resolve_authorization(development_agent.principal_id, now)
        .await
        .expect("resolve development Agent")
        .expect("development Agent authorization");
    let mut rejected_duplicate_draft = None;
    for attempt in 0_u8..=10 {
        let duplicate_draft_id = DraftId::new();
        let attempt_time = now + chrono::Duration::milliseconds(i64::from(attempt));
        store
            .record_mail_draft(MailDraftWrite {
                draft_id: duplicate_draft_id,
                organization_id: bootstrap.organization_id,
                mailbox_id: mailbox.mailbox_id,
                created_by: development_agent.principal_id,
                data_plane_id: legacy_mail_data_plane_id(),
                engine_account_ref: account_ref.clone(),
                engine_email_ref: format!("development-duplicate-draft-{attempt}"),
                engine_thread_ref: format!("development-duplicate-thread-{attempt}"),
                reply_to_message_id: None,
                reply_to_automatic: false,
                body_ref: [81; 32],
                content_sha256: [82; 32],
                recipients: vec![MailDraftRecipientEvidence {
                    recipient_ref: [83; 32],
                    domain: "example.test".into(),
                }],
                expected_version: None,
                now: attempt_time,
            })
            .await
            .expect("record duplicate-body draft");
        let result = MailSubmissionService::new(store.clone())
            .request(
                &development_context,
                RequestMailSubmissionCommand {
                    mailbox_id: mailbox.mailbox_id,
                    draft_id: duplicate_draft_id,
                    expected_draft_version: 1,
                    idempotency_key: format!("development-duplicate-key-{attempt:02}-000001"),
                    first_contact_approval_request_id: None,
                    trace_id: Uuid::now_v7().to_string(),
                    now: attempt_time,
                },
            )
            .await;
        if attempt < 10 {
            result.expect("first ten identical bodies are admitted");
        } else {
            assert!(matches!(
                result,
                Err(
                    copilotmail_application::MailSubmissionError::AgentAdmission(
                        copilotmail_application::AgentMailAdmissionFailure::
                            DuplicateContentCircuitBreaker
                    )
                )
            ));
            rejected_duplicate_draft = Some(duplicate_draft_id);
        }
    }
    let rejected_duplicate_draft =
        rejected_duplicate_draft.expect("eleventh duplicate draft was attempted");
    let duplicate_evidence: (i64, String) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM mail_submission_intents
            WHERE organization_id = $1 AND requested_by = $2),
          (SELECT status FROM mail_drafts WHERE id = $3)
        "#,
    )
    .bind(bootstrap.organization_id.as_uuid())
    .bind(development_agent.principal_id.as_uuid())
    .bind(rejected_duplicate_draft.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("read duplicate circuit-breaker evidence");
    assert_eq!(duplicate_evidence, (10, "active".into()));
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_sessions_store_only_digests_consume_once_and_revoke_idempotently() {
    let _login_gate = LOGIN_GATE.lock().await;
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "human-session-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Human session tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("session-owner-{}", Uuid::new_v4().simple()),
            display_name: "Human Session Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind Human session owner");
    let service = HumanSessionService::new(store.clone());
    let raw_state = format!("state-{}", Uuid::new_v4().simple());
    let raw_browser_nonce = format!("nonce-{}", Uuid::new_v4().simple());
    let encrypted_verifier = vec![0x5a; 96];

    let login = service
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Admin,
            return_to: "/audit?tab=events".into(),
            state: raw_state.clone(),
            browser_nonce: raw_browser_nonce.clone(),
            encrypted_pkce_verifier: encrypted_verifier.clone(),
            admission_limit: 100_000,
            cleanup_limit: 100,
            now,
            expires_at: now + chrono::Duration::minutes(5),
        })
        .await
        .expect("begin Human login");
    let stored_login: (Vec<u8>, Vec<u8>, Vec<u8>, String, String) = sqlx::query_as(
        r#"
        SELECT state_sha256, browser_nonce_sha256, encrypted_pkce_verifier,
               client_key, return_to
          FROM human_login_transactions
         WHERE id = $1
        "#,
    )
    .bind(login.login_transaction_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("load digest-only login evidence");
    assert_eq!(
        stored_login.0,
        Sha256::digest(raw_state.as_bytes()).to_vec()
    );
    assert_eq!(
        stored_login.1,
        Sha256::digest(raw_browser_nonce.as_bytes()).to_vec()
    );
    assert_eq!(stored_login.2, encrypted_verifier);
    assert_eq!(stored_login.3, "admin");
    assert_eq!(stored_login.4, "/audit?tab=events");

    assert_eq!(
        service
            .consume_login(ConsumeHumanLoginCommand {
                state: raw_state.clone(),
                browser_nonce: format!("wrong-{}", Uuid::new_v4().simple()),
                now: now + chrono::Duration::seconds(1),
            })
            .await,
        Err(HumanSessionError::NotFound)
    );
    let consumed = service
        .consume_login(ConsumeHumanLoginCommand {
            state: raw_state.clone(),
            browser_nonce: raw_browser_nonce.clone(),
            now: now + chrono::Duration::seconds(1),
        })
        .await
        .expect("consume Human login once");
    assert_eq!(consumed.login_transaction_id, login.login_transaction_id);
    assert_eq!(
        service
            .consume_login(ConsumeHumanLoginCommand {
                state: raw_state,
                browser_nonce: raw_browser_nonce,
                now: now + chrono::Duration::seconds(2),
            })
            .await,
        Err(HumanSessionError::NotFound)
    );

    let session_now = pg_micros(now + chrono::Duration::seconds(2));
    let raw_session_token = format!(
        "session_{}_{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let raw_oidc_token = format!("header.{}.signature", Uuid::new_v4().simple());
    let session = service
        .create_session(CreateHumanSessionCommand {
            organization_id: bootstrap.organization_id,
            principal_id: owner.principal_id,
            login_transaction_id: consumed.login_transaction_id,
            client_key: consumed.client_key,
            session_token: raw_session_token.clone(),
            oidc_access_token: raw_oidc_token.clone(),
            oidc_jti: None,
            upstream_expires_at: session_now + chrono::Duration::hours(2),
            expires_at: session_now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now: session_now,
        })
        .await
        .expect("create Human browser session");
    let stored_session: (Vec<u8>, Vec<u8>, Option<String>, String) = sqlx::query_as(
        r#"
        SELECT token_sha256, oidc_token_sha256, oidc_jti, client_key
          FROM human_browser_sessions
         WHERE id = $1
        "#,
    )
    .bind(session.session_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("load digest-only session evidence");
    assert_eq!(
        stored_session.0,
        Sha256::digest(raw_session_token.as_bytes()).to_vec()
    );
    assert_eq!(
        stored_session.1,
        Sha256::digest(raw_oidc_token.as_bytes()).to_vec()
    );
    assert_eq!(stored_session.2, None);
    assert_eq!(stored_session.3, "admin");
    assert_eq!(
        service
            .resolve_session(ResolveHumanSessionCommand {
                session_token: raw_session_token.clone(),
                now: session_now + chrono::Duration::minutes(1),
            })
            .await
            .expect("resolve active Human session"),
        session
    );

    let revoked = service
        .revoke_session(RevokeHumanSessionCommand {
            session_token: raw_session_token.clone(),
            cause: HumanSessionRevocationCause::Logout,
            trace_id: Uuid::now_v7().to_string(),
            now: session_now + chrono::Duration::minutes(2),
        })
        .await
        .expect("revoke Human session");
    assert!(revoked.newly_revoked);
    let replay = service
        .revoke_session(RevokeHumanSessionCommand {
            session_token: raw_session_token.clone(),
            cause: HumanSessionRevocationCause::SecurityResponse,
            trace_id: Uuid::now_v7().to_string(),
            now: session_now + chrono::Duration::minutes(3),
        })
        .await
        .expect("replay Human session revocation");
    assert!(!replay.newly_revoked);
    assert_eq!(replay.cause, HumanSessionRevocationCause::Logout);
    assert_eq!(replay.revoked_at, revoked.revoked_at);
    assert_eq!(
        service
            .resolve_session(ResolveHumanSessionCommand {
                session_token: raw_session_token,
                now: session_now + chrono::Duration::minutes(4),
            })
            .await,
        Err(HumanSessionError::NotFound)
    );

    let evidence_counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM human_login_transaction_consumptions
            WHERE login_transaction_id = $1),
          (SELECT count(*) FROM human_browser_session_revocations
            WHERE session_id = $2),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $3
              AND action = 'identity.human_session.created'
              AND resource_id = $4),
          (SELECT count(*) FROM audit_events
            WHERE organization_id = $3
              AND action = 'identity.human_session.revoked'
              AND resource_id = $4),
          (SELECT count(*) FROM outbox_events
            WHERE aggregate_id = $4
              AND event_type IN (
                  'identity.human_session.created.v1',
                  'identity.human_session.revoked.v1'
              ))
        "#,
    )
    .bind(login.login_transaction_id.as_uuid())
    .bind(session.session_id.as_uuid())
    .bind(bootstrap.organization_id.as_uuid())
    .bind(session.session_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("load Human session lifecycle evidence");
    assert_eq!(evidence_counts, (1, 1, 1, 1, 2));

    assert_append_only_error(
        sqlx::query("UPDATE human_login_transactions SET return_to = return_to WHERE id = $1")
            .bind(login.login_transaction_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM human_login_transactions WHERE id = $1")
            .bind(login.login_transaction_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE human_login_transactions CASCADE")
            .execute(store.pool())
            .await,
    );

    assert_append_only_error(
        sqlx::query(
            "UPDATE human_login_transaction_consumptions SET consumed_at = consumed_at WHERE login_transaction_id = $1",
        )
        .bind(login.login_transaction_id.as_uuid())
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query(
            "DELETE FROM human_login_transaction_consumptions WHERE login_transaction_id = $1",
        )
        .bind(login.login_transaction_id.as_uuid())
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE human_login_transaction_consumptions")
            .execute(store.pool())
            .await,
    );

    assert_append_only_error(
        sqlx::query("UPDATE human_browser_sessions SET expires_at = expires_at WHERE id = $1")
            .bind(session.session_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM human_browser_sessions WHERE id = $1")
            .bind(session.session_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE human_browser_sessions CASCADE")
            .execute(store.pool())
            .await,
    );

    assert_append_only_error(
        sqlx::query(
            "UPDATE human_browser_session_revocations SET cause = cause WHERE session_id = $1",
        )
        .bind(session.session_id.as_uuid())
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM human_browser_session_revocations WHERE session_id = $1")
            .bind(session.session_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("TRUNCATE human_browser_session_revocations")
            .execute(store.pool())
            .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_session_database_constraints_fail_closed() {
    let _login_gate = LOGIN_GATE.lock().await;
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "human-session-constraints-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Human session constraint tenant");
    let other_bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "human-session-other-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap second tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("constraint-owner-{}", Uuid::new_v4().simple()),
            display_name: "Human Session Constraint Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind constraint test Human");
    let service = HumanSessionService::new(store.clone());
    let raw_state = format!("state-{}", Uuid::new_v4().simple());
    let raw_nonce = format!("nonce-{}", Uuid::new_v4().simple());
    let login = service
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Admin,
            return_to: "/".into(),
            state: raw_state.clone(),
            browser_nonce: raw_nonce.clone(),
            encrypted_pkce_verifier: vec![0x3c; 64],
            admission_limit: 100_000,
            cleanup_limit: 100,
            now,
            expires_at: now + chrono::Duration::minutes(5),
        })
        .await
        .expect("begin constraint login");
    service
        .consume_login(ConsumeHumanLoginCommand {
            state: raw_state,
            browser_nonce: raw_nonce,
            now: now + chrono::Duration::seconds(1),
        })
        .await
        .expect("consume constraint login");
    let created_at = now + chrono::Duration::seconds(2);

    assert_check_violation(
        insert_human_browser_session_fixture(
            &store,
            bootstrap.organization_id,
            bootstrap.root_principal_id,
            login.login_transaction_id,
            "admin",
            1,
            created_at,
            created_at + chrono::Duration::hours(2),
            created_at + chrono::Duration::hours(1),
        )
        .await,
    );
    assert_check_violation(
        insert_human_browser_session_fixture(
            &store,
            other_bootstrap.organization_id,
            owner.principal_id,
            login.login_transaction_id,
            "admin",
            2,
            created_at,
            created_at + chrono::Duration::hours(2),
            created_at + chrono::Duration::hours(1),
        )
        .await,
    );
    assert_check_violation(
        insert_human_browser_session_fixture(
            &store,
            bootstrap.organization_id,
            owner.principal_id,
            login.login_transaction_id,
            "mail",
            3,
            created_at,
            created_at + chrono::Duration::hours(2),
            created_at + chrono::Duration::hours(1),
        )
        .await,
    );

    let after_login_expiry = now + chrono::Duration::minutes(6);
    assert_check_violation(
        insert_human_browser_session_fixture(
            &store,
            bootstrap.organization_id,
            owner.principal_id,
            login.login_transaction_id,
            "admin",
            4,
            after_login_expiry,
            after_login_expiry + chrono::Duration::hours(2),
            after_login_expiry + chrono::Duration::hours(1),
        )
        .await,
    );
    assert_check_violation(
        insert_human_browser_session_fixture(
            &store,
            bootstrap.organization_id,
            owner.principal_id,
            login.login_transaction_id,
            "admin",
            5,
            created_at,
            created_at + chrono::Duration::hours(10),
            created_at + chrono::Duration::hours(9),
        )
        .await,
    );
    assert_check_violation(
        insert_human_browser_session_fixture(
            &store,
            bootstrap.organization_id,
            owner.principal_id,
            login.login_transaction_id,
            "admin",
            6,
            created_at,
            created_at + chrono::Duration::minutes(30),
            created_at + chrono::Duration::hours(1),
        )
        .await,
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_login_admission_is_cluster_wide_and_recovers_at_the_next_window() {
    let _login_gate = LOGIN_GATE.lock().await;
    let store = migrated_store().await;
    let database_second: f64 =
        sqlx::query_scalar("SELECT extract(second FROM clock_timestamp())::double precision")
            .fetch_one(store.pool())
            .await
            .expect("read database fixed-window clock");
    if database_second > 50.0 {
        tokio::time::sleep(std::time::Duration::from_secs(
            (61.0 - database_second).ceil() as u64,
        ))
        .await;
    }
    sqlx::query("DELETE FROM human_login_admission_windows WHERE client_key IN ('admin', 'mail')")
        .execute(store.pool())
        .await
        .expect("reset operational Human login admission windows");
    sqlx::query(
        r#"
        INSERT INTO human_login_admission_windows(
            client_key, window_started_at, admitted_count
        )
        VALUES ('admin', date_trunc('minute', statement_timestamp()), 2)
        "#,
    )
    .execute(store.pool())
    .await
    .expect("seed nearly full shared admission window");
    let before_logins: i64 = sqlx::query_scalar("SELECT count(*) FROM human_login_transactions")
        .fetch_one(store.pool())
        .await
        .expect("count Human logins before concurrent admission");
    let now = Utc::now();
    let first_service = HumanSessionService::new(store.clone());
    let second_service = HumanSessionService::new(store.clone());
    let first = first_service.begin_login(BeginHumanLoginCommand {
        client_key: HumanSessionClient::Admin,
        return_to: "/".into(),
        state: format!("state-{}", Uuid::new_v4().simple()),
        browser_nonce: format!("nonce-{}", Uuid::new_v4().simple()),
        encrypted_pkce_verifier: vec![0x41; 64],
        admission_limit: 3,
        cleanup_limit: 10,
        now,
        expires_at: now + chrono::Duration::minutes(5),
    });
    let second = second_service.begin_login(BeginHumanLoginCommand {
        client_key: HumanSessionClient::Admin,
        return_to: "/".into(),
        state: format!("state-{}", Uuid::new_v4().simple()),
        browser_nonce: format!("nonce-{}", Uuid::new_v4().simple()),
        encrypted_pkce_verifier: vec![0x42; 64],
        admission_limit: 3,
        cleanup_limit: 10,
        now,
        expires_at: now + chrono::Duration::minutes(5),
    });
    let (first, second) = expect_no_lock_hang(async { tokio::join!(first, second) }).await;
    let outcomes = [first, second];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(HumanSessionError::RateLimited {
                        retry_after_seconds: 60
                    })
                )
            })
            .count(),
        1
    );
    let after_logins: i64 = sqlx::query_scalar("SELECT count(*) FROM human_login_transactions")
        .fetch_one(store.pool())
        .await
        .expect("count Human logins after concurrent admission");
    assert_eq!(after_logins, before_logins + 1);
    let admin_count: i64 = sqlx::query_scalar(
        r#"
        SELECT admitted_count
          FROM human_login_admission_windows
         WHERE client_key = 'admin'
           AND window_started_at = date_trunc('minute', statement_timestamp())
        "#,
    )
    .fetch_one(store.pool())
    .await
    .expect("read shared Admin admission count");
    assert_eq!(admin_count, 3);

    HumanSessionService::new(store.clone())
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Mail,
            return_to: "/".into(),
            state: format!("state-{}", Uuid::new_v4().simple()),
            browser_nonce: format!("nonce-{}", Uuid::new_v4().simple()),
            encrypted_pkce_verifier: vec![0x43; 64],
            admission_limit: 1,
            cleanup_limit: 10,
            now,
            expires_at: now + chrono::Duration::minutes(5),
        })
        .await
        .expect("Mail has an independent cluster-wide window");

    sqlx::query("DELETE FROM human_login_admission_windows WHERE client_key = 'admin'")
        .execute(store.pool())
        .await
        .expect("replace Admin admission window");
    sqlx::query(
        r#"
        INSERT INTO human_login_admission_windows(
            client_key, window_started_at, admitted_count
        )
        VALUES (
            'admin',
            date_trunc('minute', statement_timestamp()) - interval '1 minute',
            3
        )
        "#,
    )
    .execute(store.pool())
    .await
    .expect("seed completed Admin admission window");
    HumanSessionService::new(store.clone())
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Admin,
            return_to: "/".into(),
            state: format!("state-{}", Uuid::new_v4().simple()),
            browser_nonce: format!("nonce-{}", Uuid::new_v4().simple()),
            encrypted_pkce_verifier: vec![0x44; 64],
            admission_limit: 1,
            cleanup_limit: 10,
            now,
            expires_at: now + chrono::Duration::minutes(5),
        })
        .await
        .expect("a completed fixed window no longer blocks admission");
    let recovered_count: i64 = sqlx::query_scalar(
        r#"
        SELECT admitted_count
          FROM human_login_admission_windows
         WHERE client_key = 'admin'
           AND window_started_at = date_trunc('minute', statement_timestamp())
        "#,
    )
    .fetch_one(store.pool())
    .await
    .expect("read recovered Admin admission window");
    assert_eq!(recovered_count, 1);
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn human_login_cleanup_is_bounded_and_preserves_live_or_session_evidence() {
    let _login_gate = LOGIN_GATE.lock().await;
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "human-login-cleanup-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap Human login cleanup tenant");
    let now = Utc::now();
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("cleanup-owner-{}", Uuid::new_v4().simple()),
            display_name: "Human Login Cleanup Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind cleanup test Human");

    let mut orphan_ids = Vec::new();
    for offset in 0_u8..3 {
        let login_id = copilotmail_domain::HumanLoginTransactionId::new();
        let created_at =
            now - chrono::Duration::days(365) + chrono::Duration::seconds(i64::from(offset));
        insert_human_login_evidence_fixture(
            &store,
            login_id,
            "admin",
            10 + offset,
            created_at,
            created_at + chrono::Duration::minutes(5),
            Some(created_at + chrono::Duration::minutes(1)),
        )
        .await;
        orphan_ids.push(login_id.as_uuid());
    }

    let live_login_id = copilotmail_domain::HumanLoginTransactionId::new();
    insert_human_login_evidence_fixture(
        &store,
        live_login_id,
        "admin",
        20,
        now,
        now + chrono::Duration::minutes(5),
        Some(now + chrono::Duration::seconds(1)),
    )
    .await;

    let linked_login_id = copilotmail_domain::HumanLoginTransactionId::new();
    let linked_login_created_at = now - chrono::Duration::minutes(30);
    insert_human_login_evidence_fixture(
        &store,
        linked_login_id,
        "admin",
        21,
        linked_login_created_at,
        linked_login_created_at + chrono::Duration::minutes(10),
        Some(linked_login_created_at + chrono::Duration::minutes(1)),
    )
    .await;
    let linked_session_id = Uuid::now_v7();
    insert_human_browser_session_fixture_with_id(
        &store,
        linked_session_id,
        bootstrap.organization_id,
        owner.principal_id,
        linked_login_id,
        "admin",
        50,
        linked_login_created_at + chrono::Duration::minutes(2),
        now + chrono::Duration::hours(2),
        now + chrono::Duration::hours(1),
    )
    .await
    .expect("insert session-linked expired login evidence");

    HumanSessionService::new(store.clone())
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Mail,
            return_to: "/".into(),
            state: format!("state-{}", Uuid::new_v4().simple()),
            browser_nonce: format!("nonce-{}", Uuid::new_v4().simple()),
            encrypted_pkce_verifier: vec![0x51; 64],
            admission_limit: 100_000,
            cleanup_limit: 2,
            now,
            expires_at: now + chrono::Duration::minutes(5),
        })
        .await
        .expect("admit login and prune a bounded expired batch");
    let remaining_orphans: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM human_login_transactions WHERE id = ANY($1)),
          (SELECT count(*) FROM human_login_transaction_consumptions
            WHERE login_transaction_id = ANY($1))
        "#,
    )
    .bind(orphan_ids.as_slice())
    .fetch_one(store.pool())
    .await
    .expect("count bounded cleanup remainder");
    assert_eq!(remaining_orphans, (1, 1));

    let protected_counts: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
          (SELECT count(*) FROM human_login_transactions WHERE id = $1),
          (SELECT count(*) FROM human_login_transaction_consumptions
            WHERE login_transaction_id = $1),
          (SELECT count(*) FROM human_login_transactions WHERE id = $2),
          (SELECT count(*) FROM human_login_transaction_consumptions
            WHERE login_transaction_id = $2),
          (SELECT count(*) FROM human_browser_sessions WHERE id = $3)
        "#,
    )
    .bind(live_login_id.as_uuid())
    .bind(linked_login_id.as_uuid())
    .bind(linked_session_id)
    .fetch_one(store.pool())
    .await
    .expect("load protected login evidence");
    assert_eq!(protected_counts, (1, 1, 1, 1, 1));

    assert_append_only_error(
        sqlx::query(
            "DELETE FROM human_login_transaction_consumptions WHERE login_transaction_id = $1",
        )
        .bind(live_login_id.as_uuid())
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM human_login_transactions WHERE id = $1")
            .bind(live_login_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query(
            "DELETE FROM human_login_transaction_consumptions WHERE login_transaction_id = $1",
        )
        .bind(linked_login_id.as_uuid())
        .execute(store.pool())
        .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM human_login_transactions WHERE id = $1")
            .bind(linked_login_id.as_uuid())
            .execute(store.pool())
            .await,
    );
    assert_append_only_error(
        sqlx::query("DELETE FROM human_browser_sessions WHERE id = $1")
            .bind(linked_session_id)
            .execute(store.pool())
            .await,
    );

    HumanSessionService::new(store.clone())
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Mail,
            return_to: "/".into(),
            state: format!("state-{}", Uuid::new_v4().simple()),
            browser_nonce: format!("nonce-{}", Uuid::new_v4().simple()),
            encrypted_pkce_verifier: vec![0x52; 64],
            admission_limit: 100_000,
            cleanup_limit: 1,
            now,
            expires_at: now + chrono::Duration::minutes(5),
        })
        .await
        .expect("prune the remaining eligible login");
    let remaining_orphans: i64 =
        sqlx::query_scalar("SELECT count(*) FROM human_login_transactions WHERE id = ANY($1)")
            .bind(orphan_ids.as_slice())
            .fetch_one(store.pool())
            .await
            .expect("verify all eligible login evidence was pruned");
    assert_eq!(remaining_orphans, 0);
}

#[allow(clippy::too_many_arguments)]
async fn insert_human_browser_session_fixture(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    login_transaction_id: copilotmail_domain::HumanLoginTransactionId,
    client_key: &str,
    digest_marker: u8,
    created_at: chrono::DateTime<Utc>,
    upstream_expires_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    insert_human_browser_session_fixture_with_id(
        store,
        Uuid::now_v7(),
        organization_id,
        principal_id,
        login_transaction_id,
        client_key,
        digest_marker,
        created_at,
        upstream_expires_at,
        expires_at,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_human_browser_session_fixture_with_id(
    store: &PgControlPlaneStore,
    session_id: Uuid,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    login_transaction_id: copilotmail_domain::HumanLoginTransactionId,
    client_key: &str,
    digest_marker: u8,
    created_at: chrono::DateTime<Utc>,
    upstream_expires_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO human_browser_sessions(
            id, organization_id, principal_id, login_transaction_id,
            client_key, token_sha256, oidc_token_sha256, oidc_jti,
            upstream_expires_at, created_at, expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, NULL, $8, $9, $10)
        "#,
    )
    .bind(session_id)
    .bind(organization_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(login_transaction_id.as_uuid())
    .bind(client_key)
    .bind(vec![digest_marker; 32])
    .bind(vec![digest_marker.saturating_add(100); 32])
    .bind(upstream_expires_at)
    .bind(created_at)
    .bind(expires_at)
    .execute(store.pool())
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_human_login_evidence_fixture(
    store: &PgControlPlaneStore,
    login_transaction_id: copilotmail_domain::HumanLoginTransactionId,
    client_key: &str,
    digest_marker: u8,
    created_at: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
    consumed_at: Option<chrono::DateTime<Utc>>,
) {
    sqlx::query(
        r#"
        INSERT INTO human_login_transactions(
            id, client_key, return_to, state_sha256, browser_nonce_sha256,
            encrypted_pkce_verifier, created_at, expires_at
        )
        VALUES ($1, $2, '/', $3, $4, $5, $6, $7)
        "#,
    )
    .bind(login_transaction_id.as_uuid())
    .bind(client_key)
    .bind(vec![digest_marker; 32])
    .bind(vec![digest_marker.saturating_add(100); 32])
    .bind(vec![digest_marker; 64])
    .bind(created_at)
    .bind(expires_at)
    .execute(store.pool())
    .await
    .expect("insert Human login fixture");
    if let Some(consumed_at) = consumed_at {
        sqlx::query(
            r#"
            INSERT INTO human_login_transaction_consumptions(
                login_transaction_id, consumed_at
            )
            VALUES ($1, $2)
            "#,
        )
        .bind(login_transaction_id.as_uuid())
        .bind(consumed_at)
        .execute(store.pool())
        .await
        .expect("insert Human login consumption fixture");
    }
}

async fn insert_approval_test_owner(
    store: &PgControlPlaneStore,
    organization_id: copilotmail_domain::OrganizationId,
    created_by: PrincipalId,
    now: chrono::DateTime<Utc>,
) -> PrincipalId {
    let principal_id = PrincipalId::new();
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        )
        VALUES ($1, $2, 'human', 'active', 'Approval Test Owner', $3, $3)
        "#,
    )
    .bind(principal_id.as_uuid())
    .bind(organization_id.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert approval test Principal");
    sqlx::query(
        r#"
        INSERT INTO role_assignments(
            id, organization_id, principal_id, role_id,
            created_by, valid_from, created_at
        )
        SELECT $1, $2, $3, role.id, $4, $5, $5
          FROM roles AS role
         WHERE role.organization_id = $2
           AND role.system_key = 'org_owner'
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(organization_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(created_by.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("assign approval test owner role");
    principal_id
}

fn assert_append_only_error(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) {
    let error = result.expect_err("audit mutation must fail");
    // 55000 is the append-only trigger rejection; 0A000 is PostgreSQL's own
    // refusal to truncate a table referenced by foreign keys. Both preserve
    // the invariant under test.
    let code = error.as_database_error().and_then(|error| error.code());
    assert!(
        matches!(code.as_deref(), Some("55000") | Some("0A000")),
        "append-only mutation must be rejected, got {code:?}"
    );
}

fn assert_check_violation(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) {
    let error = result.expect_err("invalid Human session evidence must fail closed");
    assert_eq!(
        error.as_database_error().and_then(|error| error.code()),
        Some(std::borrow::Cow::Borrowed("23514"))
    );
}

fn test_public_key() -> String {
    RsaPrivateKey::generate_for_testing()
        .unwrap()
        .public_key()
        .spki_pem()
        .unwrap()
}

async fn create_fenced_human_session(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    now: chrono::DateTime<Utc>,
) -> (HumanSessionId, String) {
    let service = HumanSessionService::new(store.clone());
    let raw_state = format!("fence-state-{}", Uuid::new_v4().simple());
    let raw_nonce = format!("fence-nonce-{}", Uuid::new_v4().simple());
    let _login = service
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Admin,
            return_to: "/".into(),
            state: raw_state.clone(),
            browser_nonce: raw_nonce.clone(),
            encrypted_pkce_verifier: vec![0x5a; 96],
            admission_limit: 100_000,
            cleanup_limit: 100,
            now: Utc::now(),
            expires_at: now + chrono::Duration::minutes(5),
        })
        .await
        .expect("begin fencing login");
    let consumed = service
        .consume_login(ConsumeHumanLoginCommand {
            state: raw_state,
            browser_nonce: raw_nonce,
            now: Utc::now(),
        })
        .await
        .expect("consume fencing login");
    let token = format!("fence-session-{}", Uuid::new_v4().simple());
    let session = service
        .create_session(CreateHumanSessionCommand {
            organization_id,
            principal_id,
            login_transaction_id: consumed.login_transaction_id,
            client_key: consumed.client_key,
            session_token: token.clone(),
            oidc_access_token: format!("header.{}.signature", Uuid::new_v4().simple()),
            oidc_jti: None,
            upstream_expires_at: now + chrono::Duration::hours(2),
            expires_at: now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("create fencing session");
    (session.session_id, token)
}

fn fenced_context(
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    authentication: AuthenticationBinding,
) -> AuthorizationContext {
    AuthorizationContext {
        principal_id,
        organization_id,
        permissions: Permission::ORG_OWNER.into_iter().collect(),
        authentication: Some(authentication),
    }
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn request_authentication_fences_human_session_revocation() {
    let _login_gate = LOGIN_GATE.lock().await;
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "fence-session-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap fencing tenant");
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("fence-owner-{}", Uuid::new_v4().simple()),
            display_name: "Fencing Session Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind fencing owner")
        .principal_id;
    let (session_id, session_token) =
        create_fenced_human_session(&store, bootstrap.organization_id, owner, Utc::now()).await;
    let context = fenced_context(
        bootstrap.organization_id,
        owner,
        AuthenticationBinding::HumanSession { session_id },
    );

    store
        .begin_authorized(&context)
        .await
        .expect("live session binds")
        .commit()
        .await
        .expect("live session commits");

    let transaction = store
        .begin_authorized(&context)
        .await
        .expect("bind before revocation");
    expect_no_lock_hang(HumanSessionService::new(store.clone()).revoke_session(
        RevokeHumanSessionCommand {
            session_token,
            cause: HumanSessionRevocationCause::SecurityResponse,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        },
    ))
    .await
    .expect("revocation commits while the session row is only share-locked");
    assert!(
        matches!(
            expect_no_lock_hang(transaction.commit()).await,
            Err(StoreError::AuthenticationInvalid)
        ),
        "a revocation that commits first must fail the open request at commit"
    );
    assert!(
        matches!(
            expect_no_lock_hang(store.begin_authorized(&context)).await,
            Err(StoreError::AuthenticationInvalid)
        ),
        "revoked session must fail at the next bind"
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn request_authentication_rejects_suspended_principal_at_commit() {
    let _login_gate = LOGIN_GATE.lock().await;
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "fence-suspend-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap suspension fence tenant");
    let owner = BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("fence-suspended-{}", Uuid::new_v4().simple()),
            display_name: "Suspended Fence Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind suspended fence owner")
        .principal_id;
    let (session_id, _session_token) =
        create_fenced_human_session(&store, bootstrap.organization_id, owner, Utc::now()).await;
    let context = fenced_context(
        bootstrap.organization_id,
        owner,
        AuthenticationBinding::HumanSession { session_id },
    );
    let transaction = store
        .begin_authorized(&context)
        .await
        .expect("bind before principal suspension");
    expect_no_lock_hang(
        sqlx::query("UPDATE principals SET status = 'suspended' WHERE id = $1")
            .bind(owner.as_uuid())
            .execute(store.pool()),
    )
    .await
    .expect("suspend principal without a session revocation");
    assert!(
        matches!(
            expect_no_lock_hang(transaction.commit()).await,
            Err(StoreError::AuthenticationInvalid)
        ),
        "a committed principal suspension must fail the open request"
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn request_authentication_fences_agent_credential_lifecycle() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "fence-agent-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap fencing tenant");
    BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("owner-{}", Uuid::new_v4().simple()),
            display_name: "Fence Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind fencing owner");
    let public_key = test_public_key();
    let now = Utc::now();
    let registered = RegisterAgentService::new(store.clone())
        .execute(RegisterAgentCommand {
            organization_id: bootstrap.organization_id,
            display_name: "Fencing Agent".into(),
            environment: AgentEnvironment::Testing,
            risk_level: AgentRiskLevel::Low,
            purpose: "Validate credential fencing".into(),
            review_due_at: now + chrono::Duration::days(90),
            public_key_pem: public_key.clone(),
            public_key_fingerprint: rsa_public_key_fingerprint(public_key.as_bytes()).unwrap(),
            allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
            credential_expires_at: now + chrono::Duration::days(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("register fencing Agent");

    let live = fenced_context(
        bootstrap.organization_id,
        registered.principal_id,
        AuthenticationBinding::AgentCredential {
            credential_id: registered.credential_id,
            expires_at: now + chrono::Duration::minutes(30),
            scopes: Permission::AGENT_TOKEN.into_iter().collect(),
        },
    );
    store
        .begin_authorized(&live)
        .await
        .expect("live credential binds")
        .commit()
        .await
        .expect("live credential commits");

    let expired_token = fenced_context(
        bootstrap.organization_id,
        registered.principal_id,
        AuthenticationBinding::AgentCredential {
            credential_id: registered.credential_id,
            expires_at: now - chrono::Duration::seconds(1),
            scopes: Permission::AGENT_TOKEN.into_iter().collect(),
        },
    );
    assert!(
        matches!(
            store.begin_authorized(&expired_token).await,
            Err(copilotmail_application::StoreError::AuthenticationInvalid)
        ),
        "expired access token must fail at bind"
    );

    RevokeAgentCredentialService::new(store.clone())
        .execute(RevokeAgentCredentialCommand {
            organization_id: bootstrap.organization_id,
            credential_id: registered.credential_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("revoke fencing credential");
    assert!(
        matches!(
            store.begin_authorized(&live).await,
            Err(copilotmail_application::StoreError::AuthenticationInvalid)
        ),
        "revoked credential must fail at bind"
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn request_authentication_fences_scim_token_revocation() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "fence-scim-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap fencing tenant");
    BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id: bootstrap.organization_id,
            issuer: format!("https://login-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("fence-owner-{}", Uuid::new_v4().simple()),
            display_name: "Fencing SCIM Owner".into(),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("bind fencing owner");
    let plaintext = format!("fence-scim-{}", Uuid::new_v4());
    let token_hash = Sha256::digest(plaintext.as_bytes()).to_vec();
    let credential = IssueScimTokenService::new(store.clone())
        .execute(IssueScimTokenCommand {
            organization_id: bootstrap.organization_id,
            token_id: ScimTokenId::new(),
            token_hash: token_hash.clone(),
            label: "Fencing token".into(),
            expires_at: Some(pg_micros(Utc::now()) + chrono::Duration::hours(1)),
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("issue fencing SCIM token");

    let context = fenced_context(
        bootstrap.organization_id,
        credential.actor_principal_id,
        AuthenticationBinding::ScimToken {
            token_id: credential.token_id,
        },
    );
    store
        .begin_authorized(&context)
        .await
        .expect("live SCIM token binds")
        .commit()
        .await
        .expect("live SCIM token commits");

    RevokeScimTokenService::new(store.clone())
        .execute(RevokeScimTokenCommand {
            organization_id: bootstrap.organization_id,
            token_id: credential.token_id,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .expect("revoke fencing SCIM token");
    assert!(
        matches!(
            store.begin_authorized(&context).await,
            Err(copilotmail_application::StoreError::AuthenticationInvalid)
        ),
        "revoked SCIM token must fail at bind"
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn request_authentication_fences_bearer_expiry_at_commit() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "fence-bearer-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap fencing tenant");
    let owner = bootstrap.root_principal_id;
    let now = Utc::now();

    let expired = fenced_context(
        bootstrap.organization_id,
        owner,
        AuthenticationBinding::HumanBearer {
            expires_at: now - chrono::Duration::hours(1),
        },
    );
    assert!(
        matches!(
            store.begin_authorized(&expired).await,
            Err(copilotmail_application::StoreError::AuthenticationInvalid)
        ),
        "expired bearer token must fail at bind"
    );

    let database_now: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(store.pool())
        .await
        .expect("read database clock");
    let bearer_expires_at = database_now + chrono::Duration::seconds(2);
    let expiring = fenced_context(
        bootstrap.organization_id,
        owner,
        AuthenticationBinding::HumanBearer {
            expires_at: bearer_expires_at,
        },
    );
    let transaction = store
        .begin_authorized(&expiring)
        .await
        .expect("live bearer token binds");
    for _ in 0..30 {
        let database_now: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT now()")
            .fetch_one(store.pool())
            .await
            .expect("read database clock");
        if database_now > bearer_expires_at + chrono::Duration::seconds(1) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    assert!(
        matches!(
            transaction.commit().await,
            Err(copilotmail_application::StoreError::AuthenticationInvalid)
        ),
        "commit must reject a bearer token that expired mid-transaction"
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn begin_authorized_without_binding_matches_plain_begin() {
    let store = migrated_store().await;
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(bootstrap_command(format!(
            "fence-none-{}",
            Uuid::new_v4().simple()
        )))
        .await
        .expect("bootstrap fencing tenant");
    let context = AuthorizationContext {
        principal_id: bootstrap.root_principal_id,
        organization_id: bootstrap.organization_id,
        permissions: Permission::ORG_OWNER.into_iter().collect(),
        authentication: None,
    };
    store
        .begin_authorized(&context)
        .await
        .expect("unauthenticated local work binds")
        .commit()
        .await
        .expect("unauthenticated local work commits");
}
