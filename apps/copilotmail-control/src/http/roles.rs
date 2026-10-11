use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleAssignmentHttpQuery {
    pub(crate) principal_id: Option<PrincipalId>,
    pub(crate) include_inactive: Option<bool>,
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<i64>,
}

pub(crate) async fn list_roles(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<RoleListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to list roles",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let roles = RoleGovernanceService::new(state.store.clone())
        .list_roles(ListRolesCommand {
            context: authorization,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    Ok(Json(RoleListResponse {
        items: roles.into_iter().map(role_response).collect(),
    }))
}

pub(crate) async fn list_role_assignments(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<RoleAssignmentHttpQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<RoleAssignmentListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to list role assignments",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let include_inactive = query.include_inactive.unwrap_or(false);
    let limit = query.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(invalid_role_assignment_filter(&request_id));
    }
    let after_assignment_id = query
        .cursor
        .as_deref()
        .map(|cursor| {
            decode_role_assignment_cursor(
                cursor,
                organization_id,
                query.principal_id,
                include_inactive,
            )
        })
        .transpose()
        .map_err(|_| invalid_role_assignment_cursor(&request_id))?
        .map(RoleAssignmentId::from_uuid);
    let now = Utc::now();
    if let Some(after_assignment_id) = after_assignment_id
        && !state
            .store
            .role_assignment_cursor_exists(
                RoleAssignmentListQuery {
                    organization_id,
                    principal_id: query.principal_id,
                    include_inactive,
                    after_assignment_id: None,
                    limit,
                    now,
                },
                after_assignment_id,
            )
            .await
            .map_err(|_| role_governance_store_unavailable(&request_id))?
    {
        return Err(invalid_role_assignment_cursor(&request_id));
    }
    let page = RoleGovernanceService::new(state.store.clone())
        .list_assignments(ListRoleAssignmentsCommand {
            context: authorization,
            principal_id: query.principal_id,
            include_inactive,
            after_assignment_id,
            limit,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    let next_cursor = page
        .has_more
        .then(|| page.items.last())
        .flatten()
        .map(|item| {
            encode_role_assignment_cursor(
                organization_id,
                query.principal_id,
                include_inactive,
                item.assignment_id.as_uuid(),
            )
        });
    Ok(Json(RoleAssignmentListResponse {
        items: page
            .items
            .into_iter()
            .map(|assignment| role_assignment_response(assignment, now))
            .collect(),
        next_cursor,
        has_more: page.has_more,
    }))
}

pub(crate) async fn grant_role_assignment(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RoleAssignmentGrantRequest>,
) -> Result<(StatusCode, Json<RoleAssignmentResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to grant roles",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let principal_id = body
        .principal_id
        .parse::<PrincipalId>()
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let role_id = body
        .role_id
        .parse::<RoleId>()
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let valid_until = DateTime::parse_from_rfc3339(&body.valid_until)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let now = Utc::now();
    let resource_id = role_assignment_approval_resource_id(principal_id, role_id, valid_until)
        .map_err(|_| invalid_role_assignment_request(&request_id))?;
    let grant = RoleGovernanceService::new(state.store.clone())
        .grant(GrantRoleAssignmentCommand {
            context: authorization.clone(),
            principal_id,
            role_id,
            valid_until,
            approval: ApprovalProof {
                organization_id,
                request_id: approval_request_id,
                requested_by: authorization.principal_id,
                action: ApprovalAction::PrivilegeElevation,
                resource_kind: "authorization.role_assignment".into(),
                resource_id,
                now,
            },
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    let assignment = state
        .store
        .get_role_assignment(organization_id, grant.assignment_id)
        .await
        .map_err(|_| role_governance_store_unavailable(&request_id))?
        .ok_or_else(|| role_governance_store_unavailable(&request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(role_assignment_response(assignment, now)),
    ))
}

pub(crate) async fn revoke_role_assignment(
    State(state): State<Arc<AppState>>,
    Path(role_assignment_id): Path<RoleAssignmentId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(_body): Json<RoleAssignmentRevocationRequest>,
) -> Result<Json<RoleAssignmentResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to revoke roles",
        &request_id,
    )?;
    let now = Utc::now();
    let assignment = RoleGovernanceService::new(state.store.clone())
        .revoke(RevokeRoleAssignmentCommand {
            context: authorization,
            assignment_id: role_assignment_id,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| role_governance_api_error(error, &request_id))?;
    Ok(Json(role_assignment_response(assignment, now)))
}

pub(crate) fn role_governance_api_error(error: RoleGovernanceError, request_id: &str) -> Response {
    match error {
        RoleGovernanceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "role_governance_forbidden",
            "the authorization context denied role governance",
            false,
            request_id,
        ),
        RoleGovernanceError::NotFound => resource_not_found(request_id),
        RoleGovernanceError::LastActiveOwner => api_error(
            StatusCode::CONFLICT,
            "last_active_owner",
            "the last effective organization owner cannot be revoked",
            false,
            request_id,
        ),
        RoleGovernanceError::Conflict
        | RoleGovernanceError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "role_assignment_conflict",
            "the role assignment conflicts with current governance state",
            true,
            request_id,
        ),
        RoleGovernanceError::Domain(_) => invalid_role_assignment_request(request_id),
        RoleGovernanceError::Store(copilotmail_application::StoreError::AuthenticationInvalid) => {
            authentication_invalid(request_id)
        }
        RoleGovernanceError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => role_governance_store_unavailable(request_id),
    }
}

pub(crate) fn invalid_role_assignment_request(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_role_assignment",
        "the role assignment request is invalid",
        false,
        request_id,
    )
}

pub(crate) fn invalid_role_assignment_filter(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_role_assignment_filter",
        "the role assignment filters are invalid",
        false,
        request_id,
    )
}

pub(crate) fn invalid_role_assignment_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::GONE,
        "role_assignment_cursor_expired",
        "the role assignment cursor is invalid or no longer visible",
        false,
        request_id,
    )
}

pub(crate) fn role_governance_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "role_governance_unavailable",
        "role governance is temporarily unavailable",
        true,
        request_id,
    )
}

pub(crate) const fn approval_status_wire(status: ApprovalStatus) -> &'static str {
    match status {
        ApprovalStatus::Pending => "pending",
        ApprovalStatus::Approved => "approved",
        ApprovalStatus::Rejected => "rejected",
        ApprovalStatus::Expired => "expired",
        ApprovalStatus::Cancelled => "cancelled",
    }
}

pub(crate) fn role_response(role: RoleSummary) -> RoleResponse {
    RoleResponse {
        role_id: role.role_id.to_string(),
        organization_id: role.organization_id.to_string(),
        system_role: role.system_role.as_str().into(),
        display_name: role.display_name,
        permissions: role
            .permissions
            .into_iter()
            .map(|permission| permission.as_str().to_owned())
            .collect(),
        created_at: role.created_at.to_rfc3339(),
    }
}

pub(crate) fn role_assignment_response(
    assignment: RoleAssignmentSummary,
    now: DateTime<Utc>,
) -> RoleAssignmentResponse {
    let active = assignment.is_active_at(now);
    RoleAssignmentResponse {
        role_assignment_id: assignment.assignment_id.to_string(),
        organization_id: assignment.organization_id.to_string(),
        principal_id: assignment.principal_id.to_string(),
        principal_kind: principal_kind_wire(assignment.principal_kind).into(),
        principal_status: principal_status_wire(assignment.principal_status).into(),
        principal_display_name: assignment.principal_display_name,
        role_id: assignment.role_id.to_string(),
        system_role: assignment.system_role.as_str().into(),
        role_display_name: assignment.role_display_name,
        created_by: assignment.created_by.to_string(),
        approval_request_id: assignment
            .approval_request_id
            .map(|approval_id| approval_id.to_string()),
        valid_from: assignment.valid_from.to_rfc3339(),
        valid_until: assignment.valid_until.map(|value| value.to_rfc3339()),
        revoked_at: assignment.revoked_at.map(|value| value.to_rfc3339()),
        created_at: assignment.created_at.to_rfc3339(),
        active,
    }
}

pub(crate) const fn principal_kind_wire(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Human => "human",
        PrincipalKind::Agent => "agent",
        PrincipalKind::Service => "service",
    }
}

pub(crate) const fn principal_status_wire(status: PrincipalStatus) -> &'static str {
    match status {
        PrincipalStatus::Draft => "draft",
        PrincipalStatus::Active => "active",
        PrincipalStatus::Suspended => "suspended",
        PrincipalStatus::Retired => "retired",
    }
}

pub(crate) fn encode_role_assignment_cursor(
    organization_id: OrganizationId,
    principal_id: Option<PrincipalId>,
    include_inactive: bool,
    id: Uuid,
) -> String {
    format!(
        "v1.role-assignment.{}.{}.{}.{}",
        principal_id
            .map(|principal_id| principal_id.to_string())
            .unwrap_or_else(|| "all".into()),
        if include_inactive { "all" } else { "active" },
        organization_id,
        id
    )
}

pub(crate) fn decode_role_assignment_cursor(
    cursor: &str,
    organization_id: OrganizationId,
    principal_id: Option<PrincipalId>,
    include_inactive: bool,
) -> Result<Uuid, ()> {
    let mut parts = cursor.split('.');
    let expected_principal = principal_id
        .map(|principal_id| principal_id.to_string())
        .unwrap_or_else(|| "all".into());
    if parts.next() != Some("v1")
        || parts.next() != Some("role-assignment")
        || parts.next() != Some(expected_principal.as_str())
        || parts.next() != Some(if include_inactive { "all" } else { "active" })
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

pub(crate) const fn approval_view_wire(view: ApprovalRequestView) -> &'static str {
    match view {
        ApprovalRequestView::Requested => "requested",
        ApprovalRequestView::Actionable => "actionable",
    }
}
