use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentEventCursorQuery {
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<u16>,
}

pub(crate) async fn enforce_agent_ingress_limit(
    state: &AppState,
    identity: &AuthenticatedIdentity,
    authorization: &AuthorizationContext,
    request_id: &str,
) -> Result<(), Response> {
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Ok(());
    }
    let decision = AgentIngressRateLimiter::new(state.store.clone())
        .admit(
            authorization.organization_id,
            authorization.principal_id,
            state.agent_ingress_tenant_requests_per_minute,
            state.agent_ingress_principal_requests_per_minute,
        )
        .await;
    match decision {
        Ok(AgentIngressRateLimitDecision::Allowed) => {
            state
                .agent_event_stream_metrics
                .ingress_allowed_total
                .fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        Ok(AgentIngressRateLimitDecision::TenantLimited {
            retry_after_seconds,
        }) => {
            state
                .agent_event_stream_metrics
                .ingress_tenant_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            Err(agent_ingress_limited_response(
                "agent_ingress_tenant_rate_limited",
                "tenant Agent API ingress rate is exhausted",
                retry_after_seconds,
                request_id,
            ))
        }
        Ok(AgentIngressRateLimitDecision::PrincipalLimited {
            retry_after_seconds,
        }) => {
            state
                .agent_event_stream_metrics
                .ingress_principal_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            Err(agent_ingress_limited_response(
                "agent_ingress_principal_rate_limited",
                "Agent API ingress rate is exhausted",
                retry_after_seconds,
                request_id,
            ))
        }
        Err(AgentIngressRateLimitError::InvalidLimits | AgentIngressRateLimitError::Store(_)) => {
            state
                .agent_event_stream_metrics
                .ingress_failures_total
                .fetch_add(1, Ordering::Relaxed);
            Err(api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "agent_ingress_rate_limit_unavailable",
                "Agent API ingress admission is unavailable",
                true,
                request_id,
            ))
        }
    }
}

pub(crate) fn agent_ingress_limited_response(
    code: &'static str,
    message: &'static str,
    retry_after_seconds: u32,
    request_id: &str,
) -> Response {
    let mut response = api_error(
        StatusCode::TOO_MANY_REQUESTS,
        code,
        message,
        true,
        request_id,
    );
    response.headers_mut().insert(
        header::RETRY_AFTER,
        header::HeaderValue::from_str(&retry_after_seconds.clamp(1, 60).to_string())
            .expect("bounded Retry-After is a valid header value"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
        .headers_mut()
        .insert(header::PRAGMA, header::HeaderValue::from_static("no-cache"));
    response
}

pub(crate) async fn pull_agent_events(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Query(query): Query<AgentEventCursorQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentEventCursorPageResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to pull subscription events",
        &request_id,
    )?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let after_event_id = query
        .cursor
        .as_deref()
        .map(|cursor| parse_agent_event_cursor(cursor, subscription_id))
        .transpose()
        .map_err(|error| agent_event_cursor_parse_error(error, &request_id))?;
    let page = AgentEventCursorService::new(state.store.clone())
        .pull(
            &authorization,
            PullAgentEventsCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                after_event_id,
                limit: i64::from(query.limit.unwrap_or(50)),
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_event_cursor_api_error(error, &request_id))?;
    Ok(Json(AgentEventCursorPageResponse {
        items: page
            .items
            .into_iter()
            .map(agent_event_envelope_response)
            .collect(),
        next_cursor: page
            .next_event_id
            .map(|event_id| format_agent_event_cursor(subscription_id, event_id)),
        has_more: page.has_more,
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentEventCursorParseError {
    Invalid,
    ScopeMismatch,
    ConflictingResumeSources,
}

pub(crate) fn parse_agent_event_cursor(
    value: &str,
    expected_subscription_id: WebhookSubscriptionId,
) -> Result<Uuid, AgentEventCursorParseError> {
    if value.len() > 80 {
        return Err(AgentEventCursorParseError::Invalid);
    }
    let mut parts = value.split('.');
    let version = parts.next();
    let subscription = parts
        .next()
        .and_then(|value| value.parse::<WebhookSubscriptionId>().ok());
    let event = parts.next().and_then(|value| Uuid::parse_str(value).ok());
    if version != Some("v1") || parts.next().is_some() || subscription.is_none() || event.is_none()
    {
        return Err(AgentEventCursorParseError::Invalid);
    }
    if subscription != Some(expected_subscription_id) {
        return Err(AgentEventCursorParseError::ScopeMismatch);
    }
    Ok(event.expect("validated event cursor UUID"))
}

pub(crate) fn agent_event_cursor_parse_error(
    error: AgentEventCursorParseError,
    request_id: &str,
) -> Response {
    match error {
        AgentEventCursorParseError::Invalid => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_event_cursor",
            "cursor must be a valid versioned Agent event cursor",
            false,
            request_id,
        ),
        AgentEventCursorParseError::ScopeMismatch => api_error(
            StatusCode::BAD_REQUEST,
            "event_cursor_scope_mismatch",
            "cursor belongs to another webhook subscription",
            false,
            request_id,
        ),
        AgentEventCursorParseError::ConflictingResumeSources => api_error(
            StatusCode::BAD_REQUEST,
            "conflicting_event_cursor",
            "cursor and Last-Event-ID must match when both are provided",
            false,
            request_id,
        ),
    }
}

pub(crate) fn format_agent_event_cursor(
    subscription_id: WebhookSubscriptionId,
    event_id: Uuid,
) -> String {
    format!("v1.{subscription_id}.{event_id}")
}

pub(crate) const AGENT_EVENT_STREAM_BATCH_SIZE: i64 = 50;
pub(crate) const AGENT_EVENT_STREAM_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub(crate) const AGENT_EVENT_STREAM_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
pub(crate) const AGENT_EVENT_STREAM_MAX_LIFETIME: Duration = Duration::from_secs(300);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentEventStreamQuery {
    pub(crate) cursor: Option<String>,
}

pub(crate) struct AgentEventStreamState {
    pub(crate) app: Arc<AppState>,
    pub(crate) authorization: AuthorizationContext,
    pub(crate) subscription_id: WebhookSubscriptionId,
    pub(crate) cursor: Option<Uuid>,
    pub(crate) pending: VecDeque<AgentCursorEvent>,
    pub(crate) has_more: bool,
    pub(crate) request_id: String,
    pub(crate) expires_at: Instant,
    pub(crate) terminal: bool,
    pub(crate) _lease: AgentEventStreamLeaseGuard,
}

pub(crate) struct AgentEventStreamLeaseGuard {
    pub(crate) store: PgControlPlaneStore,
    pub(crate) lease: Option<AgentEventStreamLease>,
    pub(crate) metrics: AgentEventStreamMetrics,
}

impl Drop for AgentEventStreamLeaseGuard {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        self.metrics.closed();
        let store = self.store.clone();
        let metrics = self.metrics.clone();
        let lease_id = lease.lease_id;
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(async move {
            if let Err(error) = store.release_agent_event_stream_lease(&lease).await {
                metrics
                    .cleanup_failures_total
                    .fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    lease_id = %lease_id,
                    error = %error,
                    "failed to release Agent event stream lease; expiry will recover capacity"
                );
            }
        });
    }
}

pub(crate) async fn stream_agent_events(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Query(query): Query<AgentEventStreamQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to stream subscription events",
        &request_id,
    )?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let mut last_event_ids = headers.get_all("last-event-id").iter();
    let last_event_id = last_event_ids.next();
    if last_event_ids.next().is_some() {
        return Err(agent_event_cursor_parse_error(
            AgentEventCursorParseError::ConflictingResumeSources,
            &request_id,
        ));
    }
    let after_event_id =
        resolve_agent_stream_cursor(query.cursor.as_deref(), last_event_id, subscription_id)
            .map_err(|error| agent_event_cursor_parse_error(error, &request_id))?;

    // Validate the resume anchor and obtain the first bounded page before
    // sending HTTP 200. Authentication, ownership, revocation and cursor
    // failures therefore remain ordinary machine-readable API errors.
    let service = AgentEventCursorService::new(state.store.clone());
    let first_page = service
        .pull(
            &authorization,
            PullAgentEventsCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                after_event_id,
                limit: AGENT_EVENT_STREAM_BATCH_SIZE,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_event_cursor_api_error(error, &request_id))?;
    let lease_result = service
        .open_stream(
            &authorization,
            OpenAgentEventStreamCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
                lease_id: Uuid::now_v7(),
                tenant_connection_limit: state.agent_event_stream_tenant_limit,
                principal_connection_limit: state.agent_event_stream_principal_limit,
            },
        )
        .await;
    let lease = match lease_result {
        Ok(lease) => lease,
        Err(AgentEventCursorError::TenantCapacityReached) => {
            state
                .agent_event_stream_metrics
                .tenant_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            return Err(agent_event_cursor_api_error(
                AgentEventCursorError::TenantCapacityReached,
                &request_id,
            ));
        }
        Err(AgentEventCursorError::PrincipalCapacityReached) => {
            state
                .agent_event_stream_metrics
                .principal_rejected_total
                .fetch_add(1, Ordering::Relaxed);
            return Err(agent_event_cursor_api_error(
                AgentEventCursorError::PrincipalCapacityReached,
                &request_id,
            ));
        }
        Err(error) => return Err(agent_event_cursor_api_error(error, &request_id)),
    };
    state.agent_event_stream_metrics.opened();

    let stream = agent_event_stream(AgentEventStreamState {
        app: state.clone(),
        authorization,
        subscription_id,
        cursor: first_page.next_event_id,
        pending: first_page.items.into(),
        has_more: first_page.has_more,
        request_id,
        expires_at: Instant::now() + AGENT_EVENT_STREAM_MAX_LIFETIME,
        terminal: false,
        _lease: AgentEventStreamLeaseGuard {
            store: state.store.clone(),
            lease: Some(lease),
            metrics: state.agent_event_stream_metrics.clone(),
        },
    });
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(AGENT_EVENT_STREAM_HEARTBEAT_INTERVAL)
                .text("keepalive"),
        )
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-accel-buffering"),
        header::HeaderValue::from_static("no"),
    );
    Ok(response)
}

pub(crate) fn resolve_agent_stream_cursor(
    query_cursor: Option<&str>,
    last_event_id: Option<&header::HeaderValue>,
    subscription_id: WebhookSubscriptionId,
) -> Result<Option<Uuid>, AgentEventCursorParseError> {
    let last_event_id = last_event_id
        .map(|value| {
            value
                .to_str()
                .map_err(|_| AgentEventCursorParseError::Invalid)
        })
        .transpose()?;
    if let (Some(query), Some(last)) = (query_cursor, last_event_id)
        && query != last
    {
        return Err(AgentEventCursorParseError::ConflictingResumeSources);
    }
    query_cursor
        .or(last_event_id)
        .map(|cursor| parse_agent_event_cursor(cursor, subscription_id))
        .transpose()
}

pub(crate) fn agent_event_stream(
    state: AgentEventStreamState,
) -> impl Stream<Item = Result<SseEvent, Infallible>> {
    stream::unfold(state, |mut state| async move {
        if state.terminal {
            return None;
        }
        loop {
            if Instant::now() >= state.expires_at {
                return None;
            }
            if let Some(event) = state.pending.pop_front() {
                state.cursor = Some(event.event_id);
                let event_type = event.event_type.as_str();
                let event_id = format_agent_event_cursor(state.subscription_id, event.event_id);
                let envelope = agent_event_envelope_response(event);
                match serde_json::to_string(&envelope) {
                    Ok(data) => {
                        let item = SseEvent::default()
                            .id(event_id)
                            .event(event_type)
                            .data(data);
                        return Some((Ok(item), state));
                    }
                    Err(_) => {
                        state
                            .app
                            .agent_event_stream_metrics
                            .stream_errors_total
                            .fetch_add(1, Ordering::Relaxed);
                        state.terminal = true;
                        return Some((
                            Ok(agent_event_stream_error("event_serialization_failed")),
                            state,
                        ));
                    }
                }
            }
            if !state.has_more {
                tokio::time::sleep(AGENT_EVENT_STREAM_POLL_INTERVAL).await;
                if Instant::now() >= state.expires_at {
                    return None;
                }
            }
            let result = AgentEventCursorService::new(state.app.store.clone())
                .pull(
                    &state.authorization,
                    PullAgentEventsCommand {
                        organization_id: state.authorization.organization_id,
                        subscription_id: state.subscription_id,
                        after_event_id: state.cursor,
                        limit: AGENT_EVENT_STREAM_BATCH_SIZE,
                        trace_id: state.request_id.clone(),
                        now: Utc::now(),
                    },
                )
                .await;
            match result {
                Ok(page) => {
                    state.cursor = page.next_event_id;
                    state.pending = page.items.into();
                    state.has_more = page.has_more;
                }
                Err(error) => {
                    state
                        .app
                        .agent_event_stream_metrics
                        .stream_errors_total
                        .fetch_add(1, Ordering::Relaxed);
                    state.terminal = true;
                    return Some((
                        Ok(agent_event_stream_error(agent_event_stream_error_code(
                            &error,
                        ))),
                        state,
                    ));
                }
            }
        }
    })
}

pub(crate) fn agent_event_stream_error(code: &str) -> SseEvent {
    SseEvent::default()
        .event("stream.error")
        .data(format!(r#"{{"code":"{code}","retryable":true}}"#))
}

pub(crate) const fn agent_event_stream_error_code(error: &AgentEventCursorError) -> &'static str {
    match error {
        AgentEventCursorError::Forbidden | AgentEventCursorError::NotFound => {
            "stream_authorization_revoked"
        }
        AgentEventCursorError::InvalidCursor => "stream_cursor_unavailable",
        AgentEventCursorError::InvalidPageSize
        | AgentEventCursorError::InvalidQuota
        | AgentEventCursorError::Domain(_) => "stream_protocol_error",
        AgentEventCursorError::TenantCapacityReached
        | AgentEventCursorError::PrincipalCapacityReached => "stream_capacity_reached",
        AgentEventCursorError::Store(_) => "stream_temporarily_unavailable",
    }
}
