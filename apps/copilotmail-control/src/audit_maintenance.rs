use crate::*;

#[derive(Debug, serde::Serialize)]
pub(crate) struct AuditMaintenanceOutcome {
    pub(crate) trace_id: String,
    pub(crate) organization_id: OrganizationId,
    pub(crate) checkpoint_id: AuditCheckpointId,
    pub(crate) archive_id: AuditArchiveId,
    pub(crate) chain_sequence_end: i64,
    pub(crate) resumed_checkpoint: bool,
}

#[derive(Clone)]
pub(crate) struct AuditMaintenanceMetrics {
    pub(crate) organization_id: OrganizationId,
    pub(crate) failure_threshold: u32,
    pub(crate) max_pending_age_seconds: u64,
    pub(crate) max_cycle_seconds: u64,
    pub(crate) state: Arc<Mutex<AuditMaintenanceMetricState>>,
}

#[derive(Debug)]
pub(crate) struct AuditMaintenanceMetricState {
    pub(crate) started_at: i64,
    pub(crate) cycle_started_at: i64,
    pub(crate) last_completed_at: i64,
    pub(crate) last_success_at: i64,
    pub(crate) cycles_total: u64,
    pub(crate) successes_total: u64,
    pub(crate) failures_total: u64,
    pub(crate) lock_conflicts_total: u64,
    pub(crate) consecutive_failures: u32,
    pub(crate) cycle_in_progress: bool,
    pub(crate) pending_checkpoint_age_seconds: u64,
}

impl AuditMaintenanceMetrics {
    pub(crate) fn new(
        organization_id: OrganizationId,
        settings: &AuditMaintenanceSettings,
    ) -> Self {
        Self::with_limits(
            organization_id,
            settings.failure_threshold,
            settings.max_pending_age.as_secs(),
            settings.max_cycle_duration.as_secs(),
        )
    }

    pub(crate) fn with_limits(
        organization_id: OrganizationId,
        failure_threshold: u32,
        max_pending_age_seconds: u64,
        max_cycle_seconds: u64,
    ) -> Self {
        Self {
            organization_id,
            failure_threshold,
            max_pending_age_seconds,
            max_cycle_seconds,
            state: Arc::new(Mutex::new(AuditMaintenanceMetricState {
                started_at: Utc::now().timestamp(),
                cycle_started_at: 0,
                last_completed_at: 0,
                last_success_at: 0,
                cycles_total: 0,
                successes_total: 0,
                failures_total: 0,
                lock_conflicts_total: 0,
                consecutive_failures: 0,
                cycle_in_progress: false,
                pending_checkpoint_age_seconds: 0,
            })),
        }
    }

    pub(crate) fn cycle_started(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.cycles_total = state.cycles_total.saturating_add(1);
        state.cycle_started_at = Utc::now().timestamp();
        state.cycle_in_progress = true;
    }

    pub(crate) fn pending_checkpoint(
        &self,
        checkpoint: Option<&copilotmail_postgres::StoredAuditCheckpoint>,
    ) {
        let age = checkpoint.map_or(0, |stored| {
            Utc::now()
                .signed_duration_since(stored.checkpoint.payload.created_at)
                .num_seconds()
                .max(0) as u64
        });
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pending_checkpoint_age_seconds = age;
    }

    pub(crate) fn succeeded(&self) {
        let now = Utc::now().timestamp();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.successes_total = state.successes_total.saturating_add(1);
        state.consecutive_failures = 0;
        state.last_success_at = now;
        state.last_completed_at = now;
        state.cycle_in_progress = false;
        state.pending_checkpoint_age_seconds = 0;
    }

    pub(crate) fn failed(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.failures_total = state.failures_total.saturating_add(1);
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        state.last_completed_at = Utc::now().timestamp();
        state.cycle_in_progress = false;
    }

    pub(crate) fn lock_conflict(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.lock_conflicts_total = state.lock_conflicts_total.saturating_add(1);
        state.last_completed_at = Utc::now().timestamp();
        state.cycle_in_progress = false;
    }

    pub(crate) fn render(&self) -> String {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let org = self.organization_id;
        format!(
            concat!(
                "# HELP copilotmail_audit_maintenance_cycles_total Maintenance cycles started.\n",
                "# TYPE copilotmail_audit_maintenance_cycles_total counter\n",
                "copilotmail_audit_maintenance_cycles_total{{organization_id=\"{org}\"}} {cycles}\n",
                "# HELP copilotmail_audit_maintenance_successes_total Successfully archived cycles.\n",
                "# TYPE copilotmail_audit_maintenance_successes_total counter\n",
                "copilotmail_audit_maintenance_successes_total{{organization_id=\"{org}\"}} {successes}\n",
                "# HELP copilotmail_audit_maintenance_failures_total Failed maintenance cycles.\n",
                "# TYPE copilotmail_audit_maintenance_failures_total counter\n",
                "copilotmail_audit_maintenance_failures_total{{organization_id=\"{org}\"}} {failures}\n",
                "# HELP copilotmail_audit_maintenance_lock_conflicts_total Cycles skipped due to another active instance.\n",
                "# TYPE copilotmail_audit_maintenance_lock_conflicts_total counter\n",
                "copilotmail_audit_maintenance_lock_conflicts_total{{organization_id=\"{org}\"}} {conflicts}\n",
                "# TYPE copilotmail_audit_maintenance_consecutive_failures gauge\n",
                "copilotmail_audit_maintenance_consecutive_failures{{organization_id=\"{org}\"}} {consecutive}\n",
                "# TYPE copilotmail_audit_maintenance_cycle_in_progress gauge\n",
                "copilotmail_audit_maintenance_cycle_in_progress{{organization_id=\"{org}\"}} {in_progress}\n",
                "# TYPE copilotmail_audit_maintenance_started_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_started_timestamp_seconds{{organization_id=\"{org}\"}} {started}\n",
                "# TYPE copilotmail_audit_maintenance_cycle_started_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_cycle_started_timestamp_seconds{{organization_id=\"{org}\"}} {cycle_started}\n",
                "# TYPE copilotmail_audit_maintenance_last_completed_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_last_completed_timestamp_seconds{{organization_id=\"{org}\"}} {completed}\n",
                "# TYPE copilotmail_audit_maintenance_last_success_timestamp_seconds gauge\n",
                "copilotmail_audit_maintenance_last_success_timestamp_seconds{{organization_id=\"{org}\"}} {success}\n",
                "# TYPE copilotmail_audit_maintenance_pending_checkpoint_age_seconds gauge\n",
                "copilotmail_audit_maintenance_pending_checkpoint_age_seconds{{organization_id=\"{org}\"}} {pending_age}\n"
            ),
            org = org,
            cycles = state.cycles_total,
            successes = state.successes_total,
            failures = state.failures_total,
            conflicts = state.lock_conflicts_total,
            consecutive = state.consecutive_failures,
            in_progress = u8::from(state.cycle_in_progress),
            started = state.started_at,
            cycle_started = state.cycle_started_at,
            completed = state.last_completed_at,
            success = state.last_success_at,
            pending_age = state.pending_checkpoint_age_seconds,
        )
    }

    pub(crate) fn readiness(&self) -> Result<(), &'static str> {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let now = Utc::now().timestamp();
        if state.consecutive_failures >= self.failure_threshold {
            return Err("failure_threshold_exceeded");
        }
        if state.pending_checkpoint_age_seconds > self.max_pending_age_seconds {
            return Err("pending_checkpoint_overdue");
        }
        if state.cycle_in_progress
            && now.saturating_sub(state.cycle_started_at) as u64 > self.max_cycle_seconds
        {
            return Err("cycle_duration_exceeded");
        }
        Ok(())
    }
}

pub(crate) fn audit_maintenance_observability_router(metrics: AuditMaintenanceMetrics) -> Router {
    Router::new()
        .route(
            "/metrics",
            get(
                |State(metrics): State<AuditMaintenanceMetrics>| async move {
                    (
                        [(
                            header::CONTENT_TYPE,
                            "text/plain; version=0.0.4; charset=utf-8",
                        )],
                        metrics.render(),
                    )
                },
            ),
        )
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({"status": "ok"})) }),
        )
        .route(
            "/readyz",
            get(
                |State(metrics): State<AuditMaintenanceMetrics>| async move {
                    match metrics.readiness() {
                        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"status": "ready"}))),
                        Err(reason) => (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(serde_json::json!({
                                "status": "not_ready",
                                "reason": reason
                            })),
                        ),
                    }
                },
            ),
        )
        .with_state(metrics)
}

/// Runs exactly one recoverable checkpoint/export/WORM maintenance cycle.
pub async fn run_audit_maintenance_once(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
) -> Result<()> {
    let maintenance = settings
        .audit_maintenance
        .as_ref()
        .context("audit maintenance is not configured")?;
    let (organization_id, actor_principal_id, signer) =
        load_audit_maintenance_identity(maintenance).await?;
    ensure_audit_maintenance_root(&maintenance.work_directory).await?;
    let trace_id = Uuid::now_v7().to_string();
    let lock = store
        .try_acquire_audit_maintenance_lock(organization_id)
        .await
        .context("acquire tenant audit maintenance lock")?
        .context("another audit maintenance instance is active for this tenant")?;
    let outcome = audit_maintenance_cycle(
        settings,
        maintenance,
        store,
        organization_id,
        actor_principal_id,
        &signer,
        &trace_id,
    )
    .await?;
    lock.release()
        .await
        .context("release tenant audit maintenance lock")?;
    println!("{}", serde_json::to_string(&outcome)?);
    Ok(())
}

/// Runs a dedicated single-tenant audit maintenance loop.
///
/// This process is intentionally separate from Outbox delivery: large audit
/// exports and S3 retries must never hold or starve application-event leases.
pub async fn run_audit_maintenance_worker(
    settings: &ControlPlaneSettings,
    store: PgControlPlaneStore,
) -> Result<()> {
    let maintenance = settings
        .audit_maintenance
        .as_ref()
        .context("audit maintenance is not configured")?;
    let (organization_id, actor_principal_id, signer) =
        load_audit_maintenance_identity(maintenance).await?;
    ensure_audit_maintenance_root(&maintenance.work_directory).await?;
    let metrics = AuditMaintenanceMetrics::new(organization_id, maintenance);
    let metrics_router = audit_maintenance_observability_router(metrics.clone());
    let metrics_listener = tokio::net::TcpListener::bind(maintenance.metrics_addr)
        .await
        .with_context(|| {
            format!(
                "bind audit maintenance metrics listener {}",
                maintenance.metrics_addr
            )
        })?;
    let (metrics_shutdown_tx, metrics_shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let mut metrics_server = tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_router)
            .with_graceful_shutdown(async move {
                let _ = metrics_shutdown_rx.await;
            })
            .await
    });
    let mut metrics_shutdown_tx = Some(metrics_shutdown_tx);
    let mut interval = tokio::time::interval(maintenance.interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut consecutive_failures = 0_u32;
    tracing::info!(
        %organization_id,
        %actor_principal_id,
        key_id = signer.key_id(),
        interval_seconds = maintenance.interval.as_secs(),
        metrics_addr = %maintenance.metrics_addr,
        "tenant audit maintenance worker started"
    );
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            result = &mut metrics_server => {
                return match result {
                    Ok(Ok(())) => anyhow::bail!("audit maintenance metrics server stopped unexpectedly"),
                    Ok(Err(error)) => Err(error).context("audit maintenance metrics server failed"),
                    Err(error) => Err(error).context("audit maintenance metrics task failed"),
                };
            }
            _ = &mut shutdown => {
                tracing::info!(%organization_id, "tenant audit maintenance worker stopping");
                if let Some(sender) = metrics_shutdown_tx.take() {
                    let _ = sender.send(());
                }
                metrics_server
                    .await
                    .context("join audit maintenance metrics server")?
                    .context("stop audit maintenance metrics server")?;
                return Ok(());
            }
            _ = interval.tick() => {
                let cycle_id = Uuid::now_v7().to_string();
                metrics.cycle_started();
                match store
                    .oldest_unarchived_audit_checkpoint(organization_id)
                    .await
                {
                    Ok(pending) => metrics.pending_checkpoint(pending.as_ref()),
                    Err(error) => {
                        metrics.failed();
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        tracing::error!(
                            %organization_id,
                            cycle_id,
                            consecutive_failures,
                            error = %error,
                            "tenant audit maintenance pending-checkpoint observation failed"
                        );
                        continue;
                    }
                }
                let lock = match store
                    .try_acquire_audit_maintenance_lock(organization_id)
                    .await
                {
                    Ok(Some(lock)) => lock,
                    Ok(None) => {
                        metrics.lock_conflict();
                        tracing::warn!(
                            %organization_id,
                            cycle_id,
                            "tenant audit maintenance cycle skipped because another instance is active"
                        );
                        continue;
                    }
                    Err(error) => {
                        metrics.failed();
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        tracing::error!(
                            %organization_id,
                            cycle_id,
                            consecutive_failures,
                            error = %error,
                            "tenant audit maintenance lock acquisition failed"
                        );
                        continue;
                    }
                };
                match audit_maintenance_cycle(
                    settings,
                    maintenance,
                    &store,
                    organization_id,
                    actor_principal_id,
                    &signer,
                    &cycle_id,
                )
                .await
                {
                    Ok(outcome) => {
                        match lock.release().await {
                            Ok(()) => {
                                metrics.succeeded();
                                consecutive_failures = 0;
                                tracing::info!(
                                    %organization_id,
                                    cycle_id,
                                    trace_id = outcome.trace_id,
                                    checkpoint_id = %outcome.checkpoint_id,
                                    archive_id = %outcome.archive_id,
                                    chain_sequence_end = outcome.chain_sequence_end,
                                    resumed_checkpoint = outcome.resumed_checkpoint,
                                    "tenant audit maintenance cycle completed"
                                );
                            }
                            Err(error) => {
                                metrics.failed();
                                consecutive_failures = consecutive_failures.saturating_add(1);
                                tracing::error!(
                                    %organization_id,
                                    cycle_id,
                                    consecutive_failures,
                                    error = %error,
                                    "tenant audit maintenance cycle completed but lock release failed"
                                );
                            }
                        }
                    }
                    Err(error) => {
                        drop(lock);
                        metrics.failed();
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        tracing::error!(
                            %organization_id,
                            cycle_id,
                            consecutive_failures,
                            error = %error,
                            "tenant audit maintenance cycle failed; exact pending checkpoint will be retried"
                        );
                    }
                }
            }
        }
    }
}

pub(crate) async fn load_audit_maintenance_identity(
    maintenance: &AuditMaintenanceSettings,
) -> Result<(OrganizationId, PrincipalId, RsaAuditCheckpointSigner)> {
    let organization_id = maintenance
        .organization_id
        .parse::<OrganizationId>()
        .context("audit maintenance organization ID is not a UUID")?;
    let actor_principal_id = maintenance
        .actor_principal_id
        .parse::<PrincipalId>()
        .context("audit maintenance actor Principal ID is not a UUID")?;
    let private_key = tokio::fs::read(&maintenance.private_key_file)
        .await
        .with_context(|| {
            format!(
                "read audit maintenance private key {}",
                maintenance.private_key_file.display()
            )
        })?;
    let public_key = tokio::fs::read(&maintenance.public_key_file)
        .await
        .with_context(|| {
            format!(
                "read audit maintenance public key {}",
                maintenance.public_key_file.display()
            )
        })?;
    let signer =
        RsaAuditCheckpointSigner::new(maintenance.key_id.clone(), &private_key, &public_key)
            .context("validate audit maintenance signing key pair")?;
    Ok((organization_id, actor_principal_id, signer))
}

#[tracing::instrument(
    name = "audit.maintenance.cycle",
    skip_all,
    fields(
        organization.id = %organization_id,
        actor.id = %actor_principal_id,
        cycle.id = cycle_id,
        copilotmail.trace_id = tracing::field::Empty,
        audit.checkpoint_id = tracing::field::Empty,
    )
)]

pub(crate) async fn audit_maintenance_cycle(
    settings: &ControlPlaneSettings,
    maintenance: &AuditMaintenanceSettings,
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    actor_principal_id: PrincipalId,
    signer: &RsaAuditCheckpointSigner,
    cycle_id: &str,
) -> Result<AuditMaintenanceOutcome> {
    let authorization = store
        .resolve_authorization(actor_principal_id, Utc::now())
        .await
        .context("resolve audit maintenance actor")?
        .context("audit maintenance actor is inactive or missing")?;
    anyhow::ensure!(
        authorization.organization_id == organization_id
            && authorization.principal_id == actor_principal_id
            && authorization.permits(organization_id, Permission::AuditExport),
        "audit maintenance actor is not authorized for this tenant"
    );

    let pending = store
        .oldest_unarchived_audit_checkpoint(organization_id)
        .await
        .context("load oldest unarchived audit checkpoint")?;
    let resumed_checkpoint = pending.is_some();
    let stored = if let Some(stored) = pending {
        anyhow::ensure!(
            stored.checkpoint.payload.key_id == signer.key_id()
                && stored.checkpoint.payload.public_key_sha256 == signer.public_key_sha256(),
            "oldest unarchived checkpoint requires another trusted signing key"
        );
        stored
    } else {
        let candidate = store
            .audit_checkpoint_candidate(organization_id)
            .await
            .context("build verified audit checkpoint candidate")?
            .context("tenant audit chain is empty")?;
        let checkpoint_uuid = stable_maintenance_uuid(
            "checkpoint",
            &[
                organization_id.to_string().as_bytes(),
                candidate.chain_sequence_end.to_string().as_bytes(),
                signer.key_id().as_bytes(),
            ],
        );
        let checkpoint_id = AuditCheckpointId::from_uuid(checkpoint_uuid);
        let archive_object_key = format!(
            "{}/{}/{}-{}.events.jsonl",
            maintenance.object_prefix, organization_id, candidate.chain_sequence_end, checkpoint_id
        );
        anyhow::ensure!(
            archive_object_key.len() <= 1_024,
            "audit maintenance archive object key is too long"
        );
        CreateAuditCheckpointService::new(store.clone(), signer)
            .execute(CreateAuditCheckpointCommand {
                checkpoint_id,
                organization_id,
                actor_principal_id,
                archive_object_key,
                trace_id: stable_audit_artifact_trace_id(organization_id, checkpoint_id),
                now: Utc::now(),
            })
            .await
            .context("create deterministic audit checkpoint")?;
        store
            .get_signed_audit_checkpoint(organization_id, checkpoint_id)
            .await
            .context("reload deterministic audit checkpoint")?
            .context("new deterministic audit checkpoint is unavailable")?
    };
    let checkpoint_id = stored.checkpoint.payload.checkpoint_id;
    let trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
    tracing::Span::current().record("copilotmail.trace_id", &trace_id);
    tracing::Span::current().record(
        "audit.checkpoint_id",
        tracing::field::display(checkpoint_id),
    );
    tracing::info!(
        cycle_id,
        trace_id,
        %organization_id,
        %checkpoint_id,
        resumed_checkpoint,
        "tenant audit maintenance artifact selected"
    );
    let package_directory = maintenance
        .work_directory
        .join(organization_id.to_string())
        .join(checkpoint_id.to_string());
    prepare_audit_export_package(
        store,
        organization_id,
        checkpoint_id,
        &package_directory,
        &maintenance.public_key_file,
        &trace_id,
    )
    .await?;
    let archive_id = AuditArchiveId::from_uuid(stable_maintenance_uuid(
        "archive",
        &[
            organization_id.to_string().as_bytes(),
            checkpoint_id.to_string().as_bytes(),
        ],
    ));
    run_archive_audit_export_traced(
        settings,
        store,
        organization_id.to_string(),
        actor_principal_id.to_string(),
        checkpoint_id.to_string(),
        Some(archive_id.to_string()),
        package_directory.to_string_lossy().into_owned(),
        maintenance.public_key_file.to_string_lossy().into_owned(),
        Some(&trace_id),
    )
    .await
    .context("archive verified audit maintenance package")?;
    remove_completed_audit_package(&maintenance.work_directory, &package_directory, &trace_id)
        .await;
    Ok(AuditMaintenanceOutcome {
        trace_id,
        organization_id,
        checkpoint_id,
        archive_id,
        chain_sequence_end: stored.checkpoint.payload.chain_sequence_end,
        resumed_checkpoint,
    })
}

pub(crate) async fn prepare_audit_export_package(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    checkpoint_id: AuditCheckpointId,
    package_directory: &FsPath,
    public_key_file: &FsPath,
    trace_id: &str,
) -> Result<()> {
    if tokio::fs::try_exists(package_directory).await? {
        match verify_audit_export_files(package_directory, public_key_file) {
            Ok((manifest, report))
                if report.organization_id == organization_id
                    && report.checkpoint_id == checkpoint_id
                    && manifest.checkpoint.payload.checkpoint_id == checkpoint_id =>
            {
                tracing::info!(
                    trace_id,
                    %organization_id,
                    %checkpoint_id,
                    "reusing verified audit export package"
                );
                return Ok(());
            }
            _ => {
                let quarantine =
                    package_directory.with_extension(format!("incomplete-{}", Uuid::now_v7()));
                tokio::fs::rename(package_directory, &quarantine)
                    .await
                    .with_context(|| {
                        format!(
                            "quarantine incomplete audit package as {}",
                            quarantine.display()
                        )
                    })?;
                tracing::warn!(
                    trace_id,
                    %organization_id,
                    %checkpoint_id,
                    quarantine = %quarantine.display(),
                    "incomplete audit export package quarantined"
                );
            }
        }
    }
    if let Some(parent) = package_directory.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("create tenant audit maintenance work directory")?;
    }
    run_export_audit_checkpoint_traced(
        store,
        organization_id.to_string(),
        checkpoint_id.to_string(),
        package_directory.to_string_lossy().into_owned(),
        Some(trace_id),
    )
    .await
    .context("export audit maintenance checkpoint")
}

pub(crate) async fn ensure_audit_maintenance_root(path: &FsPath) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "audit maintenance work directory is empty"
    );
    tokio::fs::create_dir_all(path)
        .await
        .with_context(|| format!("create audit maintenance root {}", path.display()))?;
    let metadata = tokio::fs::symlink_metadata(path).await?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "audit maintenance root must be a real directory"
    );
    Ok(())
}

pub(crate) async fn remove_completed_audit_package(
    root: &FsPath,
    package: &FsPath,
    trace_id: &str,
) {
    let result = async {
        let root = tokio::fs::canonicalize(root).await?;
        let package = tokio::fs::canonicalize(package).await?;
        let metadata = tokio::fs::symlink_metadata(&package).await?;
        anyhow::ensure!(
            package.starts_with(&root)
                && package != root
                && metadata.is_dir()
                && !metadata.file_type().is_symlink(),
            "refusing unsafe audit package cleanup"
        );
        tokio::fs::remove_dir_all(&package).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(
            trace_id,
            package = %package.display(),
            error = %error,
            "completed audit package could not be cleaned; archive evidence is already durable"
        );
    }
}

pub(crate) fn stable_maintenance_uuid(label: &str, parts: &[&[u8]]) -> Uuid {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.audit-maintenance.v1");
    digest.update((label.len() as u64).to_be_bytes());
    digest.update(label.as_bytes());
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    let digest = digest.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub(crate) fn stable_audit_artifact_trace_id(
    organization_id: OrganizationId,
    checkpoint_id: AuditCheckpointId,
) -> String {
    stable_maintenance_uuid(
        "trace",
        &[
            organization_id.to_string().as_bytes(),
            checkpoint_id.to_string().as_bytes(),
        ],
    )
    .to_string()
}
