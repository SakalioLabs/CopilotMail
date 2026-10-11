use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditEventSearchQuery {
    pub(crate) occurred_from: Option<String>,
    pub(crate) occurred_to: Option<String>,
    pub(crate) action: Option<String>,
    pub(crate) actor_principal_id: Option<String>,
    pub(crate) resource_type: Option<String>,
    pub(crate) resource_id: Option<String>,
    pub(crate) outcome: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<u16>,
}

pub(crate) async fn search_audit_events(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<AuditEventSearchQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AuditEventSearchResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuditRead,
        "audit.read permission is required to search audit events",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }

    let now = Utc::now();
    let occurred_from = query
        .occurred_from
        .as_deref()
        .map(parse_audit_query_time)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?
        .unwrap_or(now - chrono::Duration::hours(24));
    let occurred_to = query
        .occurred_to
        .as_deref()
        .map(parse_audit_query_time)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?
        .unwrap_or(now);
    let actor_principal_id = query
        .actor_principal_id
        .as_deref()
        .map(str::parse::<PrincipalId>)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?;
    let outcome = query
        .outcome
        .as_deref()
        .map(parse_audit_query_outcome)
        .transpose()
        .map_err(|_| invalid_audit_event_query(&request_id))?;
    let before_chain_sequence = query
        .cursor
        .as_deref()
        .map(|cursor| parse_audit_event_cursor(cursor, organization_id))
        .transpose()
        .map_err(|error| audit_event_cursor_parse_error(error, &request_id))?;

    let page = AuditEventSearchService::new(state.store.clone())
        .search(
            &authorization,
            SearchAuditEventsCommand {
                organization_id,
                occurred_from,
                occurred_to,
                action: query.action,
                actor_principal_id,
                resource_type: query.resource_type,
                resource_id: query.resource_id,
                outcome,
                request_id: query.request_id,
                before_chain_sequence,
                limit: i64::from(query.limit.unwrap_or(50)),
                trace_id: request_id.clone(),
                now,
            },
        )
        .await
        .map_err(|error| audit_event_search_api_error(error, &request_id))?;
    Ok(Json(AuditEventSearchResponse {
        query_id: page.query_id.to_string(),
        occurred_from: occurred_from.to_rfc3339(),
        occurred_to: occurred_to.to_rfc3339(),
        items: page.items.into_iter().map(audit_event_response).collect(),
        next_cursor: page
            .next_chain_sequence
            .map(|sequence| format_audit_event_cursor(organization_id, sequence)),
        has_more: page.has_more,
    }))
}

pub(crate) fn parse_audit_query_time(value: &str) -> Result<DateTime<Utc>, ()> {
    if value.len() > 40 {
        return Err(());
    }
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ())
}

pub(crate) fn parse_audit_query_outcome(value: &str) -> Result<AuditOutcome, ()> {
    match value {
        "success" => Ok(AuditOutcome::Success),
        "failure" => Ok(AuditOutcome::Failure),
        "denied" => Ok(AuditOutcome::Denied),
        _ => Err(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuditEventCursorParseError {
    Invalid,
    ScopeMismatch,
}

pub(crate) fn parse_audit_event_cursor(
    value: &str,
    expected_organization_id: OrganizationId,
) -> Result<i64, AuditEventCursorParseError> {
    if value.len() > 64 {
        return Err(AuditEventCursorParseError::Invalid);
    }
    let mut parts = value.split('.');
    let version = parts.next();
    let organization_id = parts
        .next()
        .and_then(|value| value.parse::<OrganizationId>().ok());
    let chain_sequence = parts.next().and_then(|value| value.parse::<i64>().ok());
    if version != Some("v1")
        || parts.next().is_some()
        || organization_id.is_none()
        || chain_sequence.is_none_or(|sequence| sequence <= 0)
    {
        return Err(AuditEventCursorParseError::Invalid);
    }
    if organization_id != Some(expected_organization_id) {
        return Err(AuditEventCursorParseError::ScopeMismatch);
    }
    Ok(chain_sequence.expect("validated positive audit chain sequence"))
}

pub(crate) fn format_audit_event_cursor(
    organization_id: OrganizationId,
    chain_sequence: i64,
) -> String {
    format!("v1.{organization_id}.{chain_sequence}")
}

pub(crate) fn audit_event_cursor_parse_error(
    error: AuditEventCursorParseError,
    request_id: &str,
) -> Response {
    match error {
        AuditEventCursorParseError::Invalid => invalid_audit_event_query(request_id),
        AuditEventCursorParseError::ScopeMismatch => api_error(
            StatusCode::BAD_REQUEST,
            "audit_cursor_scope_mismatch",
            "the audit cursor belongs to another organization",
            false,
            request_id,
        ),
    }
}

pub(crate) fn invalid_audit_event_query(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_audit_event_query",
        "the audit event filters, time window, cursor, or limit are invalid",
        false,
        request_id,
    )
}

pub(crate) fn audit_event_response(event: AuditEventSummary) -> AuditEventResponse {
    AuditEventResponse {
        event_id: event.event_id.to_string(),
        organization_id: event.organization_id.to_string(),
        actor_principal_id: event.actor_principal_id.to_string(),
        actor_kind: event.actor_kind,
        on_behalf_of_principal_id: event.on_behalf_of_principal_id.map(|id| id.to_string()),
        credential_id: event.credential_id,
        subject_principal_id: event.subject_principal_id.map(|id| id.to_string()),
        action: event.action,
        resource_type: event.resource_type,
        resource_id: event.resource_id,
        outcome: match event.outcome {
            AuditOutcome::Success => "success",
            AuditOutcome::Failure => "failure",
            AuditOutcome::Denied => "denied",
        }
        .into(),
        trace_id: event.trace_id,
        request_id: event.request_id,
        source_ip: event.source_ip,
        client_id: event.client_id,
        session_id: event.session_id,
        token_jti: event.token_jti,
        policy_id: event.policy_id.map(|id| id.to_string()),
        policy_version_id: event.policy_version_id.map(|id| id.to_string()),
        approval_request_id: event.approval_request_id.map(|id| id.to_string()),
        ticket_reference: event.ticket_reference,
        environment: event.environment,
        region: event.region,
        details: event.details,
        occurred_at: event.occurred_at.to_rfc3339(),
        received_at: event.received_at.to_rfc3339(),
        chain_sequence: event.chain_sequence.to_string(),
        hash_version: event.hash_version,
        previous_hash: event.previous_hash_hex,
        event_hash: event.event_hash_hex,
        signature_batch_id: event.signature_batch_id.map(|id| id.to_string()),
    }
}
