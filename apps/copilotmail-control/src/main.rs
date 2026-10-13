use anyhow::Result;
use clap::{Parser, Subcommand};
use copilotmail_control::{
    connect_store, run_add_agent_credential, run_append_approval_policy_version,
    run_apply_audit_legal_hold, run_archive_audit_export, run_audit_maintenance_once,
    run_audit_maintenance_worker, run_bind_oidc_owner, run_bootstrap_organization, run_check,
    run_check_domain_dns, run_check_mail_engine, run_close_compliance_case,
    run_create_approval_policy, run_create_audit_checkpoint, run_create_compliance_case,
    run_create_mailbox, run_export_audit_checkpoint, run_grant_mailbox_delegation, run_http,
    run_issue_scim_token, run_list_human_login_failures, run_migrations, run_register_agent,
    run_release_audit_legal_hold, run_revoke_agent_credential, run_revoke_mailbox_delegation,
    run_revoke_scim_token, run_verify_audit_chain, run_verify_audit_export, run_worker,
};
use copilotmail_platform::{ControlPlaneSettings, init_observability};

#[derive(Debug, Parser)]
#[command(
    name = "copilotmail-control",
    version,
    about = "CopilotMail V2 enterprise control plane"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve the V2 HTTP API.
    Serve,
    /// Process durable transactional-outbox events.
    Worker,
    /// Run one configured tenant checkpoint/export/WORM maintenance cycle.
    AuditMaintenance,
    /// Periodically run the configured tenant audit maintenance cycle.
    AuditMaintenanceWorker,
    /// Apply versioned PostgreSQL migrations.
    Migrate,
    /// Check PostgreSQL connectivity and migration readiness.
    Check,
    /// Platform-operations read of the anonymous Human login-failure ledger.
    ListHumanLoginFailures {
        /// Only rows with this reason code are returned.
        #[arg(long)]
        reason: Option<String>,
        /// Only rows for this Human browser client (`admin` or `mail`) are returned.
        #[arg(long)]
        client: Option<String>,
        /// Only rows at or after this RFC 3339 timestamp are returned.
        #[arg(long)]
        since: Option<String>,
        /// Maximum rows returned (1-500).
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=500))]
        limit: u32,
    },
    /// Check the configured mail engine through authenticated JMAP discovery.
    CheckMailEngine,
    /// Resolve and evaluate one mail domain without changing control-plane state.
    CheckDomainDns {
        #[arg(long)]
        domain: String,
        /// File containing the current ownership token; plaintext is never printed.
        #[arg(long)]
        ownership_token_file: String,
        #[arg(long, default_value = "mail")]
        dkim_selector: String,
    },
    /// Independently recompute and verify one tenant's complete audit hash chain.
    VerifyAuditChain {
        /// Organization whose audit chain will be verified read-only.
        #[arg(long)]
        organization_id: String,
    },
    /// Sign and persist the currently verified tenant audit-chain head.
    CreateAuditCheckpoint {
        #[arg(long)]
        organization_id: String,
        /// Active Principal with the tenant audit.export permission.
        #[arg(long)]
        actor_principal_id: String,
        /// Optional stable UUIDv7 for safe operator retries.
        #[arg(long)]
        checkpoint_id: Option<String>,
        #[arg(long)]
        key_id: String,
        #[arg(long)]
        private_key_file: String,
        #[arg(long)]
        public_key_file: String,
        /// Expected WORM archive object key; no object is uploaded by this command.
        #[arg(long)]
        archive_object_key: String,
    },
    /// Export one immutable checkpoint prefix into a new portable directory.
    ExportAuditCheckpoint {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        checkpoint_id: String,
        /// Must name a directory that does not already exist.
        #[arg(long)]
        output_directory: String,
    },
    /// Verify a portable export without configuration or database access.
    VerifyAuditExport {
        #[arg(long)]
        export_directory: String,
        /// Separately trusted RSA public key; exports never define their own trust root.
        #[arg(long)]
        public_key_file: String,
    },
    /// Verify and upload a portable export to configured S3 Object Lock storage.
    ArchiveAuditExport {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        actor_principal_id: String,
        #[arg(long)]
        checkpoint_id: String,
        /// Optional stable UUIDv7 for safe retries after either remote or DB failure.
        #[arg(long)]
        archive_id: Option<String>,
        #[arg(long)]
        export_directory: String,
        /// Separately trusted RSA public key.
        #[arg(long)]
        public_key_file: String,
    },
    /// Open an auditable compliance case for Legal Hold operations.
    CreateComplianceCase {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        actor_principal_id: String,
        #[arg(long)]
        external_reference: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        purpose: String,
    },
    /// Close a compliance case only when it has no active Legal Hold.
    CloseComplianceCase {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        actor_principal_id: String,
        #[arg(long)]
        case_id: String,
    },
    /// Apply an approved Legal Hold to both exact audit archive object versions.
    ApplyAuditLegalHold {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        actor_principal_id: String,
        #[arg(long)]
        case_id: String,
        #[arg(long)]
        archive_id: String,
        #[arg(long)]
        approval_request_id: String,
        #[arg(long)]
        reason: String,
    },
    /// Release an audit Legal Hold after a separate exact approval.
    ReleaseAuditLegalHold {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        actor_principal_id: String,
        #[arg(long)]
        hold_id: String,
        #[arg(long)]
        approval_request_id: String,
    },
    /// Create the first organization and root service identity offline.
    BootstrapOrganization {
        /// Canonical organization slug, for example `example-corp`.
        #[arg(long)]
        slug: String,
        /// Human-readable organization name.
        #[arg(long)]
        display_name: String,
        /// Root service identity retained for bootstrap audit continuity.
        #[arg(long, default_value = "CopilotMail Bootstrap")]
        root_principal_name: String,
    },
    /// Hand organization ownership to the first enterprise OIDC human.
    BindOidcOwner {
        /// Organization ID returned by `bootstrap-organization`.
        #[arg(long)]
        organization_id: String,
        /// Stable OIDC `sub` claim for the initial owner.
        #[arg(long)]
        subject: String,
        /// Human-readable directory name.
        #[arg(long)]
        display_name: String,
    },
    /// Issue a tenant-scoped SCIM token and print its plaintext once.
    IssueScimToken {
        /// Organization whose enterprise directory may be managed.
        #[arg(long)]
        organization_id: String,
        /// Operator-visible credential label.
        #[arg(long)]
        label: String,
        /// Token lifetime in days (maximum 365).
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u32).range(1..=365))]
        expires_in_days: u32,
    },
    /// Immediately revoke a SCIM provisioning token.
    RevokeScimToken {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        token_id: String,
    },
    /// Create a mailbox and bind the tenant Owner.
    CreateMailbox {
        #[arg(long)]
        organization_id: String,
        #[arg(long, default_value = "shared")]
        kind: String,
        #[arg(long)]
        primary_address: String,
    },
    /// Grant explicit, expiring capabilities on one mailbox.
    GrantMailboxDelegation {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        mailbox_id: String,
        #[arg(long)]
        grantee_principal_id: String,
        #[arg(
            long = "capability",
            value_delimiter = ',',
            default_value = "mailbox.read"
        )]
        capabilities: Vec<String>,
        #[arg(long)]
        reason: String,
        #[arg(long, default_value_t = 24, value_parser = clap::value_parser!(u32).range(1..=8760))]
        valid_in_hours: u32,
    },
    /// Immediately revoke a mailbox delegation.
    RevokeMailboxDelegation {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        delegation_id: String,
    },
    /// Create and activate an immutable approval policy version 1.
    CreateApprovalPolicy {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        policy_key: String,
        #[arg(long)]
        display_name: String,
        #[arg(long)]
        action: String,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=5))]
        required_approvals: u8,
        #[arg(long, default_value = "{}")]
        rule_json: String,
    },
    /// Append and activate a new immutable approval policy version.
    AppendApprovalPolicyVersion {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        policy_key: String,
        #[arg(long)]
        action: String,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=5))]
        required_approvals: u8,
        #[arg(long, default_value = "{}")]
        rule_json: String,
    },
    /// Register an Agent and its first RSA private_key_jwt credential.
    RegisterAgent {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        display_name: String,
        #[arg(long)]
        purpose: String,
        #[arg(long, default_value = "production")]
        environment: String,
        #[arg(long, default_value = "medium")]
        risk_level: String,
        #[arg(long)]
        public_key_file: String,
        #[arg(
            long = "scope",
            value_delimiter = ',',
            default_value = "organization.read,agent.operate,mailbox.use"
        )]
        scopes: Vec<String>,
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u32).range(1..=365))]
        review_in_days: u32,
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u32).range(1..=365))]
        credential_expires_in_days: u32,
    },
    /// Immediately revoke one Agent credential.
    RevokeAgentCredential {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        credential_id: String,
    },
    /// Add an overlapping Agent credential for zero-downtime rotation.
    AddAgentCredential {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        client_id: String,
        #[arg(long)]
        public_key_file: String,
        #[arg(
            long = "scope",
            value_delimiter = ',',
            default_value = "organization.read,agent.operate,mailbox.use"
        )]
        scopes: Vec<String>,
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u32).range(1..=365))]
        credential_expires_in_days: u32,
    },
    /// Add a native TLS client-certificate credential for an Agent.
    AddAgentMtlsCredential {
        #[arg(long)]
        organization_id: String,
        #[arg(long)]
        client_id: String,
        #[arg(long)]
        certificate_file: String,
        #[arg(
            long = "scope",
            value_delimiter = ',',
            default_value = "organization.read,agent.operate,mailbox.use"
        )]
        scopes: Vec<String>,
        #[arg(long, default_value_t = 90, value_parser = clap::value_parser!(u32).range(1..=365))]
        credential_expires_in_days: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let _observability = init_observability().map_err(|error| anyhow::anyhow!(error))?;
    let cli = Cli::parse();
    if let Command::VerifyAuditExport {
        export_directory,
        public_key_file,
    } = &cli.command
    {
        return run_verify_audit_export(export_directory.clone(), public_key_file.clone()).await;
    }
    if let Command::CheckDomainDns {
        domain,
        ownership_token_file,
        dkim_selector,
    } = &cli.command
    {
        return run_check_domain_dns(
            domain.clone(),
            ownership_token_file.clone(),
            dkim_selector.clone(),
        )
        .await;
    }
    let settings = ControlPlaneSettings::from_env()?;
    let store = connect_store(&settings).await?;
    if matches!(&cli.command, Command::CheckMailEngine) {
        return run_check_mail_engine(&settings, &store).await;
    }

    match cli.command {
        Command::Serve => run_http(&settings, store).await,
        Command::Worker => run_worker(&settings, store).await,
        Command::AuditMaintenance => run_audit_maintenance_once(&settings, &store).await,
        Command::AuditMaintenanceWorker => run_audit_maintenance_worker(&settings, store).await,
        Command::Migrate => run_migrations(&settings, &store).await,
        Command::Check => run_check(&settings, &store).await,
        Command::ListHumanLoginFailures {
            reason,
            client,
            since,
            limit,
        } => run_list_human_login_failures(&store, reason, client, since, limit).await,
        Command::CheckMailEngine => unreachable!("handled after database connection"),
        Command::CheckDomainDns { .. } => unreachable!("handled before configuration"),
        Command::VerifyAuditChain { organization_id } => {
            run_verify_audit_chain(&store, organization_id).await
        }
        Command::CreateAuditCheckpoint {
            organization_id,
            actor_principal_id,
            checkpoint_id,
            key_id,
            private_key_file,
            public_key_file,
            archive_object_key,
        } => {
            run_create_audit_checkpoint(
                &store,
                organization_id,
                actor_principal_id,
                checkpoint_id,
                key_id,
                private_key_file,
                public_key_file,
                archive_object_key,
            )
            .await
        }
        Command::ExportAuditCheckpoint {
            organization_id,
            checkpoint_id,
            output_directory,
        } => {
            run_export_audit_checkpoint(&store, organization_id, checkpoint_id, output_directory)
                .await
        }
        Command::VerifyAuditExport { .. } => unreachable!("handled before database configuration"),
        Command::ArchiveAuditExport {
            organization_id,
            actor_principal_id,
            checkpoint_id,
            archive_id,
            export_directory,
            public_key_file,
        } => {
            run_archive_audit_export(
                &settings,
                &store,
                organization_id,
                actor_principal_id,
                checkpoint_id,
                archive_id,
                export_directory,
                public_key_file,
            )
            .await
        }
        Command::CreateComplianceCase {
            organization_id,
            actor_principal_id,
            external_reference,
            title,
            purpose,
        } => {
            run_create_compliance_case(
                &store,
                organization_id,
                actor_principal_id,
                external_reference,
                title,
                purpose,
            )
            .await
        }
        Command::CloseComplianceCase {
            organization_id,
            actor_principal_id,
            case_id,
        } => run_close_compliance_case(&store, organization_id, actor_principal_id, case_id).await,
        Command::ApplyAuditLegalHold {
            organization_id,
            actor_principal_id,
            case_id,
            archive_id,
            approval_request_id,
            reason,
        } => {
            run_apply_audit_legal_hold(
                &settings,
                &store,
                organization_id,
                actor_principal_id,
                case_id,
                archive_id,
                approval_request_id,
                reason,
            )
            .await
        }
        Command::ReleaseAuditLegalHold {
            organization_id,
            actor_principal_id,
            hold_id,
            approval_request_id,
        } => {
            run_release_audit_legal_hold(
                &settings,
                &store,
                organization_id,
                actor_principal_id,
                hold_id,
                approval_request_id,
            )
            .await
        }
        Command::BootstrapOrganization {
            slug,
            display_name,
            root_principal_name,
        } => run_bootstrap_organization(&store, slug, display_name, root_principal_name).await,
        Command::BindOidcOwner {
            organization_id,
            subject,
            display_name,
        } => run_bind_oidc_owner(&settings, &store, organization_id, subject, display_name).await,
        Command::IssueScimToken {
            organization_id,
            label,
            expires_in_days,
        } => run_issue_scim_token(&store, organization_id, label, expires_in_days).await,
        Command::RevokeScimToken {
            organization_id,
            token_id,
        } => run_revoke_scim_token(&store, organization_id, token_id).await,
        Command::CreateMailbox {
            organization_id,
            kind,
            primary_address,
        } => run_create_mailbox(&store, organization_id, kind, primary_address).await,
        Command::GrantMailboxDelegation {
            organization_id,
            mailbox_id,
            grantee_principal_id,
            capabilities,
            reason,
            valid_in_hours,
        } => {
            run_grant_mailbox_delegation(
                &store,
                organization_id,
                mailbox_id,
                grantee_principal_id,
                capabilities,
                reason,
                valid_in_hours,
            )
            .await
        }
        Command::RevokeMailboxDelegation {
            organization_id,
            delegation_id,
        } => run_revoke_mailbox_delegation(&store, organization_id, delegation_id).await,
        Command::CreateApprovalPolicy {
            organization_id,
            policy_key,
            display_name,
            action,
            required_approvals,
            rule_json,
        } => {
            run_create_approval_policy(
                &store,
                organization_id,
                policy_key,
                display_name,
                action,
                required_approvals,
                rule_json,
            )
            .await
        }
        Command::AppendApprovalPolicyVersion {
            organization_id,
            policy_key,
            action,
            required_approvals,
            rule_json,
        } => {
            run_append_approval_policy_version(
                &store,
                organization_id,
                policy_key,
                action,
                required_approvals,
                rule_json,
            )
            .await
        }
        Command::RegisterAgent {
            organization_id,
            display_name,
            purpose,
            environment,
            risk_level,
            public_key_file,
            scopes,
            review_in_days,
            credential_expires_in_days,
        } => {
            run_register_agent(
                &settings,
                &store,
                organization_id,
                display_name,
                purpose,
                environment,
                risk_level,
                public_key_file,
                scopes,
                review_in_days,
                credential_expires_in_days,
            )
            .await
        }
        Command::RevokeAgentCredential {
            organization_id,
            credential_id,
        } => run_revoke_agent_credential(&store, organization_id, credential_id).await,
        Command::AddAgentCredential {
            organization_id,
            client_id,
            public_key_file,
            scopes,
            credential_expires_in_days,
        } => {
            run_add_agent_credential(
                &store,
                organization_id,
                client_id,
                public_key_file,
                scopes,
                credential_expires_in_days,
            )
            .await
        }
        Command::AddAgentMtlsCredential {
            organization_id,
            client_id,
            certificate_file,
            scopes,
            credential_expires_in_days,
        } => {
            copilotmail_control::run_add_agent_mtls_credential(
                &settings,
                &store,
                organization_id,
                client_id,
                certificate_file,
                scopes,
                credential_expires_in_days,
            )
            .await
        }
    }
}
