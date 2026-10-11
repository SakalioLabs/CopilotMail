use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ComplianceCaseListQuery {
    pub(crate) status: Option<String>,
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LegalHoldListQuery {
    pub(crate) status: Option<String>,
    pub(crate) case_id: Option<String>,
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<i64>,
}

pub(crate) async fn list_compliance_cases(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<ComplianceCaseListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ComplianceCaseListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let status = parse_compliance_case_filter(query.status.as_deref())
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let limit = bounded_compliance_limit(query.limit)
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let after_case_id = query
        .cursor
        .as_deref()
        .map(|cursor| decode_compliance_cursor(cursor, "case", organization_id))
        .transpose()
        .map_err(|_| invalid_compliance_cursor(&request_id))?
        .map(ComplianceCaseId::from_uuid);
    if let Some(after_case_id) = after_case_id
        && !state
            .store
            .compliance_case_cursor_exists(organization_id, status, after_case_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
    {
        return Err(invalid_compliance_cursor(&request_id));
    }
    let mut cases = state
        .store
        .list_compliance_cases(organization_id, status, after_case_id, limit + 1)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    let has_more = cases.len() > limit as usize;
    if has_more {
        cases.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| cases.last())
        .flatten()
        .map(|case| encode_compliance_cursor("case", organization_id, case.id().as_uuid()));
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::Case,
        organization_id.to_string(),
        serde_json::json!({
            "operation": "list",
            "status": query.status,
            "result_count": cases.len(),
            "has_more": has_more,
            "cursor_present": after_case_id.is_some(),
        }),
        &request_id,
    )
    .await?;
    Ok(Json(ComplianceCaseListResponse {
        items: cases.into_iter().map(compliance_case_response).collect(),
        next_cursor,
        has_more,
    }))
}

pub(crate) async fn list_legal_holds(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<LegalHoldListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<LegalHoldListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let status = parse_legal_hold_filter(query.status.as_deref())
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let case_id = query
        .case_id
        .as_deref()
        .map(str::parse::<ComplianceCaseId>)
        .transpose()
        .map_err(|_| invalid_compliance_list_filter(&request_id))?;
    let limit = bounded_compliance_limit(query.limit)
        .ok_or_else(|| invalid_compliance_list_filter(&request_id))?;
    let after_hold_id = query
        .cursor
        .as_deref()
        .map(|cursor| decode_compliance_cursor(cursor, "hold", organization_id))
        .transpose()
        .map_err(|_| invalid_compliance_cursor(&request_id))?
        .map(LegalHoldId::from_uuid);
    if let Some(after_hold_id) = after_hold_id
        && !state
            .store
            .legal_hold_cursor_exists(organization_id, status, case_id, after_hold_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
    {
        return Err(invalid_compliance_cursor(&request_id));
    }
    let mut holds = state
        .store
        .list_legal_holds(organization_id, status, case_id, after_hold_id, limit + 1)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    let has_more = holds.len() > limit as usize;
    if has_more {
        holds.truncate(limit as usize);
    }
    let next_cursor = has_more
        .then(|| holds.last())
        .flatten()
        .map(|hold| encode_compliance_cursor("hold", organization_id, hold.id().as_uuid()));
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::LegalHold,
        organization_id.to_string(),
        serde_json::json!({
            "operation": "list",
            "status": query.status,
            "case_id": case_id,
            "result_count": holds.len(),
            "has_more": has_more,
            "cursor_present": after_hold_id.is_some(),
        }),
        &request_id,
    )
    .await?;
    Ok(Json(LegalHoldListResponse {
        items: holds.into_iter().map(legal_hold_response).collect(),
        next_cursor,
        has_more,
    }))
}

pub(crate) fn parse_compliance_case_filter(
    value: Option<&str>,
) -> Option<Option<ComplianceCaseStatus>> {
    match value {
        None => Some(None),
        Some("open") => Some(Some(ComplianceCaseStatus::Open)),
        Some("closed") => Some(Some(ComplianceCaseStatus::Closed)),
        Some(_) => None,
    }
}

pub(crate) fn parse_legal_hold_filter(value: Option<&str>) -> Option<Option<LegalHoldStatus>> {
    match value {
        None => Some(None),
        Some("active") => Some(Some(LegalHoldStatus::Active)),
        Some("released") => Some(Some(LegalHoldStatus::Released)),
        Some(_) => None,
    }
}

pub(crate) fn bounded_compliance_limit(limit: Option<i64>) -> Option<i64> {
    let limit = limit.unwrap_or(50);
    (1..=100).contains(&limit).then_some(limit)
}

pub(crate) fn encode_compliance_cursor(
    kind: &str,
    organization_id: OrganizationId,
    id: Uuid,
) -> String {
    format!("v1.{kind}.{organization_id}.{id}")
}

pub(crate) fn decode_compliance_cursor(
    cursor: &str,
    expected_kind: &str,
    organization_id: OrganizationId,
) -> Result<Uuid, ()> {
    let mut parts = cursor.split('.');
    if parts.next() != Some("v1")
        || parts.next() != Some(expected_kind)
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

pub(crate) fn invalid_compliance_list_filter(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_compliance_list_filter",
        "status, case_id, or limit is invalid",
        false,
        request_id,
    )
}

pub(crate) fn invalid_compliance_cursor(request_id: &str) -> Response {
    api_error(
        StatusCode::GONE,
        "compliance_cursor_expired_or_invalid",
        "the cursor is unavailable or does not match the tenant and active filters",
        false,
        request_id,
    )
}
