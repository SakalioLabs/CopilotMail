use crate::*;

pub(crate) async fn create_retention_policy(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RetentionPolicyCreateRequest>,
) -> Result<(StatusCode, Json<RetentionPolicyResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let resource_kind = retention_resource_kind_from_wire(&body.resource_kind)
        .ok_or_else(|| invalid_retention_request(&request_id))?;
    let draft = RetentionPolicyService::new(state.store.clone())
        .create(CreateRetentionPolicyCommand {
            context: authorization,
            policy_key: body.policy_key,
            display_name: body.display_name,
            resource_kind,
            minimum_retention_days: body.minimum_retention_days,
            archive_after_days: body.archive_after_days,
            delete_after_days: body.delete_after_days,
            rule: body.rule,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(retention_policy_response(draft.policy, None, draft.version)),
    ))
}

pub(crate) async fn append_retention_policy_version(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<RetentionPolicyId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RetentionPolicyVersionCreateRequest>,
) -> Result<(StatusCode, Json<RetentionPolicyVersionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    let version = RetentionPolicyService::new(state.store.clone())
        .append_version(AppendRetentionPolicyVersionCommand {
            context: authorization,
            policy_id,
            minimum_retention_days: body.minimum_retention_days,
            archive_after_days: body.archive_after_days,
            delete_after_days: body.delete_after_days,
            rule: body.rule,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(retention_policy_version_response(version)),
    ))
}

pub(crate) async fn activate_retention_policy_version(
    State(state): State<Arc<AppState>>,
    Path((policy_id, version_id)): Path<(RetentionPolicyId, RetentionPolicyVersionId)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<RetentionPolicyActivationRequest>,
) -> Result<Json<RetentionPolicyVersionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| invalid_retention_request(&request_id))?;
    let now = Utc::now();
    let version = RetentionPolicyService::new(state.store.clone())
        .activate_version(ActivateRetentionPolicyVersionCommand {
            context: authorization.clone(),
            policy_id,
            policy_version_id: version_id,
            approval: ApprovalProof {
                organization_id: authorization.organization_id,
                request_id: approval_request_id,
                requested_by: authorization.principal_id,
                action: ApprovalAction::RetentionChange,
                resource_kind: "compliance.retention_policy_version".into(),
                resource_id: version_id.to_string(),
                now,
            },
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok(Json(retention_policy_version_response(version)))
}

pub(crate) async fn activate_agent_mail_policy_version(
    State(state): State<Arc<AppState>>,
    Path((agent_id, version_id)): Path<(PrincipalId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMailPolicyActivationRequest>,
) -> Result<Json<AgentMailPolicyActivationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_mail_policy_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| invalid_agent_mail_policy_request(&request_id))?;
    let now = Utc::now();
    let activation = AgentMailPolicyService::new(state.store.clone())
        .activate(ActivateAgentMailPolicyCommand {
            context: authorization.clone(),
            agent_id,
            policy_version_id: version_id,
            approval: ApprovalProof {
                organization_id: authorization.organization_id,
                request_id: approval_request_id,
                requested_by: authorization.principal_id,
                action: ApprovalAction::ProductionAgentMail,
                resource_kind: "agent.mail_policy_version".into(),
                resource_id: version_id.to_string(),
                now,
            },
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| agent_mail_policy_api_error(error, &request_id))?;
    Ok(Json(AgentMailPolicyActivationResponse {
        activation_id: activation.id().to_string(),
        organization_id: activation.organization_id().to_string(),
        agent_id: activation.agent_id().to_string(),
        policy_version: activation.policy_version(),
        approval_request_id: activation.approval_request_id().to_string(),
        activated_by: activation.activated_by().to_string(),
        activated_at: activation.activated_at().to_rfc3339(),
    }))
}

pub(crate) async fn create_agent_mail_policy_version(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<PrincipalId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMailPolicyVersionCreateRequest>,
) -> Result<(StatusCode, Json<AgentMailPolicyVersionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_mail_policy_request(&state, &headers, connection, &request_id).await?;
    let record = AgentMailPolicyService::new(state.store.clone())
        .create_version(CreateAgentMailPolicyVersionCommand {
            context: authorization,
            agent_id,
            allowed_recipient_domains: body.allowed_recipient_domains.into_iter().collect(),
            max_recipients_per_message: body.max_recipients_per_message,
            max_messages_per_hour: body.max_messages_per_hour,
            max_messages_per_recipient_per_day: body.max_messages_per_recipient_per_day,
            require_first_contact_approval: body.require_first_contact_approval,
            max_auto_replies_per_thread_per_day: body.max_auto_replies_per_thread_per_day,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_mail_policy_api_error(error, &request_id))?;
    let policy = &record.policy;
    Ok((
        StatusCode::CREATED,
        Json(AgentMailPolicyVersionResponse {
            policy_version_id: record.id.to_string(),
            organization_id: policy.organization_id().to_string(),
            agent_id: policy.agent_id().to_string(),
            version: policy.version(),
            allowed_recipient_domains: policy.allowed_recipient_domains().iter().cloned().collect(),
            max_recipients_per_message: policy.max_recipients_per_message(),
            max_messages_per_hour: policy.max_messages_per_hour(),
            max_messages_per_recipient_per_day: policy.max_messages_per_recipient_per_day(),
            require_first_contact_approval: policy.require_first_contact_approval(),
            max_auto_replies_per_thread_per_day: policy.max_auto_replies_per_thread_per_day(),
            created_by: record.created_by.to_string(),
            created_at: record.created_at.to_rfc3339(),
        }),
    ))
}

pub(crate) async fn get_agent_mail_policy_version(
    State(state): State<Arc<AppState>>,
    Path((agent_id, version_id)): Path<(PrincipalId, Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AgentMailPolicyVersionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_agent_mail_policy_request(&state, &headers, connection, &request_id).await?;
    let record = AgentMailPolicyService::new(state.store.clone())
        .get_version(GetAgentMailPolicyVersionCommand {
            context: authorization,
            agent_id,
            policy_version_id: version_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| agent_mail_policy_api_error(error, &request_id))?;
    let policy = &record.policy;
    Ok(Json(AgentMailPolicyVersionResponse {
        policy_version_id: record.id.to_string(),
        organization_id: policy.organization_id().to_string(),
        agent_id: policy.agent_id().to_string(),
        version: policy.version(),
        allowed_recipient_domains: policy.allowed_recipient_domains().iter().cloned().collect(),
        max_recipients_per_message: policy.max_recipients_per_message(),
        max_messages_per_hour: policy.max_messages_per_hour(),
        max_messages_per_recipient_per_day: policy.max_messages_per_recipient_per_day(),
        require_first_contact_approval: policy.require_first_contact_approval(),
        max_auto_replies_per_thread_per_day: policy.max_auto_replies_per_thread_per_day(),
        created_by: record.created_by.to_string(),
        created_at: record.created_at.to_rfc3339(),
    }))
}

pub(crate) async fn get_retention_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<RetentionPolicyId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<RetentionPolicyResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_retention_request(&state, &headers, connection, &request_id).await?;
    let detail = RetentionPolicyService::new(state.store.clone())
        .get(GetRetentionPolicyCommand {
            context: authorization,
            policy_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| retention_policy_api_error(error, &request_id))?;
    Ok(Json(retention_policy_response(
        detail.policy,
        detail.active_version,
        detail.latest_version,
    )))
}

pub(crate) async fn authenticate_retention_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::RetentionManage,
        "retention.manage permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| retention_policy_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "retention policy management requires an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

pub(crate) async fn authenticate_agent_mail_policy_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| agent_mail_policy_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "Agent mail policy activation requires an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

pub(crate) fn retention_resource_kind_from_wire(value: &str) -> Option<RetentionResourceKind> {
    match value {
        "audit_evidence" => Some(RetentionResourceKind::AuditEvidence),
        "mail_content" => Some(RetentionResourceKind::MailContent),
        _ => None,
    }
}

pub(crate) fn retention_policy_version_response(
    version: RetentionPolicyVersion,
) -> RetentionPolicyVersionResponse {
    RetentionPolicyVersionResponse {
        retention_policy_version_id: version.id().to_string(),
        version: version.version(),
        minimum_retention_days: version.minimum_retention_days(),
        archive_after_days: version.archive_after_days(),
        delete_after_days: version.delete_after_days(),
        rule: version.rule().clone(),
        created_by: version.created_by().to_string(),
        created_at: version.created_at().to_rfc3339(),
    }
}

pub(crate) fn retention_policy_response(
    policy: RetentionPolicy,
    active_version: Option<RetentionPolicyVersion>,
    latest_version: RetentionPolicyVersion,
) -> RetentionPolicyResponse {
    RetentionPolicyResponse {
        retention_policy_id: policy.id().to_string(),
        organization_id: policy.organization_id().to_string(),
        policy_key: policy.policy_key().into(),
        display_name: policy.display_name().into(),
        resource_kind: match policy.resource_kind() {
            RetentionResourceKind::AuditEvidence => "audit_evidence",
            RetentionResourceKind::MailContent => "mail_content",
        }
        .into(),
        active_version: active_version.map(retention_policy_version_response),
        latest_version: retention_policy_version_response(latest_version),
        created_by: policy.created_by().to_string(),
        created_at: policy.created_at().to_rfc3339(),
    }
}

pub(crate) fn retention_policy_api_error(
    error: RetentionPolicyError,
    request_id: &str,
) -> Response {
    match error {
        RetentionPolicyError::Domain(_) => invalid_retention_request(request_id),
        RetentionPolicyError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "retention_policy_operation_forbidden",
            "the retention policy operation is not authorized",
            false,
            request_id,
        ),
        RetentionPolicyError::NotFound => resource_not_found(request_id),
        RetentionPolicyError::Conflict
        | RetentionPolicyError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "retention_policy_conflict",
            "the retention policy changed or the approval was already consumed",
            false,
            request_id,
        ),
        RetentionPolicyError::Store(copilotmail_application::StoreError::AuthenticationInvalid) => {
            authentication_invalid(request_id)
        }
        RetentionPolicyError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => retention_policy_store_unavailable(request_id),
    }
}

pub(crate) fn invalid_retention_request(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_retention_policy",
        "the retention policy violates its key, duration, scope, or rule constraints",
        false,
        request_id,
    )
}

pub(crate) fn retention_policy_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "retention_policy_store_unavailable",
        "retention policy state is temporarily unavailable",
        true,
        request_id,
    )
}

pub(crate) fn agent_mail_policy_api_error(
    error: AgentMailPolicyError,
    request_id: &str,
) -> Response {
    match error {
        AgentMailPolicyError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_mail_policy_activation_forbidden",
            "the exact live approval and active Human authorization are required",
            false,
            request_id,
        ),
        AgentMailPolicyError::NotFound => api_error(
            StatusCode::NOT_FOUND,
            "agent_mail_policy_version_not_found",
            "the policy version is unavailable",
            false,
            request_id,
        ),
        AgentMailPolicyError::Conflict => api_error(
            StatusCode::CONFLICT,
            "agent_mail_policy_activation_conflict",
            "the approval or active policy state conflicts with this request",
            false,
            request_id,
        ),
        AgentMailPolicyError::Store(_) => agent_mail_policy_store_unavailable(request_id),
        AgentMailPolicyError::Domain(_) => invalid_agent_mail_policy_request(request_id),
    }
}

pub(crate) fn agent_mail_policy_query_api_error(
    error: AgentMailPolicyQueryError,
    request_id: &str,
) -> Response {
    match error {
        AgentMailPolicyQueryError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_mail_policy_query_forbidden",
            "the authenticated Agent cannot inspect mail policy state",
            false,
            request_id,
        ),
        AgentMailPolicyQueryError::Unavailable | AgentMailPolicyQueryError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_mail_policy_query_unavailable",
            "Agent mail policy state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) const fn agent_environment_wire(environment: AgentEnvironment) -> &'static str {
    match environment {
        AgentEnvironment::Development => "development",
        AgentEnvironment::Testing => "testing",
        AgentEnvironment::Production => "production",
    }
}

pub(crate) const fn agent_risk_level_wire(risk_level: AgentRiskLevel) -> &'static str {
    match risk_level {
        AgentRiskLevel::Low => "low",
        AgentRiskLevel::Medium => "medium",
        AgentRiskLevel::High => "high",
    }
}

pub(crate) fn invalid_agent_mail_policy_request(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_agent_mail_policy_request",
        "the Agent mail policy activation request is invalid",
        false,
        request_id,
    )
}

pub(crate) fn agent_mail_policy_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "agent_mail_policy_store_unavailable",
        "Agent mail policy persistence is unavailable",
        true,
        request_id,
    )
}
