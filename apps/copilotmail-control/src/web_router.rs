use crate::*;

#[cfg(test)]
pub(crate) fn router(store: PgControlPlaneStore) -> Router {
    router_with_state(
        AppState {
            store,
            authenticator: Arc::new(UnconfiguredAuthenticator),
            authentication_ready: false,
            human_auth_required: false,
            human_session: None,
            human_session_required: false,
            agent_token_service: None,
            agent_authentication_ready: false,
            agent_auth_required: false,
            agent_event_stream_tenant_limit: 100,
            agent_event_stream_principal_limit: 5,
            agent_ingress_tenant_requests_per_minute: 6_000,
            agent_ingress_principal_requests_per_minute: 600,
            agent_event_stream_metrics: AgentEventStreamMetrics::default(),
            mail_data_plane_id: None,
            mail_engine: None,
            mail_engine_required: false,
            mail_content: None,
            mail_content_required: false,
            mail_drafts: None,
            mail_draft_content: None,
            mail_drafts_required: false,
            mail_event_ingress: None,
            mail_event_ingress_required: false,
            audit_archive: None,
        },
        None,
    )
}

pub(crate) fn router_with_state(state: AppState, scim_identity_provider: Option<String>) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    let mut router = Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(readiness))
        .route("/api/v2/system/info", get(system_info))
        .route(
            "/api/v2/auth/{client}/login",
            get(begin_human_login),
        )
        .route(
            "/api/v2/auth/{client}/callback",
            get(complete_human_login),
        )
        .route(
            "/api/v2/auth/{client}/session",
            get(get_human_session),
        )
        .route(
            "/api/v2/auth/{client}/logout",
            axum::routing::post(logout_human_session),
        )
        .route("/oauth2/token", axum::routing::post(issue_agent_token))
        .route("/oauth2/jwks.json", get(agent_jwks))
        .route(
            "/internal/v2/mail-engine/stalwart/events",
            axum::routing::post(ingest_stalwart_mail_events),
        )
        .route(
            "/api/v2/organizations/{organization_id}",
            get(get_organization),
        )
        .route(
            "/api/v2/organizations/{organization_id}/audit-events",
            get(search_audit_events),
        )
        .route(
            "/api/v2/organizations/{organization_id}/compliance-cases",
            get(list_compliance_cases).post(create_compliance_case),
        )
        .route(
            "/api/v2/organizations/{organization_id}/legal-holds",
            get(list_legal_holds),
        )
        .route(
            "/api/v2/organizations/{organization_id}/approval-requests",
            get(list_approval_requests),
        )
        .route(
            "/api/v2/organizations/{organization_id}/roles",
            get(list_roles),
        )
        .route(
            "/api/v2/organizations/{organization_id}/role-assignments",
            get(list_role_assignments).post(grant_role_assignment),
        )
        .route(
            "/api/v2/compliance-cases/{case_id}",
            get(get_compliance_case),
        )
        .route(
            "/api/v2/compliance-cases/{case_id}/closure",
            axum::routing::post(close_compliance_case),
        )
        .route(
            "/api/v2/audit-archives/{archive_id}/legal-holds",
            axum::routing::post(apply_audit_legal_hold),
        )
        .route(
            "/api/v2/audit-archives/{archive_id}/exports",
            axum::routing::post(authorize_audit_export),
        )
        .route(
            "/api/v2/audit-archives/{archive_id}/export-grants/{approval_request_id}",
            get(reconcile_audit_export_grant),
        )
        .route(
            "/api/v2/audit-export-grants/{grant_id}",
            get(get_audit_export_grant),
        )
        .route(
            "/api/v2/audit-export-grants/{grant_id}/artifacts/{artifact}",
            axum::routing::post(download_audit_export_artifact),
        )
        .route(
            "/api/v2/organizations/{organization_id}/retention-policies",
            axum::routing::post(create_retention_policy),
        )
        .route(
            "/api/v2/retention-policies/{policy_id}",
            get(get_retention_policy),
        )
        .route(
            "/api/v2/retention-policies/{policy_id}/versions",
            axum::routing::post(append_retention_policy_version),
        )
        .route(
            "/api/v2/retention-policies/{policy_id}/versions/{version_id}/activation",
            axum::routing::post(activate_retention_policy_version),
        )
        .route(
            "/api/v2/agents/{agent_id}/mail-policy-versions",
            axum::routing::post(create_agent_mail_policy_version),
        )
        .route(
            "/api/v2/agents/{agent_id}/mail-policy-versions/{version_id}",
            get(get_agent_mail_policy_version),
        )
        .route(
            "/api/v2/agents/{agent_id}/mail-policy-versions/{version_id}/activation",
            axum::routing::post(activate_agent_mail_policy_version),
        )
        .route(
            "/api/v2/agents/{agent_id}/inspector",
            get(get_agent_inspector),
        )
        .route(
            "/api/v2/agents/{agent_id}/mail-submissions/{submission_id}/evidence",
            axum::routing::post(get_agent_mail_submission_evidence),
        )
        .route("/api/v2/legal-holds/{hold_id}", get(get_legal_hold))
        .route(
            "/api/v2/legal-holds/{hold_id}/release",
            axum::routing::post(release_audit_legal_hold),
        )
        .route(
            "/api/v2/organizations/{organization_id}/mailboxes",
            axum::routing::post(create_mailbox),
        )
        .route(
            "/api/v2/organizations/{organization_id}/mail-domains",
            get(list_mail_domains).post(create_mail_domain),
        )
        .route(
            "/api/v2/organizations/{organization_id}/webhook-subscriptions",
            get(list_webhook_subscriptions).post(create_webhook_subscription),
        )
        .route(
            "/api/v2/organizations/{organization_id}/webhook-dead-letters",
            get(list_webhook_dead_letters),
        )
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}",
            get(get_webhook_subscription).put(update_webhook_subscription),
        )
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}/events",
            get(pull_agent_events),
        )
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}/stream",
            get(stream_agent_events),
        )
        .route(
            "/api/v2/agent/mailboxes/{mailbox_id}/messages/{message_id}/content",
            get(read_agent_mail_content),
        )
        .route(
            "/api/v2/agent/mailboxes/{mailbox_id}/processing-subscription",
            axum::routing::put(ensure_agent_processing_subscription),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/claims",
            axum::routing::post(claim_agent_message),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}",
            axum::routing::patch(change_agent_processing_subscription),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/dead-letters",
            get(list_agent_processing_dead_letters),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/lease-extension",
            axum::routing::post(extend_agent_message_lease),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/acknowledgement",
            axum::routing::post(acknowledge_agent_message),
        )
        .route(
            "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/nack",
            axum::routing::post(nack_agent_message),
        )
        .route("/api/v2/agent/mail-policy", get(get_own_agent_mail_policy))
        .route(
            "/api/v2/webhook-subscriptions/{subscription_id}/dead-letters/{event_id}/replay",
            axum::routing::post(replay_webhook_dead_letter),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}",
            get(get_mail_domain),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/dns-verifications",
            axum::routing::post(verify_mail_domain_dns),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/ownership-challenge",
            axum::routing::post(rotate_mail_domain_ownership_challenge),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/activation",
            axum::routing::post(activate_mail_domain),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/suspension",
            axum::routing::post(suspend_mail_domain),
        )
        .route(
            "/api/v2/mail-domains/{mail_domain_id}/route",
            axum::routing::put(update_mail_domain_route),
        )
        .route("/api/v2/mailboxes", get(list_human_mailboxes))
        .route("/api/v2/mailboxes/{mailbox_id}", get(get_mailbox))
        .route(
            "/api/v2/mailboxes/{mailbox_id}/messages",
            get(list_inbox_messages),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/messages/search",
            axum::routing::post(search_inbox_messages),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/messages/{message_id}",
            get(get_inbox_message).patch(update_inbox_message_state),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/drafts",
            get(list_human_drafts).post(save_text_draft),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/drafts/{draft_id}/restore",
            axum::routing::post(restore_human_draft),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/submissions",
            get(list_human_sent_submissions)
                .head(reject_human_sent_submissions_head)
                .post(request_mail_submission),
        )
        .route(
            "/api/v2/agent/mailboxes/{mailbox_id}/sandbox-submissions",
            axum::routing::post(capture_agent_mail_sandbox_submission),
        )
        .route(
            "/api/v2/mail-submissions/{submission_id}",
            get(get_mail_submission),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding",
            axum::routing::put(update_mail_engine_mailbox_binding),
        )
        .route(
            "/api/v2/mailboxes/{mailbox_id}/delegations",
            axum::routing::post(create_mailbox_delegation),
        )
        .route(
            "/api/v2/mailbox-delegations/{delegation_id}",
            axum::routing::delete(revoke_mailbox_delegation),
        )
        .route(
            "/api/v2/approval-requests",
            axum::routing::post(create_approval_request),
        )
        .route(
            "/api/v2/approval-requests/{approval_request_id}/decisions",
            axum::routing::post(decide_approval_request),
        )
        .route(
            "/api/v2/approval-requests/{approval_request_id}",
            get(get_approval_request),
        )
        .route(
            "/api/v2/approval-requests/{approval_request_id}/cancellation",
            axum::routing::post(cancel_approval_request),
        )
        .route(
            "/api/v2/role-assignments/{role_assignment_id}/revocation",
            axum::routing::post(revoke_role_assignment),
        )
        .fallback(not_found);
    if let Some(identity_provider) = scim_identity_provider {
        router = router.nest_service(
            "/scim/v2",
            copilotmail_scim::router(state.store.clone(), identity_provider),
        );
    }
    let state = Arc::new(state);
    router
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing");
                let span = tracing::info_span!(
                    "http.request",
                    http.request.method = %request.method(),
                    url.path = %request.uri().path(),
                    request.id = request_id,
                );
                let parent = opentelemetry::global::get_text_map_propagator(|propagator| {
                    propagator.extract(&RequestHeaderExtractor(request.headers()))
                });
                // A malformed or unsupported remote context is deliberately
                // ignored; the request keeps its fresh local root span.
                let _ = span.set_parent(parent);
                span
            }),
        )
        .layer(middleware::from_fn(human_auth_response_headers))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            human_session_csrf_guard,
        ))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .with_state(state)
}

pub(crate) struct RequestHeaderExtractor<'a>(pub(crate) &'a HeaderMap);

impl Extractor for RequestHeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(HeaderName::as_str).collect()
    }
}

pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        service: SERVICE.into(),
        version: BUILD_VERSION.into(),
    })
}

pub(crate) async fn readiness(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<ReadinessResponse>) {
    let mut dependencies = BTreeMap::new();
    let database = match state.store.schema_ready().await {
        Ok(true) => "ready",
        Ok(false) => "migration_required",
        Err(error) => {
            tracing::warn!(%error, "PostgreSQL readiness check failed");
            "unavailable"
        }
    };
    dependencies.insert("postgresql".into(), database.into());
    dependencies.insert(
        "human_authentication".into(),
        if state.authentication_ready {
            "ready"
        } else if state.human_auth_required {
            "required_but_unavailable"
        } else {
            "not_configured"
        }
        .into(),
    );
    dependencies.insert(
        "human_browser_session".into(),
        if state.human_session.is_some() {
            "ready"
        } else if state.human_session_required {
            "required_but_unavailable"
        } else {
            "not_configured"
        }
        .into(),
    );
    dependencies.insert(
        "agent_authentication".into(),
        if state.agent_authentication_ready {
            "ready"
        } else if state.agent_auth_required {
            "required_but_unavailable"
        } else {
            "not_configured"
        }
        .into(),
    );
    let mail_engine_ready = if let Some(engine) = &state.mail_engine {
        match engine.probe().await {
            Ok(_) => {
                dependencies.insert("mail_data_plane".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail data-plane readiness check failed");
                dependencies.insert("mail_data_plane".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_data_plane".into(),
            if state.mail_engine_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let mail_event_ingress_ready = if let Some(settings) = &state.mail_event_ingress {
        match load_mail_event_ingress_secrets(settings).await {
            Ok(_) => {
                dependencies.insert("mail_event_ingress".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail event ingress secret readiness check failed");
                dependencies.insert("mail_event_ingress".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_event_ingress".into(),
            if state.mail_event_ingress_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let mail_content_ready = if let Some(reader) = &state.mail_content {
        match reader.probe_content().await {
            Ok(()) => {
                dependencies.insert("mail_content".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail-content readiness check failed");
                dependencies.insert("mail_content".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_content".into(),
            if state.mail_content_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let mail_drafts_ready = if let Some(drafts) = &state.mail_drafts {
        match drafts.probe_drafts().await {
            Ok(()) => {
                dependencies.insert("mail_compose".into(), "ready".into());
                true
            }
            Err(error) => {
                tracing::warn!(%error, "mail-compose readiness check failed");
                dependencies.insert("mail_compose".into(), "unavailable".into());
                false
            }
        }
    } else {
        dependencies.insert(
            "mail_compose".into(),
            if state.mail_drafts_required {
                "required_but_unavailable"
            } else {
                "not_configured"
            }
            .into(),
        );
        false
    };
    let ready = database == "ready"
        && (!state.human_auth_required || state.authentication_ready)
        && (!state.human_session_required || state.human_session.is_some())
        && (!state.agent_auth_required || state.agent_authentication_ready)
        && (!state.mail_engine_required || mail_engine_ready)
        && (!state.mail_content_required || mail_content_ready)
        && (!state.mail_drafts_required || mail_drafts_ready)
        && (!state.mail_event_ingress_required || mail_event_ingress_ready);
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(ReadinessResponse {
            status: if ready { "ready" } else { "not_ready" }.into(),
            dependencies,
        }),
    )
}

pub(crate) async fn system_info() -> Json<SystemInfoResponse> {
    Json(SystemInfoResponse {
        api_version: API_VERSION.into(),
        build_version: BUILD_VERSION.into(),
        principal_model: "human_agent_service".into(),
    })
}

pub(crate) async fn get_own_agent_mail_policy(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent mail policy self-service requires an authenticated Agent",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to inspect Agent mail policy",
        &request_id,
    )?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;

    let snapshot = AgentMailPolicyQueryService::new(state.store.clone())
        .get_own_policy(GetOwnAgentMailPolicyCommand {
            context: authorization.clone(),
        })
        .await
        .map_err(|error| agent_mail_policy_query_api_error(error, &request_id))?;
    let effective_permissions = Permission::AGENT_TOKEN
        .into_iter()
        .filter(|permission| {
            authorization.permits(authorization.organization_id, *permission)
                && identity
                    .scope_restriction
                    .as_ref()
                    .is_none_or(|scopes| scopes.contains(permission))
        })
        .map(|permission| permission.as_str().to_owned())
        .collect();
    let mut response = Json(own_agent_mail_policy_response(
        snapshot,
        effective_permissions,
    ))
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-store".parse().expect("static Cache-Control is valid"),
    );
    Ok(response)
}

pub(crate) fn own_agent_mail_policy_response(
    snapshot: AgentMailPolicyRuntimeSnapshot,
    effective_permissions: Vec<String>,
) -> OwnAgentMailPolicyResponse {
    let active_policy =
        snapshot
            .active_policy
            .as_ref()
            .map(|policy| OwnAgentMailPolicyLimitsResponse {
                version: policy.version(),
                allowed_recipient_domains: policy
                    .allowed_recipient_domains()
                    .iter()
                    .cloned()
                    .collect(),
                max_recipients_per_message: policy.max_recipients_per_message(),
                max_messages_per_hour: policy.max_messages_per_hour(),
                max_messages_per_recipient_per_day: policy.max_messages_per_recipient_per_day(),
                require_first_contact_approval: policy.require_first_contact_approval(),
                max_auto_replies_per_thread_per_day: policy.max_auto_replies_per_thread_per_day(),
            });
    let admission_state = match (snapshot.environment, active_policy.is_some()) {
        (AgentEnvironment::Production, true) => "active",
        (AgentEnvironment::Production, false) => "blocked_no_active_policy",
        (AgentEnvironment::Development | AgentEnvironment::Testing, _) => "not_required",
    };
    let remaining_messages = snapshot.active_policy.as_ref().map(|policy| {
        policy
            .max_messages_per_hour()
            .saturating_sub(snapshot.admitted_messages_in_hour)
    });
    OwnAgentMailPolicyResponse {
        agent_id: snapshot.agent_id.to_string(),
        environment: agent_environment_wire(snapshot.environment).into(),
        admission_state: admission_state.into(),
        effective_permissions,
        active_policy,
        hourly_usage: OwnAgentMailHourlyUsageResponse {
            window_started_at: snapshot.hourly_window_started_at.to_rfc3339(),
            admitted_messages: snapshot.admitted_messages_in_hour,
            remaining_messages,
        },
        observed_at: snapshot.observed_at.to_rfc3339(),
    }
}

pub(crate) async fn get_organization(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<OrganizationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;

    if authorization.organization_id != organization_id {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "resource_not_found",
            "the requested organization does not exist",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::OrganizationRead,
        "organization.read permission is required",
        &request_id,
    )?;

    let organization = state
        .store
        .get_organization(organization_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "organization_store_unavailable",
                "organization data is temporarily unavailable",
                true,
                &request_id,
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "resource_not_found",
                "the requested organization does not exist",
                false,
                &request_id,
            )
        })?;

    Ok(Json(OrganizationResponse {
        organization_id: organization.organization_id.to_string(),
        slug: organization.slug,
        display_name: organization.display_name,
    }))
}

pub(crate) async fn not_found(
    Extension(request_id): Extension<RequestId>,
) -> (StatusCode, Json<copilotmail_contracts::ErrorResponse>) {
    let request_id = request_id_value(&request_id);
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            code: "route_not_found".into(),
            message: "the requested API route does not exist".into(),
            request_id: request_id.clone(),
            trace_id: request_id,
            retryable: false,
            details: serde_json::Value::Null,
        }),
    )
}
