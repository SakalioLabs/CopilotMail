use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalRequestListQuery {
    pub(crate) view: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<i64>,
}

pub(crate) async fn list_approval_requests(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<ApprovalRequestListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ApprovalRequestListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let view = match query.view.as_deref().unwrap_or("actionable") {
        "actionable" => ApprovalRequestView::Actionable,
        "requested" => ApprovalRequestView::Requested,
        _ => return Err(invalid_approval_list_filter(&request_id)),
    };
    if view == ApprovalRequestView::Actionable {
        enforce_permission_and_scope(
            &identity,
            &authorization,
            Permission::AuthorizationManage,
            "authorization.manage permission is required to read the approval inbox",
            &request_id,
        )?;
    }
    let status = parse_approval_status_filter(query.status.as_deref())
        .ok_or_else(|| invalid_approval_list_filter(&request_id))?;
    if view == ApprovalRequestView::Actionable
        && status.is_some()
        && status != Some(ApprovalStatus::Pending)
    {
        return Err(invalid_approval_list_filter(&request_id));
    }
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(invalid_approval_list_filter(&request_id));
    }
    let after_request_id = query
        .cursor
        .as_deref()
        .map(|cursor| decode_approval_cursor(cursor, organization_id, view, status))
        .transpose()
        .map_err(|_| invalid_approval_cursor(&request_id))?
        .map(ApprovalRequestId::from_uuid);
    let now = Utc::now();
    if let Some(after_request_id) = after_request_id
        && !state
            .store
            .approval_request_cursor_exists(
                organization_id,
                view,
                authorization.principal_id,
                status,
                after_request_id,
                now,
            )
            .await
            .map_err(|_| approval_store_unavailable(&request_id))?
    {
        return Err(invalid_approval_cursor(&request_id));
    }
    let page = ApprovalWorkflowService::new(state.store.clone())
        .list(ListApprovalRequestsCommand {
            context: authorization,
            view,
            status,
            after_request_id,
            limit,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    let next_cursor = page
        .has_more
        .then(|| page.items.last())
        .flatten()
        .map(|item| {
            encode_approval_cursor(organization_id, view, status, item.request_id.as_uuid())
        });
    Ok(Json(ApprovalRequestListResponse {
        items: page
            .items
            .into_iter()
            .map(approval_request_summary_response)
            .collect(),
        next_cursor,
        has_more: page.has_more,
    }))
}

pub(crate) async fn get_approval_request(
    State(state): State<Arc<AppState>>,
    Path(approval_request_id): Path<ApprovalRequestId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ApprovalRequestDetailResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    let detail = ApprovalWorkflowService::new(state.store.clone())
        .get(GetApprovalRequestCommand {
            context: authorization,
            request_id: approval_request_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok(Json(approval_request_detail_response(detail)))
}

pub(crate) async fn cancel_approval_request(
    State(state): State<Arc<AppState>>,
    Path(approval_request_id): Path<ApprovalRequestId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(_body): Json<ApprovalCancellationRequest>,
) -> Result<Json<ApprovalDecisionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    let result = ApprovalWorkflowService::new(state.store.clone())
        .cancel(CancelApprovalRequestCommand {
            context: authorization,
            request_id: approval_request_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok(Json(ApprovalDecisionResponse {
        approval_request_id: result.request_id.to_string(),
        status: approval_status_wire(result.status).into(),
        approval_count: result.approval_count,
    }))
}

pub(crate) async fn create_approval_request(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<ApprovalRequestCreateRequest>,
) -> Result<(StatusCode, Json<ApprovalRequestResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    let Some(action) = approval_action_from_wire(&body.action) else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_approval_action",
            "the approval action is not supported",
            false,
            &request_id,
        ));
    };
    let required = approval_request_permission(action);
    enforce_permission_and_scope(
        &identity,
        &authorization,
        required,
        "the requested high-risk action is not permitted",
        &request_id,
    )?;
    let expires_at = chrono::DateTime::parse_from_rfc3339(&body.expires_at)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_expiration",
                "expires_at must be an RFC 3339 instant",
                false,
                &request_id,
            )
        })?;
    let approval_request_id = ApprovalWorkflowService::new(state.store.clone())
        .request(RequestApprovalCommand {
            context: authorization,
            policy_key: body.policy_key,
            action,
            resource_kind: body.resource_kind,
            resource_id: body.resource_id,
            reason: body.reason,
            ticket_reference: body.ticket_reference,
            expires_at,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(ApprovalRequestResponse {
            approval_request_id: approval_request_id.to_string(),
            status: approval_status_wire(ApprovalStatus::Pending).into(),
        }),
    ))
}

pub(crate) async fn decide_approval_request(
    State(state): State<Arc<AppState>>,
    Path(approval_request_id): Path<ApprovalRequestId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<ApprovalDecisionRequest>,
) -> Result<Json<ApprovalDecisionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to decide approvals",
        &request_id,
    )?;
    let decision = match body.decision.as_str() {
        "approved" => ApprovalDecision::Approved,
        "rejected" => ApprovalDecision::Rejected,
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "invalid_approval_decision",
                "decision must be approved or rejected",
                false,
                &request_id,
            ));
        }
    };
    let result = ApprovalWorkflowService::new(state.store.clone())
        .decide(DecideApprovalCommand {
            context: authorization,
            request_id: approval_request_id,
            decision,
            reason: body.reason,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| approval_api_error(error, &request_id))?;
    Ok(Json(ApprovalDecisionResponse {
        approval_request_id: result.request_id.to_string(),
        status: approval_status_wire(result.status).into(),
        approval_count: result.approval_count,
    }))
}

pub(crate) fn approval_api_error(error: ApprovalWorkflowError, request_id: &str) -> Response {
    match error {
        ApprovalWorkflowError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_approval",
            "the approval request or decision is invalid",
            false,
            request_id,
        ),
        ApprovalWorkflowError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the approval policy or authorization context denied the operation",
            false,
            request_id,
        ),
        ApprovalWorkflowError::Store(copilotmail_application::StoreError::Conflict) => api_error(
            StatusCode::CONFLICT,
            "approval_conflict",
            "the approval was already decided, expired, or violates separation of duty",
            false,
            request_id,
        ),
        ApprovalWorkflowError::Store(copilotmail_application::StoreError::MissingReference) => {
            api_error(
                StatusCode::NOT_FOUND,
                "resource_not_found",
                "the requested approval policy or request does not exist",
                false,
                request_id,
            )
        }
        ApprovalWorkflowError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        ApprovalWorkflowError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "approval_unavailable",
            "approval state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn parse_approval_status_filter(value: Option<&str>) -> Option<Option<ApprovalStatus>> {
    match value {
        None => Some(None),
        Some("pending") => Some(Some(ApprovalStatus::Pending)),
        Some("approved") => Some(Some(ApprovalStatus::Approved)),
        Some("rejected") => Some(Some(ApprovalStatus::Rejected)),
        Some("expired") => Some(Some(ApprovalStatus::Expired)),
        Some("cancelled") => Some(Some(ApprovalStatus::Cancelled)),
        Some(_) => None,
    }
}

pub(crate) fn encode_approval_cursor(
    organization_id: OrganizationId,
    view: ApprovalRequestView,
    status: Option<ApprovalStatus>,
    id: Uuid,
) -> String {
    format!(
        "v1.approval.{}.{}.{}.{}",
        approval_view_wire(view),
        status.map(approval_status_wire).unwrap_or("all"),
        organization_id,
        id
    )
}

pub(crate) fn decode_approval_cursor(
    cursor: &str,
    organization_id: OrganizationId,
    view: ApprovalRequestView,
    status: Option<ApprovalStatus>,
) -> Result<Uuid, ()> {
    let mut parts = cursor.split('.');
    if parts.next() != Some("v1")
        || parts.next() != Some("approval")
        || parts.next() != Some(approval_view_wire(view))
        || parts.next() != Some(status.map(approval_status_wire).unwrap_or("all"))
        || parts
            .next()
            .and_then(|value| value.parse::<OrganizationId>().ok())
            != Some(organization_id)
    {
        return Err(());
    }
    let id = parts.next().and_then(|value| Uuid::parse_str(value).ok());
    if parts.next().is_some() {
        return Err(());
    }
    id.ok_or(())
}

pub(crate) fn approval_request_summary_response(
    request: ApprovalRequestSummary,
) -> ApprovalRequestSummaryResponse {
    ApprovalRequestSummaryResponse {
        approval_request_id: request.request_id.to_string(),
        organization_id: request.organization_id.to_string(),
        policy_version_id: request.policy_version_id.to_string(),
        action: request.action.as_str().into(),
        resource_kind: request.resource_kind,
        resource_id: request.resource_id,
        requested_by: request.requested_by.to_string(),
        reason: request.reason,
        ticket_reference: request.ticket_reference,
        required_approvals: request.required_approvals,
        approval_count: request.approval_count,
        rejection_count: request.rejection_count,
        status: approval_status_wire(request.status).into(),
        created_at: request.created_at.to_rfc3339(),
        expires_at: request.expires_at.to_rfc3339(),
        completed_at: request.completed_at.map(|value| value.to_rfc3339()),
        cancelled_at: request.cancelled_at.map(|value| value.to_rfc3339()),
    }
}

pub(crate) fn approval_request_detail_response(
    detail: ApprovalRequestDetail,
) -> ApprovalRequestDetailResponse {
    ApprovalRequestDetailResponse {
        request: approval_request_summary_response(detail.request),
        steps: detail
            .steps
            .into_iter()
            .map(|step| ApprovalStepResponse {
                approval_step_id: step.step_id.to_string(),
                approver_principal_id: step.approver_principal_id.to_string(),
                decision: match step.decision {
                    ApprovalDecision::Approved => "approved",
                    ApprovalDecision::Rejected => "rejected",
                }
                .into(),
                reason: step.reason,
                decided_at: step.decided_at.to_rfc3339(),
            })
            .collect(),
    }
}

pub(crate) fn invalid_approval_list_filter(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_approval_list_filter",
        "view, status, or limit is invalid",
        false,
        request_id,
    )
}

pub(crate) fn invalid_approval_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::GONE,
        "approval_cursor_expired_or_invalid",
        "the cursor is unavailable or does not match the tenant and active filters",
        false,
        request_id,
    )
}

pub(crate) fn approval_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "approval_unavailable",
        "approval state is temporarily unavailable",
        true,
        request_id,
    )
}
