use crate::*;

/// Serves the V2 health, readiness, and system information endpoints.
pub async fn run_http(settings: &ControlPlaneSettings, store: PgControlPlaneStore) -> Result<()> {
    let agent_event_stream_metrics = AgentEventStreamMetrics::default();
    let scim_identity_provider = settings.oidc.as_ref().map(|oidc| oidc.issuer.clone());
    let mail_data_plane_id = settings.mail_data_plane_id.map(MailDataPlaneId::from_uuid);
    ensure_mail_data_plane_registration(
        &store,
        mail_data_plane_id,
        settings.mail_data_plane_origin.as_deref(),
    )
    .await?;
    let mail_engine = settings
        .mail_engine
        .as_ref()
        .map(|adapter| {
            build_mail_engine(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_content = settings
        .mail_content
        .as_ref()
        .map(|adapter| {
            build_mail_content_reader(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_drafts = settings
        .mail_compose
        .as_ref()
        .map(|adapter| {
            build_mail_draft_store(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_draft_content = settings
        .mail_compose
        .as_ref()
        .map(|adapter| {
            build_mail_draft_content_reader(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let human_session = match (&settings.oidc, &settings.human_session) {
        (Some(oidc), Some(session)) => {
            Some(build_human_session_runtime(oidc, session, store.clone()).await?)
        }
        (None, Some(_)) => {
            anyhow::bail!("Human browser sessions require enterprise OIDC configuration")
        }
        (_, None) => None,
    };
    let (human_authenticator, authentication_ready) = if let Some(oidc) = &settings.oidc {
        let config = OidcConfig::new(
            oidc.issuer.clone(),
            oidc.audience.clone(),
            oidc.allow_insecure_http,
        )
        .context("validate enterprise OIDC configuration")?;
        let authenticator = OidcAuthenticator::discover(config, store.clone())
            .await
            .context("initialize enterprise OIDC provider")?;
        (
            Arc::new(authenticator) as Arc<dyn AccessTokenAuthenticator>,
            true,
        )
    } else {
        (
            Arc::new(UnconfiguredAuthenticator) as Arc<dyn AccessTokenAuthenticator>,
            false,
        )
    };
    let (agent_authenticator, agent_token_service, agent_authentication_ready) =
        if let Some(agent) = &settings.agent_oauth {
            let key_set = load_agent_signing_key_set(agent).await?;
            let config = AgentOAuthConfig::new_with_signing_keys(
                agent.issuer.clone(),
                agent.audience.clone(),
                agent.token_endpoint.clone(),
                key_set.active_key_id.clone(),
                key_set.keys,
                agent.access_token_ttl_seconds,
                agent.allow_insecure_http,
            )
            .context("validate Agent OAuth configuration")?;
            let service = Arc::new(AgentTokenService::new(store.clone(), config));
            if agent.signing_key_set_file.is_some() {
                spawn_signing_key_reload(
                    agent.clone(),
                    service.signing_key_manager(),
                    key_set.digest,
                );
            }
            let authenticator =
                Arc::new(service.authenticator()) as Arc<dyn AccessTokenAuthenticator>;
            (Some(authenticator), Some(service), true)
        } else {
            (None, None, false)
        };
    let authenticator: Arc<dyn AccessTokenAuthenticator> = match agent_authenticator {
        Some(agent) => Arc::new(CompositeAuthenticator {
            agent,
            human: if authentication_ready {
                human_authenticator
            } else {
                Arc::new(RejectingAuthenticator)
            },
        }),
        None => human_authenticator,
    };
    let app = router_with_state(
        AppState {
            store,
            authenticator,
            authentication_ready,
            human_auth_required: settings.require_human_auth,
            human_session,
            human_session_required: settings.require_human_session,
            agent_token_service,
            agent_authentication_ready,
            agent_auth_required: settings.require_agent_auth,
            agent_event_stream_tenant_limit: settings.agent_event_stream_tenant_limit,
            agent_event_stream_principal_limit: settings.agent_event_stream_principal_limit,
            agent_ingress_tenant_requests_per_minute: settings
                .agent_ingress_tenant_requests_per_minute,
            agent_ingress_principal_requests_per_minute: settings
                .agent_ingress_principal_requests_per_minute,
            agent_event_stream_metrics: agent_event_stream_metrics.clone(),
            mail_data_plane_id,
            mail_engine,
            mail_engine_required: settings.require_mail_engine,
            mail_content,
            mail_content_required: settings.require_mail_content,
            mail_drafts,
            mail_draft_content,
            mail_drafts_required: settings.require_mail_compose,
            mail_event_ingress: settings.mail_event_ingress.clone(),
            mail_event_ingress_required: settings.require_mail_event_ingress,
            audit_archive: settings.audit_archive.clone(),
        },
        scim_identity_provider,
    );
    tracing::info!(
        address = %settings.http_addr,
        environment = %settings.environment,
        native_tls = settings.native_tls.is_some(),
        "V2 control plane listening"
    );
    let metrics_listener = tokio::net::TcpListener::bind(settings.agent_event_stream_metrics_addr)
        .await
        .with_context(|| {
            format!(
                "bind Agent event stream metrics listener {}",
                settings.agent_event_stream_metrics_addr
            )
        })?;
    let metrics_router = agent_event_stream_metrics_router(agent_event_stream_metrics);
    let metrics_task =
        tokio::spawn(async move { axum::serve(metrics_listener, metrics_router).await });
    let result = if let Some(native_tls) = &settings.native_tls {
        let listener = NativeTlsListener::bind(settings.http_addr, native_tls).await?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<TlsConnectionInfo>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve V2 native TLS control plane")
    } else {
        let listener = tokio::net::TcpListener::bind(settings.http_addr)
            .await
            .context("bind V2 HTTP listener")?;
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<TlsConnectionInfo>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve V2 HTTP control plane")
    };
    metrics_task.abort();
    let _ = metrics_task.await;
    result
}

/// Runs the initial durable outbox worker using a structured-log event sink.
///
/// The log sink is deliberately an adapter: later phases replace it with
/// signed webhooks and other configured event publishers without changing
/// the outbox leasing semantics.
pub async fn run_worker(settings: &ControlPlaneSettings, store: PgControlPlaneStore) -> Result<()> {
    let worker_id = Uuid::now_v7();
    let mail_data_plane_id = settings.mail_data_plane_id.map(MailDataPlaneId::from_uuid);
    ensure_mail_data_plane_registration(
        &store,
        mail_data_plane_id,
        settings.mail_data_plane_origin.as_deref(),
    )
    .await?;
    let mail_domain_projector = settings
        .mail_engine
        .as_ref()
        .map(|adapter| {
            build_mail_domain_projector(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    let mail_submission_adapter = settings
        .mail_submission
        .as_ref()
        .map(|adapter| {
            build_mail_submission_adapter(
                adapter,
                mail_data_plane_id.context("mail data-plane ID is not configured")?,
            )
        })
        .transpose()?;
    if let Some(adapter) = &mail_submission_adapter {
        adapter
            .probe_submissions()
            .await
            .context("probe dedicated mail-submission credential")?;
    } else if settings.require_mail_submission {
        anyhow::bail!("mail submission is required but not configured");
    }
    let webhook_client = settings
        .webhook_delivery
        .as_ref()
        .map(|webhook| {
            let dns = HickoryDnsLookup::from_system().context("initialize webhook DNS resolver")?;
            SignedWebhookClient::new(
                dns,
                WebhookDeliveryConfig {
                    timeout: webhook.timeout,
                    allow_insecure_http: webhook.allow_insecure_http,
                    allow_private_network: webhook.allow_private_network,
                },
            )
            .context("initialize signed webhook client")
        })
        .transpose()?;
    if let Some(webhook) = &settings.webhook_delivery {
        WebhookSigningKeySet::load(&webhook.signing_key_set_file)
            .context("load webhook signing key set")?;
    }
    let mut interval = tokio::time::interval(settings.worker_poll_interval);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut consecutive_failures = 0_u32;
    tracing::info!(%worker_id, "durable outbox worker started");

    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!(%worker_id, "durable outbox worker stopping");
                return Ok(());
            }
            _ = interval.tick() => {
                let tick_result: Result<()> = async {
                    let events = store
                        .claim_outbox_batch(
                            worker_id,
                            mail_data_plane_id,
                            settings.worker_batch_size,
                        )
                        .await
                        .context("claim outbox batch")?;
                    for event in events {
                    if mail_event_requires_data_plane_route(&event.event_type)
                        && (event.data_plane_id.is_none()
                            || event.data_plane_id != mail_data_plane_id)
                    {
                        let released = store
                            .release_outbox_event(
                                event.id,
                                worker_id,
                                "mail_data_plane_route_invalid",
                            )
                            .await
                            .context("release incorrectly routed mail event")?;
                        tracing::error!(
                            event_id = %event.id,
                            event_type = %event.event_type,
                            released,
                            "mail event route is missing or does not match this worker"
                        );
                        continue;
                    }
                    if event.event_type == "mail.submission.requested.v1" {
                        let Some(adapter) = mail_submission_adapter.clone() else {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_submission_not_configured",
                                )
                                .await
                                .context("schedule mail submission after missing configuration")?;
                            tracing::warn!(
                                event_id = %event.id,
                                released,
                                "mail submission credential is not configured"
                            );
                            continue;
                        };
                        let submission_id = match Uuid::parse_str(&event.aggregate_id) {
                            Ok(id) => MailSubmissionId::from_uuid(id),
                            Err(_) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "mail_submission_event_invalid",
                                    )
                                    .await
                                    .context("schedule malformed mail submission event")?;
                                tracing::error!(
                                    event_id = %event.id,
                                    released,
                                    "mail submission event aggregate ID is invalid"
                                );
                                continue;
                            }
                        };
                        let execution = MailSubmissionExecutor::new(store.clone(), adapter)
                            .execute(ExecuteMailSubmissionCommand {
                                submission_id,
                                trace_id: event.id.to_string(),
                                now: Utc::now(),
                            })
                            .await;
                        let terminal = match execution {
                            Ok(
                                MailSubmissionExecutionResult::Accepted
                                | MailSubmissionExecutionResult::Rejected
                                | MailSubmissionExecutionResult::ManualReview,
                            ) => true,
                            Ok(MailSubmissionExecutionResult::NotAcquired) => store
                                .get_mail_submission_intent_for_execution(submission_id)
                                .await
                                .context("inspect unacquired mail submission intent")?
                                .is_some_and(|intent| {
                                    matches!(
                                        intent.status,
                                        MailSubmissionStatus::Accepted
                                            | MailSubmissionStatus::Rejected
                                            | MailSubmissionStatus::ManualReview
                                    )
                                }),
                            Ok(
                                MailSubmissionExecutionResult::Deferred
                                | MailSubmissionExecutionResult::LostLease,
                            ) => false,
                            Err(error) => {
                                tracing::warn!(
                                    event_id = %event.id,
                                    submission_id = %submission_id,
                                    error = %error,
                                    "mail submission execution failed safely"
                                );
                                false
                            }
                        };
                        if !terminal {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_submission_reconciling",
                                )
                                .await
                                .context("schedule mail submission reconciliation")?;
                            tracing::warn!(
                                event_id = %event.id,
                                submission_id = %submission_id,
                                released,
                                "mail submission remains non-terminal"
                            );
                            continue;
                        }
                    }
                    if event.event_type == "mail.delivery.reconciliation.requested.v1" {
                        let Some(adapter) = mail_submission_adapter.clone() else {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_delivery_not_configured",
                                )
                                .await
                                .context("schedule delivery reconciliation after missing configuration")?;
                            tracing::warn!(
                                event_id = %event.id,
                                released,
                                "mail delivery reconciliation credential is not configured"
                            );
                            continue;
                        };
                        let submission_id = match Uuid::parse_str(&event.aggregate_id) {
                            Ok(id) => MailSubmissionId::from_uuid(id),
                            Err(_) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "mail_delivery_event_invalid",
                                    )
                                    .await
                                    .context("schedule malformed delivery reconciliation event")?;
                                tracing::error!(
                                    event_id = %event.id,
                                    released,
                                    "mail delivery event aggregate ID is invalid"
                                );
                                continue;
                            }
                        };
                        let fingerprint_scheme =
                            mail_data_plane_fingerprint_scheme(adapter.data_plane_id());
                        let reconciliation = MailDeliveryReconciliationService::new(
                            store.clone(),
                            adapter,
                            fingerprint_scheme,
                        )
                                .reconcile(ReconcileMailDeliveryCommand {
                                    submission_id,
                                    now: Utc::now(),
                                })
                                .await;
                        if !matches!(
                            reconciliation,
                            Ok(MailDeliveryReconciliationResult::Terminal { .. })
                        ) {
                            let error_code = match reconciliation {
                                Ok(MailDeliveryReconciliationResult::Pending { .. }) => {
                                    "mail_delivery_pending"
                                }
                                Ok(MailDeliveryReconciliationResult::Terminal { .. }) => {
                                    unreachable!("terminal result handled above")
                                }
                                Err(_) => "mail_delivery_reconciliation_failed",
                            };
                            let released = store
                                .release_outbox_event(event.id, worker_id, error_code)
                                .await
                                .context("schedule mail delivery reconciliation")?;
                            tracing::warn!(
                                event_id = %event.id,
                                submission_id = %submission_id,
                                released,
                                error_code,
                                "mail delivery remains non-terminal"
                            );
                            continue;
                        }
                    }
                    if matches!(
                        event.event_type.as_str(),
                        "mail_domain.activated.v1" | "mail_domain.suspended.v1"
                    ) {
                        let projection = project_mail_domain_event(
                            &store,
                            mail_domain_projector.as_deref(),
                            &event,
                        )
                        .await;
                        if let Err(error) = projection {
                            let released = store
                                .release_outbox_event(
                                    event.id,
                                    worker_id,
                                    "mail_domain_projection_failed",
                                )
                                .await
                                .context("schedule mail domain projection retry")?;
                            tracing::warn!(
                                event_id = %event.id,
                                event_type = %event.event_type,
                                released,
                                error = %error,
                                "mail domain projection failed and was scheduled for retry"
                            );
                            continue;
                        }
                    }
                    if let (Some(webhook), Some(client)) =
                        (&settings.webhook_delivery, webhook_client.as_ref())
                    {
                        match publish_event_to_webhooks(
                            &store,
                            client,
                            &webhook.signing_key_set_file,
                            webhook.max_attempts,
                            &event,
                        )
                        .await
                        {
                            Ok(true) => {}
                            Ok(false) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "webhook_delivery_retrying",
                                    )
                                    .await
                                    .context("schedule webhook delivery retry")?;
                                tracing::warn!(
                                    event_id = %event.id,
                                    event_type = %event.event_type,
                                    released,
                                    "webhook delivery remains retryable"
                                );
                                continue;
                            }
                            Err(error) => {
                                let released = store
                                    .release_outbox_event(
                                        event.id,
                                        worker_id,
                                        "webhook_delivery_infrastructure_failed",
                                    )
                                    .await
                                    .context("schedule webhook infrastructure retry")?;
                                tracing::warn!(
                                    event_id = %event.id,
                                    event_type = %event.event_type,
                                    released,
                                    error = %error,
                                    "webhook delivery infrastructure failed"
                                );
                                continue;
                            }
                        }
                    }
                    tracing::info!(
                        event_id = %event.id,
                        event_type = %event.event_type,
                        aggregate_type = %event.aggregate_type,
                        aggregate_id = %event.aggregate_id,
                        schema_version = event.schema_version,
                        attempts = event.attempts,
                        occurred_at = %event.occurred_at,
                        webhook_delivery_configured = settings.webhook_delivery.is_some(),
                        "application event reached a terminal publication state"
                    );
                    let marked = store
                        .mark_outbox_published(event.id, worker_id)
                        .await
                        .context("mark outbox event published")?;
                        if !marked {
                            tracing::warn!(event_id = %event.id, "outbox claim was lost before acknowledgement");
                        }
                    }
                    Ok(())
                }
                .await;
                match tick_result {
                    Ok(()) => consecutive_failures = 0,
                    Err(error) => {
                        consecutive_failures = consecutive_failures.saturating_add(1);
                        let retry_after = worker_error_backoff(consecutive_failures);
                        tracing::error!(
                            %worker_id,
                            consecutive_failures,
                            retry_after_ms = retry_after.as_millis(),
                            error = %error,
                            "durable outbox worker tick failed; retrying"
                        );
                        tokio::select! {
                            _ = &mut shutdown => {
                                tracing::info!(%worker_id, "durable outbox worker stopping during retry backoff");
                                return Ok(());
                            }
                            _ = tokio::time::sleep(retry_after) => {}
                        }
                    }
                }
            }
        }
    }
}

pub(crate) fn mail_event_requires_data_plane_route(event_type: &str) -> bool {
    matches!(
        event_type,
        "mail_engine.mailbox_binding.updated.v1"
            | "mail_engine.mailbox_binding.updated.v2"
            | "message.received.v1"
            | "mail_engine.delivery_trace.observed.v1"
            | "mail.submission.requested.v1"
            | "agent.mail.admitted.v1"
            | "mail.submission.accepted.v1"
            | "mail.submission.rejected.v1"
            | "mail.submission.review_required.v1"
            | "mail.delivery.reconciliation.requested.v1"
            | "message.delivery_status_changed.v1"
            | "mail_domain.activated.v1"
            | "mail_domain.suspended.v1"
    )
}

pub(crate) async fn publish_event_to_webhooks(
    store: &PgControlPlaneStore,
    client: &SignedWebhookClient<HickoryDnsLookup>,
    signing_key_set_file: &FsPath,
    max_attempts: i32,
    event: &copilotmail_postgres::OutboxRecord,
) -> Result<bool> {
    let organization_id = event
        .organization_id()
        .context("Outbox event has no valid organization_id")?;
    let targets = store
        .pending_webhook_delivery_targets(
            organization_id,
            event.id,
            &event.event_type,
            event.occurred_at,
        )
        .await
        .context("load pending webhook delivery targets")?;
    if targets.is_empty() {
        return Ok(true);
    }
    let key_set =
        WebhookSigningKeySet::load(signing_key_set_file).context("reload webhook signing keys")?;
    let webhook_event = WebhookEvent {
        id: event.id,
        organization_id: organization_id.as_uuid(),
        event_type: event.event_type.clone(),
        aggregate_type: event.aggregate_type.clone(),
        aggregate_id: event.aggregate_id.clone(),
        schema_version: event.schema_version,
        payload: event.payload.clone(),
        occurred_at: event.occurred_at,
    };
    let mut all_terminal = true;
    for target in targets {
        let attempted_at = Utc::now();
        let next_attempt = target.previous_attempts.saturating_add(1);
        let delivery = match key_set.get(&target.signing_key_reference) {
            Some(secret) => {
                client
                    .deliver(
                        &target.endpoint_url,
                        &target.signing_key_reference,
                        secret,
                        &webhook_event,
                        attempted_at,
                    )
                    .await
            }
            None => Err(WebhookError::SigningKeyNotFound),
        };
        let outcome = match delivery {
            Ok(receipt) => WebhookDeliveryOutcome::Delivered {
                response_status: receipt.response_status,
            },
            Err(error) => {
                let response_status = match &error {
                    WebhookError::Rejected(status) => Some(*status),
                    _ => None,
                };
                if error.retryable() && next_attempt < max_attempts {
                    all_terminal = false;
                    WebhookDeliveryOutcome::Retrying {
                        response_status,
                        error_code: error.code().into(),
                        next_attempt_at: attempted_at + webhook_retry_delay(next_attempt),
                    }
                } else {
                    tracing::error!(
                        event_id = %event.id,
                        subscription_id = %target.subscription_id,
                        error_code = error.code(),
                        attempts = next_attempt,
                        "webhook delivery entered dead letter"
                    );
                    WebhookDeliveryOutcome::DeadLettered {
                        response_status,
                        error_code: error.code().into(),
                    }
                }
            }
        };
        let recorded = store
            .append_webhook_delivery(target.subscription_id, event.id, outcome, attempted_at)
            .await
            .context("append webhook delivery outcome")?;
        anyhow::ensure!(
            recorded.is_some(),
            "webhook subscription disappeared during delivery"
        );
    }
    Ok(all_terminal)
}

pub(crate) fn webhook_retry_delay(attempt: i32) -> chrono::Duration {
    let exponent = u32::try_from(attempt.clamp(0, 8)).unwrap_or(8);
    chrono::Duration::seconds(5_i64.saturating_mul(2_i64.saturating_pow(exponent)))
}

pub(crate) async fn project_mail_domain_event(
    store: &PgControlPlaneStore,
    projector: Option<&dyn MailDomainProjector>,
    event: &copilotmail_postgres::OutboxRecord,
) -> Result<()> {
    let mail_domain_id = event
        .aggregate_id
        .parse::<MailDomainId>()
        .context("mail domain Outbox aggregate ID is invalid")?;
    let target = store
        .load_mail_domain_projection_target(mail_domain_id)
        .await
        .context("load mail domain projection target")?
        .context("mail domain projection target no longer exists")?;
    let Some(projector) = projector else {
        anyhow::bail!("mail engine is not configured");
    };
    let data_plane_id = projector.data_plane_id();
    anyhow::ensure!(
        event.data_plane_id == Some(data_plane_id)
            && target.mail_data_plane_id == Some(data_plane_id),
        "mail domain event, durable assignment, and projector data plane do not match"
    );
    let attempted_at = Utc::now();
    anyhow::ensure!(
        store
            .mark_mail_domain_projection_attempt(
                mail_domain_id,
                target.status,
                data_plane_id,
                target.projection_last_attempt_at,
                attempted_at,
            )
            .await
            .context("record mail domain projection attempt")?,
        "mail domain changed before projection attempt"
    );
    let enabled = target.status == MailDomainStatus::Active;
    let projection = match projector
        .reconcile_domain(
            &target.name,
            target.mail_engine_domain_ref.as_deref(),
            enabled,
        )
        .await
    {
        Ok(projection) => projection,
        Err(error) => {
            store
                .fail_mail_domain_projection(
                    mail_domain_id,
                    target.status,
                    data_plane_id,
                    attempted_at,
                    mail_engine_error_code(error),
                    Utc::now(),
                )
                .await
                .context("record mail engine projection failure")?;
            return Err(error).context("reconcile mail engine domain");
        }
    };
    if projection.name != target.name.as_str() || projection.enabled != enabled {
        store
            .fail_mail_domain_projection(
                mail_domain_id,
                target.status,
                data_plane_id,
                attempted_at,
                "mail_engine_projection_mismatch",
                Utc::now(),
            )
            .await
            .context("record mismatched mail engine projection")?;
        anyhow::bail!("mail engine returned a mismatched domain projection");
    }
    anyhow::ensure!(
        store
            .complete_mail_domain_projection(
                mail_domain_id,
                target.status,
                data_plane_id,
                attempted_at,
                &projection.engine_ref,
                enabled,
                Utc::now(),
            )
            .await
            .context("complete mail engine domain projection")?,
        "mail domain changed or engine reference conflicted before projection completion"
    );
    Ok(())
}

pub(crate) const fn mail_engine_error_code(error: MailEngineError) -> &'static str {
    match error {
        MailEngineError::InvalidConfiguration => "mail_engine_invalid_configuration",
        MailEngineError::CredentialUnavailable => "mail_engine_credential_unavailable",
        MailEngineError::Unavailable => "mail_engine_unavailable",
        MailEngineError::InvalidSession => "mail_engine_invalid_session",
        MailEngineError::InvalidResponse => "mail_engine_invalid_response",
        MailEngineError::Rejected => "mail_engine_rejected",
        MailEngineError::ProjectionConflict => "mail_engine_projection_conflict",
    }
}
