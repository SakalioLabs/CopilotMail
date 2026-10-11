use crate::*;

#[derive(Debug, Deserialize)]
pub(crate) struct AgentTokenForm {
    pub(crate) grant_type: String,
    pub(crate) client_id: String,
    pub(crate) client_assertion_type: Option<String>,
    pub(crate) client_assertion: Option<String>,
    pub(crate) scope: Option<String>,
}

pub(crate) struct ConnectionProof(pub(crate) Option<TlsConnectionInfo>);

impl<S> FromRequestParts<S> for ConnectionProof
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<axum::extract::ConnectInfo<TlsConnectionInfo>>()
                .map(|axum::extract::ConnectInfo(info)| info.clone()),
        ))
    }
}

pub(crate) async fn agent_jwks(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(service) = &state.agent_token_service else {
        return oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "Agent OAuth is not configured",
        );
    };
    let jwks = service.jwks();
    let bytes = serde_json::to_vec(&jwks).expect("JWKS serializes");
    let etag = format!("\"{}\"", hex_digest(&bytes));
    let mut response = if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        == Some(etag.as_str())
    {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        Json(jwks).into_response()
    };
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "public, max-age=60, must-revalidate".parse().unwrap(),
    );
    response
        .headers_mut()
        .insert(header::ETAG, etag.parse().unwrap());
    response
}

pub(crate) async fn issue_agent_token(
    State(state): State<Arc<AppState>>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    form: Result<Form<AgentTokenForm>, FormRejection>,
) -> Response {
    let Ok(Form(form)) = form else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the token request form is invalid",
        );
    };
    let Some(service) = &state.agent_token_service else {
        return oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "Agent OAuth is not configured",
        );
    };
    let certificate_fingerprint = connection
        .as_ref()
        .and_then(|info| info.client_certificate_fingerprint.as_deref());
    let source = connection
        .as_ref()
        .map(|info| info.remote_addr.ip().to_string())
        .or_else(|| forwarded_source(&headers));
    match service
        .exchange(
            TokenRequest {
                grant_type: form.grant_type,
                client_id: form.client_id,
                client_assertion_type: form.client_assertion_type,
                client_assertion: form.client_assertion,
                scope: form.scope,
            },
            certificate_fingerprint,
            source.as_deref(),
            Utc::now(),
        )
        .await
    {
        Ok(token) => oauth_json(
            StatusCode::OK,
            serde_json::to_value(token).expect("token response serializes"),
        ),
        Err(TokenError::InvalidRequest) => oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "the token request is invalid",
        ),
        Err(TokenError::InvalidClient) => oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        ),
        Err(TokenError::InvalidScope) => oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "the requested scope is not permitted",
        ),
        Err(TokenError::TemporarilyUnavailable) => oauth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "the authorization server is temporarily unavailable",
        ),
    }
}

pub(crate) fn hex_digest(value: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub(crate) fn encode_bytes_hex(value: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub(crate) fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    let mut response = oauth_json(
        status,
        serde_json::json!({"error": error, "error_description": description}),
    );
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            "Bearer error=\"invalid_client\"".parse().unwrap(),
        );
    }
    response
}

pub(crate) fn oauth_json(status: StatusCode, value: serde_json::Value) -> Response {
    let mut response = (status, Json(value)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::PRAGMA, "no-cache".parse().unwrap());
    response
}

pub(crate) fn forwarded_source(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}
