use crate::*;

/// Reason-tagged login rejection; `None` marks a post-commit failure where
/// the login already succeeded and no failure-ledger row may be written.
type LoginFailure = (Option<HumanLoginFailureReason>, Response);

fn login_failure(reason: HumanLoginFailureReason, response: Response) -> LoginFailure {
    (Some(reason), response)
}

fn post_login_failure(response: Response) -> LoginFailure {
    (None, response)
}

/// Appends one anonymous login-failure ledger row on a best-effort basis.
///
/// The original rejection is always authoritative: a ledger outage is logged
/// but must never change the response the browser already earned.
async fn record_human_login_failure(
    state: &AppState,
    runtime: &HumanSessionRuntime,
    client_key: HumanSessionClient,
    reason: HumanLoginFailureReason,
    connection: Option<&TlsConnectionInfo>,
    headers: &HeaderMap,
) {
    // The peer IP comes from the observed socket; the X-Forwarded-For
    // fallback matches the token endpoint convention. Raw values are hashed
    // with the deployment pepper in memory and never persisted.
    let peer = connection
        .map(|info| info.remote_addr.ip().to_string())
        .or_else(|| forwarded_source(headers));
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok());
    let source_sha256 = runtime
        .failure_source_hasher
        .hash(peer.as_deref(), user_agent);
    if let Err(error) = HumanLoginFailureService::new(state.store.clone())
        .record(RecordHumanLoginFailureCommand {
            client_key,
            reason,
            source_sha256,
        })
        .await
    {
        tracing::warn!(%error, "human login failure ledger append failed");
    }
}

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
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let Some(runtime) = state.human_session.as_ref() else {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser sessions are not configured",
            true,
            &request_id,
        ));
    };
    let Some(client) = human_session_client_from_segment(&client_key) else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "human_session_client_not_found",
            "the Human browser client is not configured",
            false,
            &request_id,
        ));
    };
    match begin_human_login_inner(&state, runtime, client, query, &request_id).await {
        Ok(response) => Ok(response),
        Err((reason, response)) => {
            if let Some(reason) = reason {
                record_human_login_failure(
                    &state,
                    runtime,
                    client,
                    reason,
                    connection.as_ref(),
                    &headers,
                )
                .await;
            }
            Err(response)
        }
    }
}

async fn begin_human_login_inner(
    state: &AppState,
    runtime: &HumanSessionRuntime,
    client_key: HumanSessionClient,
    query: HumanLoginQuery,
    request_id: &str,
) -> Result<Response, LoginFailure> {
    let Some(client) = runtime.client(client_key.as_str()) else {
        return Err(login_failure(
            HumanLoginFailureReason::LoginBeginRejected,
            api_error(
                StatusCode::NOT_FOUND,
                "human_session_client_not_found",
                "the Human browser client is not configured",
                false,
                request_id,
            ),
        ));
    };
    let authorization = client
        .relying_party
        .begin_authorization()
        .map_err(|error| {
            login_failure(
                HumanLoginFailureReason::LoginBeginRejected,
                oidc_flow_api_error(error, request_id),
            )
        })?;
    let (authorization_url, transaction) = authorization.into_parts();
    let (state_value, verifier, issued_at, expires_at) = transaction.into_parts();
    let state_sha256: [u8; 32] = Sha256::digest(state_value.as_bytes()).into();
    let encrypted_pkce_verifier = runtime
        .protector
        .protect(verifier.expose_secret(), &state_sha256)
        .map_err(|_| {
            login_failure(
                HumanLoginFailureReason::LoginBeginRejected,
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "human_session_unavailable",
                    "the Human login transaction could not be protected",
                    true,
                    request_id,
                ),
            )
        })?;
    let browser_nonce = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let now = Utc::now();
    let issued_at = datetime_from_unix_seconds(issued_at).ok_or_else(|| {
        login_failure(
            HumanLoginFailureReason::LoginBeginRejected,
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "the Human login transaction has an invalid timestamp",
                true,
                request_id,
            ),
        )
    })?;
    let expires_at = datetime_from_unix_seconds(expires_at).ok_or_else(|| {
        login_failure(
            HumanLoginFailureReason::LoginBeginRejected,
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "the Human login transaction has an invalid expiry",
                true,
                request_id,
            ),
        )
    })?;
    if issued_at > now + chrono::Duration::seconds(60) {
        return Err(login_failure(
            HumanLoginFailureReason::LoginBeginRejected,
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "the Human login transaction clock is invalid",
                true,
                request_id,
            ),
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
        .map_err(|error| {
            let reason = match error {
                HumanSessionError::RateLimited { .. } => {
                    HumanLoginFailureReason::LoginAdmissionLimited
                }
                _ => HumanLoginFailureReason::LoginBeginRejected,
            };
            login_failure(reason, human_session_api_error(error, request_id))
        })?;

    let lifetime = (expires_at - now).num_seconds().clamp(1, 600);
    let cookie = browser_cookie_header(
        runtime.login_cookie_name(client.client_key),
        &browser_nonce,
        lifetime,
        runtime.secure_cookie,
    )
    .map_err(|_| {
        // The login transaction was already admitted; a cookie-build fault is
        // a post-commit failure, not a rejected login attempt.
        post_login_failure(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human login cookie could not be created",
            true,
            request_id,
        ))
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
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let Some(runtime) = state.human_session.as_ref() else {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser sessions are not configured",
            true,
            &request_id,
        ));
    };
    let Some(client_key) = human_session_client_from_segment(&client_key) else {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "human_session_client_not_found",
            "the Human browser client is not configured",
            false,
            &request_id,
        ));
    };
    match complete_human_login_inner(&state, runtime, client_key, response, &headers, &request_id)
        .await
    {
        Ok(response) => Ok(response),
        Err((reason, response)) => {
            if let Some(reason) = reason {
                record_human_login_failure(
                    &state,
                    runtime,
                    client_key,
                    reason,
                    connection.as_ref(),
                    &headers,
                )
                .await;
            }
            Err(response)
        }
    }
}

async fn complete_human_login_inner(
    state: &AppState,
    runtime: &HumanSessionRuntime,
    client_key: HumanSessionClient,
    response: OidcAuthorizationResponse,
    headers: &HeaderMap,
    request_id: &str,
) -> Result<Response, LoginFailure> {
    let Some(client) = runtime.client(client_key.as_str()) else {
        return Err(login_failure(
            HumanLoginFailureReason::LoginTransactionRejected,
            api_error(
                StatusCode::NOT_FOUND,
                "human_session_client_not_found",
                "the Human browser client is not configured",
                false,
                request_id,
            ),
        ));
    };
    let Some(state_value) = response.state() else {
        return Err(login_failure(
            HumanLoginFailureReason::CallbackStateMissing,
            api_error(
                StatusCode::UNAUTHORIZED,
                "invalid_human_login_callback",
                "the Human login callback is invalid or expired",
                false,
                request_id,
            ),
        ));
    };
    let browser_nonce = browser_cookie_value(headers, runtime.login_cookie_name(client.client_key))
        .ok()
        .flatten()
        .ok_or_else(|| {
            login_failure(
                HumanLoginFailureReason::LoginCookieRejected,
                invalid_human_session_cookie(request_id),
            )
        })?;
    let now = Utc::now();
    let consumed = HumanSessionService::new(state.store.clone())
        .consume_login(ConsumeHumanLoginCommand {
            state: state_value.into(),
            browser_nonce,
            now,
        })
        .await
        .map_err(|error| {
            login_failure(
                HumanLoginFailureReason::LoginTransactionRejected,
                human_session_api_error(error, request_id),
            )
        })?;
    if consumed.client_key != client.client_key {
        return Err(login_failure(
            HumanLoginFailureReason::CallbackClientMismatch,
            api_error(
                StatusCode::UNAUTHORIZED,
                "invalid_human_login_callback",
                "the Human login callback is invalid or expired",
                false,
                request_id,
            ),
        ));
    }
    let state_sha256: [u8; 32] = Sha256::digest(state_value.as_bytes()).into();
    let verifier = runtime
        .protector
        .unprotect(&consumed.encrypted_pkce_verifier, &state_sha256)
        .map_err(|_| {
            login_failure(
                HumanLoginFailureReason::LoginMaterialRejected,
                invalid_human_session_cookie(request_id),
            )
        })?;
    let issued_at = unix_seconds_from_datetime(consumed.created_at).ok_or_else(|| {
        login_failure(
            HumanLoginFailureReason::LoginMaterialRejected,
            invalid_human_session_cookie(request_id),
        )
    })?;
    let expires_at = unix_seconds_from_datetime(consumed.expires_at).ok_or_else(|| {
        login_failure(
            HumanLoginFailureReason::LoginMaterialRejected,
            invalid_human_session_cookie(request_id),
        )
    })?;
    let transaction =
        OidcAuthorizationTransaction::from_persisted(state_value, verifier, issued_at, expires_at)
            .map_err(|error| {
                login_failure(
                    HumanLoginFailureReason::LoginMaterialRejected,
                    oidc_flow_api_error(error, request_id),
                )
            })?;
    let token = client
        .relying_party
        .complete_authorization(transaction, response)
        .await
        .map_err(|error| {
            login_failure(
                HumanLoginFailureReason::OidcCallbackRejected,
                oidc_flow_api_error(error, request_id),
            )
        })?;
    let upstream_expires_at = datetime_from_unix_seconds(
        token.credential_expires_at_unix_seconds(),
    )
    .ok_or_else(|| {
        login_failure(
            HumanLoginFailureReason::OidcCallbackRejected,
            invalid_human_session_cookie(request_id),
        )
    })?;
    let identity = token.identity().clone();
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(login_failure(
            HumanLoginFailureReason::PrincipalRejected,
            invalid_human_session_cookie(request_id),
        ));
    }
    let authorization = state
        .store
        .resolve_authorization(identity.principal_id, now)
        .await
        .map_err(|_| {
            login_failure(
                HumanLoginFailureReason::AuthorizationUnavailable,
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "authorization_unavailable",
                    "authorization could not be evaluated",
                    true,
                    request_id,
                ),
            )
        })?
        .ok_or_else(|| {
            login_failure(
                HumanLoginFailureReason::PrincipalRejected,
                invalid_human_session_cookie(request_id),
            )
        })?;
    let configured_expiry = now
        + chrono::Duration::from_std(runtime.maximum_lifetime).map_err(|_| {
            login_failure(
                HumanLoginFailureReason::SessionEstablishFailed,
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "human_session_unavailable",
                    "the Human session lifetime is invalid",
                    true,
                    request_id,
                ),
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
            trace_id: request_id.to_owned(),
            now,
        })
        .await
        .map_err(|error| {
            login_failure(
                HumanLoginFailureReason::SessionEstablishFailed,
                human_session_api_error(error, request_id),
            )
        })?;
    let lifetime = (record.expires_at - now).num_seconds().clamp(1, 28_800);
    let [session_cookie, clear_other_session] = browser_session_cookie_headers(
        client.client_key,
        &session_token,
        lifetime,
        runtime.secure_cookie,
    )
    .map_err(|_| {
        // The session row is already committed; a cookie-build fault is a
        // post-commit failure, not a rejected login attempt.
        post_login_failure(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "the Human session cookie could not be created",
            true,
            request_id,
        ))
    })?;
    let clear_login = clear_browser_cookie_header(
        runtime.login_cookie_name(client.client_key),
        runtime.secure_cookie,
    )
    .map_err(|_| post_login_failure(invalid_human_session_cookie(request_id)))?;
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

pub(crate) async fn list_own_human_sessions(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Json<HumanBrowserSessionListResponse>, Response> {
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
    let sessions = HumanSessionService::new(state.store.clone())
        .list_own_sessions(ListOwnHumanSessionsCommand {
            session_token: token,
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    Ok(Json(HumanBrowserSessionListResponse {
        sessions: sessions
            .into_iter()
            .map(|session| HumanBrowserSessionItemResponse {
                session_id: session.session_id.to_string(),
                client: session.client_key.as_str().into(),
                created_at: session.created_at.to_rfc3339(),
                expires_at: session.expires_at.to_rfc3339(),
                current: session.current,
            })
            .collect(),
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

const SESSION_BODY_LIMIT: usize = 8 * 1024;

pub(crate) async fn logout_all_human_sessions(
    State(state): State<Arc<AppState>>,
    Path(client_key): Path<String>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    request: Request,
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
            session_token: token,
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_api_error(error, &request_id))?;
    if existing.client_key != cookie_client {
        return Err(invalid_human_session_cookie(&request_id));
    }
    let retain_current_session =
        logout_all_retains_current_session(&headers, request, &request_id).await?;
    let revoked = HumanSessionGovernanceService::new(state.store.clone())
        .revoke_own_browser_sessions(RevokeOwnBrowserSessionsCommand {
            context: AuthorizationContext {
                principal_id: existing.principal_id,
                organization_id: existing.organization_id,
                permissions: BTreeSet::new(),
                authentication: Some(AuthenticationBinding::HumanSession {
                    session_id: existing.session_id,
                }),
            },
            retain_current_session,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_session_governance_api_error(error, &request_id))?;
    let mut response = StatusCode::NO_CONTENT.into_response();
    if revoked.clear_current_cookie {
        let clear_cookie = clear_browser_cookie_header(
            runtime.session_cookie_name(cookie_client),
            runtime.secure_cookie,
        )
        .map_err(|_| invalid_human_session_cookie(&request_id))?;
        response
            .headers_mut()
            .append(header::SET_COOKIE, clear_cookie);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

pub(crate) async fn revoke_principal_browser_sessions(
    State(state): State<Arc<AppState>>,
    Path((organization_id, principal_id)): Path<(OrganizationId, PrincipalId)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    request: Request,
) -> Response {
    let request_id = request_id_value(&request_id);
    let response = match revoke_principal_browser_sessions_authorized(
        &state,
        organization_id,
        principal_id,
        connection,
        &headers,
        request,
        &request_id,
    )
    .await
    {
        Ok(response) => response,
        Err(response) => response,
    };
    sensitive_admin_session_response(response)
}

async fn revoke_principal_browser_sessions_authorized(
    state: &AppState,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    connection: Option<TlsConnectionInfo>,
    headers: &HeaderMap,
    request: Request,
    request_id: &str,
) -> Result<Response, Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "revoking browser sessions requires an authenticated Human Principal",
            false,
            request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::SessionRevokeOthers,
        "session.revoke_others permission is required",
        request_id,
    )?;
    if !request_content_type_is_json(headers) {
        return Err(api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
            "Content-Type must be application/json",
            false,
            request_id,
        ));
    }
    let body: RevokePrincipalBrowserSessionsRequest = read_closed_body(request, request_id).await?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| invalid_session_governance_request(request_id))?;
    let now = Utc::now();
    HumanSessionGovernanceService::new(state.store.clone())
        .revoke_principal_browser_sessions(RevokePrincipalBrowserSessionsCommand {
            context: authorization.clone(),
            path_organization_id: organization_id,
            target_principal_id: principal_id,
            approval: ApprovalProof {
                organization_id: authorization.organization_id,
                request_id: approval_request_id,
                requested_by: authorization.principal_id,
                action: ApprovalAction::SessionRevokeOthers,
                resource_kind: "identity.principal".into(),
                resource_id: principal_id.to_string(),
                now,
            },
            trace_id: request_id.to_owned(),
            now,
        })
        .await
        .map_err(|error| human_session_governance_api_error(error, request_id))?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn logout_all_retains_current_session(
    headers: &HeaderMap,
    request: Request,
    request_id: &str,
) -> Result<bool, Response> {
    let bytes = to_bytes(request.into_body(), SESSION_BODY_LIMIT)
        .await
        .map_err(|_| session_payload_too_large(request_id))?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(false);
    }
    if !request_content_type_is_json(headers) {
        return Err(api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
            "Content-Type must be application/json",
            false,
            request_id,
        ));
    }
    let body: LogoutAllHumanSessionsRequest =
        parse_closed_json(&bytes).map_err(|_| invalid_session_governance_request(request_id))?;
    Ok(body.retain_current_session)
}

async fn read_closed_body<T>(request: Request, request_id: &str) -> Result<T, Response>
where
    T: serde::de::DeserializeOwned,
{
    let bytes = to_bytes(request.into_body(), SESSION_BODY_LIMIT)
        .await
        .map_err(|_| session_payload_too_large(request_id))?;
    parse_closed_json(&bytes).map_err(|_| invalid_session_governance_request(request_id))
}

fn request_content_type_is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value
                    .strip_prefix("application/")
                    .is_some_and(|subtype| subtype.to_ascii_lowercase().ends_with("+json"))
        })
}

fn invalid_session_governance_request(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_human_session_request",
        "the Human session request is invalid",
        false,
        request_id,
    )
}

fn session_payload_too_large(request_id: &str) -> Response {
    api_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "request_payload_too_large",
        "request payload exceeds the accepted limit",
        false,
        request_id,
    )
}

fn human_session_governance_api_error(
    error: HumanSessionGovernanceError,
    request_id: &str,
) -> Response {
    match error {
        HumanSessionGovernanceError::InvalidRequest => {
            invalid_session_governance_request(request_id)
        }
        HumanSessionGovernanceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "human_session_governance_forbidden",
            "the session revocation approval is not accepted",
            false,
            request_id,
        ),
        HumanSessionGovernanceError::PermissionDenied => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "session.revoke_others permission is required",
            false,
            request_id,
        ),
        HumanSessionGovernanceError::Conflict | HumanSessionGovernanceError::Unavailable => {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "human_session_unavailable",
                "Human browser session revocation could not be completed",
                true,
                request_id,
            )
        }
        HumanSessionGovernanceError::AuthenticationInvalid => {
            let mut response = authentication_invalid(request_id);
            response.headers_mut().remove(header::WWW_AUTHENTICATE);
            response
        }
    }
}

fn sensitive_admin_session_response(mut response: Response) -> Response {
    response.headers_mut().remove(header::WWW_AUTHENTICATE);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("referrer-policy"),
        header::HeaderValue::from_static("no-referrer"),
    );
    response
}
