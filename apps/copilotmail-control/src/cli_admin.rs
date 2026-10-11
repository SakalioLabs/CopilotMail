use crate::*;

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

pub(crate) fn parse_mailbox_kind(value: &str) -> Result<MailboxKind> {
    mailbox_kind_from_wire(value).context("mailbox kind must be human, agent, shared, or group")
}

pub(crate) fn mailbox_kind_from_wire(value: &str) -> Option<MailboxKind> {
    match value {
        "human" => Some(MailboxKind::Human),
        "agent" => Some(MailboxKind::Agent),
        "shared" => Some(MailboxKind::Shared),
        "group" => Some(MailboxKind::Group),
        _ => None,
    }
}

pub(crate) fn parse_mailbox_capabilities(
    capabilities: Vec<String>,
) -> Result<BTreeSet<MailboxCapability>> {
    capabilities
        .into_iter()
        .map(|capability| {
            mailbox_capability_from_wire(&capability)
                .with_context(|| format!("unsupported mailbox capability: {capability}"))
        })
        .collect()
}

pub(crate) fn mailbox_capability_from_wire(value: &str) -> Option<MailboxCapability> {
    MailboxCapability::DELEGABLE
        .into_iter()
        .find(|candidate| candidate.as_str() == value)
}

pub(crate) fn parse_approval_action(value: &str) -> Result<ApprovalAction> {
    approval_action_from_wire(value)
        .with_context(|| format!("unsupported approval action: {value}"))
}

pub(crate) fn approval_action_from_wire(value: &str) -> Option<ApprovalAction> {
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

pub(crate) fn print_policy_result(
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

pub(crate) fn parse_agent_scopes(scopes: Vec<String>) -> Result<BTreeSet<Permission>> {
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

pub(crate) async fn load_agent_public_key(public_key_file: &str) -> Result<(String, String)> {
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
pub(crate) struct AgentSigningKeySetManifest {
    pub(crate) active_key_id: String,
    pub(crate) keys: Vec<AgentSigningKeyFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentSigningKeyFile {
    pub(crate) key_id: String,
    pub(crate) private_key_file: Option<PathBuf>,
    pub(crate) public_key_file: PathBuf,
}

pub(crate) struct LoadedAgentSigningKeySet {
    pub(crate) active_key_id: String,
    pub(crate) keys: Vec<AgentSigningKeyInput>,
    pub(crate) digest: Vec<u8>,
}

pub(crate) async fn load_agent_signing_key_set(
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

pub(crate) fn spawn_signing_key_reload(
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

pub(crate) fn resolve_key_path(base: &FsPath, path: &FsPath) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

pub(crate) async fn build_human_session_runtime(
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
    let mut session_clients = Vec::new();
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
        session_clients.push(client_key);
    }
    anyhow::ensure!(!clients.is_empty(), "Human browser clients are empty");
    Ok(HumanSessionRuntime {
        clients: Arc::new(clients),
        session_clients: Arc::new(session_clients),
        protector,
        maximum_lifetime: settings.maximum_lifetime,
        login_requests_per_minute: settings.login_requests_per_minute,
        secure_cookie: !settings.allow_insecure_cookie,
    })
}

pub(crate) async fn read_bounded_secret_file(
    path: &FsPath,
    maximum_bytes: usize,
) -> Result<Vec<u8>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_parsers_accept_only_the_exact_catalog_and_round_trip() {
        for (wire, kind) in [
            ("human", MailboxKind::Human),
            ("agent", MailboxKind::Agent),
            ("shared", MailboxKind::Shared),
            ("group", MailboxKind::Group),
        ] {
            assert_eq!(mailbox_kind_from_wire(wire), Some(kind));
        }
        for bad in ["Human", "HUMAN", "", " human", "human\x00", "operator"] {
            assert_eq!(mailbox_kind_from_wire(bad), None);
            assert!(parse_mailbox_kind(bad).is_err());
        }
        for capability in MailboxCapability::DELEGABLE {
            assert_eq!(
                mailbox_capability_from_wire(capability.as_str()),
                Some(capability)
            );
        }
        assert_eq!(mailbox_capability_from_wire("send_as"), None);
        assert_eq!(mailbox_capability_from_wire(""), None);
        let parsed = parse_mailbox_capabilities(
            MailboxCapability::DELEGABLE
                .iter()
                .map(|capability| capability.as_str().to_owned())
                .collect(),
        )
        .unwrap();
        assert_eq!(
            parsed,
            MailboxCapability::DELEGABLE
                .into_iter()
                .collect::<BTreeSet<_>>()
        );
        assert!(parse_mailbox_capabilities(vec!["read,write".into()]).is_err());
    }

    #[test]
    fn approval_action_parser_round_trips_the_full_catalog_and_rejects_unknown() {
        for action in [
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
        ] {
            assert_eq!(approval_action_from_wire(action.as_str()), Some(action));
            assert_eq!(parse_approval_action(action.as_str()).unwrap(), action);
        }
        for bad in ["send-as", "audit-export", "", "ALL", "break_glass "] {
            assert_eq!(approval_action_from_wire(bad), None);
            assert!(parse_approval_action(bad).is_err());
        }
    }

    #[test]
    fn agent_scope_parser_accepts_only_agent_token_permissions() {
        let parsed = parse_agent_scopes(
            Permission::AGENT_TOKEN
                .iter()
                .map(|permission| permission.as_str().to_owned())
                .collect(),
        )
        .unwrap();
        assert_eq!(
            parsed,
            Permission::AGENT_TOKEN.into_iter().collect::<BTreeSet<_>>()
        );
        for bad in ["mail.admin", "organization.admin", "", "agent.operate "] {
            assert!(parse_agent_scopes(vec![bad.into()]).is_err());
        }
    }
}
