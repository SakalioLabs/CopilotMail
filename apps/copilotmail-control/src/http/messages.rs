use crate::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InboxListQuery {
    pub(crate) cursor: Option<String>,
    pub(crate) page_size: Option<u32>,
}

pub(crate) async fn list_inbox_messages(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Query(query): Query<InboxListQuery>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<InboxPageResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let position = query
        .cursor
        .as_deref()
        .map(|cursor| decode_inbox_cursor(cursor, mailbox_id))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "the inbox cursor is invalid",
                false,
                &request_id,
            )
        })?
        .unwrap_or(0);
    let page = InboxService::new(state.store.clone(), reader)
        .list(
            &authorization,
            ListInboxCommand {
                mailbox_id,
                position,
                page_size: query.page_size.unwrap_or(30),
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    let next_cursor = page
        .next_position
        .map(|next| encode_inbox_cursor(mailbox_id, next));
    let items = page.items.into_iter().map(inbox_summary_response).collect();
    Ok(Json(InboxPageResponse {
        items,
        has_more: next_cursor.is_some(),
        next_cursor,
    }))
}

pub(crate) async fn search_inbox_messages(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<InboxSearchRequest>,
) -> Result<Json<InboxPageResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let search_digest = inbox_search_digest(&body.text);
    let position = body
        .cursor
        .as_deref()
        .map(|cursor| decode_inbox_search_cursor(cursor, mailbox_id, &search_digest))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "the inbox search cursor is invalid",
                false,
                &request_id,
            )
        })?
        .unwrap_or(0);
    let page = InboxService::new(state.store.clone(), reader)
        .search(
            &authorization,
            SearchInboxCommand {
                mailbox_id,
                text: body.text,
                position,
                page_size: body.page_size.unwrap_or(30),
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    let next_cursor = page
        .next_position
        .map(|next| encode_inbox_search_cursor(mailbox_id, next, &search_digest));
    Ok(Json(InboxPageResponse {
        items: page.items.into_iter().map(inbox_summary_response).collect(),
        has_more: next_cursor.is_some(),
        next_cursor,
    }))
}

pub(crate) async fn get_inbox_message(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, message_id)): Path<(MailboxId, uuid::Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MessageDetailResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let detail = InboxService::new(state.store.clone(), reader)
        .get(
            &authorization,
            GetMessageCommand {
                mailbox_id,
                message_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    Ok(Json(MessageDetailResponse {
        message: inbox_summary_response(detail.summary),
        reply_to_message_id: Some(message_id.to_string()),
        to: detail.to.into_iter().map(inbox_address_response).collect(),
        cc: detail.cc.into_iter().map(inbox_address_response).collect(),
        text_body: detail.text_body,
        text_body_truncated: detail.text_body_truncated,
        attachments: detail
            .attachments
            .into_iter()
            .map(|attachment| InboxAttachmentResponse {
                name: attachment.name,
                media_type: attachment.media_type,
                size_bytes: attachment.size_bytes,
            })
            .collect(),
    }))
}

pub(crate) async fn update_inbox_message_state(
    State(state): State<Arc<AppState>>,
    Path((mailbox_id, message_id)): Path<(MailboxId, uuid::Uuid)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MessageStateUpdateRequest>,
) -> Result<Json<MessageStateResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_identity_required",
            "this mailbox interface requires an authenticated Human",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        &request_id,
    )?;
    let reader = state.mail_content.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let state = InboxService::new(state.store.clone(), reader)
        .update_state(
            &authorization,
            UpdateMessageStateCommand {
                mailbox_id,
                message_id,
                is_read: body.is_read,
                is_flagged: body.is_flagged,
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| inbox_api_error(error, &request_id))?;
    Ok(Json(MessageStateResponse {
        message_id: message_id.to_string(),
        is_read: state.is_read,
        is_flagged: state.is_flagged,
    }))
}

pub(crate) fn inbox_summary_response(
    item: HumanInboxMessageSummary,
) -> InboxMessageSummaryResponse {
    InboxMessageSummaryResponse {
        message_id: item.message_id.to_string(),
        subject: item.subject,
        from: item.from.into_iter().map(inbox_address_response).collect(),
        received_at: item.received_at.to_rfc3339(),
        preview: item.preview,
        is_read: item.is_read,
        is_flagged: item.is_flagged,
        has_attachment: item.has_attachment,
    }
}

pub(crate) fn inbox_address_response(address: MailAddress) -> InboxAddressResponse {
    InboxAddressResponse {
        name: address.name,
        email: address.email,
    }
}

pub(crate) fn encode_inbox_cursor(mailbox_id: MailboxId, position: u32) -> String {
    URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "v": 2,
            "mailbox_id": mailbox_id,
            "position": position,
        }))
        .expect("inbox cursor consists only of serializable primitives"),
    )
}

pub(crate) fn decode_inbox_cursor(cursor: &str, mailbox_id: MailboxId) -> Result<u32, ()> {
    if cursor.is_empty() || cursor.len() > 2048 {
        return Err(());
    }
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| ())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    if object.len() != 3
        || object.get("v").and_then(serde_json::Value::as_u64) != Some(2)
        || object.get("mailbox_id").and_then(serde_json::Value::as_str)
            != Some(mailbox_id.to_string().as_str())
    {
        return Err(());
    }
    object
        .get("position")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value <= 10_000)
        .ok_or(())
}

pub(crate) fn inbox_search_digest(text: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"copilotmail.inbox-search.v1");
    digest.update((text.len() as u64).to_be_bytes());
    digest.update(text.as_bytes());
    URL_SAFE_NO_PAD.encode(digest.finalize())
}

pub(crate) fn encode_inbox_search_cursor(
    mailbox_id: MailboxId,
    position: u32,
    search_digest: &str,
) -> String {
    URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "v": 2,
            "mailbox_id": mailbox_id,
            "position": position,
            "search_digest": search_digest,
        }))
        .expect("inbox search cursor consists only of serializable primitives"),
    )
}

pub(crate) fn decode_inbox_search_cursor(
    cursor: &str,
    mailbox_id: MailboxId,
    expected_search_digest: &str,
) -> Result<u32, ()> {
    if cursor.is_empty() || cursor.len() > 2048 {
        return Err(());
    }
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|_| ())?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let object = value.as_object().ok_or(())?;
    if object.len() != 4
        || object.get("v").and_then(serde_json::Value::as_u64) != Some(2)
        || object.get("mailbox_id").and_then(serde_json::Value::as_str)
            != Some(mailbox_id.to_string().as_str())
        || object
            .get("search_digest")
            .and_then(serde_json::Value::as_str)
            != Some(expected_search_digest)
    {
        return Err(());
    }
    object
        .get("position")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value <= 10_000)
        .ok_or(())
}

pub(crate) fn inbox_api_error(error: InboxError, request_id: &str) -> Response {
    match error {
        InboxError::NotFound => resource_not_found(request_id),
        InboxError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_inbox_request",
            "the inbox request is invalid",
            false,
            request_id,
        ),
        InboxError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_content_unavailable",
            "mailbox content is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn draft_api_error(error: DraftError, request_id: &str) -> Response {
    match error {
        DraftError::NotFound => resource_not_found(request_id),
        DraftError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_draft_request",
            "the draft request is invalid",
            false,
            request_id,
        ),
        DraftError::Conflict => api_error(
            StatusCode::CONFLICT,
            "draft_conflict",
            "the draft changed or is no longer editable",
            false,
            request_id,
        ),
        DraftError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            request_id,
        ),
    }
}
