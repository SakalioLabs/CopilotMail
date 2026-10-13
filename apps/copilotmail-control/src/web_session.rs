use crate::*;

pub(crate) async fn human_session_csrf_guard(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    let Some(runtime) = state.human_session.as_ref() else {
        return next.run(request).await;
    };
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing-request-id")
        .to_owned();
    if human_oidc_redirect_route(request.method(), request.uri().path()) {
        let authorization_present = request
            .headers()
            .get_all(header::AUTHORIZATION)
            .iter()
            .next()
            .is_some();
        if authorization_present
            && any_human_session_cookie_present(request.headers(), runtime.secure_cookie)
        {
            return ambiguous_authentication(&request_id);
        }
        if authorization_present && bearer_token(request.headers()).is_err() {
            return invalid_access_token(&request_id);
        }
        return next.run(request).await;
    }
    let (cookie_client, session_token) =
        match select_presented_credential(request.headers(), Some(runtime.secure_cookie)) {
            Ok(PresentedCredential::BrowserSession { client, token }) => (client, token),
            Ok(PresentedCredential::Bearer(_) | PresentedCredential::Missing) => {
                return next.run(request).await;
            }
            Err(CredentialSelectionError::Ambiguous) => {
                return ambiguous_authentication(&request_id);
            }
            Err(CredentialSelectionError::InvalidBrowserSession) => {
                return invalid_human_session_cookie(&request_id);
            }
            Err(CredentialSelectionError::InvalidBearer) => {
                return invalid_access_token(&request_id);
            }
        };
    if !human_client_route_permitted(cookie_client, request.method(), request.uri().path()) {
        return human_client_route_denied(&request_id);
    }
    let safe_method = matches!(
        request.method(),
        &Method::GET | &Method::HEAD | &Method::OPTIONS
    );
    let record = match HumanSessionService::new(state.store.clone())
        .resolve_session(ResolveHumanSessionCommand {
            session_token,
            now: Utc::now(),
        })
        .await
    {
        Ok(record) => record,
        Err(error) => return human_session_api_error(error, &request_id),
    };
    if record.client_key != cookie_client {
        return invalid_human_session_cookie(&request_id);
    }
    // Safe methods do not compare Origin. The session allowlist is enough
    // to accept the cookie client; the OIDC relying party exists so an
    // unsafe method can require that client's exact public origin.
    let client = runtime.client(record.client_key.as_str());
    if safe_method {
        if client.is_none() && !runtime.session_client_is_configured(record.client_key) {
            return invalid_human_session_cookie(&request_id);
        }
        return next.run(request).await;
    }
    let Some(client) = client else {
        return invalid_human_session_cookie(&request_id);
    };
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    let origin = origins.next().and_then(|value| value.to_str().ok());
    if origin != Some(client.public_origin.as_str()) || origins.next().is_some() {
        return api_error(
            StatusCode::FORBIDDEN,
            "csrf_origin_rejected",
            "the request Origin does not match the Human browser session",
            false,
            &request_id,
        );
    }
    next.run(request).await
}

pub(crate) fn human_oidc_redirect_route(method: &Method, path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    matches!(
        segments.as_slice(),
        ["api", "v2", "auth", client, action]
            if matches!(*client, "admin" | "mail")
                && matches!(*action, "login" | "callback")
                && (method == Method::GET || method == Method::HEAD)
    )
}

pub(crate) fn human_client_route_permitted(
    client: HumanSessionClient,
    method: &Method,
    path: &str,
) -> bool {
    if human_oidc_redirect_route(method, path) {
        return true;
    }
    if let Some(required_client) = human_session_api_route_client(method, path) {
        return client == required_client;
    }
    let mail_route = human_mail_route(method, path);
    match client {
        HumanSessionClient::Admin => !mail_route,
        HumanSessionClient::Mail => mail_route,
    }
}

pub(crate) fn human_session_api_route_client(
    method: &Method,
    path: &str,
) -> Option<HumanSessionClient> {
    let segments = exact_path_segments(path)?;
    match segments.as_slice() {
        ["api", "v2", "auth", client, "session" | "sessions"]
            if method == Method::GET || method == Method::HEAD =>
        {
            human_session_client_from_segment(client)
        }
        ["api", "v2", "auth", client, "logout" | "logout-all"] if method == Method::POST => {
            human_session_client_from_segment(client)
        }
        _ => None,
    }
}

pub(crate) fn human_session_client_from_segment(value: &str) -> Option<HumanSessionClient> {
    match value {
        "admin" => Some(HumanSessionClient::Admin),
        "mail" => Some(HumanSessionClient::Mail),
        _ => None,
    }
}

pub(crate) fn human_mail_route(method: &Method, path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    match segments.as_slice() {
        ["api", "v2", "mailboxes"] => method == Method::GET || method == Method::HEAD,
        ["api", "v2", "draft-attempts"] => method == Method::GET || method == Method::HEAD,
        ["api", "v2", "mailboxes", _, "messages"] => {
            method == Method::GET || method == Method::HEAD
        }
        ["api", "v2", "mailboxes", _, "messages", "search"] => method == Method::POST,
        ["api", "v2", "mailboxes", _, "messages", _] => {
            method == Method::GET || method == Method::HEAD || method == Method::PATCH
        }
        ["api", "v2", "mailboxes", _, "drafts"] => {
            method == Method::GET || method == Method::HEAD || method == Method::POST
        }
        ["api", "v2", "mailboxes", _, "drafts", _, "restore"] => method == Method::POST,
        ["api", "v2", "mailboxes", _, "submissions"] => {
            method == Method::GET || method == Method::HEAD || method == Method::POST
        }
        ["api", "v2", "mail-submissions", _] => method == Method::GET || method == Method::HEAD,
        _ => false,
    }
}

#[allow(clippy::result_large_err)]
pub(crate) fn require_human_mail_browser_session(
    headers: &HeaderMap,
    secure_session_cookies: bool,
    request_id: &str,
) -> Result<(), Response> {
    match select_presented_credential(headers, Some(secure_session_cookies)) {
        Ok(PresentedCredential::BrowserSession {
            client: HumanSessionClient::Mail,
            ..
        }) => Ok(()),
        Ok(PresentedCredential::BrowserSession {
            client: HumanSessionClient::Admin,
            ..
        }) => Err(human_client_route_denied(request_id)),
        Ok(PresentedCredential::Bearer(_) | PresentedCredential::Missing) => {
            Err(invalid_human_session_cookie(request_id))
        }
        Err(CredentialSelectionError::Ambiguous) => Err(ambiguous_authentication(request_id)),
        Err(
            CredentialSelectionError::InvalidBrowserSession
            | CredentialSelectionError::InvalidBearer,
        ) => Err(invalid_human_session_cookie(request_id)),
    }
}

pub(crate) async fn authenticate_human_mail_session_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<AuthorizationContext, Response> {
    let secure_session_cookies = state
        .human_session
        .as_ref()
        .map(|runtime| runtime.secure_cookie)
        .ok_or_else(|| invalid_human_session_cookie(request_id))?;
    require_human_mail_browser_session(headers, secure_session_cookies, request_id)?;
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(invalid_human_session_cookie(request_id));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        request_id,
    )?;
    Ok(authorization)
}

pub(crate) fn exact_path_segments(path: &str) -> Option<Vec<&str>> {
    let path = path.strip_prefix('/')?;
    let segments = path.split('/').collect::<Vec<_>>();
    (!segments.is_empty() && segments.iter().all(|segment| !segment.is_empty())).then_some(segments)
}

pub(crate) async fn human_auth_response_headers(request: Request, next: Next) -> Response {
    let sensitive_human_route = request.uri().path().starts_with("/api/v2/auth/")
        || human_mail_route(request.method(), request.uri().path());
    let mut response = next.run(request).await;
    if sensitive_human_route {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        response.headers_mut().insert(
            header::HeaderName::from_static("referrer-policy"),
            header::HeaderValue::from_static("no-referrer"),
        );
    }
    response
}
