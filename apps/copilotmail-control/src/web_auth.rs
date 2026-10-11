use crate::*;

pub(crate) async fn authenticate_api_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let secure_session_cookies = state
        .human_session
        .as_ref()
        .map(|runtime| runtime.secure_cookie);
    let credential = select_presented_credential(headers, secure_session_cookies).map_err(
        |error| match error {
            CredentialSelectionError::Ambiguous => ambiguous_authentication(request_id),
            CredentialSelectionError::InvalidBrowserSession => {
                invalid_human_session_cookie(request_id)
            }
            CredentialSelectionError::InvalidBearer => invalid_access_token(request_id),
        },
    )?;
    let (identity, session_organization_id) = match credential {
        PresentedCredential::BrowserSession {
            client: cookie_client,
            token: session_token,
        } => {
            let runtime = state
                .human_session
                .as_ref()
                .ok_or_else(|| invalid_human_session_cookie(request_id))?;
            let session = HumanSessionService::new(state.store.clone())
                .resolve_session(ResolveHumanSessionCommand {
                    session_token,
                    now: Utc::now(),
                })
                .await
                .map_err(|error| human_session_api_error(error, request_id))?;
            if session.client_key != cookie_client
                || !runtime.session_client_is_configured(session.client_key)
            {
                return Err(invalid_human_session_cookie(request_id));
            }
            (
                AuthenticatedIdentity {
                    principal_id: session.principal_id,
                    principal_kind: AuthenticatedPrincipalKind::Human,
                    credential_id: session.oidc_jti.clone(),
                    scope_restriction: None,
                    authentication: AuthenticationBinding::HumanSession {
                        session_id: session.session_id,
                    },
                },
                Some(session.organization_id),
            )
        }
        PresentedCredential::Bearer(token) => {
            let identity = state
                .authenticator
                .authenticate_with_context(
                    token,
                    &AuthenticationContext {
                        client_certificate_fingerprint: connection
                            .and_then(|info| info.client_certificate_fingerprint),
                    },
                )
                .await
                .map_err(|error| match error {
                    AuthenticationError::InvalidCredential => invalid_access_token(request_id),
                    AuthenticationError::Unavailable => api_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "authentication_unavailable",
                        "trusted identity verification is not available",
                        true,
                        request_id,
                    ),
                })?;
            if identity.principal_kind == AuthenticatedPrincipalKind::Human
                && state.human_session_required
            {
                return Err(api_error(
                    StatusCode::UNAUTHORIZED,
                    "human_session_required",
                    "Human API access requires a server-side browser session",
                    false,
                    request_id,
                ));
            }
            (identity, None)
        }
        PresentedCredential::Missing => {
            return Err(api_error(
                StatusCode::UNAUTHORIZED,
                "authentication_required",
                "a Human browser session or bearer access token is required",
                false,
                request_id,
            ));
        }
    };
    let mut authorization = state
        .store
        .resolve_authorization(identity.principal_id, Utc::now())
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "authorization_unavailable",
                "authorization could not be evaluated",
                true,
                request_id,
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::UNAUTHORIZED,
                "principal_inactive",
                "the authenticated principal is not active",
                false,
                request_id,
            )
        })?;
    if session_organization_id.is_some_and(|expected| expected != authorization.organization_id) {
        return Err(invalid_human_session_cookie(request_id));
    }
    authorization.authentication = Some(identity.authentication.clone());
    Ok((identity, authorization))
}

#[allow(clippy::result_large_err)]
pub(crate) fn enforce_permission_and_scope(
    identity: &AuthenticatedIdentity,
    authorization: &AuthorizationContext,
    permission: Permission,
    message: &'static str,
    request_id: &str,
) -> Result<(), Response> {
    if !authorization.permits(authorization.organization_id, permission) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            message,
            false,
            request_id,
        ));
    }
    if identity
        .scope_restriction
        .as_ref()
        .is_some_and(|scopes| !scopes.contains(&permission))
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "insufficient_scope",
            "the access token does not include the required permission",
            false,
            request_id,
        ));
    }
    Ok(())
}

pub(crate) fn approval_request_permission(action: ApprovalAction) -> Permission {
    match action {
        ApprovalAction::SendAs | ApprovalAction::SendOnBehalfOf | ApprovalAction::BulkDelete => {
            Permission::MailboxUse
        }
        ApprovalAction::ContentReview => Permission::ContentReview,
        ApprovalAction::AuditExport => Permission::AuditExport,
        ApprovalAction::RetentionChange => Permission::RetentionManage,
        ApprovalAction::PrivilegeElevation => Permission::AuthorizationManage,
        ApprovalAction::ProductionAgentMail => Permission::AuthorizationManage,
        ApprovalAction::AgentFirstContact => Permission::AgentOperate,
        ApprovalAction::LegalHold => Permission::LegalHoldManage,
        ApprovalAction::BreakGlass => Permission::BreakGlass,
    }
}

pub(crate) enum PresentedCredential<'a> {
    BrowserSession {
        client: HumanSessionClient,
        token: String,
    },
    Bearer(&'a str),
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CredentialSelectionError {
    Ambiguous,
    InvalidBrowserSession,
    InvalidBearer,
}

pub(crate) fn select_presented_credential<'a>(
    headers: &'a HeaderMap,
    secure_session_cookies: Option<bool>,
) -> Result<PresentedCredential<'a>, CredentialSelectionError> {
    let authorization_present = headers
        .get_all(header::AUTHORIZATION)
        .iter()
        .next()
        .is_some();
    let browser_cookie_present = secure_session_cookies.is_some_and(|secure| {
        [HumanSessionClient::Admin, HumanSessionClient::Mail]
            .into_iter()
            .any(|client| {
                browser_cookie_name_present(headers, human_session_cookie_name(client, secure))
            })
    });
    if browser_cookie_present && authorization_present {
        return Err(CredentialSelectionError::Ambiguous);
    }

    let mut browser_session = None;
    if let Some(secure) = secure_session_cookies {
        for client in [HumanSessionClient::Admin, HumanSessionClient::Mail] {
            let name = human_session_cookie_name(client, secure);
            let token = browser_cookie_value(headers, name)
                .map_err(|()| CredentialSelectionError::InvalidBrowserSession)?;
            if let Some(token) = token {
                if browser_session.is_some() {
                    return Err(CredentialSelectionError::Ambiguous);
                }
                browser_session = Some((client, token));
            }
        }
    }
    if let Some((client, token)) = browser_session {
        return Ok(PresentedCredential::BrowserSession { client, token });
    }

    match bearer_token(headers).map_err(|()| CredentialSelectionError::InvalidBearer)? {
        Some(token) => Ok(PresentedCredential::Bearer(token)),
        None => Ok(PresentedCredential::Missing),
    }
}

pub(crate) fn select_human_browser_session(
    headers: &HeaderMap,
    secure_session_cookies: bool,
) -> Result<(HumanSessionClient, String), CredentialSelectionError> {
    match select_presented_credential(headers, Some(secure_session_cookies))? {
        PresentedCredential::BrowserSession { client, token } => Ok((client, token)),
        PresentedCredential::Bearer(_) | PresentedCredential::Missing => {
            Err(CredentialSelectionError::InvalidBrowserSession)
        }
    }
}

pub(crate) fn any_human_session_cookie_present(
    headers: &HeaderMap,
    secure_session_cookies: bool,
) -> bool {
    [HumanSessionClient::Admin, HumanSessionClient::Mail]
        .into_iter()
        .any(|client| {
            browser_cookie_name_present(
                headers,
                human_session_cookie_name(client, secure_session_cookies),
            )
        })
}

pub(crate) fn bearer_token(headers: &HeaderMap) -> Result<Option<&str>, ()> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(());
    }
    let value = value.to_str().map_err(|_| ())?;
    let (scheme, token) = value.split_once(' ').ok_or(())?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(());
    }
    Ok(Some(token))
}

pub(crate) fn browser_cookie_name_present(headers: &HeaderMap, name: &str) -> bool {
    headers.get_all(header::COOKIE).iter().any(|header_value| {
        header_value.to_str().ok().is_some_and(|value| {
            value.split(';').any(|pair| {
                let pair = pair.trim();
                pair == name
                    || pair
                        .strip_prefix(name)
                        .is_some_and(|suffix| suffix.starts_with('='))
            })
        })
    })
}

pub(crate) fn browser_cookie_value(headers: &HeaderMap, name: &str) -> Result<Option<String>, ()> {
    let mut found = None;
    for header_value in headers.get_all(header::COOKIE) {
        let value = header_value.to_str().map_err(|_| ())?;
        if value.len() > 8 * 1024 {
            return Err(());
        }
        for pair in value.split(';') {
            let Some((candidate_name, candidate_value)) = pair.trim().split_once('=') else {
                return Err(());
            };
            if candidate_name == name {
                if found.is_some()
                    || candidate_value.len() != 43
                    || !candidate_value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                    || URL_SAFE_NO_PAD
                        .decode(candidate_value)
                        .ok()
                        .filter(|decoded| decoded.len() == 32)
                        .is_none()
                {
                    return Err(());
                }
                found = Some(candidate_value.to_owned());
            }
        }
    }
    Ok(found)
}

pub(crate) fn browser_cookie_header(
    name: &str,
    value: &str,
    maximum_age_seconds: i64,
    secure: bool,
) -> Result<header::HeaderValue> {
    let same_site = match name {
        "__Host-copilotmail-admin-login"
        | "__Host-copilotmail-mail-login"
        | "copilotmail-admin-login"
        | "copilotmail-mail-login" => "Lax",
        "__Host-copilotmail-admin-session"
        | "__Host-copilotmail-mail-session"
        | "copilotmail-admin-session"
        | "copilotmail-mail-session" => "Strict",
        _ => anyhow::bail!("browser cookie name is invalid"),
    };
    anyhow::ensure!(
        value.is_empty()
            || value.len() == 43
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "browser cookie value is invalid"
    );
    anyhow::ensure!(
        (0..=28_800).contains(&maximum_age_seconds),
        "browser cookie lifetime is invalid"
    );
    let secure_attribute = if secure { "; Secure" } else { "" };
    let value = format!(
        "{name}={value}; Path=/; HttpOnly{secure_attribute}; SameSite={same_site}; Max-Age={maximum_age_seconds}"
    );
    header::HeaderValue::from_str(&value).context("build browser cookie")
}

pub(crate) fn clear_browser_cookie_header(name: &str, secure: bool) -> Result<header::HeaderValue> {
    browser_cookie_header(name, "", 0, secure)
}

pub(crate) fn browser_session_cookie_headers(
    client: HumanSessionClient,
    value: &str,
    maximum_age_seconds: i64,
    secure: bool,
) -> Result<[header::HeaderValue; 2]> {
    Ok([
        browser_cookie_header(
            human_session_cookie_name(client, secure),
            value,
            maximum_age_seconds,
            secure,
        )?,
        clear_browser_cookie_header(
            human_session_cookie_name(other_human_session_client(client), secure),
            secure,
        )?,
    ])
}

pub(crate) fn datetime_from_unix_seconds(value: u64) -> Option<DateTime<Utc>> {
    i64::try_from(value)
        .ok()
        .and_then(|value| DateTime::from_timestamp(value, 0))
}

pub(crate) fn unix_seconds_from_datetime(value: DateTime<Utc>) -> Option<u64> {
    u64::try_from(value.timestamp()).ok()
}

pub(crate) fn request_id_value(request_id: &RequestId) -> String {
    request_id
        .header_value()
        .to_str()
        .unwrap_or("invalid-request-id")
        .to_owned()
}
