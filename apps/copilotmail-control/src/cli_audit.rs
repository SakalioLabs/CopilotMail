use crate::*;

/// Independently verifies a tenant-scoped audit chain and prints machine-readable evidence.
pub async fn run_verify_audit_chain(
    store: &PgControlPlaneStore,
    organization_id: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let report = store
        .verify_audit_chain(organization_id)
        .await
        .context("verify tenant audit hash chain")?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_create_audit_checkpoint(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    checkpoint_id: Option<String>,
    key_id: String,
    private_key_file: String,
    public_key_file: String,
    archive_object_key: String,
) -> Result<()> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let actor_principal_id = actor_principal_id
        .parse::<PrincipalId>()
        .context("actor Principal ID is not a UUID")?;
    let checkpoint_id = checkpoint_id
        .map(|value| {
            value
                .parse::<AuditCheckpointId>()
                .context("checkpoint ID is not a UUID")
        })
        .transpose()?
        .unwrap_or_default();
    let private_key = tokio::fs::read(&private_key_file)
        .await
        .with_context(|| format!("read audit private key file {private_key_file}"))?;
    let public_key = tokio::fs::read(&public_key_file)
        .await
        .with_context(|| format!("read audit public key file {public_key_file}"))?;
    let signer = RsaAuditCheckpointSigner::new(key_id, &private_key, &public_key)
        .context("validate audit checkpoint signing key pair")?;
    let result = CreateAuditCheckpointService::new(store.clone(), signer)
        .execute(CreateAuditCheckpointCommand {
            checkpoint_id,
            organization_id,
            actor_principal_id,
            archive_object_key,
            trace_id: Uuid::now_v7().to_string(),
            now: Utc::now(),
        })
        .await
        .context("create signed audit checkpoint")?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

pub async fn run_export_audit_checkpoint(
    store: &PgControlPlaneStore,
    organization_id: String,
    checkpoint_id: String,
    output_directory: String,
) -> Result<()> {
    run_export_audit_checkpoint_traced(
        store,
        organization_id,
        checkpoint_id,
        output_directory,
        None,
    )
    .await
}

pub(crate) async fn run_export_audit_checkpoint_traced(
    store: &PgControlPlaneStore,
    organization_id: String,
    checkpoint_id: String,
    output_directory: String,
    trace_id: Option<&str>,
) -> Result<()> {
    const PAGE_SIZE: i64 = 1_000;
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let checkpoint_id = checkpoint_id
        .parse::<AuditCheckpointId>()
        .context("checkpoint ID is not a UUID")?;
    let generated_trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
    let trace_id = trace_id.unwrap_or(&generated_trace_id);
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        "audit checkpoint export started"
    );
    let stored = store
        .get_signed_audit_checkpoint(organization_id, checkpoint_id)
        .await
        .context("load signed audit checkpoint")?
        .context("signed audit checkpoint does not exist in this organization")?;
    anyhow::ensure!(
        stored.checkpoint.payload.organization_id == organization_id
            && stored.checkpoint.payload.checkpoint_id == checkpoint_id,
        "stored checkpoint identity does not match request"
    );
    let chain_report = store
        .verify_audit_chain(organization_id)
        .await
        .context("reverify tenant audit chain before export")?;
    anyhow::ensure!(
        chain_report.event_count >= stored.checkpoint.payload.event_count,
        "current audit chain is shorter than signed checkpoint"
    );

    let output_directory = PathBuf::from(output_directory);
    anyhow::ensure!(
        !output_directory.as_os_str().is_empty(),
        "output directory must not be empty"
    );
    tokio::fs::create_dir(&output_directory)
        .await
        .with_context(|| {
            format!(
                "create new export directory {} (it must not already exist)",
                output_directory.display()
            )
        })?;
    let events_temporary = output_directory.join(".events.jsonl.tmp");
    let events_final = output_directory.join(copilotmail_audit::EVENTS_FILE_NAME);
    let manifest_temporary = output_directory.join(".manifest.json.tmp");
    let manifest_final = output_directory.join("manifest.json");
    let mut events_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&events_temporary)
        .await
        .context("create temporary audit event export")?;
    let mut digest = Sha256::new();
    let mut after_sequence = 0_i64;
    let mut exported_count = 0_u64;

    loop {
        let page = store
            .load_audit_export_page(
                organization_id,
                after_sequence,
                stored.checkpoint.payload.chain_sequence_end,
                PAGE_SIZE,
            )
            .await
            .context("load audit export page")?;
        if page.is_empty() {
            break;
        }
        for record in &page {
            let mut line = serde_json::to_vec(record)?;
            line.push(b'\n');
            digest.update(&line);
            events_file.write_all(&line).await?;
            after_sequence = record.chain_sequence;
            exported_count = exported_count
                .checked_add(1)
                .context("audit export event count overflow")?;
        }
        if page.len() < usize::try_from(PAGE_SIZE).expect("positive page size") {
            break;
        }
    }
    events_file.sync_all().await?;
    drop(events_file);
    anyhow::ensure!(
        exported_count == stored.checkpoint.payload.event_count
            && after_sequence == stored.checkpoint.payload.chain_sequence_end,
        "exported audit rows do not match signed checkpoint"
    );
    let events_sha256 = encode_bytes_hex(&digest.finalize());
    let manifest = AuditExportManifest::new(stored.checkpoint, events_sha256, Utc::now())?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let mut manifest_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&manifest_temporary)
        .await
        .context("create temporary audit manifest")?;
    manifest_file.write_all(&manifest_bytes).await?;
    manifest_file.write_all(b"\n").await?;
    manifest_file.sync_all().await?;
    drop(manifest_file);
    tokio::fs::rename(&events_temporary, &events_final)
        .await
        .context("publish audit events file")?;
    tokio::fs::rename(&manifest_temporary, &manifest_final)
        .await
        .context("publish audit manifest")?;

    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "organization_id": organization_id,
            "checkpoint_id": checkpoint_id,
            "event_count": exported_count,
            "manifest_file": manifest_final,
            "events_file": events_final,
            "archive_object_key": stored.archive_object_key,
        }))?
    );
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        event_count = exported_count,
        "audit checkpoint export completed"
    );
    Ok(())
}

pub async fn run_verify_audit_export(
    export_directory: String,
    public_key_file: String,
) -> Result<()> {
    let (_, report) = verify_audit_export_files(
        &PathBuf::from(export_directory),
        &PathBuf::from(public_key_file),
    )?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

pub(crate) fn verify_audit_export_files(
    export_directory: &FsPath,
    public_key_file: &FsPath,
) -> Result<(AuditExportManifest, AuditExportVerificationReport)> {
    const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
    const MAX_EVENT_LINE_BYTES: usize = 256 * 1024;
    let manifest_path = export_directory.join("manifest.json");
    let manifest_metadata = std::fs::metadata(&manifest_path)
        .with_context(|| format!("inspect {}", manifest_path.display()))?;
    anyhow::ensure!(
        manifest_metadata.is_file() && manifest_metadata.len() <= MAX_MANIFEST_BYTES,
        "audit manifest is not a bounded regular file"
    );
    let manifest: AuditExportManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path)
            .with_context(|| format!("read {}", manifest_path.display()))?,
    )
    .context("parse audit export manifest")?;
    let public_key = std::fs::read(public_key_file).with_context(|| {
        format!(
            "read trusted audit public key {}",
            public_key_file.display()
        )
    })?;
    let mut verifier =
        AuditExportVerifier::new(&manifest, &public_key).context("verify checkpoint signature")?;
    let events_path = export_directory.join(&manifest.events_file);
    let mut reader = BufReader::new(
        File::open(&events_path).with_context(|| format!("open {}", events_path.display()))?,
    );
    let mut digest = Sha256::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        anyhow::ensure!(
            line.len() <= MAX_EVENT_LINE_BYTES,
            "audit export record exceeds size limit"
        );
        anyhow::ensure!(
            line.last() == Some(&b'\n'),
            "audit event export must end every record with LF"
        );
        digest.update(&line);
        line.pop();
        anyhow::ensure!(
            !line.is_empty(),
            "audit event export contains an empty line"
        );
        let record: AuditExportRecord =
            serde_json::from_slice(&line).context("parse audit export record")?;
        verifier
            .push(&record)
            .context("verify audit export chain")?;
    }
    let events_sha256 = encode_bytes_hex(&digest.finalize());
    let report = verifier
        .finish(&manifest, &events_sha256)
        .context("verify audit export against signed checkpoint")?;
    Ok((manifest, report))
}

#[allow(clippy::too_many_arguments)]
pub async fn run_archive_audit_export(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    checkpoint_id: String,
    archive_id: Option<String>,
    export_directory: String,
    public_key_file: String,
) -> Result<()> {
    run_archive_audit_export_traced(
        settings,
        store,
        organization_id,
        actor_principal_id,
        checkpoint_id,
        archive_id,
        export_directory,
        public_key_file,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_archive_audit_export_traced(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    checkpoint_id: String,
    archive_id: Option<String>,
    export_directory: String,
    public_key_file: String,
    trace_id: Option<&str>,
) -> Result<()> {
    let archive_settings = settings
        .audit_archive
        .as_ref()
        .context("immutable audit archive is not configured")?;
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let actor_principal_id = actor_principal_id
        .parse::<PrincipalId>()
        .context("actor Principal ID is not a UUID")?;
    let checkpoint_id = checkpoint_id
        .parse::<AuditCheckpointId>()
        .context("checkpoint ID is not a UUID")?;
    let archive_id = archive_id
        .map(|value| {
            value
                .parse::<AuditArchiveId>()
                .context("archive ID is not a UUID")
        })
        .transpose()?
        .unwrap_or_default();
    let generated_trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
    let trace_id = trace_id.unwrap_or(&generated_trace_id);
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        %archive_id,
        "immutable audit archive started"
    );
    let export_directory = PathBuf::from(export_directory);
    let (manifest, report) =
        verify_audit_export_files(&export_directory, &PathBuf::from(public_key_file))
            .context("offline verification failed; nothing was uploaded")?;
    anyhow::ensure!(
        report.organization_id == organization_id && report.checkpoint_id == checkpoint_id,
        "verified export identity does not match archive request"
    );
    let stored = store
        .get_signed_audit_checkpoint(organization_id, checkpoint_id)
        .await
        .context("load signed audit checkpoint")?
        .context("signed audit checkpoint does not exist in this organization")?;
    anyhow::ensure!(
        manifest.checkpoint == stored.checkpoint,
        "portable export checkpoint differs from the database checkpoint"
    );

    let events_key = stored.archive_object_key;
    let manifest_key = format!("{events_key}.manifest.json");
    anyhow::ensure!(
        manifest_key.len() <= 1_024,
        "checkpoint archive object key leaves no room for manifest suffix"
    );
    let events_path = export_directory.join(&manifest.events_file);
    let manifest_path = export_directory.join("manifest.json");
    let manifest_bytes = tokio::fs::read(&manifest_path)
        .await
        .context("read verified audit manifest")?;
    let manifest_sha256 = encode_bytes_hex(&Sha256::digest(&manifest_bytes));
    let requested_retain_until = manifest
        .exported_at
        .checked_add_signed(chrono::Duration::days(i64::from(
            archive_settings.retention_days,
        )))
        .context("audit archive retention timestamp overflow")?;
    let upload_started_at = Utc::now();
    anyhow::ensure!(
        requested_retain_until > upload_started_at,
        "configured retention period has already elapsed for this export; create a fresh export"
    );
    let retention_mode = match archive_settings.retention_mode {
        AuditArchiveRetentionMode::Governance => WormRetentionMode::Governance,
        AuditArchiveRetentionMode::Compliance => WormRetentionMode::Compliance,
    };
    let archive = S3WormArchive::connect(S3WormArchiveConfig {
        endpoint_url: archive_settings.endpoint_url.clone(),
        region: archive_settings.region.clone(),
        bucket: archive_settings.bucket.clone(),
        force_path_style: archive_settings.force_path_style,
        allow_insecure_http: archive_settings.allow_insecure_http,
    })
    .await
    .context("initialize immutable audit archive")?;
    let metadata = audit_archive_object_metadata(
        organization_id,
        checkpoint_id,
        archive_id,
        manifest.format_version,
        trace_id,
    );
    let events = archive
        .put_immutable(
            &WormObjectSource {
                key: events_key,
                content_type: "application/x-ndjson".into(),
                path: events_path,
                sha256_hex: manifest.events_sha256.clone(),
                metadata: metadata.clone(),
            },
            retention_mode,
            requested_retain_until,
            upload_started_at,
        )
        .await
        .context("upload and verify immutable audit event object")?;
    let manifest_object = archive
        .put_immutable(
            &WormObjectSource {
                key: manifest_key,
                content_type: "application/json".into(),
                path: manifest_path,
                sha256_hex: manifest_sha256,
                metadata,
            },
            retention_mode,
            requested_retain_until,
            upload_started_at,
        )
        .await
        .context("upload and verify immutable audit manifest object")?;
    let verified_at = Utc::now();
    let retain_until = events.retain_until.min(manifest_object.retain_until);
    let record = AuditArchiveRecord {
        archive_id,
        organization_id,
        checkpoint_id,
        provider: "s3".into(),
        bucket: archive_settings.bucket.clone(),
        events: archived_object_record(events),
        manifest: archived_object_record(manifest_object),
        retention_mode: retention_mode.as_str().into(),
        retain_until,
        verified_at,
        created_by: actor_principal_id,
        created_at: Utc::now(),
    };
    let result = RecordAuditArchiveService::new(store.clone())
        .execute(RecordAuditArchiveCommand {
            archive: record,
            actor_principal_id,
            trace_id: trace_id.to_owned(),
        })
        .await
        .context("record verified immutable archive evidence")?;
    println!("{}", serde_json::to_string(&result)?);
    tracing::info!(
        trace_id,
        %organization_id,
        %checkpoint_id,
        %archive_id,
        "immutable audit archive completed"
    );
    Ok(())
}

pub(crate) fn archived_object_record(evidence: ArchivedObjectEvidence) -> ArchivedObjectRecord {
    ArchivedObjectRecord {
        key: evidence.key,
        version_id: evidence.version_id,
        sha256_hex: evidence.sha256_hex,
        size_bytes: evidence.size_bytes,
    }
}

pub async fn run_create_compliance_case(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    external_reference: String,
    title: String,
    purpose: String,
) -> Result<()> {
    let (organization_id, actor_principal_id, context, now) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let case = ComplianceGovernanceService::new(store.clone())
        .create_case(CreateComplianceCaseCommand {
            context,
            external_reference,
            title,
            purpose,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("create compliance case")?;
    anyhow::ensure!(
        case.organization_id() == organization_id && case.created_by() == actor_principal_id,
        "created compliance case identity does not match the request"
    );
    println!("{}", serde_json::to_string(&case)?);
    Ok(())
}

pub async fn run_close_compliance_case(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    case_id: String,
) -> Result<()> {
    let (_, _, context, now) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let case_id = case_id
        .parse::<ComplianceCaseId>()
        .context("compliance case ID is not a UUID")?;
    let case = ComplianceGovernanceService::new(store.clone())
        .close_case(CloseComplianceCaseCommand {
            context,
            case_id,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .context("close compliance case")?;
    println!("{}", serde_json::to_string(&case)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn run_apply_audit_legal_hold(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    case_id: String,
    archive_id: String,
    approval_request_id: String,
    reason: String,
) -> Result<()> {
    let (organization_id, requested_by, context, _) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let case_id = case_id
        .parse::<ComplianceCaseId>()
        .context("compliance case ID is not a UUID")?;
    let archive_id = archive_id
        .parse::<AuditArchiveId>()
        .context("audit archive ID is not a UUID")?;
    let approval_request_id = approval_request_id
        .parse::<ApprovalRequestId>()
        .context("approval request ID is not a UUID")?;
    let approval = legal_hold_approval(
        store,
        organization_id,
        approval_request_id,
        requested_by,
        "audit.archive",
        archive_id.to_string(),
    )
    .await?;
    if let Some(existing) = store
        .get_legal_hold_by_apply_approval(organization_id, approval_request_id)
        .await
        .context("read existing Legal Hold replay state")?
    {
        anyhow::ensure!(
            existing.case_id() == case_id
                && existing.archive_id() == archive_id
                && existing.reason() == reason
                && existing.status() == LegalHoldStatus::Active,
            "approval request was already consumed by a different or released Legal Hold"
        );
        println!("{}", serde_json::to_string(&existing)?);
        return Ok(());
    }
    let lock = store
        .try_acquire_audit_legal_hold_lock(organization_id, archive_id)
        .await
        .context("acquire archive Legal Hold operation lock")?
        .context("another Legal Hold operation is active for this archive")?;
    let result = async {
        if let Some(existing) = store
            .get_legal_hold_by_apply_approval(organization_id, approval_request_id)
            .await
            .context("recheck Legal Hold replay state")?
        {
            anyhow::ensure!(
                existing.case_id() == case_id
                    && existing.archive_id() == archive_id
                    && existing.reason() == reason
                    && existing.status() == LegalHoldStatus::Active,
                "approval request was already consumed by a different or released Legal Hold"
            );
            return Ok(existing);
        }
        let archive_record = store
            .get_audit_archive(organization_id, archive_id)
            .await
            .context("load immutable audit archive")?
            .context("audit archive does not exist in this organization")?;
        let archive = connect_legal_hold_archive(settings, &archive_record).await?;
        let events = archive
            .set_legal_hold(
                &archive_record.events.key,
                &archive_record.events.version_id,
                true,
            )
            .await
            .context("apply Legal Hold to audit events object version")?;
        let manifest = archive
            .set_legal_hold(
                &archive_record.manifest.key,
                &archive_record.manifest.version_id,
                true,
            )
            .await
            .context("apply Legal Hold to audit manifest object version")?;
        let now = Utc::now();
        ComplianceGovernanceService::new(store.clone())
            .apply_hold(ApplyLegalHoldCommand {
                context,
                case_id,
                archive_id,
                reason,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: true,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .context("record applied Legal Hold")
    }
    .await;
    lock.release()
        .await
        .context("release archive Legal Hold operation lock")?;
    let hold = result?;
    println!("{}", serde_json::to_string(&hold)?);
    Ok(())
}

pub async fn run_release_audit_legal_hold(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
    hold_id: String,
    approval_request_id: String,
) -> Result<()> {
    let (organization_id, requested_by, context, _) =
        resolve_compliance_actor(store, organization_id, actor_principal_id).await?;
    let hold_id = hold_id
        .parse::<LegalHoldId>()
        .context("Legal Hold ID is not a UUID")?;
    let approval_request_id = approval_request_id
        .parse::<ApprovalRequestId>()
        .context("approval request ID is not a UUID")?;
    let approval = legal_hold_approval(
        store,
        organization_id,
        approval_request_id,
        requested_by,
        "compliance.legal_hold",
        hold_id.to_string(),
    )
    .await?;
    let current = store
        .get_legal_hold(organization_id, hold_id)
        .await
        .context("load Legal Hold")?
        .context("Legal Hold does not exist in this organization")?;
    if current.status() == LegalHoldStatus::Released {
        anyhow::ensure!(
            current.release_approval_request_id() == Some(approval_request_id),
            "Legal Hold was released using a different approval request"
        );
        println!("{}", serde_json::to_string(&current)?);
        return Ok(());
    }
    let archive_id = current.archive_id();
    let lock = store
        .try_acquire_audit_legal_hold_lock(organization_id, archive_id)
        .await
        .context("acquire archive Legal Hold operation lock")?
        .context("another Legal Hold operation is active for this archive")?;
    let result = async {
        let current = store
            .get_legal_hold(organization_id, hold_id)
            .await
            .context("recheck Legal Hold state")?
            .context("Legal Hold no longer exists")?;
        if current.status() == LegalHoldStatus::Released {
            anyhow::ensure!(
                current.release_approval_request_id() == Some(approval_request_id),
                "Legal Hold was released using a different approval request"
            );
            return Ok(current);
        }
        let archive_record = store
            .get_audit_archive(organization_id, archive_id)
            .await
            .context("load immutable audit archive")?
            .context("audit archive does not exist in this organization")?;
        let archive = connect_legal_hold_archive(settings, &archive_record).await?;
        // The database remains Active until both exact object versions read
        // back Off. A partial remote release is therefore recoverable by retry.
        let manifest = archive
            .set_legal_hold(
                &archive_record.manifest.key,
                &archive_record.manifest.version_id,
                false,
            )
            .await
            .context("release Legal Hold from audit manifest object version")?;
        let events = archive
            .set_legal_hold(
                &archive_record.events.key,
                &archive_record.events.version_id,
                false,
            )
            .await
            .context("release Legal Hold from audit events object version")?;
        let now = Utc::now();
        ComplianceGovernanceService::new(store.clone())
            .release_hold(ReleaseLegalHoldCommand {
                context,
                hold_id,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: false,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .context("record released Legal Hold")
    }
    .await;
    lock.release()
        .await
        .context("release archive Legal Hold operation lock")?;
    let hold = result?;
    println!("{}", serde_json::to_string(&hold)?);
    Ok(())
}

pub(crate) async fn resolve_compliance_actor(
    store: &PgControlPlaneStore,
    organization_id: String,
    actor_principal_id: String,
) -> Result<(
    OrganizationId,
    PrincipalId,
    AuthorizationContext,
    DateTime<Utc>,
)> {
    let organization_id = organization_id
        .parse::<OrganizationId>()
        .context("organization ID is not a UUID")?;
    let actor_principal_id = actor_principal_id
        .parse::<PrincipalId>()
        .context("actor Principal ID is not a UUID")?;
    let now = Utc::now();
    let context = store
        .resolve_authorization(actor_principal_id, now)
        .await
        .context("resolve live actor authorization")?
        .context("actor Principal is inactive or has no current role assignment")?;
    anyhow::ensure!(
        context.organization_id == organization_id
            && context.permits(organization_id, Permission::LegalHoldManage),
        "actor is not authorized to manage Legal Holds in this organization"
    );
    anyhow::ensure!(
        store
            .active_human_compliance_actor_exists(organization_id, actor_principal_id)
            .await
            .context("verify Active Human compliance actor")?,
        "Legal Hold operations require an Active Human actor"
    );
    Ok((organization_id, actor_principal_id, context, now))
}

pub(crate) async fn legal_hold_approval(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    request_id: ApprovalRequestId,
    requested_by: PrincipalId,
    resource_kind: &str,
    resource_id: String,
) -> Result<ApprovalProof> {
    let proof = ApprovalProof {
        organization_id,
        request_id,
        requested_by,
        action: ApprovalAction::LegalHold,
        resource_kind: resource_kind.into(),
        resource_id,
        now: Utc::now(),
    };
    anyhow::ensure!(
        store
            .approval_permits(&proof)
            .await
            .context("verify exact Legal Hold approval")?,
        "Legal Hold approval is missing, incomplete, expired, or targets another resource"
    );
    Ok(proof)
}

pub(crate) async fn connect_legal_hold_archive(
    settings: &ControlPlaneSettings,
    record: &AuditArchiveRecord,
) -> Result<S3WormArchive> {
    let archive_settings = settings
        .audit_archive
        .as_ref()
        .context("immutable audit archive is not configured")?;
    connect_legal_hold_archive_settings(archive_settings, record).await
}

pub(crate) async fn connect_legal_hold_archive_settings(
    archive_settings: &AuditArchiveSettings,
    record: &AuditArchiveRecord,
) -> Result<S3WormArchive> {
    anyhow::ensure!(
        record.provider == "s3" && record.bucket == archive_settings.bucket,
        "persisted audit archive does not match the configured immutable bucket"
    );
    S3WormArchive::connect(S3WormArchiveConfig {
        endpoint_url: archive_settings.endpoint_url.clone(),
        region: archive_settings.region.clone(),
        bucket: archive_settings.bucket.clone(),
        force_path_style: archive_settings.force_path_style,
        allow_insecure_http: archive_settings.allow_insecure_http,
    })
    .await
    .context("initialize immutable audit archive")
}

pub(crate) fn audit_archive_object_metadata(
    organization_id: OrganizationId,
    checkpoint_id: AuditCheckpointId,
    archive_id: AuditArchiveId,
    format_version: u16,
    trace_id: &str,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("organization-id".into(), organization_id.to_string()),
        ("checkpoint-id".into(), checkpoint_id.to_string()),
        ("archive-id".into(), archive_id.to_string()),
        ("format-version".into(), format_version.to_string()),
        ("trace-id".into(), trace_id.to_owned()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archived_object_record_copies_exact_evidence_fields() {
        let evidence = ArchivedObjectEvidence {
            bucket: "audit-archive".to_owned(),
            key: "org/checkpoint/archive.json".to_owned(),
            version_id: "v1".to_owned(),
            sha256_hex: "a".repeat(64),
            size_bytes: 42,
            retention_mode: WormRetentionMode::Compliance,
            retain_until: Utc::now(),
        };
        let record = archived_object_record(evidence);
        assert_eq!(record.key, "org/checkpoint/archive.json");
        assert_eq!(record.version_id, "v1");
        assert_eq!(record.sha256_hex.len(), 64);
        assert_eq!(record.size_bytes, 42);
    }

    #[test]
    fn audit_archive_object_metadata_carries_exact_trace_fields() {
        let organization_id = OrganizationId::new();
        let checkpoint_id = AuditCheckpointId::new();
        let archive_id = AuditArchiveId::new();
        let metadata =
            audit_archive_object_metadata(organization_id, checkpoint_id, archive_id, 1, "trace-1");
        assert_eq!(
            metadata.get("organization-id").unwrap(),
            &organization_id.to_string()
        );
        assert_eq!(
            metadata.get("checkpoint-id").unwrap(),
            &checkpoint_id.to_string()
        );
        assert_eq!(metadata.get("archive-id").unwrap(), &archive_id.to_string());
        assert_eq!(metadata.get("format-version").unwrap(), "1");
        assert_eq!(metadata.get("trace-id").unwrap(), "trace-1");
        assert_eq!(metadata.len(), 5);
        assert!(!metadata.values().any(|value| value.contains("body")));
    }
}
