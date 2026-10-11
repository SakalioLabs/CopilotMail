use crate::*;

#[allow(clippy::result_large_err)]
pub(crate) fn enforce_mail_submission_scopes(
    identity: &AuthenticatedIdentity,
    authorization: &AuthorizationContext,
    request_id: &str,
) -> Result<(), Response> {
    enforce_permission_and_scope(
        identity,
        authorization,
        Permission::MailboxUse,
        "mailbox.use permission is required",
        request_id,
    )?;
    if identity.principal_kind == AuthenticatedPrincipalKind::Agent {
        enforce_permission_and_scope(
            identity,
            authorization,
            Permission::AgentOperate,
            "agent.operate permission is required for Agent mail submission",
            request_id,
        )?;
    }
    Ok(())
}

pub(crate) async fn save_text_draft(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<TextDraftSaveRequest>,
) -> Result<(StatusCode, Json<TextDraftResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let drafts = state.mail_drafts.clone().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            &request_id,
        )
    })?;
    let draft_id = body
        .draft_id
        .map(|value| value.parse())
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_draft_request",
                "the draft request is invalid",
                false,
                &request_id,
            )
        })?;
    let reply_to_message_id = body
        .reply_to_message_id
        .map(|value| Uuid::parse_str(&value))
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_draft_request",
                "the draft request is invalid",
                false,
                &request_id,
            )
        })?;
    let draft = DraftService::new(state.store.clone(), drafts)
        .save(
            &authorization,
            SaveTextDraftCommand {
                mailbox_id,
                draft_id,
                expected_version: body.expected_version,
                reply_to_message_id,
                to: body.to,
                cc: body.cc,
                bcc: body.bcc,
                subject: body.subject,
                text_body: body.text_body,
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| draft_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(TextDraftResponse {
            draft_id: draft.draft_id.to_string(),
            version: draft.version,
            reply_to_message_id: draft.reply_to_message_id.map(|value| value.to_string()),
            previous_draft_destroyed: draft.previous_draft_destroyed,
        }),
    ))
}

pub(crate) async fn request_mail_submission(
    State(state): State<Arc<AppState>>,
    OriginalUri(uri): OriginalUri,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<MailSubmissionCreateRequest>,
) -> Result<(StatusCode, Json<MailSubmissionResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let mailbox_id = mail_submission_mailbox_id_from_path(uri.path()).ok_or_else(|| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_submission",
            "the mail submission request is invalid",
            false,
            &request_id,
        )
    })?;
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                "idempotency_key_required",
                "a valid Idempotency-Key header is required",
                false,
                &request_id,
            )
        })?;
    let draft_id = body.draft_id.parse().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_submission",
            "the mail submission request is invalid",
            false,
            &request_id,
        )
    })?;
    let first_contact_approval_request_id = body
        .first_contact_approval_request_id
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_mail_submission",
                "the mail submission request is invalid",
                false,
                &request_id,
            )
        })?;
    let intent = MailSubmissionService::new(state.store.clone())
        .request(
            &authorization,
            RequestMailSubmissionCommand {
                mailbox_id,
                draft_id,
                expected_draft_version: body.expected_draft_version,
                idempotency_key,
                first_contact_approval_request_id,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| mail_submission_api_error(error, &request_id))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(mail_submission_response(intent, None)),
    ))
}

pub(crate) async fn capture_agent_mail_sandbox_submission(
    State(state): State<Arc<AppState>>,
    Path(mailbox_id): Path<MailboxId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AgentMailSandboxCreateRequest>,
) -> Result<(StatusCode, Json<AgentMailSandboxResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Agent {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "agent_identity_required",
            "Agent mail sandbox capture requires an authenticated Agent",
            false,
            &request_id,
        ));
    }
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let idempotency_key = required_idempotency_key(&headers, &request_id)?;
    let draft_id = body.draft_id.parse().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_sandbox_request",
            "the Agent mail sandbox request is invalid",
            false,
            &request_id,
        )
    })?;
    let capture = AgentMailSandboxService::new(state.store.clone())
        .capture(
            &authorization,
            CaptureAgentMailSandboxCommand {
                mailbox_id,
                draft_id,
                expected_draft_version: body.expected_draft_version,
                idempotency_key,
                trace_id: request_id.clone(),
                now: Utc::now(),
            },
        )
        .await
        .map_err(|error| agent_mail_sandbox_api_error(error, &request_id))?;
    let status = if capture.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        Json(AgentMailSandboxResponse {
            sandbox_submission_id: capture.record.submission_id.to_string(),
            mailbox_id: capture.record.mailbox_id.to_string(),
            draft_id: capture.record.draft_id.to_string(),
            draft_version: capture.record.draft_version,
            recipient_count: capture.record.recipient_count,
            delivery_suppressed: true,
            captured_at: capture.record.captured_at.to_rfc3339(),
            replayed: capture.replayed,
        }),
    ))
}

pub(crate) async fn get_mail_submission(
    State(state): State<Arc<AppState>>,
    Path(submission_id): Path<MailSubmissionId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<MailSubmissionResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    enforce_mail_submission_scopes(&identity, &authorization, &request_id)?;
    enforce_agent_ingress_limit(&state, &identity, &authorization, &request_id).await?;
    let intent = state
        .store
        .get_mail_submission_intent(
            authorization.organization_id,
            authorization.principal_id,
            submission_id,
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_submission_unavailable",
                "mail submission status is temporarily unavailable",
                true,
                &request_id,
            )
        })?
        .ok_or_else(|| resource_not_found(&request_id))?;
    let allowed = AuthorizeMailboxActionService::new(state.store.clone())
        .authorize(
            &authorization,
            intent.mailbox_id,
            MailboxCapability::Send,
            None,
            Utc::now(),
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_submission_unavailable",
                "mail submission status is temporarily unavailable",
                true,
                &request_id,
            )
        })?;
    if allowed.is_none() {
        return Err(resource_not_found(&request_id));
    }
    let delivery = state
        .store
        .get_latest_mail_delivery_snapshot(
            authorization.organization_id,
            authorization.principal_id,
            submission_id,
        )
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_submission_unavailable",
                "mail submission delivery status is temporarily unavailable",
                true,
                &request_id,
            )
        })?;
    Ok(Json(mail_submission_response(intent, delivery)))
}

pub(crate) fn mail_submission_response(
    intent: copilotmail_application::MailSubmissionIntentRecord,
    delivery: Option<copilotmail_application::MailDeliverySnapshotRecord>,
) -> MailSubmissionResponse {
    MailSubmissionResponse {
        submission_id: intent.submission_id.to_string(),
        mailbox_id: intent.mailbox_id.to_string(),
        draft_id: intent.draft_id.to_string(),
        draft_version: intent.draft_version,
        status: match intent.status {
            MailSubmissionStatus::Pending => "pending",
            MailSubmissionStatus::Reconciling => "reconciling",
            MailSubmissionStatus::Accepted => "accepted",
            MailSubmissionStatus::Rejected => "rejected",
            MailSubmissionStatus::ManualReview => "manual_review",
        }
        .into(),
        state_version: intent.state_version,
        created_at: intent.created_at.to_rfc3339(),
        updated_at: intent.updated_at.to_rfc3339(),
        delivery: delivery.map(mail_delivery_response),
    }
}

pub(crate) fn mail_delivery_response(
    snapshot: copilotmail_application::MailDeliverySnapshotRecord,
) -> MailDeliveryResponse {
    let mut delivered_count = 0;
    let mut failed_count = 0;
    let mut queued_count = 0;
    let mut unknown_count = 0;
    let recipients = snapshot
        .recipients
        .into_iter()
        .map(|recipient| {
            let status = match recipient.state {
                MailDeliveryRecipientState::Delivered => {
                    delivered_count += 1;
                    "delivered"
                }
                MailDeliveryRecipientState::PermanentFailure => {
                    failed_count += 1;
                    "permanent_failure"
                }
                MailDeliveryRecipientState::Queued => {
                    queued_count += 1;
                    "queued"
                }
                MailDeliveryRecipientState::Unknown => {
                    unknown_count += 1;
                    "unknown"
                }
            };
            MailDeliveryRecipientResponse {
                recipient_ref: sha256_hex(&recipient.recipient_ref),
                status: status.into(),
                smtp_reply: recipient.smtp_reply,
            }
        })
        .collect::<Vec<_>>();
    MailDeliveryResponse {
        observed_at: snapshot.observed_at.to_rfc3339(),
        terminal: snapshot.terminal,
        recipient_count: recipients.len(),
        delivered_count,
        failed_count,
        queued_count,
        unknown_count,
        recipients,
    }
}

pub(crate) fn sha256_hex(value: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in value {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub(crate) fn mail_submission_api_error(error: MailSubmissionError, request_id: &str) -> Response {
    match error {
        MailSubmissionError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_submission",
            "the mail submission request is invalid",
            false,
            request_id,
        ),
        MailSubmissionError::NotFound => resource_not_found(request_id),
        MailSubmissionError::Conflict => api_error(
            StatusCode::CONFLICT,
            "mail_submission_conflict",
            "the draft changed or the idempotency key has another meaning",
            false,
            request_id,
        ),
        MailSubmissionError::AgentAdmission(reason) => {
            agent_mail_admission_api_error(reason, request_id)
        }
        MailSubmissionError::Domain(_) | MailSubmissionError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_submission_unavailable",
            "mail submission is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn agent_mail_sandbox_api_error(
    error: AgentMailSandboxError,
    request_id: &str,
) -> Response {
    match error {
        AgentMailSandboxError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_sandbox_request",
            "the Agent mail sandbox request is invalid",
            false,
            request_id,
        ),
        AgentMailSandboxError::NotFound => resource_not_found(request_id),
        AgentMailSandboxError::Conflict => api_error(
            StatusCode::CONFLICT,
            "agent_mail_sandbox_conflict",
            "the draft changed or the idempotency key has another meaning",
            false,
            request_id,
        ),
        AgentMailSandboxError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_mail_sandbox_unavailable",
            "the Agent mail sandbox is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn agent_mail_admission_api_error(
    reason: AgentMailAdmissionFailure,
    request_id: &str,
) -> Response {
    let (status, code, message, retryable, details, retry_after) = match reason {
        AgentMailAdmissionFailure::RecipientPolicy => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "agent_mail_recipient_policy_denied",
            "one or more recipients are outside the active Agent mail policy",
            false,
            json!({"scope": "recipient_policy"}),
            None,
        ),
        AgentMailAdmissionFailure::HourlyQuota => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_hourly_quota_exceeded",
            "the production Agent hourly mail quota is exhausted",
            true,
            json!({"scope": "agent_hour"}),
            Some(60),
        ),
        AgentMailAdmissionFailure::RecipientDailyQuota => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_recipient_daily_quota_exceeded",
            "a recipient daily mail quota is exhausted",
            true,
            json!({"scope": "recipient_day"}),
            Some(3_600),
        ),
        AgentMailAdmissionFailure::ThreadCircuitBreaker => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_thread_circuit_breaker_open",
            "the production Agent thread circuit breaker is open",
            true,
            json!({"scope": "thread_day"}),
            Some(3_600),
        ),
        AgentMailAdmissionFailure::AutomaticReplyLoop => (
            StatusCode::CONFLICT,
            "agent_mail_automatic_reply_blocked",
            "Agents cannot reply to an automatically submitted message",
            false,
            json!({"scope": "automatic_reply_loop"}),
            None,
        ),
        AgentMailAdmissionFailure::DuplicateContentCircuitBreaker => (
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_duplicate_content_circuit_breaker_open",
            "the Agent duplicate-content circuit breaker is open",
            true,
            json!({"scope": "identical_body_day"}),
            Some(3_600),
        ),
        AgentMailAdmissionFailure::SandboxRequired => (
            StatusCode::CONFLICT,
            "agent_mail_sandbox_required",
            "testing Agents must use the non-delivering mail sandbox",
            false,
            json!({"scope": "testing_environment"}),
            None,
        ),
        AgentMailAdmissionFailure::FirstContactApproval => (
            StatusCode::CONFLICT,
            "agent_first_contact_approval_required",
            "an exact live first-contact approval is required",
            false,
            json!({
                "approval_action": "agent.first_contact",
                "resource_kind": "agent.mail_draft",
                "resource_id_format": "{draft_id}:{expected_draft_version}",
            }),
            None,
        ),
    };
    let mut response =
        api_error_with_details(status, code, message, retryable, details, request_id);
    if let Some(retry_after) = retry_after {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            retry_after.to_string().parse().unwrap(),
        );
    }
    response
}
