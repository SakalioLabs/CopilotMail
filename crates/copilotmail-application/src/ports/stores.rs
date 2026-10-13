use super::*;

/// Resolves effective permissions only for active principals and assignments.
#[async_trait]
pub trait AuthorizationStore: Send + Sync {
    /// Resolves the authenticated principal's tenant-scoped permission snapshot.
    async fn resolve_authorization(
        &self,
        principal_id: PrincipalId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<AuthorizationContext>, StoreError>;
}

/// Short-lived, pre-authentication OIDC login transaction storage.
///
/// Both operations are single-statement atomic adapter boundaries because no
/// authenticated tenant actor exists yet. Session creation after the callback
/// uses the regular audited control-plane transaction.
#[async_trait]
pub trait HumanLoginTransactionStore: Send + Sync {
    /// Applies one database-authoritative fixed-window admission for
    /// `write.client_key`, prunes at most `cleanup_limit` eligible expired
    /// transactions, and persists the new digest-only login evidence in one
    /// transaction. A rejected attempt must not insert a login transaction.
    async fn admit_human_login_transaction(
        &self,
        write: &HumanLoginTransactionWrite,
        admission_limit: u32,
        cleanup_limit: u32,
    ) -> Result<HumanLoginAdmissionResult, StoreError>;

    /// Atomically consumes one unexpired transaction matching both browser
    /// correlation digests. A transaction can be consumed only once.
    async fn consume_human_login_transaction(
        &self,
        state_sha256: &[u8; 32],
        browser_nonce_sha256: &[u8; 32],
        consumed_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<HumanLoginTransactionRecord>, StoreError>;
}

/// Resolves an active Human browser session from a token digest.
///
/// Implementations must re-check session expiry, revocation, tenant binding,
/// and current Human Principal activity on every lookup.
#[async_trait]
pub trait HumanSessionQueryStore: Send + Sync {
    async fn resolve_human_session_by_token_sha256(
        &self,
        token_sha256: &[u8; 32],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<HumanSessionRecord>, StoreError>;

    /// Lists the still-active browser sessions of one Human, newest first.
    ///
    /// Rows are returned only while the principal remains an active Human in
    /// the same tenant; expired, revoked, and cross-principal sessions must
    /// never appear. The result is the self-service device view: identifiers,
    /// client boundary, and lifecycle times only.
    async fn list_human_sessions_for_principal(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<HumanSessionListItem>, StoreError> {
        Ok(Vec::new())
    }
}

/// Append-only anonymous login-failure ledger for platform operations.
///
/// Rows carry no tenant identity and no reversible source identifier, so the
/// write path deliberately runs without an authenticated transaction. Every
/// append also prunes a bounded number of entries past the retention horizon
/// in the same transaction.
#[async_trait]
pub trait HumanLoginFailureStore: Send + Sync {
    /// Appends one failure row stamped by the database clock and prunes at
    /// most `cleanup_limit` expired rows atomically.
    async fn record_human_login_failure(
        &self,
        write: &HumanLoginFailureWrite,
        cleanup_limit: u32,
    ) -> Result<(), StoreError>;

    /// Platform-operations-only read. Results are bounded, ordered by the
    /// database clock newest first, and never expose tenant data.
    async fn list_human_login_failures(
        &self,
        query: &HumanLoginFailureQuery,
    ) -> Result<Vec<HumanLoginFailureRecord>, StoreError>;
}

/// Resolves resource-level mailbox grants for an authenticated principal.
#[async_trait]
pub trait MailboxAuthorizationStore: Send + Sync {
    /// Allows active Humans, non-production Agents, and production Agents with
    /// a committed active policy candidate. Per-message production admission
    /// remains a separate transactional decision.
    async fn principal_can_submit_mail(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Returns the exact latest active policy only when the principal is an
    /// active production Agent in this tenant.
    async fn active_production_agent_mail_policy(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyVersion>, StoreError> {
        Ok(None)
    }

    /// Returns the environment only for an active Agent in the exact tenant.
    async fn active_agent_environment(
        &self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<Option<AgentEnvironment>, StoreError> {
        Ok(None)
    }

    /// Returns only grants that are active at `now`; missing and inaccessible
    /// mailboxes deliberately share the same `None` representation.
    async fn resolve_mailbox_access(
        &self,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MailboxAccessSnapshot>, StoreError>;
}

/// Reads the Human mailbox switcher directory from one consistent snapshot.
#[async_trait]
pub trait HumanMailboxDirectoryStore: Send + Sync {
    /// Returns only same-tenant mailboxes with a current data-plane binding and
    /// a SQL-level `Read(None)` grant. The application layer intentionally
    /// repeats the pure authorization decision before returning any item.
    async fn list_human_mailbox_directory(
        &self,
        query: &HumanMailboxDirectoryQuery,
    ) -> Result<HumanMailboxDirectoryBatch, StoreError>;
}

/// Reads the authenticated Agent's own outbound policy and aggregate usage.
#[async_trait]
pub trait AgentMailPolicyQueryStore: Send + Sync {
    async fn get_agent_mail_policy_runtime(
        &self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<Option<AgentMailPolicyRuntimeSnapshot>, StoreError>;
}

#[async_trait]
pub trait AgentProcessingQueryStore: Send + Sync {
    async fn get_agent_processing_subscription(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        subscription_id: SubscriptionId,
    ) -> Result<Option<AgentProcessingSubscriptionRecord>, StoreError>;

    async fn list_agent_processing_dead_letters(
        &self,
        query: &AgentProcessingDeadLetterQuery,
    ) -> Result<AgentProcessingDeadLetterBatch, StoreError>;
}

#[async_trait]
pub trait MailboxQueryStore: Send + Sync {
    async fn get_mailbox(
        &self,
        mailbox_id: MailboxId,
    ) -> Result<Option<MailboxSummary>, StoreError>;

    /// Resolves only the exact tenant mailbox binding used for outbound
    /// content access. Missing and cross-tenant rows share `None`.
    async fn get_mail_engine_mailbox_binding(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError>;
}

#[async_trait]
pub trait MailDraftMetadataStore: Send + Sync {
    async fn get_mail_draft(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        created_by: PrincipalId,
        draft_id: DraftId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDraftRecord>, StoreError>;

    /// Inserts a first remote version or replaces an active version using CAS.
    async fn record_mail_draft(&self, write: MailDraftWrite)
    -> Result<MailDraftRecord, StoreError>;
}

/// Database-authoritative queries for an active Human's private draft view.
///
/// Shared mailbox membership is not draft collaboration authority: both
/// methods must scope rows to `created_by == principal_id`. Missing, denied,
/// cross-tenant, stale-binding, and other-creator targets must not leak a row.
#[async_trait]
pub trait HumanDraftQueryStore: Send + Sync {
    async fn list_human_drafts(
        &self,
        query: &HumanDraftListQuery,
    ) -> Result<HumanDraftListBatch, StoreError>;

    async fn resolve_human_draft_restore_target(
        &self,
        query: &HumanDraftRestoreQuery,
    ) -> Result<HumanDraftRestoreResolution, StoreError>;
}

/// Database-authoritative queries for an active Human's private accepted-
/// submission directory. Shared mailbox membership is never permission to
/// browse another sender's history; rows must be scoped to
/// `requested_by == principal_id` and must not load message content or
/// recipient evidence.
#[async_trait]
pub trait HumanSentQueryStore: Send + Sync {
    async fn list_human_sent_submissions(
        &self,
        query: &HumanSentListQuery,
    ) -> Result<HumanSentListBatch, StoreError>;
}

#[async_trait]
pub trait MailSubmissionQueryStore: Send + Sync {
    /// Human-facing lookup. Missing, cross-tenant, and other-actor intents are
    /// deliberately indistinguishable.
    async fn get_mail_submission_intent(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError>;

    /// Internal worker lookup used only to distinguish an already-terminal
    /// Intent from a currently leased Intent after Outbox redelivery.
    async fn get_mail_submission_intent_for_execution(
        &self,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailSubmissionIntentRecord>, StoreError>;
}

#[async_trait]
pub trait MailDeliveryQueryStore: Send + Sync {
    /// Internal worker lookup. Only accepted submissions with complete remote
    /// bindings are eligible for reconciliation.
    async fn get_mail_delivery_reconciliation_target(
        &self,
        submission_id: MailSubmissionId,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailDeliveryReconciliationTarget>, StoreError>;

    /// Human-facing latest snapshot lookup with tenant/requester isolation.
    async fn get_latest_mail_delivery_snapshot(
        &self,
        organization_id: OrganizationId,
        requested_by: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<MailDeliverySnapshotRecord>, StoreError>;
}

#[async_trait]
pub trait MailDomainQueryStore: Send + Sync {
    async fn get_mail_domain(
        &self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MailDomainSummary>, StoreError>;

    /// Lists domains after an exclusive canonical-name cursor.
    async fn list_mail_domains(
        &self,
        organization_id: OrganizationId,
        after_name: Option<&MailDomainName>,
        limit: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<MailDomainSummary>, StoreError>;
}

#[async_trait]
pub trait WebhookSubscriptionQueryStore: Send + Sync {
    async fn get_webhook_subscription(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscriptionSummary>, StoreError>;

    async fn list_webhook_subscriptions(
        &self,
        organization_id: OrganizationId,
        principal_id: Option<PrincipalId>,
        after_id: Option<WebhookSubscriptionId>,
        limit: i64,
    ) -> Result<Vec<WebhookSubscriptionSummary>, StoreError>;
}

#[async_trait]
pub trait WebhookDeadLetterQueryStore: Send + Sync {
    async fn list_webhook_dead_letters(
        &self,
        organization_id: OrganizationId,
        after_delivery_id: Option<uuid::Uuid>,
        limit: i64,
    ) -> Result<Vec<WebhookDeadLetterSummary>, StoreError>;
}

#[async_trait]
pub trait AgentEventCursorQueryStore: Send + Sync {
    /// Proves that a cursor anchor is an eligible event for the exact
    /// subscription. Missing anchors are rejected rather than treated as an
    /// arbitrary ordering key.
    async fn webhook_cursor_event_exists(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        event_id: uuid::Uuid,
    ) -> Result<bool, StoreError>;

    async fn list_webhook_cursor_events(
        &self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        principal_id: PrincipalId,
        after_event_id: Option<uuid::Uuid>,
        limit: i64,
    ) -> Result<Vec<AgentCursorEvent>, StoreError>;

    async fn release_agent_event_stream_lease(
        &self,
        _lease: &AgentEventStreamLease,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }
}

/// Cluster-wide admission result for one authenticated Agent Cursor/SSE
/// request. Retry delays are derived from the database clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentIngressRateLimitDecision {
    Allowed,
    TenantLimited { retry_after_seconds: u32 },
    PrincipalLimited { retry_after_seconds: u32 },
}

#[async_trait]
pub trait AgentIngressRateLimitStore: Send + Sync {
    /// Atomically consumes both tenant and Agent capacity. A principal
    /// rejection must not consume tenant capacity.
    async fn consume_agent_ingress_capacity(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        tenant_requests_per_minute: u32,
        principal_requests_per_minute: u32,
    ) -> Result<AgentIngressRateLimitDecision, StoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMailContentTarget {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub message_id: uuid::Uuid,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_document_ref: String,
}

/// One existing inbound message projected from an engine document identifier
/// to the externally safe CopilotMail UUID.
///
/// Batch callers must require exactly one unique projection for every unique
/// requested document reference. Missing or duplicate rows are deliberately
/// returned for the application layer to reject fail closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailMessageProjection {
    pub message_id: uuid::Uuid,
    pub engine_document_ref: String,
}

/// Exact current engine target resolved from an externally safe Human-mail
/// message UUID. Authorization remains an application-layer responsibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanMailContentTarget {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub message_id: uuid::Uuid,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_document_ref: String,
}

#[async_trait]
pub trait HumanMailContentQueryStore: Send + Sync {
    /// Resolves existing inbound engine document references in caller order.
    ///
    /// Implementations must join the current mailbox binding and must never
    /// create an identifier for an unprojected document. Missing inputs
    /// produce no row and duplicate inputs remain duplicate rows so callers
    /// can reject either condition without disclosing a partial page.
    async fn resolve_human_mail_message_projections(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        expected_data_plane_id: MailDataPlaneId,
        engine_account_ref: &str,
        document_refs: &[String],
    ) -> Result<Vec<HumanMailMessageProjection>, StoreError>;

    /// Resolves one safe UUID only when its immutable inbound event still
    /// matches the exact tenant, mailbox, and current engine binding.
    async fn resolve_human_mail_content_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: uuid::Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<HumanMailContentTarget>, StoreError>;
}

/// Exact current engine target resolved from an externally safe inbound
/// message UUID. Authorization remains an application-layer responsibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailReplyTarget {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub message_id: uuid::Uuid,
    pub data_plane_id: MailDataPlaneId,
    pub engine_account_ref: String,
    pub engine_document_ref: String,
}

#[async_trait]
pub trait MailReplyTargetQueryStore: Send + Sync {
    async fn resolve_mail_reply_target(
        &self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        message_id: uuid::Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<MailReplyTarget>, StoreError>;

    /// Maps an already-authorized Human inbox engine document back to its
    /// externally safe inbound UUID. Implementations must enforce the current
    /// mailbox binding and exact tenant/mailbox tuple.
    async fn resolve_mail_reply_message_id_by_engine_document(
        &self,
        _organization_id: OrganizationId,
        _mailbox_id: MailboxId,
        _expected_data_plane_id: MailDataPlaneId,
        _engine_document_ref: &str,
    ) -> Result<Option<uuid::Uuid>, StoreError> {
        Err(StoreError::Unavailable)
    }
}

#[async_trait]
pub trait AgentMailContentQueryStore: Send + Sync {
    /// Resolves an externally safe CopilotMail message UUID to one exact
    /// current engine document only for an active Agent.
    async fn resolve_agent_mail_content_target(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        mailbox_id: MailboxId,
        message_id: uuid::Uuid,
        expected_data_plane_id: MailDataPlaneId,
    ) -> Result<Option<AgentMailContentTarget>, StoreError>;
}

/// Read-side proof that an approval is exact, complete, and unexpired.
#[async_trait]
pub trait ApprovalQueryStore: Send + Sync {
    async fn approval_permits(&self, proof: &ApprovalProof) -> Result<bool, StoreError>;

    async fn approved_request_evidence(
        &self,
        _proof: &ApprovalProof,
    ) -> Result<Option<ApprovedRequestEvidence>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn get_approval_request(
        &self,
        _organization_id: OrganizationId,
        _request_id: ApprovalRequestId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ApprovalRequestDetail>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn approval_request_cursor_exists(
        &self,
        _organization_id: OrganizationId,
        _view: ApprovalRequestView,
        _actor: PrincipalId,
        _status: Option<ApprovalStatus>,
        _request_id: ApprovalRequestId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn list_approval_requests(
        &self,
        _query: ApprovalRequestListQuery,
    ) -> Result<Vec<ApprovalRequestSummary>, StoreError> {
        Err(StoreError::Unavailable)
    }
}

#[async_trait]
pub trait RoleGovernanceQueryStore: Send + Sync {
    async fn list_roles(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Vec<RoleSummary>, StoreError>;

    async fn get_role_assignment(
        &self,
        organization_id: OrganizationId,
        assignment_id: RoleAssignmentId,
    ) -> Result<Option<RoleAssignmentSummary>, StoreError>;

    async fn role_assignment_cursor_exists(
        &self,
        query: RoleAssignmentListQuery,
        assignment_id: RoleAssignmentId,
    ) -> Result<bool, StoreError>;

    async fn list_role_assignments(
        &self,
        query: RoleAssignmentListQuery,
    ) -> Result<Vec<RoleAssignmentSummary>, StoreError>;
}

#[async_trait]
pub trait AuditExportQueryStore: Send + Sync {
    async fn get_audit_export_grant(
        &self,
        organization_id: OrganizationId,
        grant_id: AuditExportId,
    ) -> Result<Option<AuditExportGrant>, StoreError>;

    async fn get_audit_export_grant_by_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: ApprovalRequestId,
    ) -> Result<Option<AuditExportGrant>, StoreError>;

    /// Returns whether the grant is inside its active window as measured by the
    /// database clock. `audit_export_grants.created_at` is stamped with
    /// `clock_timestamp()` by the insert trigger, so caller clocks must not be
    /// compared against the persisted bounds directly.
    async fn audit_export_grant_is_active(
        &self,
        organization_id: OrganizationId,
        grant_id: AuditExportId,
    ) -> Result<bool, StoreError>;
}

#[async_trait]
pub trait RetentionPolicyQueryStore: Send + Sync {
    async fn get_retention_policy(
        &self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError>;
}

/// Tenant-scoped read model used by Legal Hold orchestration before remote
/// object-version operations begin.
#[async_trait]
pub trait ComplianceQueryStore: Send + Sync {
    async fn active_human_compliance_actor_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError>;

    async fn get_compliance_case(
        &self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError>;

    async fn compliance_case_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::ComplianceCaseStatus>,
        case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError>;

    async fn list_compliance_cases(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::ComplianceCaseStatus>,
        after_case_id: Option<ComplianceCaseId>,
        limit: i64,
    ) -> Result<Vec<ComplianceCase>, StoreError>;

    async fn get_audit_archive(
        &self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError>;

    async fn get_legal_hold(
        &self,
        organization_id: OrganizationId,
        hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError>;

    async fn legal_hold_cursor_exists(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        hold_id: LegalHoldId,
    ) -> Result<bool, StoreError>;

    async fn list_legal_holds(
        &self,
        organization_id: OrganizationId,
        status: Option<copilotmail_domain::LegalHoldStatus>,
        case_id: Option<ComplianceCaseId>,
        after_hold_id: Option<LegalHoldId>,
        limit: i64,
    ) -> Result<Vec<LegalHold>, StoreError>;

    async fn get_legal_hold_by_apply_approval(
        &self,
        organization_id: OrganizationId,
        approval_request_id: ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError>;
}

/// Produces a fully reverified chain-head candidate for checkpoint signing.
#[async_trait]
pub trait AuditCheckpointQueryStore: Send + Sync {
    async fn audit_checkpoint_candidate(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<AuditCheckpointCandidate>, StoreError>;
}

#[async_trait]
pub trait AuditEventQueryStore: Send + Sync {
    /// Cheap pre-query guard. The Human status is checked again in the audit
    /// write transaction before any result is returned.
    async fn active_human_audit_reader_exists(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError>;

    /// Rejects fabricated ordering positions by proving the cursor anchor
    /// matches the exact tenant and active filters.
    async fn audit_event_search_anchor_exists(
        &self,
        search: &AuditEventSearch,
        chain_sequence: i64,
    ) -> Result<bool, StoreError>;

    async fn search_audit_events(
        &self,
        search: &AuditEventSearch,
    ) -> Result<Vec<AuditEventSummary>, StoreError>;
}

/// Read-side organization projection.
#[async_trait]
pub trait OrganizationQueryStore: Send + Sync {
    /// Loads an organization by stable ID.
    async fn get_organization(
        &self,
        organization_id: OrganizationId,
    ) -> Result<Option<OrganizationSummary>, StoreError>;
}

/// Maps a verified enterprise identity to an internal human principal.
#[async_trait]
pub trait ExternalIdentityResolver: Send + Sync {
    /// Resolves an exact issuer and subject pair for an active human principal.
    async fn resolve_human_identity(
        &self,
        issuer: &str,
        subject: &str,
    ) -> Result<Option<PrincipalId>, StoreError>;
}

/// Authenticates SCIM credentials and serves tenant-isolated directory projections.
#[async_trait]
pub trait ScimDirectoryQueryStore: Send + Sync {
    /// Resolves an active, non-expired credential by a fixed-length token digest.
    async fn authenticate_scim_token(
        &self,
        token_hash: &[u8],
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ScimCredential>, StoreError>;

    /// Loads one SCIM user, scoped to the credential's organization.
    async fn get_scim_user(
        &self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<Option<ExistingScimUser>, StoreError>;

    /// Lists SCIM users with bounded offset pagination and optional exact filters.
    async fn list_scim_users(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        user_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimUser>, i64), StoreError>;

    async fn get_scim_group(
        &self,
        organization_id: OrganizationId,
        group_id: ScimGroupId,
    ) -> Result<Option<ExistingScimGroup>, StoreError>;

    async fn list_scim_groups(
        &self,
        organization_id: OrganizationId,
        external_id: Option<&str>,
        display_name: Option<&str>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<ExistingScimGroup>, i64), StoreError>;
}

/// Public-key Agent credential and token-revocation persistence boundary.
#[async_trait]
pub trait AgentCredentialStore: Send + Sync {
    /// Loads an active credential for cryptographic client assertion validation.
    async fn get_agent_credential(
        &self,
        client_id: PrincipalId,
        credential_id: CredentialId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ExistingAgentCredential>, StoreError>;

    /// Atomically consumes a client assertion JTI and updates usage metadata.
    async fn consume_agent_assertion(
        &self,
        credential_id: CredentialId,
        jti_hash: &[u8],
        assertion_expires_at: chrono::DateTime<chrono::Utc>,
        source: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError>;

    /// Resolves and records use of an active credential from a TLS-verified
    /// leaf certificate fingerprint.
    async fn authenticate_agent_mtls(
        &self,
        client_id: PrincipalId,
        certificate_fingerprint: &str,
        source: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<ExistingAgentMtlsCredential>, StoreError>;

    /// Rechecks live Principal/credential state and token scopes on every request.
    async fn validate_agent_token_binding(
        &self,
        principal_id: PrincipalId,
        credential_id: CredentialId,
        scopes: &std::collections::BTreeSet<Permission>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError>;
}
