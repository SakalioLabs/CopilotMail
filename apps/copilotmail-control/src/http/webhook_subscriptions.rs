use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebhookSubscriptionListQuery {
    pub(crate) principal_id: Option<String>,
    pub(crate) after: Option<String>,
    pub(crate) limit: Option<u16>,
}

pub(crate) async fn create_webhook_subscription(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<WebhookSubscriptionCreateRequest>,
) -> Result<(StatusCode, Json<WebhookSubscriptionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to manage webhook subscriptions",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let principal_id = body.principal_id.parse::<PrincipalId>().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_principal_id",
            "principal_id must be a valid UUID",
            false,
            &request_id,
        )
    })?;
    if principal_id != authorization.principal_id {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::IdentityManage,
            "identity.manage permission is required to manage another Agent's subscription",
            &request_id,
        )?;
    }
    let result = WebhookSubscriptionService::new(state.store.clone())
        .create(
            &authorization,
            CreateWebhookSubscriptionCommand {
                organization_id,
                principal_id,
                endpoint_url: body.endpoint_url,
                event_types: body.event_types,
                signing_key_reference: body.signing_key_reference,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| webhook_subscription_api_error(error, &request_id))?;
    let subscription = state
        .store
        .get_webhook_subscription(organization_id, result.subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| webhook_subscription_query_unavailable(&request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(webhook_subscription_response(subscription)),
    ))
}

pub(crate) async fn list_webhook_subscriptions(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<WebhookSubscriptionListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<WebhookSubscriptionListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to list webhook subscriptions",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let requested_principal = query
        .principal_id
        .map(|value| value.parse::<PrincipalId>())
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_principal_id",
                "principal_id must be a valid UUID",
                false,
                &request_id,
            )
        })?;
    let can_manage_all = authorization.permits(organization_id, Permission::IdentityManage)
        && identity
            .scope_restriction
            .as_ref()
            .is_none_or(|scopes| scopes.contains(&Permission::IdentityManage));
    let principal_id = if can_manage_all {
        requested_principal
    } else {
        if requested_principal.is_some_and(|value| value != authorization.principal_id) {
            return Err(api_error(
                StatusCode::FORBIDDEN,
                "permission_denied",
                "identity.manage permission is required to list another Agent's subscriptions",
                false,
                &request_id,
            ));
        }
        Some(authorization.principal_id)
    };
    let after = query
        .after
        .map(|value| value.parse::<WebhookSubscriptionId>())
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_page_cursor",
                "after must be a valid webhook subscription ID",
                false,
                &request_id,
            )
        })?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_page_size",
            "limit must be between 1 and 100",
            false,
            &request_id,
        ));
    }
    let mut subscriptions = state
        .store
        .list_webhook_subscriptions(organization_id, principal_id, after, i64::from(limit) + 1)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?;
    let has_more = subscriptions.len() > usize::from(limit);
    if has_more {
        subscriptions.pop();
    }
    let next_after = has_more.then(|| {
        subscriptions
            .last()
            .expect("a page with more results is non-empty")
            .subscription_id
            .to_string()
    });
    Ok(Json(WebhookSubscriptionListResponse {
        items: subscriptions
            .into_iter()
            .map(webhook_subscription_response)
            .collect(),
        next_after,
    }))
}

pub(crate) async fn get_webhook_subscription(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<WebhookSubscriptionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to read webhook subscriptions",
        &request_id,
    )?;
    let subscription = state
        .store
        .get_webhook_subscription(authorization.organization_id, subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    if subscription.principal_id != authorization.principal_id {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::IdentityManage,
            "identity.manage permission is required to read another Agent's subscription",
            &request_id,
        )?;
    }
    Ok(Json(webhook_subscription_response(subscription)))
}

pub(crate) async fn update_webhook_subscription(
    State(state): State<Arc<AppState>>,
    Path(subscription_id): Path<WebhookSubscriptionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<WebhookSubscriptionUpdateRequest>,
) -> Result<Json<WebhookSubscriptionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AgentOperate,
        "agent.operate permission is required to update webhook subscriptions",
        &request_id,
    )?;
    let existing = state
        .store
        .get_webhook_subscription(authorization.organization_id, subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    if existing.principal_id != authorization.principal_id {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::IdentityManage,
            "identity.manage permission is required to update another Agent's subscription",
            &request_id,
        )?;
    }
    let result = WebhookSubscriptionService::new(state.store.clone())
        .update(
            &authorization,
            UpdateWebhookSubscriptionCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                expected_version: body.expected_version,
                endpoint_url: body.endpoint_url,
                event_types: body.event_types,
                signing_key_reference: body.signing_key_reference,
                enabled: body.enabled,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| webhook_subscription_api_error(error, &request_id))?;
    let subscription = state
        .store
        .get_webhook_subscription(authorization.organization_id, result.subscription_id)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?
        .ok_or_else(|| webhook_subscription_query_unavailable(&request_id))?;
    Ok(Json(webhook_subscription_response(subscription)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebhookDeadLetterListQuery {
    pub(crate) after: Option<String>,
    pub(crate) limit: Option<u16>,
}

pub(crate) async fn list_webhook_dead_letters(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<WebhookDeadLetterListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<WebhookDeadLetterListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::SecurityAdmin,
        "security.admin permission is required to investigate webhook dead letters",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let after = query
        .after
        .map(|value| Uuid::parse_str(&value))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_page_cursor",
                "after must be a valid delivery ID",
                false,
                &request_id,
            )
        })?;
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_page_size",
            "limit must be between 1 and 100",
            false,
            &request_id,
        ));
    }
    let mut dead_letters = state
        .store
        .list_webhook_dead_letters(organization_id, after, i64::from(limit) + 1)
        .await
        .map_err(|_| webhook_subscription_query_unavailable(&request_id))?;
    let has_more = dead_letters.len() > usize::from(limit);
    if has_more {
        dead_letters.pop();
    }
    let next_after = has_more.then(|| {
        dead_letters
            .last()
            .expect("a page with more results is non-empty")
            .delivery_id
            .to_string()
    });
    Ok(Json(WebhookDeadLetterListResponse {
        items: dead_letters
            .into_iter()
            .map(webhook_dead_letter_response)
            .collect(),
        next_after,
    }))
}

pub(crate) async fn replay_webhook_dead_letter(
    State(state): State<Arc<AppState>>,
    Path((subscription_id, event_id)): Path<(WebhookSubscriptionId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(_body): Json<WebhookDeadLetterReplayRequest>,
) -> Result<Json<WebhookDeadLetterReplayResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::SecurityAdmin,
        "security.admin permission is required to replay webhook dead letters",
        &request_id,
    )?;
    let result = WebhookDeadLetterService::new(state.store.clone())
        .replay(
            &authorization,
            ReplayWebhookDeadLetterCommand {
                organization_id: authorization.organization_id,
                subscription_id,
                event_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| webhook_dead_letter_api_error(error, &request_id))?;
    Ok(Json(WebhookDeadLetterReplayResponse {
        subscription_id: result.subscription_id.to_string(),
        event_id: result.event_id.to_string(),
        attempt: result.attempt,
        status: result.status.into(),
    }))
}
