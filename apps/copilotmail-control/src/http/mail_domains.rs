use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MailDomainListQuery {
    pub(crate) after: Option<String>,
    pub(crate) limit: Option<u16>,
}

pub(crate) async fn list_mail_domains(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Query(query): Query<MailDomainListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailDomainListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to list mail domains",
        &request_id,
    )?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
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
    let after = query
        .after
        .map(MailDomainName::parse)
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_page_cursor",
                "after must be a canonicalizable mail domain name",
                false,
                &request_id,
            )
        })?;
    let mut domains = state
        .store
        .list_mail_domains(
            organization_id,
            after.as_ref(),
            i64::from(limit) + 1,
            Utc::now(),
        )
        .await
        .map_err(|_| mail_domain_query_unavailable(&request_id))?;
    let has_more = domains.len() > usize::from(limit);
    if has_more {
        domains.pop();
    }
    let next_after = has_more
        .then(|| domains.last().map(|domain| domain.name.to_string()))
        .flatten();
    Ok(Json(MailDomainListResponse {
        items: domains.into_iter().map(mail_domain_response).collect(),
        next_after,
    }))
}

pub(crate) async fn get_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailDomainResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to read mail domains",
        &request_id,
    )?;
    let domain = state
        .store
        .get_mail_domain(authorization.organization_id, mail_domain_id, Utc::now())
        .await
        .map_err(|_| mail_domain_query_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    Ok(Json(mail_domain_response(domain)))
}

pub(crate) async fn create_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainCreateRequest>,
) -> Result<(StatusCode, Json<MailDomainCreateResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to register mail domains",
        &request_id,
    )?;
    let route_mode = match body.route_mode.as_str() {
        "direct_mx" => MailDomainRouteMode::DirectMx,
        "smart_host" => MailDomainRouteMode::SmartHost,
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "invalid_route_mode",
                "route_mode must be direct_mx or smart_host",
                false,
                &request_id,
            ));
        }
    };
    let ownership_token = format!(
        "cmv1_{}",
        URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
    );
    let ownership_challenge_sha256: [u8; 32] = Sha256::digest(ownership_token.as_bytes()).into();
    let result = MailDomainGovernanceService::new(state.store.clone())
        .register(
            &authorization,
            RegisterMailDomainCommand {
                organization_id,
                name: body.name,
                route_mode,
                ownership_challenge_sha256,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(MailDomainCreateResponse {
            mail_domain_id: result.mail_domain_id.to_string(),
            organization_id: organization_id.to_string(),
            ownership_record_name: format!("_copilotmail-verification.{}", result.name),
            ownership_token,
            name: result.name,
            status: mail_domain_status_wire(result.status).into(),
            ownership_challenge_version: result.ownership_challenge_version,
            route_version: result.route_version,
        }),
    ))
}

pub(crate) async fn rotate_mail_domain_ownership_challenge(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainChallengeRotationRequest>,
) -> Result<Json<MailDomainChallengeRotationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to rotate domain ownership challenges",
        &request_id,
    )?;
    let ownership_token = format!(
        "cmv1_{}",
        URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
    );
    let ownership_challenge_sha256: [u8; 32] = Sha256::digest(ownership_token.as_bytes()).into();
    let result = MailDomainGovernanceService::new(state.store.clone())
        .rotate_ownership_challenge(
            &authorization,
            RotateMailDomainChallengeCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                expected_version: body.expected_version,
                ownership_challenge_sha256,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainChallengeRotationResponse {
        mail_domain_id: result.mail_domain_id.to_string(),
        status: mail_domain_status_wire(result.status).into(),
        ownership_challenge_version: result.ownership_challenge_version,
        ownership_record_name: format!("_copilotmail-verification.{}", result.name),
        ownership_token,
    }))
}

pub(crate) async fn verify_mail_domain_dns(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainDnsVerificationRequest>,
) -> Result<Json<MailDomainDnsVerificationResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to verify mail domains",
        &request_id,
    )?;
    let domain = MailDomainName::parse(body.domain).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_domain",
            "domain is not a valid canonicalizable DNS name",
            false,
            &request_id,
        )
    })?;
    let config = DnsVerifierConfig::new(body.dkim_selector, Duration::from_secs(10))
        .map_err(|error| dns_api_error(error, &request_id))?;
    let lookup =
        HickoryDnsLookup::from_system().map_err(|error| dns_api_error(error, &request_id))?;
    let now = Utc::now();
    let report = DnsVerifier::new(lookup, config)
        .verify(&domain, &body.ownership_token, now)
        .await
        .map_err(|error| dns_api_error(error, &request_id))?;
    let activation_ready = report.evidence.activation_ready(now);
    let valid_until = report.evidence.valid_until;
    let failure_codes = report.failure_codes.clone();
    let status = MailDomainGovernanceService::new(state.store.clone())
        .record_dns(
            &authorization,
            RecordMailDomainDnsCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                observed_domain: domain,
                evidence: report.evidence,
                raw_answers: report.raw_answers,
                failure_codes: report.failure_codes,
                trace_id: request_id.clone(),
                now,
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainDnsVerificationResponse {
        mail_domain_id: mail_domain_id.to_string(),
        status: mail_domain_status_wire(status).into(),
        activation_ready,
        valid_until: valid_until.to_rfc3339(),
        failure_codes,
    }))
}

pub(crate) async fn activate_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailDomainActionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to activate mail domains",
        &request_id,
    )?;
    let data_plane_id = state.mail_data_plane_id.ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_data_plane_unavailable",
            "mail data plane is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    MailDomainGovernanceService::new(state.store.clone())
        .activate(
            &authorization,
            ActivateMailDomainCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                data_plane_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainActionResponse {
        mail_domain_id: mail_domain_id.to_string(),
        status: "active".into(),
    }))
}

pub(crate) async fn suspend_mail_domain(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainSuspendRequest>,
) -> Result<Json<MailDomainActionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to suspend mail domains",
        &request_id,
    )?;
    MailDomainGovernanceService::new(state.store.clone())
        .suspend(
            &authorization,
            SuspendMailDomainCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                reason_code: body.reason_code,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainActionResponse {
        mail_domain_id: mail_domain_id.to_string(),
        status: "suspended".into(),
    }))
}

pub(crate) async fn update_mail_domain_route(
    State(state): State<Arc<AppState>>,
    Path(mail_domain_id): Path<MailDomainId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailDomainRouteUpdateRequest>,
) -> Result<Json<MailDomainRouteResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to update mail routes",
        &request_id,
    )?;
    let result = MailDomainGovernanceService::new(state.store.clone())
        .update_route(
            &authorization,
            UpdateMailDomainRouteCommand {
                organization_id: authorization.organization_id,
                mail_domain_id,
                expected_version: body.expected_version,
                inbound_enabled: body.inbound_enabled,
                outbound_enabled: body.outbound_enabled,
                smart_host_ref: body.smart_host_ref,
                dkim_key_ref: body.dkim_key_ref,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_domain_api_error(error, &request_id))?;
    Ok(Json(MailDomainRouteResponse {
        mail_domain_id: result.mail_domain_id.to_string(),
        version: result.version,
        inbound_enabled: result.inbound_enabled,
        outbound_enabled: result.outbound_enabled,
    }))
}
