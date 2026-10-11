use super::*;

/// Transaction boundary required by application use cases.
#[async_trait]
pub trait ControlPlaneTransaction: Send {
    /// Bind immediately after begin, before any business write. Implementations
    /// must fence the credential through commit and recheck expiry at commit.
    /// Default deny prevents adapters from silently ignoring a request proof.
    async fn bind_request_authentication(
        &mut self,
        context: &AuthorizationContext,
    ) -> Result<(), StoreError> {
        if context.authentication.is_some() {
            Err(StoreError::AuthenticationInvalid)
        } else {
            Ok(())
        }
    }
    /// Serializes bootstrap attempts for a slug and returns an existing result.
    async fn find_organization_bootstrap(
        &mut self,
        slug: &OrganizationSlug,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError>;

    /// Inserts an organization.
    async fn insert_organization(&mut self, organization: &Organization) -> Result<(), StoreError>;

    /// Inserts a principal.
    async fn insert_principal(&mut self, principal: &Principal) -> Result<(), StoreError>;

    /// Inserts a tenant-scoped role and its permissions.
    async fn insert_role(&mut self, role: &Role) -> Result<(), StoreError>;

    /// Grants a role to a principal.
    async fn insert_role_assignment(
        &mut self,
        assignment: &RoleAssignment,
    ) -> Result<(), StoreError>;

    /// Links a newly inserted bounded assignment to one exact dual approval.
    async fn insert_role_assignment_grant(
        &mut self,
        _assignment: &RoleAssignment,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_role_assignment_grant_by_approval(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<RoleAssignmentGrantRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes one assignment under a tenant lock and refuses to remove the
    /// last effective Organization Owner.
    async fn revoke_governed_role_assignment(
        &mut self,
        _organization_id: OrganizationId,
        _assignment_id: RoleAssignmentId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<RoleAssignmentRevocationResult, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Records the root service identity created by organization bootstrap.
    async fn record_organization_bootstrap(
        &mut self,
        organization_id: OrganizationId,
        root_principal_id: PrincipalId,
        created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError>;

    /// Serializes owner handover and returns an existing binding for the tenant.
    async fn find_oidc_owner_binding(
        &mut self,
        _organization_id: OrganizationId,
    ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Loads the immutable bootstrap identity for an organization.
    async fn find_organization_bootstrap_by_id(
        &mut self,
        _organization_id: OrganizationId,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Loads a built-in tenant role.
    async fn find_system_role(
        &mut self,
        _organization_id: OrganizationId,
        _system_role: SystemRole,
    ) -> Result<Option<RoleId>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts the exact OIDC issuer/subject mapping for a human.
    async fn insert_human_profile(&mut self, _profile: &HumanProfile) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Records completion of the one-time owner handover.
    async fn record_oidc_owner_binding(
        &mut self,
        _binding: &ExistingOidcOwnerBinding,
        _created_by: PrincipalId,
        _created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Permanently retires a bootstrap principal.
    async fn retire_principal(
        &mut self,
        _principal_id: PrincipalId,
        _retired_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes every active role assignment held by a principal.
    async fn revoke_active_role_assignments(
        &mut self,
        _principal_id: PrincipalId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists a tenant-scoped SCIM credential digest; plaintext is never accepted.
    async fn insert_scim_token(
        &mut self,
        _credential: &ScimCredential,
        _token_hash: &[u8],
        _created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes one active SCIM token within its tenant.
    async fn revoke_scim_token(
        &mut self,
        _organization_id: OrganizationId,
        _token_id: ScimTokenId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Serializes and locates a user by the IdP's immutable external identifier.
    async fn find_scim_user_by_external_id(
        &mut self,
        _organization_id: OrganizationId,
        _external_id: &str,
    ) -> Result<Option<ExistingScimUser>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Adds the SCIM-specific projection for a newly inserted Human Principal.
    async fn insert_scim_user(&mut self, _user: &ExistingScimUser) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locates an existing tenant HumanProfile for SCIM adoption.
    async fn find_human_identity(
        &mut self,
        _organization_id: OrganizationId,
        _identity_provider: &str,
        _external_id: &str,
    ) -> Result<Option<ExistingHumanIdentity>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Updates an existing Human Principal and attaches its SCIM projection.
    async fn adopt_scim_user(&mut self, _user: &ExistingScimUser) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Replaces mutable SCIM user fields and Principal lifecycle using CAS versioning.
    async fn replace_scim_user(
        &mut self,
        _user: &ExistingScimUser,
        _expected_version: i64,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_scim_group_by_external_id(
        &mut self,
        _organization_id: OrganizationId,
        _external_id: &str,
    ) -> Result<Option<ExistingScimGroup>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_scim_group(&mut self, _group: &ExistingScimGroup) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn replace_scim_group(
        &mut self,
        _group: &ExistingScimGroup,
        _expected_version: i64,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn delete_scim_group(
        &mut self,
        _organization_id: OrganizationId,
        _group_id: ScimGroupId,
        _expected_version: Option<i64>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_profile(&mut self, _profile: &AgentProfile) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_credential(
        &mut self,
        _credential: &AgentCredential,
        _public_key_pem: &str,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists an Agent mTLS certificate fingerprint and permitted scopes.
    async fn insert_agent_mtls_credential(
        &mut self,
        _credential: &AgentMtlsCredential,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn active_agent_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn revoke_agent_credential(
        &mut self,
        _organization_id: OrganizationId,
        _credential_id: CredentialId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Returns whether an active principal exists inside the exact tenant.
    async fn active_principal_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Default-deny proof that the target is an active Agent in the tenant.
    async fn active_agent_principal_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Default-deny proof that the actor is an active Human in the tenant.
    async fn active_human_principal_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Reproves one current Human permission on this transaction's connection.
    /// Implementations must serialize the principal, assignment and permission
    /// rows through commit and recheck time after acquiring locks. A previously
    /// authenticated HTTP snapshot is not a substitute for this proof.
    async fn authorize_human_permission(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
        _permission: Permission,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Same transaction-local proof for Human or Agent requesters. This does
    /// not grant Token Scope or override the caller's base authorization.
    async fn authorize_principal_permission(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
        _permission: Permission,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Loads retention detail on the same connection as its authorization and
    /// disclosure audit, avoiding a second pool checkout while holding locks.
    async fn retention_policy_detail(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Final, same-transaction authorization for the privileged Agent
    /// Inspector. Implementations must prove that `principal_id` is still an
    /// active Human in `organization_id` and still holds effective
    /// `identity.manage`, and lock or otherwise serialize that proof through
    /// the transaction commit. They must not trust a stale HTTP authorization
    /// snapshot alone. Default-deny prevents an adapter from exposing an
    /// Inspector candidate before it implements both checks.
    async fn authorize_agent_inspector_reader(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Builds one bounded Agent Inspector candidate after
    /// [`Self::authorize_agent_inspector_reader`] proves the caller is still
    /// an active, currently authorized Human in this transaction. This default
    /// is deliberately unavailable rather than empty: adapters must implement
    /// the complete tenant-bound aggregate before the Inspector can disclose
    /// anything.
    async fn get_agent_inspector_snapshot(
        &mut self,
        _organization_id: OrganizationId,
        _agent_id: PrincipalId,
    ) -> Result<Option<AgentInspectorSnapshotCandidate>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Final, same-transaction authorization for one Human administrative
    /// Agent mail-evidence read. Implementations must prove that the caller
    /// remains an active Human in the tenant and holds both `identity.manage`
    /// and `audit.read`, locking or otherwise serializing the proof through
    /// commit. Default-deny prevents accidental disclosure by incomplete
    /// adapters.
    async fn authorize_agent_mail_submission_evidence_reader(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Builds one exact production-Agent mail-admission evidence candidate
    /// after [`Self::authorize_agent_mail_submission_evidence_reader`] has
    /// succeeded. A missing target, a cross-tenant pair, a non-Agent actor, or
    /// a submission without an immutable production admission must be
    /// indistinguishable as `Ok(None)`.
    async fn get_agent_mail_submission_evidence(
        &mut self,
        _organization_id: OrganizationId,
        _agent_id: PrincipalId,
        _submission_id: MailSubmissionId,
    ) -> Result<Option<AgentMailSubmissionEvidenceCandidate>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Final disclosure guard for an exact Human draft body. This method is
    /// intentionally default-deny so an adapter cannot expose content merely
    /// by compiling against a newly extended transaction trait.
    async fn human_draft_read_still_authorized(
        &mut self,
        _proof: &HumanDraftReadRecheck,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Final authorization guard for a content-free Human draft list. Like
    /// body reads, list metadata is returned only after this proof and its
    /// Audit/Outbox evidence commit atomically.
    async fn human_draft_list_still_authorized(
        &mut self,
        _proof: &HumanDraftListRecheck,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Final authorization guard for the content-free Human accepted-
    /// submission directory. Default deny prevents adapters which have not
    /// implemented the exact ownership and snapshot recheck from disclosing
    /// historical delivery metadata.
    async fn human_sent_list_still_authorized(
        &mut self,
        _proof: &HumanSentListRecheck,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Inserts one browser session using only a SHA-256 token digest.
    async fn insert_human_session(&mut self, _write: &HumanSessionWrite) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends an immutable revocation selected by an exact token digest.
    async fn revoke_human_session_by_token_sha256(
        &mut self,
        _token_sha256: &[u8; 32],
        _revocation_id: HumanSessionRevocationId,
        _cause: HumanSessionRevocationCause,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<HumanSessionRevocationResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends one revocation for every still-valid browser session of one
    /// principal. A unique conflict aborts the caller transaction; nothing in
    /// the batch may commit after a partial failure.
    async fn revoke_active_human_browser_sessions(
        &mut self,
        _write: &RevokeActiveHumanSessions,
    ) -> Result<Vec<HumanSessionRevocationResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks one human principal row when it exists. Any status counts: a
    /// suspended principal can still have live browser sessions. `Ok(false)`
    /// means that principal is not a human in this tenant. The default is
    /// unavailable so a forgotten adapter cannot report success and skip the
    /// revocation.
    async fn human_principal_exists(
        &mut self,
        _organization_id: OrganizationId,
        _principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Records that one approval has been used for session revocation.
    /// Replaying the same approval returns the original target and requester.
    async fn insert_human_session_governance_execution(
        &mut self,
        _write: &HumanSessionGovernanceExecutionWrite,
    ) -> Result<HumanSessionGovernanceExecution, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks one approved request and rechecks dual control, separation of
    /// duty, exact resource, and database expiry. Default deny.
    async fn lock_exact_dual_approval(
        &mut self,
        _proof: &ApprovalProof,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    async fn insert_webhook_subscription(
        &mut self,
        _subscription: &WebhookSubscription,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_webhook_subscription_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscription>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn update_webhook_subscription(
        &mut self,
        _subscription: &WebhookSubscription,
        _expected_version: i64,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn acquire_agent_event_stream_lease(
        &mut self,
        _lease: &AgentEventStreamLease,
        _tenant_limit: u32,
        _principal_limit: u32,
    ) -> Result<AgentEventStreamLeaseAcquisition, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends a replay marker after an exact latest dead letter and makes the
    /// original Outbox event claimable again. Existing delivery evidence is
    /// never updated or deleted.
    async fn replay_dead_lettered_webhook_delivery(
        &mut self,
        _organization_id: OrganizationId,
        _subscription_id: WebhookSubscriptionId,
        _event_id: uuid::Uuid,
        _requested_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<i32>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts a mailbox owned by the tenant.
    async fn insert_mailbox(&mut self, _mailbox: &Mailbox) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// CAS-upserts the only mail-engine account binding for a tenant mailbox.
    async fn upsert_mail_engine_mailbox_binding(
        &mut self,
        _write: &MailEngineMailboxBindingWrite,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Proves a mailbox exists inside the exact tenant without exposing a
    /// cross-tenant distinction to the caller.
    async fn tenant_mailbox_exists(
        &mut self,
        _organization_id: OrganizationId,
        _mailbox_id: MailboxId,
    ) -> Result<bool, StoreError> {
        Ok(false)
    }

    /// Inserts a globally unique managed domain and its challenge digest.
    async fn insert_mail_domain(
        &mut self,
        _domain: &MailDomain,
        _created_by: PrincipalId,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts the initial disabled route projection.
    async fn insert_mail_domain_route(
        &mut self,
        _route: &MailDomainRoute,
        _updated_by: PrincipalId,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks and loads one tenant-owned domain.
    async fn find_mail_domain_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomain>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists lifecycle state after the row has been locked.
    async fn update_mail_domain(
        &mut self,
        _domain: &MailDomain,
        _updated_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends challenge-bound DNS evidence.
    async fn insert_mail_domain_dns_observation(
        &mut self,
        _observation: &MailDomainDnsObservation,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Confirms a complete, unexpired observation exists for the current challenge.
    async fn latest_mail_domain_dns_is_activation_ready(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
        _ownership_challenge_sha256: &[u8; 32],
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Assigns a verified domain to one exact mail data plane before its
    /// activation event can become visible. Implementations must use CAS
    /// semantics: an existing different assignment is a conflict.
    async fn assign_mail_domain_data_plane(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
        _data_plane_id: MailDataPlaneId,
        _assigned_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Reads the durable assignment while the domain row is locked. A missing
    /// assignment must fail closed for operations that project remote state.
    async fn assigned_mail_domain_data_plane(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDataPlaneId>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks the versioned route for a domain.
    async fn find_mail_domain_route_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomainRoute>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// CAS-updates a route so stale administrators cannot overwrite each other.
    async fn update_mail_domain_route(
        &mut self,
        _route: &MailDomainRoute,
        _expected_version: i64,
        _updated_by: PrincipalId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts a tenant-bound mailbox membership.
    async fn insert_mailbox_membership(
        &mut self,
        _membership: &MailboxMembership,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Checks an effective, non-expired owner membership.
    async fn mailbox_owned_by(
        &mut self,
        _organization_id: OrganizationId,
        _mailbox_id: MailboxId,
        _principal_id: PrincipalId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts one explicit, time-bounded mailbox delegation.
    async fn insert_mailbox_delegation(
        &mut self,
        _delegation: &MailboxDelegation,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Revokes an active delegation within the exact tenant.
    async fn revoke_mailbox_delegation(
        &mut self,
        _organization_id: OrganizationId,
        _delegation_id: DelegationId,
        _revoked_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Checks whether the actor currently owns the mailbox referenced by a delegation.
    async fn mailbox_delegation_managed_by(
        &mut self,
        _organization_id: OrganizationId,
        _delegation_id: DelegationId,
        _actor: PrincipalId,
        _now: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Creates the stable policy identity.
    async fn insert_approval_policy(
        &mut self,
        _policy_id: PolicyId,
        _organization_id: OrganizationId,
        _policy_key: &str,
        _display_name: &str,
        _created_by: PrincipalId,
        _created_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends one immutable policy version.
    async fn insert_approval_policy_version(
        &mut self,
        _version: &ApprovalPolicyVersion,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Atomically switches the policy's active immutable version.
    async fn activate_approval_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: PolicyId,
        _policy_version_id: PolicyVersionId,
        _expected_active_version_id: Option<PolicyVersionId>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Resolves an active policy under transaction consistency.
    async fn find_active_approval_policy(
        &mut self,
        _organization_id: OrganizationId,
        _policy_key: &str,
    ) -> Result<Option<ActiveApprovalPolicy>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Inserts a policy-version-bound approval request.
    async fn insert_approval_request(
        &mut self,
        _request: &ApprovalRequest,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Locks the request and appends one irreversible decision.
    async fn record_approval_decision(
        &mut self,
        _decision: &ApprovalDecisionWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Withdraws one live pending request only when the actor is its original
    /// requester. The guarded status transition is committed with audit/outbox.
    async fn cancel_approval_request(
        &mut self,
        _cancellation: &ApprovalCancellationWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists an immutable signed audit checkpoint.
    async fn insert_audit_checkpoint(
        &mut self,
        _checkpoint: &SignedAuditCheckpoint,
        _archive_object_key: &str,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Finds an immutable checkpoint for exact idempotent replay.
    async fn find_audit_checkpoint(
        &mut self,
        _organization_id: OrganizationId,
        _checkpoint_id: copilotmail_domain::AuditCheckpointId,
    ) -> Result<Option<ExistingAuditCheckpoint>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists evidence only after exact remote Object Lock verification.
    async fn insert_audit_archive(
        &mut self,
        _archive: &AuditArchiveRecord,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Finds immutable archive evidence for exact idempotent replay.
    async fn find_audit_archive(
        &mut self,
        _organization_id: OrganizationId,
        _archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Creates one immutable, approval-bound export grant or returns the
    /// existing grant when the exact approval is replayed.
    async fn authorize_audit_export_grant(
        &mut self,
        _grant: &AuditExportGrant,
    ) -> Result<AuditExportGrant, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends immutable evidence that one exact archive object version was
    /// served. The database independently reproves the grant, the requesting
    /// Human, the grant lifetime, and the exact object identity. Returns the
    /// persisted, database-clock timestamp after all authorization lock waits.
    async fn record_audit_export_download(
        &mut self,
        _download: &AuditExportDownloadRecord,
    ) -> Result<AuditExportDownloadRecord, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_retention_policy(
        &mut self,
        _policy: &RetentionPolicy,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_retention_policy_version(
        &mut self,
        _version: &RetentionPolicyVersion,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_retention_policy_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_retention_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: RetentionPolicyId,
        _version_id: RetentionPolicyVersionId,
    ) -> Result<Option<RetentionPolicyVersion>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Persists the exact approved activation and returns the database's
    /// authoritative timestamp after lock waits and final approval validation.
    async fn insert_retention_policy_activation(
        &mut self,
        _activation: &RetentionPolicyActivationRecord,
    ) -> Result<RetentionPolicyActivationRecord, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_retention_policy_activation_by_approval(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<RetentionPolicyActivationRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn activate_retention_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _policy_id: RetentionPolicyId,
        _version_id: RetentionPolicyVersionId,
        _expected_active_version_id: Option<RetentionPolicyVersionId>,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_agent_mail_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _agent_id: PrincipalId,
        _policy_version_id: Uuid,
    ) -> Result<Option<AgentMailPolicyVersionRecord>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn next_agent_mail_policy_version(
        &mut self,
        _organization_id: OrganizationId,
        _agent_id: PrincipalId,
    ) -> Result<u32, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_mail_policy_version(
        &mut self,
        _record: &AgentMailPolicyVersionRecord,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_agent_mail_policy_activation_by_approval(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<AgentMailPolicyActivation>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_agent_mail_policy_activation(
        &mut self,
        _activation: &AgentMailPolicyActivation,
        _policy_version_id: Uuid,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn ensure_agent_processing_subscription(
        &mut self,
        _record: &AgentProcessingSubscriptionRecord,
    ) -> Result<(AgentProcessingSubscriptionRecord, bool), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn change_agent_processing_subscription(
        &mut self,
        _write: &AgentProcessingSubscriptionChangeWrite,
    ) -> Result<Option<AgentProcessingSubscriptionChangeResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn claim_next_agent_message(
        &mut self,
        _write: &AgentMessageClaimWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn transition_agent_message(
        &mut self,
        _write: &AgentMessageTransitionWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_compliance_case(&mut self, _case: &ComplianceCase) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_compliance_case_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn update_compliance_case(&mut self, _case: &ComplianceCase) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn active_legal_hold_exists_for_case(
        &mut self,
        _organization_id: OrganizationId,
        _case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn insert_legal_hold(
        &mut self,
        _hold: &LegalHold,
        _evidence: &LegalHoldObjectEvidence,
    ) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_legal_hold_by_apply_approval_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _approval_request_id: ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn find_legal_hold_for_update(
        &mut self,
        _organization_id: OrganizationId,
        _hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError> {
        Err(StoreError::Unavailable)
    }

    async fn update_legal_hold(
        &mut self,
        _hold: &LegalHold,
        _evidence: &LegalHoldObjectEvidence,
    ) -> Result<bool, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Freezes one exact owned draft and creates its idempotent submission
    /// intent. Exact replay returns the existing intent without new writes.
    async fn create_mail_submission_intent(
        &mut self,
        _write: &MailSubmissionIntentWrite,
    ) -> Result<MailSubmissionIntentCreation, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Freezes one exact testing-Agent draft and captures immutable sandbox
    /// evidence without creating a deliverable mail submission Intent.
    async fn create_agent_mail_sandbox_submission(
        &mut self,
        _write: &AgentMailSandboxSubmissionWrite,
    ) -> Result<AgentMailSandboxSubmissionCreation, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Commits ownership before any remote JMAP operation. Expired leases are
    /// reclaimable; active and terminal intents are not.
    async fn acquire_mail_submission_lease(
        &mut self,
        _submission_id: MailSubmissionId,
        _expected_data_plane_id: MailDataPlaneId,
        _lease_id: uuid::Uuid,
        _now: chrono::DateTime<chrono::Utc>,
        _lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<MailSubmissionLease>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Applies one lease-CAS terminal transition. Accepted transitions also
    /// move the exact frozen draft to `sent` in this transaction. `None`
    /// means that the lease-CAS did not match; a returned transition supplies
    /// the persistence-authoritative time for Audit and Outbox evidence.
    async fn finalize_mail_submission_intent(
        &mut self,
        _write: &MailSubmissionTerminalWrite,
    ) -> Result<Option<MailSubmissionTerminalTransition>, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends one complete recipient-level delivery snapshot. Exact replay is
    /// idempotent; changed content under an existing source identity conflicts.
    async fn append_mail_delivery_snapshot(
        &mut self,
        _write: &MailDeliverySnapshotWrite,
    ) -> Result<MailDeliverySnapshotInsert, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Appends an audit event in the same transaction as the state change.
    async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError>;

    /// Enqueues an integration event in the same transaction as the state change.
    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError>;

    /// Atomically commits all writes.
    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

/// Factory for application transaction boundaries.
#[async_trait]
pub trait ControlPlaneStore: Send + Sync {
    /// Begins an isolated unit of work.
    async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError>;

    /// A request-bound unit of work. Keep raw begin for trusted background/CLI
    /// operations; authenticated use cases must not discard this binding.
    async fn begin_authorized(
        &self,
        context: &AuthorizationContext,
    ) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
        let mut transaction = self.begin().await?;
        transaction.bind_request_authentication(context).await?;
        Ok(transaction)
    }
}

/// Specialized transaction for high-volume operational mail events. It keeps
/// source receipts and Outbox publication atomic without misattributing a
/// machine-generated event to a Human audit actor.
#[async_trait]
pub trait MailEventIngestionTransaction: Send {
    async fn resolve_mail_engine_mailbox_binding(
        &mut self,
        data_plane_id: MailDataPlaneId,
        account_ref: &str,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError>;

    async fn insert_inbound_mail_event(
        &mut self,
        record: &InboundMailEventRecord,
    ) -> Result<InboundMailEventInsert, StoreError>;

    async fn fanout_agent_message_states(
        &mut self,
        _record: &InboundMailEventRecord,
    ) -> Result<u64, StoreError> {
        Err(StoreError::Unavailable)
    }

    /// Resolves an outbound Envelope From only when it maps to exactly one
    /// currently bound control-plane mailbox.
    async fn resolve_mail_engine_mailbox_by_primary_address(
        &mut self,
        data_plane_id: MailDataPlaneId,
        primary_address: &MailboxAddress,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError>;

    async fn insert_outbound_mail_event(
        &mut self,
        record: &OutboundMailEventRecord,
    ) -> Result<OutboundMailEventInsert, StoreError>;

    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError>;

    async fn commit(self: Box<Self>) -> Result<(), StoreError>;
}

/// Factory for the isolated authenticated mail-event ingestion boundary.
#[async_trait]
pub trait MailEventIngestionStore: Send + Sync {
    async fn begin_mail_event_ingestion(
        &self,
    ) -> Result<Box<dyn MailEventIngestionTransaction>, StoreError>;
}
