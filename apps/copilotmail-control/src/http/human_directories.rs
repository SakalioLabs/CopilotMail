use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HumanMailboxDirectoryQuery {
    pub(crate) limit: Option<u16>,
    pub(crate) cursor: Option<String>,
}

pub(crate) async fn list_human_mailboxes(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<HumanMailboxDirectoryResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let authorization =
        authenticate_human_mail_session_request(&state, &headers, connection, &request_id).await?;
    let Query(query) = Query::<HumanMailboxDirectoryQuery>::try_from_uri(&uri).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_directory_query",
            "the mailbox directory query is invalid",
            false,
            &request_id,
        )
    })?;
    let page = HumanMailboxDirectoryService::new(state.store.clone())
        .list(ListHumanMailboxDirectoryCommand {
            context: authorization,
            limit: query.limit.unwrap_or(25),
            cursor: query.cursor,
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_mailbox_directory_api_error(error, &request_id))?;
    let has_more = page.next_cursor.is_some();
    Ok(Json(HumanMailboxDirectoryResponse {
        items: page
            .items
            .into_iter()
            .map(|item| HumanMailboxDirectoryItemResponse {
                mailbox_id: item.mailbox_id.to_string(),
                kind: mailbox_kind_wire(item.kind).into(),
                primary_address: item.primary_address.to_string(),
                can_send: item.can_send,
            })
            .collect(),
        next_cursor: page.next_cursor,
        has_more,
    }))
}

pub(crate) fn human_mailbox_directory_api_error(
    error: HumanMailboxDirectoryError,
    request_id: &str,
) -> Response {
    match error {
        HumanMailboxDirectoryError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "mailbox_directory_forbidden",
            "the Human is not permitted to use mailboxes",
            false,
            request_id,
        ),
        HumanMailboxDirectoryError::InvalidPageSize => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_directory_limit",
            "limit must be between 1 and 50",
            false,
            request_id,
        ),
        HumanMailboxDirectoryError::InvalidCursor => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox_directory_cursor",
            "the mailbox directory cursor is invalid",
            false,
            request_id,
        ),
        HumanMailboxDirectoryError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mailbox_directory_unavailable",
            "the mailbox directory is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HumanDraftListQuery {
    pub(crate) limit: Option<u16>,
    pub(crate) cursor: Option<String>,
}

pub(crate) async fn list_human_drafts(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    OriginalUri(uri): OriginalUri,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<HumanDraftListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let authorization =
        authenticate_human_mail_session_request(&state, &headers, connection, &request_id).await?;
    let Query(query) = Query::<HumanDraftListQuery>::try_from_uri(&uri).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_query",
            "the Human draft query is invalid",
            false,
            &request_id,
        )
    })?;
    let reader = state.mail_draft_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let page = HumanDraftService::new(state.store.clone(), reader)
        .list(ListHumanDraftsCommand {
            context: authorization,
            mailbox_id,
            limit: query.limit.unwrap_or(25),
            cursor: query.cursor,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_draft_api_error(error, &request_id))?;
    let has_more = page.next_cursor.is_some();
    Ok(Json(HumanDraftListResponse {
        items: page
            .items
            .into_iter()
            .map(|item| HumanDraftListItemResponse {
                draft_id: item.draft_id.to_string(),
                version: item.version,
                reply_to_message_id: item.reply_to_message_id.map(|value| value.to_string()),
                recipient_count: item.recipient_count,
                created_at: item.created_at.to_rfc3339(),
                updated_at: item.updated_at.to_rfc3339(),
            })
            .collect(),
        next_cursor: page.next_cursor,
        has_more,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HumanSentListQuery {
    pub(crate) limit: Option<u16>,
    pub(crate) cursor: Option<String>,
}

/// Returns only the current Human's own accepted submissions and aggregate
/// delivery evidence. This browser route intentionally has a distinct,
/// minimized contract from the generic single-submission status endpoint.
pub(crate) async fn list_human_sent_submissions(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<HumanSentListResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let authorization =
        authenticate_human_mail_session_request(&state, &headers, connection, &request_id).await?;
    let mailbox_id = mail_submission_mailbox_id_from_path(uri.path()).ok_or_else(|| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_sent_mailbox",
            "the Human sent-directory mailbox is invalid",
            false,
            &request_id,
        )
    })?;
    let Query(query) = Query::<HumanSentListQuery>::try_from_uri(&uri).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_sent_query",
            "the Human sent-directory query is invalid",
            false,
            &request_id,
        )
    })?;
    let page = HumanSentService::new(state.store.clone())
        .list(ListHumanSentSubmissionsCommand {
            context: authorization,
            mailbox_id,
            limit: query.limit.unwrap_or(25),
            cursor: query.cursor,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_sent_api_error(error, &request_id))?;
    let has_more = page.next_cursor.is_some();
    Ok(Json(HumanSentListResponse {
        items: page
            .items
            .into_iter()
            .map(|item| HumanSentListItemResponse {
                submission_id: item.submission_id.to_string(),
                accepted_at: item.accepted_at.to_rfc3339(),
                recipient_count: item.recipient_count,
                delivery: item
                    .delivery
                    .map(|delivery| HumanSentDeliverySummaryResponse {
                        observed_at: delivery.observed_at.to_rfc3339(),
                        terminal: delivery.terminal,
                        recipient_count: delivery.recipient_count,
                        delivered_count: delivery.delivered_count,
                        failed_count: delivery.failed_count,
                        queued_count: delivery.queued_count,
                        unknown_count: delivery.unknown_count,
                    }),
            })
            .collect(),
        next_cursor: page.next_cursor,
        has_more,
    }))
}

/// `GET` records immutable disclosure evidence. Do not let Axum's implicit
/// GET-to-HEAD fallback create audit/Outbox records for a request that has no
/// response body. The Mail-session check remains first so HEAD cannot probe
/// this route or bypass the client partition.
pub(crate) async fn reject_human_sent_submissions_head(
    State(state): State<Arc<AppState>>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    authenticate_human_mail_session_request(&state, &headers, connection, &request_id).await?;
    let mut response = api_error(
        StatusCode::METHOD_NOT_ALLOWED,
        "human_sent_head_not_supported",
        "HEAD is not supported for the Human sent-directory",
        false,
        &request_id,
    );
    response
        .headers_mut()
        .insert(header::ALLOW, header::HeaderValue::from_static("GET, POST"));
    Ok(response)
}

/// Parses the routed mailbox segment only after the Human Mail session has
/// been authenticated. Keeping this out of Axum's typed `Path` extractor
/// prevents unauthenticated callers from observing a framework 400 for a
/// malformed UUID before the Mail-client boundary rejects them.
/// Decodes the common mailbox segment of the GET/HEAD/POST submissions route.
/// Callers must authenticate their respective client boundary before invoking
/// it so malformed identifiers never become an unauthenticated routing oracle.
pub(crate) fn mail_submission_mailbox_id_from_path(path: &str) -> Option<MailboxId> {
    exact_path_segments(path)
        .and_then(|segments| match segments.as_slice() {
            ["api", "v2", "mailboxes", mailbox_id, "submissions"] => Some(*mailbox_id),
            _ => None,
        })
        .and_then(|value| Uuid::parse_str(value).ok())
        .filter(|value| !value.is_nil())
        .map(MailboxId::from_uuid)
}

pub(crate) async fn restore_human_draft(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, draft_id)): Path<(MailboxId, DraftId)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    request: Request,
) -> Result<Json<HumanDraftRestoreResponse>, Response> {
    const MAX_BODY_BYTES: usize = 4 * 1024;
    let request_id = request_id_value(&request_id);
    let authorization =
        authenticate_human_mail_session_request(&state, request.headers(), connection, &request_id)
            .await?;
    let content_type_is_json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_some_and(|value| value.eq_ignore_ascii_case("application/json"));
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
    let body: HumanDraftRestoreRequest = serde_json::from_slice(&bytes).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_request",
            "the Human draft request is invalid",
            false,
            &request_id,
        )
    })?;
    let reader = state.mail_draft_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let restored = HumanDraftService::new(state.store.clone(), reader)
        .restore(RestoreHumanDraftCommand {
            context: authorization,
            mailbox_id,
            draft_id,
            expected_version: body.expected_version,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| human_draft_api_error(error, &request_id))?;
    Ok(Json(HumanDraftRestoreResponse {
        draft_id: restored.draft_id.to_string(),
        version: restored.version,
        reply_to_message_id: restored.reply_to_message_id.map(|value| value.to_string()),
        to: restored.to,
        cc: restored.cc,
        bcc: restored.bcc,
        subject: restored.subject,
        text_body: restored.text_body,
    }))
}
