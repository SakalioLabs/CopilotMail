use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HumanLoginQuery {
    pub(crate) return_to: Option<String>,
}

pub(crate) async fn begin_human_login(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Query(query): Query<HumanLoginQuery>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state.human_session.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser sessions are not configured",
            true,
            &request_id,
        )
    })?;
    let client = runtime.client(&client_key).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "human_session_client_not_found",
            "the Human browser client is not configured",
            false,
            &request_id,
        )
    })?;
    let authorization = client
        .relying_party
        .begin_authorization()
        .map_err(|error| oidc_flow_api_error(error, &request_id))?;
    let (authorization_url, transaction) = authorization.into_parts();
    let (state_value, verifier, issued_at, expires_at) = transaction.into_parts();
    let state_sha256: [u8; 32] = Sha256::digest(state_value.as_bytes()).into();
    let encrypted_pkce_verifier = runtime
        .protector
        .protect(verifier.expose_secret(), &state_sha256)
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "the Human login transaction could not be protected",
                true,
                &request_id,
            )
        })?;
    let browser_nonce = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let now = Utc::now();
    let issued_at = datetime_from_unix_seconds(issued_at).ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login transaction has an invalid timestamp",
            true,
            &request_id,
        )
    })?;
    let expires_at = datetime_from_unix_seconds(expires_at).ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login transaction has an invalid expiry",
            true,
            &request_id,
        )
    })?;
    if issued_at > now + chrono::Duration::seconds(60) {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login transaction clock is invalid",
            true,
            &request_id,
        ));
    }
    HumanSessionService::new(state.store.clone())
        .begin_login(BeginHumanLoginCommand {
            client_key: client.client_key,
            return_to: query.return_to.unwrap_or_else(|| "/".into()),
            state: state_value,
            browser_nonce: browser_nonce.clone(),
            encrypted_pkce_verifier,
            now,
            expires_at,
            admission_limit: runtime.login_requests_per_minute,
            cleanup_limit: HUMAN_LOGIN_CLEANUP_LIMIT,
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;

    let lifetime = (expires_at - now).num_seconds().clamp(1, 600);
    let cookie = browser_cookie_header(
        runtime.login_cookie_name(client.client_key),
        &browser_nonce,
        lifetime,
        runtime.secure_cookie,
    )
    .map_err(|_| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login cookie could not be created",
            true,
            &request_id,
        )
    })?;
    let mut response = Redirect::temporary(authorization_url.as_str()).into_response();
    response.headers_mut().append(header::SET_COOKIE, cookie);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub(crate) async fn complete_human_login(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Query(response): Query<OidcAuthorizationResponse>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state.human_session.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser sessions are not configured",
            true,
            &request_id,
        )
    })?;
    let client = runtime.client(&client_key).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "human_session_client_not_found",
            "the Human browser client is not configured",
            false,
            &request_id,
        )
    })?;
    let state_value = response.state().ok_or_else(|| {
        api_error(
            StatusCode::UNAUTHORIZED,
            "invalid_human_login_callback",
            "the Human login callback is invalid or expired",
            false,
            &request_id,
        )
    })?;
    let browser_nonce =
        browser_cookie_value(&headers, runtime.login_cookie_name(client.client_key))
            .map_err(|_| invalid_human_session_cookie(&request_id))?
            .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let now = Utc::now();
    let consumed = HumanSessionService::new(state.store.clone())
        .consume_login(ConsumeHumanLoginCommand {
            state: state_value.into(),
            browser_nonce,
            now,
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if consumed.client_key != client.client_key {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "invalid_human_login_callback",
            "the Human login callback is invalid or expired",
            false,
            &request_id,
        ));
    }
    let state_sha256: [u8; 32] = Sha256::digest(state_value.as_bytes()).into();
    let verifier = runtime
        .protector
        .unprotect(&consumed.encrypted_pkce_verifier, &state_sha256)
        .map_err(|_| invalid_human_session_cookie(&request_id))?;
    let issued_at = unix_seconds_from_datetime(consumed.created_at)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let expires_at = unix_seconds_from_datetime(consumed.expires_at)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let transaction =
        OidcAuthorizationTransaction::from_persisted(state_value, verifier, issued_at, expires_at)
            .map_err(|error| oidc_flow_api_error(error, &request_id))?;
    let token = client
        .relying_party
        .complete_authorization(transaction, response)
        .await
        .map_err(|error| oidc_flow_api_error(error, &request_id))?;
    let upstream_expires_at =
        datetime_from_unix_seconds(token.credential_expires_at_unix_seconds())
            .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let identity = token.identity().clone();
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(invalid_human_session_cookie(&request_id));
    }
    let authorization = state
        .store
        .resolve_authorization(identity.principal_id, now)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "authorization could not be evaluated",
                true,
                &request_id,
            )
        })?
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let configured_expiry = now
        + chrono::Duration::from_std(runtime.maximum_lifetime).map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "the Human session lifetime is invalid",
                true,
                &request_id,
            )
        })?;
    let expires_at = upstream_expires_at.min(configured_expiry);
    let session_token = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let record = HumanSessionService::new(state.store.clone())
        .create_session(CreateHumanSessionCommand {
            organization_id: authorization.organization_id,
            principal_id: identity.principal_id,
            login_transaction_id: consumed.login_transaction_id,
            client_key: consumed.client_key,
            session_token: session_token.clone(),
            oidc_access_token: token.access_token().expose_secret().to_owned(),
            oidc_jti: identity.credential_id,
            upstream_expires_at,
            expires_at,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    let lifetime = (record.expires_at - now).num_seconds().clamp(1, 28_800);
    let [session_cookie, clear_other_session] = browser_session_cookie_headers(
        client.client_key,
        &session_token,
        lifetime,
        runtime.secure_cookie,
    )
    .map_err(|_| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human session cookie could not be created",
            true,
            &request_id,
        )
    })?;
    let clear_login = clear_browser_cookie_header(
        runtime.login_cookie_name(client.client_key),
        runtime.secure_cookie,
    )
    .map_err(|_| invalid_human_session_cookie(&request_id))?;
    let target = format!("{}{}", client.public_origin, consumed.return_to);
    let mut response = Redirect::to(&target).into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, session_cookie);
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_login);
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_other_session);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub(crate) async fn get_human_session(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Json<HumanSessionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state
        .human_session
        .as_ref()
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let expected_client = runtime
        .client(&client_key)
        .map(|client| client.client_key)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let (cookie_client, token) = select_human_browser_session(&headers, runtime.secure_cookie)
        .map_err(|error| credential_selection_api_error(error, &request_id))?;
    if cookie_client != expected_client {
        return Err(human_client_route_denied(&request_id));
    }
    let record = HumanSessionService::new(state.store.clone())
        .resolve_session(ResolveHumanSessionCommand {
            session_token: token,
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if record.client_key != cookie_client {
        return Err(invalid_human_session_cookie(&request_id));
    }
    state
        .store
        .resolve_authorization(record.principal_id, Utc::now())
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "authorization could not be evaluated",
                true,
                &request_id,
            )
        })?
        .filter(|context| context.organization_id == record.organization_id)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    Ok(Json(HumanSessionResponse {
        principal_id: record.principal_id.to_string(),
        organization_id: record.organization_id.to_string(),
        client: record.client_key.as_str().into(),
        expires_at: record.expires_at.to_rfc3339(),
    }))
}

pub(crate) async fn logout_human_session(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let runtime = state
        .human_session
        .as_ref()
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let expected_client = runtime
        .client(&client_key)
        .map(|client| client.client_key)
        .ok_or_else(|| invalid_human_session_cookie(&request_id))?;
    let (cookie_client, token) = select_human_browser_session(&headers, runtime.secure_cookie)
        .map_err(|error| credential_selection_api_error(error, &request_id))?;
    if cookie_client != expected_client {
        return Err(human_client_route_denied(&request_id));
    }
    let existing = HumanSessionService::new(state.store.clone())
        .resolve_session(ResolveHumanSessionCommand {
            session_token: token.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if existing.client_key != cookie_client {
        return Err(invalid_human_session_cookie(&request_id));
    }
    let revoked = HumanSessionService::new(state.store.clone())
        .revoke_session(RevokeHumanSessionCommand {
            session_token: token,
            cause: HumanSessionRevocationCause::Logout,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if revoked.session.client_key != cookie_client {
        return Err(invalid_human_session_cookie(&request_id));
    }
    let clear_cookie = clear_browser_cookie_header(
        runtime.session_cookie_name(cookie_client),
        runtime.secure_cookie,
    )
    .map_err(|_| invalid_human_session_cookie(&request_id))?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .append(header::SET_COOKIE, clear_cookie);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
