use crate::*;

pub(crate) async fn read_agent_mail_content(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, message_id)): Path<(MailboxId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentMailContentResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent mail content requires an authenticated Agent",
            false,
            &request_id,
        ));
    }
    for permission in [
        Permission::AgentOperate,
        Permission::AgentMailRead,
        Permission::MailboxUse,
    ] {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            permission,
            "Agent mail content permission is required",
            &request_id,
        )?;
    }
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let content = AgentMailContentService::new(state.store.clone(), reader)
        .read(
            &authorization,
            ReadAgentMailContentCommand {
                mailbox_id,
                message_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_mail_content_api_error(error, &request_id))?;
    Ok(Json(AgentMailContentResponse {
        message_id: content.message_id.to_string(),
        subject: content.subject,
        from: content
            .from
            .into_iter()
            .map(inbox_address_response)
            .collect(),
        received_at: content.received_at.to_rfc3339(),
        text_body: content.text_body,
        text_body_truncated: content.text_body_truncated,
        has_attachment: content.has_attachment,
        untrusted_external_content: true,
        html_omitted: true,
        attachments_omitted: true,
    }))
}

pub(crate) fn agent_mail_content_api_error(
    error: AgentMailContentError,
    request_id: &str,
) -> Response {
    match error {
        AgentMailContentError::NotFound => resource_not_found(request_id),
        AgentMailContentError::Unavailable | AgentMailContentError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_mail_content_unavailable",
            "Agent mail content is temporarily unavailable",
            true,
            request_id,
        ),
        AgentMailContentError::Domain(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "agent_mail_content_evidence_failed",
            "Agent mail content evidence could not be committed",
            true,
            request_id,
        ),
    }
}

pub(crate) async fn ensure_agent_processing_subscription(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentProcessingSubscriptionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let subscription = AgentProcessingService::new(state.store.clone())
        .ensure_subscription(EnsureAgentProcessingSubscriptionCommand {
            context: authorization,
            mailbox_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(AgentProcessingSubscriptionResponse {
        subscription_id: subscription.subscription_id.to_string(),
        organization_id: subscription.organization_id.to_string(),
        agent_id: subscription.principal_id.to_string(),
        mailbox_id: subscription.mailbox_id.to_string(),
        status: subscription.status.as_str().into(),
        created_at: subscription.created_at.to_rfc3339(),
        updated_at: subscription.updated_at.to_rfc3339(),
    }))
}

pub(crate) async fn change_agent_processing_subscription(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<SubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentProcessingSubscriptionChangeRequest>,
) -> Result<Json<AgentProcessingSubscriptionChangeResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let expected_status =
        parse_agent_processing_subscription_status(&body.expected_status, &request_id)?;
    let target_status = parse_agent_processing_subscription_status(&body.status, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .change_subscription(ChangeAgentProcessingSubscriptionCommand {
            context: authorization,
            subscription_id,
            expected_status,
            target_status,
            idempotency_key,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(AgentProcessingSubscriptionChangeResponse {
        operation_id: result.operation_id.to_string(),
        subscription_id: result.subscription.subscription_id.to_string(),
        organization_id: result.subscription.organization_id.to_string(),
        agent_id: result.subscription.principal_id.to_string(),
        mailbox_id: result.subscription.mailbox_id.to_string(),
        previous_status: result.previous_status.as_str().into(),
        status: result.subscription.status.as_str().into(),
        replayed: result.replayed,
        created_at: result.subscription.created_at.to_rfc3339(),
        updated_at: result.subscription.updated_at.to_rfc3339(),
    }))
}

pub(crate) async fn claim_agent_message(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<SubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageClaimRequest>,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .claim(ClaimAgentMessageCommand {
            context: authorization,
            subscription_id,
            idempotency_key,
            lease_seconds: body.lease_seconds,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    match result {
        Some(result) => Ok(Json(agent_message_processing_response(result)).into_response()),
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentProcessingDeadLetterQueryParams {
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<u16>,
}

pub(crate) async fn list_agent_processing_dead_letters(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<SubscriptionId>,
    Query(query): Query<AgentProcessingDeadLetterQueryParams>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let before_message_id = query
        .cursor
        .as_deref()
        .map(|cursor| parse_agent_processing_dead_letter_cursor(cursor, subscription_id))
        .transpose()
        .map_err(|_| invalid_agent_processing_cursor(&request_id))?;
    let page = AgentProcessingService::new(state.store.clone())
        .list_dead_letters(ListAgentProcessingDeadLettersCommand {
            context: authorization,
            subscription_id,
            before_message_id,
            limit: query.limit.unwrap_or(50),
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    let mut response = Json(AgentProcessingDeadLetterListResponse {
        items: page
            .items
            .into_iter()
            .map(|item| AgentProcessingDeadLetterResponse {
                message_id: item.message_id.to_string(),
                attempt_count: item.attempt_count,
                reason_code: item.reason_code,
                failed_at: item.failed_at.to_rfc3339(),
            })
            .collect(),
        next_cursor: page.next_before_message_id.map(|message_id| {
            format_agent_processing_dead_letter_cursor(subscription_id, message_id)
        }),
        has_more: page.has_more,
    })
    .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub(crate) async fn extend_agent_message_lease(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, message_id)): Path<(SubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageLeaseExtensionRequest>,
) -> Result<Json<AgentMessageProcessingResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let lease_id = parse_agent_processing_lease_id(&body.lease_id, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .extend_lease(ExtendAgentMessageLeaseCommand {
            context: authorization,
            subscription_id,
            message_id,
            lease_id,
            idempotency_key,
            lease_seconds: body.lease_seconds,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(agent_message_processing_response(result)))
}

pub(crate) async fn acknowledge_agent_message(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, message_id)): Path<(SubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageAcknowledgementRequest>,
) -> Result<Json<AgentMessageProcessingResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let lease_id = parse_agent_processing_lease_id(&body.lease_id, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .acknowledge(AcknowledgeAgentMessageCommand {
            context: authorization,
            subscription_id,
            message_id,
            lease_id,
            idempotency_key,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(agent_message_processing_response(result)))
}

pub(crate) async fn nack_agent_message(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, message_id)): Path<(SubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMessageNackRequest>,
) -> Result<Json<AgentMessageProcessingResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_processing_request(&state, &headers, connection, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let lease_id = parse_agent_processing_lease_id(&body.lease_id, &request_id)?;
    let result = AgentProcessingService::new(state.store.clone())
        .nack(NackAgentMessageCommand {
            context: authorization,
            subscription_id,
            message_id,
            lease_id,
            idempotency_key,
            reason_code: body.reason_code,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_processing_api_error(error, &request_id))?;
    Ok(Json(agent_message_processing_response(result)))
}

pub(crate) async fn authenticate_agent_processing_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent message processing requires an authenticated Agent",
            false,
            request_id,
        ));
    }
    for permission in [Permission::AgentOperate, Permission::MailboxUse] {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            permission,
            "Agent message processing permission is required",
            request_id,
        )?;
    }
    enforce_agent_ingress_limit(state, &identity, &authorization, request_id).await?;
    Ok((identity, authorization))
}

#[allow(clippy::result_large_err)]
pub(crate) fn required_idempotency_key(
    headers: &HeaderMap,
    request_id: &str,
) -> Result<String, Response> {
    headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "idempotency_key_required",
                "a valid Idempotency-Key header is required",
                false,
                request_id,
            )
        })
}

#[allow(clippy::result_large_err)]
pub(crate) fn parse_agent_processing_lease_id(
    value: &str,
    request_id: &str,
) -> Result<LeaseId, Response> {
    value.parse().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_processing_request",
            "the Agent processing request is invalid",
            false,
            request_id,
        )
    })
}

#[allow(clippy::result_large_err)]
pub(crate) fn parse_agent_processing_subscription_status(
    value: &str,
    request_id: &str,
) -> Result<AgentProcessingSubscriptionStatus, Response> {
    match value {
        "active" => Ok(AgentProcessingSubscriptionStatus::Active),
        "paused" => Ok(AgentProcessingSubscriptionStatus::Paused),
        "retired" => Ok(AgentProcessingSubscriptionStatus::Retired),
        _ => Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_processing_request",
            "the Agent processing subscription status is invalid",
            false,
            request_id,
        )),
    }
}

pub(crate) fn format_agent_processing_dead_letter_cursor(
    subscription_id: SubscriptionId,
    message_id: Uuid,
) -> String {
    format!("v1.{subscription_id}.{message_id}")
}

pub(crate) fn parse_agent_processing_dead_letter_cursor(
    value: &str,
    expected_subscription_id: SubscriptionId,
) -> Result<Uuid, ()> {
    if value.len() > 80 {
        return Err(());
    }
    let mut segments = value.split('.');
    let version = segments.next().ok_or(())?;
    let subscription_id = segments
        .next()
        .ok_or(())?
        .parse::<SubscriptionId>()
        .map_err(|_| ())?;
    let message_id = segments.next().ok_or(())?.parse::<Uuid>().map_err(|_| ())?;
    if version != "v1" || subscription_id != expected_subscription_id || segments.next().is_some() {
        return Err(());
    }
    Ok(message_id)
}

pub(crate) fn invalid_agent_processing_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_agent_processing_cursor",
        "the Agent processing dead-letter cursor is invalid or belongs to another subscription",
        false,
        request_id,
    )
}

pub(crate) fn agent_message_processing_response(
    result: AgentMessageProcessingResult,
) -> AgentMessageProcessingResponse {
    AgentMessageProcessingResponse {
        operation_id: result.operation_id.to_string(),
        subscription_id: result.subscription_id.to_string(),
        message_id: result.message_id.to_string(),
        lease_id: result.lease_id.to_string(),
        operation: result.operation.as_str().into(),
        state: result.state.as_str().into(),
        attempt_count: result.attempt_count,
        lease_expires_at: result.lease_expires_at.map(|value| value.to_rfc3339()),
        reason_code: result.reason_code,
        replayed: result.replayed,
        occurred_at: result.occurred_at.to_rfc3339(),
    }
}

pub(crate) fn agent_processing_api_error(
    error: AgentProcessingError,
    request_id: &str,
) -> Response {
    match error {
        AgentProcessingError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_processing_request",
            "the Agent processing request is invalid",
            false,
            request_id,
        ),
        AgentProcessingError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_processing_forbidden",
            "Agent message processing is forbidden",
            false,
            request_id,
        ),
        AgentProcessingError::NotFound => resource_not_found(request_id),
        AgentProcessingError::Conflict => api_error(
            StatusCode::CONFLICT,
            "agent_processing_conflict",
            "the subscription, lease, processing state, or idempotency key conflicts",
            false,
            request_id,
        ),
        AgentProcessingError::Unavailable | AgentProcessingError::Domain(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_processing_unavailable",
            "Agent message processing is temporarily unavailable",
            true,
            request_id,
        ),
    }
}
