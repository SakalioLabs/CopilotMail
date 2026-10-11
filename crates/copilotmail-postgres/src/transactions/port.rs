use crate::*;

#[async_trait]
impl ControlPlaneTransaction for PgControlPlaneTransaction {
    async fn bind_request_authentication(
        &mut self,
        context: &AuthorizationContext,
    ) -> Result<(), StoreError> {
        self.tx_bind_request_authentication(context).await
    }

    async fn authorize_principal_permission(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        permission: Permission,
    ) -> Result<bool, StoreError> {
        self.lock_principal_permission(organization_id, principal_id, permission, false)
            .await
    }
    async fn authorize_human_permission(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
        permission: Permission,
    ) -> Result<bool, StoreError> {
        self.tx_authorize_human_permission(organization_id, principal_id, permission)
            .await
    }

    async fn retention_policy_detail(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyDetail>, StoreError> {
        self.tx_retention_policy_detail(organization_id, policy_id)
            .await
    }
    async fn find_organization_bootstrap(
        &mut self,
        slug: &OrganizationSlug,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        self.tx_find_organization_bootstrap(slug).await
    }

    async fn insert_organization(&mut self, organization: &Organization) -> Result<(), StoreError> {
        self.tx_insert_organization(organization).await
    }

    async fn insert_principal(&mut self, principal: &Principal) -> Result<(), StoreError> {
        self.tx_insert_principal(principal).await
    }

    async fn insert_role(&mut self, role: &Role) -> Result<(), StoreError> {
        self.tx_insert_role(role).await
    }

    async fn insert_role_assignment(
        &mut self,
        assignment: &RoleAssignment,
    ) -> Result<(), StoreError> {
        self.tx_insert_role_assignment(assignment).await
    }

    async fn insert_role_assignment_grant(
        &mut self,
        assignment: &RoleAssignment,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<(), StoreError> {
        self.tx_insert_role_assignment_grant(assignment, approval_request_id)
            .await
    }

    async fn find_role_assignment_grant_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<RoleAssignmentGrantRecord>, StoreError> {
        self.tx_find_role_assignment_grant_by_approval(organization_id, approval_request_id)
            .await
    }

    async fn revoke_governed_role_assignment(
        &mut self,
        organization_id: OrganizationId,
        assignment_id: RoleAssignmentId,
        revoked_at: DateTime<Utc>,
    ) -> Result<RoleAssignmentRevocationResult, StoreError> {
        self.tx_revoke_governed_role_assignment(organization_id, assignment_id, revoked_at)
            .await
    }

    async fn record_organization_bootstrap(
        &mut self,
        organization_id: OrganizationId,
        root_principal_id: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.tx_record_organization_bootstrap(organization_id, root_principal_id, created_at)
            .await
    }

    async fn find_oidc_owner_binding(
        &mut self,
        organization_id: OrganizationId,
    ) -> Result<Option<ExistingOidcOwnerBinding>, StoreError> {
        self.tx_find_oidc_owner_binding(organization_id).await
    }

    async fn find_organization_bootstrap_by_id(
        &mut self,
        organization_id: OrganizationId,
    ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
        self.tx_find_organization_bootstrap_by_id(organization_id)
            .await
    }

    async fn find_system_role(
        &mut self,
        organization_id: OrganizationId,
        system_role: SystemRole,
    ) -> Result<Option<RoleId>, StoreError> {
        self.tx_find_system_role(organization_id, system_role).await
    }

    async fn insert_human_profile(&mut self, profile: &HumanProfile) -> Result<(), StoreError> {
        self.tx_insert_human_profile(profile).await
    }

    async fn record_oidc_owner_binding(
        &mut self,
        binding: &ExistingOidcOwnerBinding,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.tx_record_oidc_owner_binding(binding, created_by, created_at)
            .await
    }

    async fn retire_principal(
        &mut self,
        principal_id: PrincipalId,
        retired_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.tx_retire_principal(principal_id, retired_at).await
    }

    async fn revoke_active_role_assignments(
        &mut self,
        principal_id: PrincipalId,
        revoked_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.tx_revoke_active_role_assignments(principal_id, revoked_at)
            .await
    }

    async fn insert_scim_token(
        &mut self,
        credential: &ScimCredential,
        token_hash: &[u8],
        created_at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        self.tx_insert_scim_token(credential, token_hash, created_at)
            .await
    }

    async fn revoke_scim_token(
        &mut self,
        organization_id: OrganizationId,
        token_id: ScimTokenId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_revoke_scim_token(organization_id, token_id, revoked_at)
            .await
    }

    async fn find_scim_user_by_external_id(
        &mut self,
        organization_id: OrganizationId,
        external_id: &str,
    ) -> Result<Option<ExistingScimUser>, StoreError> {
        self.tx_find_scim_user_by_external_id(organization_id, external_id)
            .await
    }

    async fn insert_scim_user(&mut self, user: &ExistingScimUser) -> Result<(), StoreError> {
        self.tx_insert_scim_user(user).await
    }

    async fn find_human_identity(
        &mut self,
        organization_id: OrganizationId,
        identity_provider: &str,
        external_id: &str,
    ) -> Result<Option<ExistingHumanIdentity>, StoreError> {
        self.tx_find_human_identity(organization_id, identity_provider, external_id)
            .await
    }

    async fn adopt_scim_user(&mut self, user: &ExistingScimUser) -> Result<(), StoreError> {
        self.tx_adopt_scim_user(user).await
    }

    async fn replace_scim_user(
        &mut self,
        user: &ExistingScimUser,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        self.tx_replace_scim_user(user, expected_version).await
    }

    async fn find_scim_group_by_external_id(
        &mut self,
        organization_id: OrganizationId,
        external_id: &str,
    ) -> Result<Option<ExistingScimGroup>, StoreError> {
        self.tx_find_scim_group_by_external_id(organization_id, external_id)
            .await
    }

    async fn insert_scim_group(&mut self, group: &ExistingScimGroup) -> Result<(), StoreError> {
        self.tx_insert_scim_group(group).await
    }

    async fn replace_scim_group(
        &mut self,
        group: &ExistingScimGroup,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        self.tx_replace_scim_group(group, expected_version).await
    }

    async fn delete_scim_group(
        &mut self,
        organization_id: OrganizationId,
        group_id: ScimGroupId,
        expected_version: Option<i64>,
    ) -> Result<bool, StoreError> {
        self.tx_delete_scim_group(organization_id, group_id, expected_version)
            .await
    }

    async fn insert_agent_profile(&mut self, profile: &AgentProfile) -> Result<(), StoreError> {
        self.tx_insert_agent_profile(profile).await
    }

    async fn insert_agent_credential(
        &mut self,
        credential: &AgentCredential,
        public_key_pem: &str,
    ) -> Result<(), StoreError> {
        self.tx_insert_agent_credential(credential, public_key_pem)
            .await
    }

    async fn insert_agent_mtls_credential(
        &mut self,
        credential: &AgentMtlsCredential,
    ) -> Result<(), StoreError> {
        self.tx_insert_agent_mtls_credential(credential).await
    }

    async fn active_agent_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_active_agent_exists(organization_id, principal_id)
            .await
    }

    async fn revoke_agent_credential(
        &mut self,
        organization_id: OrganizationId,
        credential_id: CredentialId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_revoke_agent_credential(organization_id, credential_id, revoked_at)
            .await
    }

    async fn active_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_active_principal_exists(organization_id, principal_id)
            .await
    }

    async fn active_agent_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_active_agent_principal_exists(organization_id, principal_id)
            .await
    }

    async fn acquire_agent_event_stream_lease(
        &mut self,
        lease: &AgentEventStreamLease,
        tenant_limit: u32,
        principal_limit: u32,
    ) -> Result<AgentEventStreamLeaseAcquisition, StoreError> {
        self.tx_acquire_agent_event_stream_lease(lease, tenant_limit, principal_limit)
            .await
    }

    async fn active_human_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_active_human_principal_exists(organization_id, principal_id)
            .await
    }

    async fn authorize_agent_inspector_reader(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_authorize_agent_inspector_reader(organization_id, principal_id)
            .await
    }

    async fn get_agent_inspector_snapshot(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<Option<AgentInspectorSnapshotCandidate>, StoreError> {
        self.tx_get_agent_inspector_snapshot(organization_id, agent_id)
            .await
    }

    async fn authorize_agent_mail_submission_evidence_reader(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_authorize_agent_mail_submission_evidence_reader(organization_id, principal_id)
            .await
    }

    async fn get_agent_mail_submission_evidence(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        submission_id: MailSubmissionId,
    ) -> Result<Option<AgentMailSubmissionEvidenceCandidate>, StoreError> {
        self.tx_get_agent_mail_submission_evidence(organization_id, agent_id, submission_id)
            .await
    }

    async fn human_draft_list_still_authorized(
        &mut self,
        proof: &HumanDraftListRecheck,
    ) -> Result<bool, StoreError> {
        self.tx_human_draft_list_still_authorized(proof).await
    }

    async fn human_sent_list_still_authorized(
        &mut self,
        proof: &HumanSentListRecheck,
    ) -> Result<bool, StoreError> {
        self.tx_human_sent_list_still_authorized(proof).await
    }

    async fn human_draft_read_still_authorized(
        &mut self,
        proof: &HumanDraftReadRecheck,
    ) -> Result<bool, StoreError> {
        self.tx_human_draft_read_still_authorized(proof).await
    }

    async fn insert_human_session(&mut self, write: &HumanSessionWrite) -> Result<(), StoreError> {
        self.tx_insert_human_session(write).await
    }

    async fn revoke_human_session_by_token_sha256(
        &mut self,
        token_sha256: &[u8; 32],
        revocation_id: HumanSessionRevocationId,
        cause: HumanSessionRevocationCause,
        revoked_at: DateTime<Utc>,
    ) -> Result<Option<HumanSessionRevocationResult>, StoreError> {
        self.tx_revoke_human_session_by_token_sha256(token_sha256, revocation_id, cause, revoked_at)
            .await
    }

    async fn revoke_active_human_browser_sessions(
        &mut self,
        write: &RevokeActiveHumanSessions,
    ) -> Result<Vec<HumanSessionRevocationResult>, StoreError> {
        self.tx_revoke_active_human_browser_sessions(write).await
    }

    async fn human_principal_exists(
        &mut self,
        organization_id: OrganizationId,
        principal_id: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_human_principal_exists(organization_id, principal_id)
            .await
    }

    async fn insert_human_session_governance_execution(
        &mut self,
        write: &HumanSessionGovernanceExecutionWrite,
    ) -> Result<HumanSessionGovernanceExecution, StoreError> {
        self.tx_insert_human_session_governance_execution(write)
            .await
    }

    async fn lock_exact_dual_approval(
        &mut self,
        proof: &ApprovalProof,
    ) -> Result<bool, StoreError> {
        self.tx_lock_exact_dual_approval(proof).await
    }

    async fn insert_webhook_subscription(
        &mut self,
        subscription: &WebhookSubscription,
    ) -> Result<(), StoreError> {
        self.tx_insert_webhook_subscription(subscription).await
    }

    async fn find_webhook_subscription_for_update(
        &mut self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
    ) -> Result<Option<WebhookSubscription>, StoreError> {
        self.tx_find_webhook_subscription_for_update(organization_id, subscription_id)
            .await
    }

    async fn update_webhook_subscription(
        &mut self,
        subscription: &WebhookSubscription,
        expected_version: i64,
    ) -> Result<bool, StoreError> {
        self.tx_update_webhook_subscription(subscription, expected_version)
            .await
    }

    async fn replay_dead_lettered_webhook_delivery(
        &mut self,
        organization_id: OrganizationId,
        subscription_id: WebhookSubscriptionId,
        event_id: Uuid,
        requested_at: DateTime<Utc>,
    ) -> Result<Option<i32>, StoreError> {
        self.tx_replay_dead_lettered_webhook_delivery(
            organization_id,
            subscription_id,
            event_id,
            requested_at,
        )
        .await
    }

    async fn insert_mail_domain(
        &mut self,
        domain: &MailDomain,
        created_by: PrincipalId,
    ) -> Result<(), StoreError> {
        self.tx_insert_mail_domain(domain, created_by).await
    }

    async fn insert_mail_domain_route(
        &mut self,
        route: &MailDomainRoute,
        updated_by: PrincipalId,
    ) -> Result<(), StoreError> {
        self.tx_insert_mail_domain_route(route, updated_by).await
    }

    async fn find_mail_domain_for_update(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomain>, StoreError> {
        self.tx_find_mail_domain_for_update(organization_id, mail_domain_id)
            .await
    }

    async fn update_mail_domain(
        &mut self,
        domain: &MailDomain,
        updated_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_update_mail_domain(domain, updated_at).await
    }

    async fn insert_mail_domain_dns_observation(
        &mut self,
        observation: &MailDomainDnsObservation,
    ) -> Result<(), StoreError> {
        self.tx_insert_mail_domain_dns_observation(observation)
            .await
    }

    async fn latest_mail_domain_dns_is_activation_ready(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        ownership_challenge_sha256: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_latest_mail_domain_dns_is_activation_ready(
            organization_id,
            mail_domain_id,
            ownership_challenge_sha256,
            now,
        )
        .await
    }

    async fn assign_mail_domain_data_plane(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
        data_plane_id: MailDataPlaneId,
        assigned_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_assign_mail_domain_data_plane(
            organization_id,
            mail_domain_id,
            data_plane_id,
            assigned_at,
        )
        .await
    }

    async fn assigned_mail_domain_data_plane(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDataPlaneId>, StoreError> {
        self.tx_assigned_mail_domain_data_plane(organization_id, mail_domain_id)
            .await
    }
    async fn find_mail_domain_route_for_update(
        &mut self,
        organization_id: OrganizationId,
        mail_domain_id: MailDomainId,
    ) -> Result<Option<MailDomainRoute>, StoreError> {
        self.tx_find_mail_domain_route_for_update(organization_id, mail_domain_id)
            .await
    }

    async fn update_mail_domain_route(
        &mut self,
        route: &MailDomainRoute,
        expected_version: i64,
        updated_by: PrincipalId,
    ) -> Result<bool, StoreError> {
        self.tx_update_mail_domain_route(route, expected_version, updated_by)
            .await
    }

    async fn insert_mailbox(&mut self, mailbox: &Mailbox) -> Result<(), StoreError> {
        self.tx_insert_mailbox(mailbox).await
    }

    async fn upsert_mail_engine_mailbox_binding(
        &mut self,
        write: &MailEngineMailboxBindingWrite,
    ) -> Result<Option<MailEngineMailboxBinding>, StoreError> {
        self.tx_upsert_mail_engine_mailbox_binding(write).await
    }

    async fn tenant_mailbox_exists(
        &mut self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
    ) -> Result<bool, StoreError> {
        self.tx_tenant_mailbox_exists(organization_id, mailbox_id)
            .await
    }

    async fn insert_mailbox_membership(
        &mut self,
        membership: &MailboxMembership,
    ) -> Result<(), StoreError> {
        self.tx_insert_mailbox_membership(membership).await
    }

    async fn mailbox_owned_by(
        &mut self,
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        principal_id: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_mailbox_owned_by(organization_id, mailbox_id, principal_id, now)
            .await
    }

    async fn insert_mailbox_delegation(
        &mut self,
        delegation: &MailboxDelegation,
    ) -> Result<(), StoreError> {
        self.tx_insert_mailbox_delegation(delegation).await
    }

    async fn revoke_mailbox_delegation(
        &mut self,
        organization_id: OrganizationId,
        delegation_id: DelegationId,
        revoked_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_revoke_mailbox_delegation(organization_id, delegation_id, revoked_at)
            .await
    }

    async fn mailbox_delegation_managed_by(
        &mut self,
        organization_id: OrganizationId,
        delegation_id: DelegationId,
        actor: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        self.tx_mailbox_delegation_managed_by(organization_id, delegation_id, actor, now)
            .await
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
        self.tx_insert_approval_policy(
            policy_id,
            organization_id,
            policy_key,
            display_name,
            created_by,
            created_at,
        )
        .await
    }

    async fn insert_approval_policy_version(
        &mut self,
        version: &ApprovalPolicyVersion,
    ) -> Result<(), StoreError> {
        self.tx_insert_approval_policy_version(version).await
    }

    async fn activate_approval_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: PolicyId,
        policy_version_id: PolicyVersionId,
        expected_active_version_id: Option<PolicyVersionId>,
    ) -> Result<bool, StoreError> {
        self.tx_activate_approval_policy_version(
            organization_id,
            policy_id,
            policy_version_id,
            expected_active_version_id,
        )
        .await
    }

    async fn find_active_approval_policy(
        &mut self,
        organization_id: OrganizationId,
        policy_key: &str,
    ) -> Result<Option<ActiveApprovalPolicy>, StoreError> {
        self.tx_find_active_approval_policy(organization_id, policy_key)
            .await
    }

    async fn insert_approval_request(
        &mut self,
        request: &ApprovalRequest,
    ) -> Result<(), StoreError> {
        self.tx_insert_approval_request(request).await
    }

    async fn record_approval_decision(
        &mut self,
        decision_write: &ApprovalDecisionWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        self.tx_record_approval_decision(decision_write).await
    }

    async fn cancel_approval_request(
        &mut self,
        cancellation: &ApprovalCancellationWrite,
    ) -> Result<Option<ApprovalDecisionRecord>, StoreError> {
        self.tx_cancel_approval_request(cancellation).await
    }

    async fn insert_audit_checkpoint(
        &mut self,
        checkpoint: &SignedAuditCheckpoint,
        archive_object_key: &str,
    ) -> Result<bool, StoreError> {
        self.tx_insert_audit_checkpoint(checkpoint, archive_object_key)
            .await
    }

    async fn find_audit_checkpoint(
        &mut self,
        organization_id: OrganizationId,
        checkpoint_id: AuditCheckpointId,
    ) -> Result<Option<ExistingAuditCheckpoint>, StoreError> {
        self.tx_find_audit_checkpoint(organization_id, checkpoint_id)
            .await
    }

    async fn insert_audit_archive(
        &mut self,
        archive: &AuditArchiveRecord,
    ) -> Result<bool, StoreError> {
        self.tx_insert_audit_archive(archive).await
    }

    async fn find_audit_archive(
        &mut self,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
    ) -> Result<Option<AuditArchiveRecord>, StoreError> {
        self.tx_find_audit_archive(organization_id, archive_id)
            .await
    }

    async fn authorize_audit_export_grant(
        &mut self,
        grant: &AuditExportGrant,
    ) -> Result<AuditExportGrant, StoreError> {
        self.tx_authorize_audit_export_grant(grant).await
    }

    async fn record_audit_export_download(
        &mut self,
        download: &AuditExportDownloadRecord,
    ) -> Result<AuditExportDownloadRecord, StoreError> {
        self.tx_record_audit_export_download(download).await
    }

    async fn insert_retention_policy(
        &mut self,
        policy: &RetentionPolicy,
    ) -> Result<(), StoreError> {
        self.tx_insert_retention_policy(policy).await
    }

    async fn insert_retention_policy_version(
        &mut self,
        version: &RetentionPolicyVersion,
    ) -> Result<(), StoreError> {
        self.tx_insert_retention_policy_version(version).await
    }

    async fn find_retention_policy_for_update(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
    ) -> Result<Option<RetentionPolicyRecord>, StoreError> {
        self.tx_find_retention_policy_for_update(organization_id, policy_id)
            .await
    }

    async fn find_retention_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
        version_id: RetentionPolicyVersionId,
    ) -> Result<Option<RetentionPolicyVersion>, StoreError> {
        self.tx_find_retention_policy_version(organization_id, policy_id, version_id)
            .await
    }

    async fn insert_retention_policy_activation(
        &mut self,
        activation: &RetentionPolicyActivationRecord,
    ) -> Result<RetentionPolicyActivationRecord, StoreError> {
        self.tx_insert_retention_policy_activation(activation).await
    }

    async fn find_retention_policy_activation_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<RetentionPolicyActivationRecord>, StoreError> {
        self.tx_find_retention_policy_activation_by_approval(organization_id, approval_request_id)
            .await
    }

    async fn activate_retention_policy_version(
        &mut self,
        organization_id: OrganizationId,
        policy_id: RetentionPolicyId,
        version_id: RetentionPolicyVersionId,
        expected_active_version_id: Option<RetentionPolicyVersionId>,
    ) -> Result<bool, StoreError> {
        self.tx_activate_retention_policy_version(
            organization_id,
            policy_id,
            version_id,
            expected_active_version_id,
        )
        .await
    }

    async fn find_agent_mail_policy_version(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        policy_version_id: Uuid,
    ) -> Result<Option<AgentMailPolicyVersionRecord>, StoreError> {
        self.tx_find_agent_mail_policy_version(organization_id, agent_id, policy_version_id)
            .await
    }

    async fn next_agent_mail_policy_version(
        &mut self,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
    ) -> Result<u32, StoreError> {
        self.tx_next_agent_mail_policy_version(organization_id, agent_id)
            .await
    }

    async fn insert_agent_mail_policy_version(
        &mut self,
        record: &AgentMailPolicyVersionRecord,
    ) -> Result<(), StoreError> {
        self.tx_insert_agent_mail_policy_version(record).await
    }

    async fn find_agent_mail_policy_activation_by_approval(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<AgentMailPolicyActivation>, StoreError> {
        self.tx_find_agent_mail_policy_activation_by_approval(organization_id, approval_request_id)
            .await
    }

    async fn insert_agent_mail_policy_activation(
        &mut self,
        activation: &AgentMailPolicyActivation,
        policy_version_id: Uuid,
    ) -> Result<(), StoreError> {
        self.tx_insert_agent_mail_policy_activation(activation, policy_version_id)
            .await
    }

    async fn ensure_agent_processing_subscription(
        &mut self,
        record: &AgentProcessingSubscriptionRecord,
    ) -> Result<(AgentProcessingSubscriptionRecord, bool), StoreError> {
        self.tx_ensure_agent_processing_subscription(record).await
    }

    async fn change_agent_processing_subscription(
        &mut self,
        write: &AgentProcessingSubscriptionChangeWrite,
    ) -> Result<Option<AgentProcessingSubscriptionChangeResult>, StoreError> {
        self.tx_change_agent_processing_subscription(write).await
    }

    async fn claim_next_agent_message(
        &mut self,
        write: &AgentMessageClaimWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        self.tx_claim_next_agent_message(write).await
    }

    async fn transition_agent_message(
        &mut self,
        write: &AgentMessageTransitionWrite,
    ) -> Result<Option<AgentMessageProcessingResult>, StoreError> {
        self.tx_transition_agent_message(write).await
    }

    async fn insert_compliance_case(&mut self, case: &ComplianceCase) -> Result<(), StoreError> {
        self.tx_insert_compliance_case(case).await
    }

    async fn find_compliance_case_for_update(
        &mut self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<Option<ComplianceCase>, StoreError> {
        self.tx_find_compliance_case_for_update(organization_id, case_id)
            .await
    }

    async fn update_compliance_case(&mut self, case: &ComplianceCase) -> Result<bool, StoreError> {
        self.tx_update_compliance_case(case).await
    }

    async fn active_legal_hold_exists_for_case(
        &mut self,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
    ) -> Result<bool, StoreError> {
        self.tx_active_legal_hold_exists_for_case(organization_id, case_id)
            .await
    }

    async fn insert_legal_hold(
        &mut self,
        hold: &LegalHold,
        evidence: &LegalHoldObjectEvidence,
    ) -> Result<(), StoreError> {
        self.tx_insert_legal_hold(hold, evidence).await
    }

    async fn find_legal_hold_by_apply_approval_for_update(
        &mut self,
        organization_id: OrganizationId,
        approval_request_id: copilotmail_domain::ApprovalRequestId,
    ) -> Result<Option<LegalHold>, StoreError> {
        self.tx_find_legal_hold_by_apply_approval_for_update(organization_id, approval_request_id)
            .await
    }

    async fn find_legal_hold_for_update(
        &mut self,
        organization_id: OrganizationId,
        hold_id: LegalHoldId,
    ) -> Result<Option<LegalHold>, StoreError> {
        self.tx_find_legal_hold_for_update(organization_id, hold_id)
            .await
    }

    async fn update_legal_hold(
        &mut self,
        hold: &LegalHold,
        evidence: &LegalHoldObjectEvidence,
    ) -> Result<bool, StoreError> {
        self.tx_update_legal_hold(hold, evidence).await
    }

    async fn create_mail_submission_intent(
        &mut self,
        write: &MailSubmissionIntentWrite,
    ) -> Result<MailSubmissionIntentCreation, StoreError> {
        self.tx_create_mail_submission_intent(write).await
    }

    async fn create_agent_mail_sandbox_submission(
        &mut self,
        write: &AgentMailSandboxSubmissionWrite,
    ) -> Result<AgentMailSandboxSubmissionCreation, StoreError> {
        self.tx_create_agent_mail_sandbox_submission(write).await
    }

    async fn acquire_mail_submission_lease(
        &mut self,
        submission_id: MailSubmissionId,
        expected_data_plane_id: MailDataPlaneId,
        lease_id: Uuid,
        now: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
    ) -> Result<Option<MailSubmissionLease>, StoreError> {
        self.tx_acquire_mail_submission_lease(
            submission_id,
            expected_data_plane_id,
            lease_id,
            now,
            lease_expires_at,
        )
        .await
    }

    async fn finalize_mail_submission_intent(
        &mut self,
        write: &MailSubmissionTerminalWrite,
    ) -> Result<Option<MailSubmissionTerminalTransition>, StoreError> {
        self.tx_finalize_mail_submission_intent(write).await
    }

    async fn append_mail_delivery_snapshot(
        &mut self,
        write: &MailDeliverySnapshotWrite,
    ) -> Result<MailDeliverySnapshotInsert, StoreError> {
        self.tx_append_mail_delivery_snapshot(write).await
    }

    async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
        self.tx_append_audit_event(event).await
    }

    async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
        self.tx_enqueue_event(event).await
    }

    async fn commit(mut self: Box<Self>) -> Result<(), StoreError> {
        self.tx_commit().await
    }
}
