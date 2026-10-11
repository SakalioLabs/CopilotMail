use crate::*;

pub(crate) async fn ingest_stalwart_mail_events(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    request: Request,
) -> Result<(StatusCode, Json<MailEventIngressResponse>), Response> {
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    let request_id = request_id_value(&request_id);
    let Some(settings) = &state.mail_event_ingress else {
        return Err(api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_event_ingress_unavailable",
            "mail event ingestion is not configured",
            true,
            &request_id,
        ));
    };
    let (expected_bearer, signature_key) = load_mail_event_ingress_secrets(settings)
        .await
        .map_err(|error| {
            tracing::error!(%error, "mail event ingress credentials could not be loaded");
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_event_ingress_unavailable",
                "mail event ingestion is temporarily unavailable",
                true,
                &request_id,
            )
        })?;
    let provided_bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if !provided_bearer
        .is_some_and(|provided| secure_token_matches(&expected_bearer, provided.as_bytes()))
    {
        return Err(mail_event_ingress_unauthorized(&request_id));
    }
    let signature = request
        .headers()
        .get("x-signature")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| mail_event_ingress_unauthorized(&request_id))?;
    let body = to_bytes(request.into_body(), MAX_BODY_BYTES)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "mail_event_payload_too_large",
                "mail event payload exceeds the accepted limit",
                false,
                &request_id,
            )
        })?;
    let events = StalwartWebhookParser::verify_and_parse_all(&body, &signature, &signature_key)
        .map_err(|error| mail_engine_webhook_api_error(error, &request_id))?;
    let data_plane_id = state.mail_data_plane_id.ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_data_plane_unavailable",
            "mail data plane is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let fingerprint_scheme = mail_event_fingerprint_scheme(data_plane_id)
        .map_err(|error| mail_event_ingress_api_error(error, &request_id))?;
    let result = MailEngineEventIngestionService::new(
        state.store.clone(),
        data_plane_id,
        fingerprint_scheme,
    )
    .ingest(IngestMailEngineEventsCommand {
        events,
        received_at: Utc::now(),
    })
    .await
    .map_err(|error| mail_event_ingress_api_error(error, &request_id))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(MailEventIngressResponse {
            accepted: result.accepted,
            inserted: result.inserted,
            duplicates: result.duplicates,
        }),
    ))
}

pub(crate) async fn load_mail_event_ingress_secrets(
    settings: &MailEventIngressSettings,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let bearer = read_bounded_secret_file(&settings.bearer_token_file, 4 * 1024).await?;
    let signature_key = read_bounded_secret_file(&settings.signature_key_file, 16 * 1024).await?;
    let bearer = normalize_ingress_secret(bearer, 32, 4 * 1024)
        .context("mail event ingress bearer token file is invalid")?;
    let signature_key = normalize_ingress_secret(signature_key, 32, 16 * 1024)
        .context("mail event ingress signature key file is invalid")?;
    Ok((bearer, signature_key))
}

pub(crate) fn normalize_ingress_secret(
    mut value: Vec<u8>,
    minimum_bytes: usize,
    maximum_bytes: usize,
) -> Option<Vec<u8>> {
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
    {
        value.pop();
    }
    (value.len() >= minimum_bytes
        && value.len() <= maximum_bytes
        && value.iter().all(|byte| (0x21..=0x7e).contains(byte)))
    .then_some(value)
}

pub(crate) fn secure_token_matches(expected: &[u8], provided: &[u8]) -> bool {
    const COMPARISON_KEY: &[u8] = b"CopilotMail/mail-event-ingress/bearer/v1";
    let mut expected_mac =
        Hmac::<Sha256>::new_from_slice(COMPARISON_KEY).expect("HMAC accepts fixed key");
    expected_mac.update(expected);
    let expected_tag = expected_mac.finalize().into_bytes();
    let mut provided_mac =
        Hmac::<Sha256>::new_from_slice(COMPARISON_KEY).expect("HMAC accepts fixed key");
    provided_mac.update(provided);
    provided_mac.verify_slice(&expected_tag).is_ok()
}

pub(crate) fn mail_event_ingress_unauthorized(request_id: &str) -> Response {
    let mut response = api_error(
        StatusCode::UNAUTHORIZED,
        "mail_event_authentication_failed",
        "mail event source authentication failed",
        false,
        request_id,
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        "Bearer realm=\"mail-event-ingress\"".parse().unwrap(),
    );
    response
}

pub(crate) fn mail_engine_webhook_api_error(
    error: MailEngineWebhookError,
    request_id: &str,
) -> Response {
    match error {
        MailEngineWebhookError::InvalidConfiguration => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_event_ingress_unavailable",
            "mail event ingestion is temporarily unavailable",
            true,
            request_id,
        ),
        MailEngineWebhookError::InvalidSignature => mail_event_ingress_unauthorized(request_id),
        MailEngineWebhookError::InvalidPayload | MailEngineWebhookError::UnsupportedEvent => {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_mail_event_payload",
                "mail event payload is not accepted",
                false,
                request_id,
            )
        }
    }
}

pub(crate) fn mail_event_ingress_api_error(
    error: MailEventIngestionError,
    request_id: &str,
) -> Response {
    match error {
        MailEventIngestionError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "a Human MailAdmin is required to manage this binding",
            false,
            request_id,
        ),
        MailEventIngestionError::InvalidBinding
        | MailEventIngestionError::InvalidTimestamp
        | MailEventIngestionError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_event_configuration",
            "mail event input or mailbox binding is invalid",
            false,
            request_id,
        ),
        MailEventIngestionError::UnmappedMailbox => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_engine_mailbox_unmapped",
            "mail engine account is not mapped to a mailbox",
            true,
            request_id,
        ),
        MailEventIngestionError::Store(copilotmail_application::StoreError::Conflict) => api_error(
            StatusCode::CONFLICT,
            "mail_event_replay_conflict",
            "mail event identity conflicts with existing immutable evidence",
            false,
            request_id,
        ),
        MailEventIngestionError::Store(copilotmail_application::StoreError::MissingReference) => {
            resource_not_found(request_id)
        }
        MailEventIngestionError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        MailEventIngestionError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_event_store_unavailable",
            "mail event state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}
