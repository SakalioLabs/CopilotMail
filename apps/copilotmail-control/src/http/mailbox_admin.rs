use crate::*;

pub(crate) async fn create_mailbox(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailboxCreateRequest>,
) -> Result<(StatusCode, Json<MailboxCreateResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to create mailboxes",
        &request_id,
    )?;
    let Some(kind) = mailbox_kind_from_wire(&body.kind) else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_kind",
            "mailbox kind must be human, agent, shared, or group",
            false,
            &request_id,
        ));
    };
    let result = MailboxManagementService::new(state.store.clone())
        .create_authorized(
            &authorization,
            CreateMailboxCommand {
                organization_id,
                kind,
                primary_address: body.primary_address,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mailbox_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(MailboxCreateResponse {
            mailbox_id: result.mailbox_id.to_string(),
            organization_id: result.organization_id.to_string(),
            owner_principal_id: result.owner_principal_id.to_string(),
            primary_address: result.primary_address,
        }),
    ))
}

pub(crate) async fn update_mail_engine_mailbox_binding(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    request: Request,
) -> Result<Json<MailEngineMailboxBindingResponse>, Response> {
    const MAX_BODY_BYTES: usize = 16 * 1024;
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailAdmin,
        "mail.admin permission is required to bind mail-engine mailboxes",
        &request_id,
    )?;
    let content_type_is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| {
            value.eq_ignore_ascii_case("application/json")
                || value
                    .strip_prefix("application/")
                    .is_some_and(|subtype| subtype.to_ascii_lowercase().ends_with("+json"))
        });
    if !content_type_is_json {
        return Err(api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid_content_type",
            "Content-Type must be application/json",
            false,
            &request_id,
        ));
    }
    let bytes = to_bytes(request.into_body(), MAX_BODY_BYTES)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request_payload_too_large",
                "request payload exceeds the accepted limit",
                false,
                &request_id,
            )
        })?;
    let body: MailEngineMailboxBindingUpdateRequest =
        serde_json::from_slice(&bytes).map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "request payload is invalid",
                false,
                &request_id,
            )
        })?;
    let data_plane_id = state.mail_data_plane_id.ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_data_plane_unavailable",
            "mail data plane is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let result = MailEngineMailboxBindingService::try_new(
        state.store.clone(),
        data_plane_id,
        LEGACY_MAIL_DATA_PLANE_SOURCE,
    )
    .map_err(|error| mail_event_ingress_api_error(error, &request_id))?
    .bind_authorized(
        &authorization,
        BindMailEngineMailboxCommand {
            organization_id: authorization.organization_id,
            mailbox_id,
            account_ref: body.account_ref,
            expected_version: body.expected_version,
            trace_id: request_id.clone(),
            now: Utc::now(),
        },
    )
    .await
    .map_err(|error| mail_event_ingress_api_error(error, &request_id))?;
    Ok(Json(MailEngineMailboxBindingResponse {
        organization_id: result.organization_id.to_string(),
        mailbox_id: result.mailbox_id.to_string(),
        data_plane_id: result.data_plane_id.to_string(),
        account_ref: result.account_ref,
        version: result.version,
        updated_at: result.updated_at.to_rfc3339(),
    }))
}

pub(crate) async fn create_mailbox_delegation(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailboxDelegationCreateRequest>,
) -> Result<(StatusCode, Json<MailboxDelegationResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to grant mailbox delegations",
        &request_id,
    )?;
    let grantee_principal_id = body
        .grantee_principal_id
        .parse::<PrincipalId>()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_principal_id",
                "grantee_principal_id must be a UUID",
                false,
                &request_id,
            )
        })?;
    let capabilities = body
        .capabilities
        .into_iter()
        .map(|value| mailbox_capability_from_wire(&value))
        .collect::<Option<BTreeSet<_>>>();
    let Some(capabilities) = capabilities else {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_capability",
            "delegation capabilities must be mailbox.read, mailbox.send, or mailbox.send_as",
            false,
            &request_id,
        ));
    };
    let valid_until = chrono::DateTime::parse_from_rfc3339(&body.valid_until)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_expiration",
                "valid_until must be an RFC 3339 instant",
                false,
                &request_id,
            )
        })?;
    let now = Utc::now();
    let result = MailboxDelegationService::new(state.store.clone())
        .grant_authorized(
            &authorization,
            GrantMailboxDelegationCommand {
                organization_id: authorization.organization_id,
                mailbox_id,
                grantee_principal_id,
                capabilities,
                reason: body.reason,
                valid_from: now,
                valid_until,
                trace_id: request_id.clone(),
                now,
            },
        )
        .await
        .map_err(|error| mailbox_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(MailboxDelegationResponse {
            delegation_id: result.delegation_id.to_string(),
            mailbox_id: result.mailbox_id.to_string(),
            grantor_principal_id: result.grantor_principal_id.to_string(),
            grantee_principal_id: result.grantee_principal_id.to_string(),
        }),
    ))
}

pub(crate) async fn revoke_mailbox_delegation(
    State(state): State<Arc<AppState>>,
    Path(delegation_id): Path<DelegationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailboxDelegationRevokedResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuthorizationManage,
        "authorization.manage permission is required to revoke mailbox delegations",
        &request_id,
    )?;
    MailboxDelegationService::new(state.store.clone())
        .revoke_authorized(
            &authorization,
            RevokeMailboxDelegationCommand {
                organization_id: authorization.organization_id,
                delegation_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mailbox_api_error(error, &request_id))?;
    Ok(Json(MailboxDelegationRevokedResponse {
        delegation_id: delegation_id.to_string(),
        revoked: true,
    }))
}

pub(crate) fn mailbox_api_error(
    error: copilotmail_application::MailboxAuthorizationError,
    request_id: &str,
) -> Response {
    match error {
        copilotmail_application::MailboxAuthorizationError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_authorization",
            "the mailbox or delegation policy is invalid",
            false,
            request_id,
        ),
        copilotmail_application::MailboxAuthorizationError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authenticated Principal cannot manage this mailbox",
            false,
            request_id,
        ),
        copilotmail_application::MailboxAuthorizationError::Store(
            copilotmail_application::StoreError::Conflict,
        ) => api_error(
            StatusCode::CONFLICT,
            "mailbox_conflict",
            "the mailbox address or active delegation already exists",
            false,
            request_id,
        ),
        copilotmail_application::MailboxAuthorizationError::Store(
            copilotmail_application::StoreError::MissingReference,
        ) => resource_not_found(request_id),
        copilotmail_application::MailboxAuthorizationError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        copilotmail_application::MailboxAuthorizationError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mailbox_store_unavailable",
            "mailbox state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) async fn get_mailbox(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailboxResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let now = Utc::now();
    let decision = AuthorizeMailboxActionService::new(state.store.clone())
        .authorize(
            &authorization,
            mailbox_id,
            MailboxCapability::Read,
            None,
            now,
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mailbox_authorization_unavailable",
                "mailbox authorization could not be evaluated",
                true,
                &request_id,
            )
        })?;
    if decision.is_none() {
        return Err(resource_not_found(&request_id));
    }
    let mailbox = state
        .store
        .get_mailbox(mailbox_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mailbox_store_unavailable",
                "mailbox data is temporarily unavailable",
                true,
                &request_id,
            )
        })?
        .filter(|mailbox| mailbox.organization_id == authorization.organization_id)
        .ok_or_else(|| resource_not_found(&request_id))?;
    Ok(Json(MailboxResponse {
        mailbox_id: mailbox.mailbox_id.to_string(),
        organization_id: mailbox.organization_id.to_string(),
        kind: mailbox_kind_wire(mailbox.kind).into(),
        primary_address: mailbox.primary_address.to_string(),
        created_at: mailbox.created_at.to_rfc3339(),
    }))
}
