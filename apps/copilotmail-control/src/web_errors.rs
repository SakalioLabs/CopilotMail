use crate::*;

pub(crate) fn human_draft_api_error(error: HumanDraftError, request_id: &str) -> Response {
    match error {
        HumanDraftError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "human_draft_forbidden",
            "the Human is not permitted to access drafts",
            false,
            request_id,
        ),
        HumanDraftError::NotFound => resource_not_found(request_id),
        HumanDraftError::InvalidRequest | HumanDraftError::InvalidPageSize => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_request",
            "the Human draft request is invalid",
            false,
            request_id,
        ),
        HumanDraftError::InvalidCursor => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_draft_cursor",
            "the Human draft cursor is invalid",
            false,
            request_id,
        ),
        HumanDraftError::Conflict => api_error(
            StatusCode::CONFLICT,
            "draft_conflict",
            "the draft changed or is no longer editable",
            false,
            request_id,
        ),
        HumanDraftError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_compose_unavailable",
            "mailbox draft storage is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn human_sent_api_error(error: HumanSentError, request_id: &str) -> Response {
    match error {
        HumanSentError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "human_sent_forbidden",
            "the Human is not permitted to access sent submissions",
            false,
            request_id,
        ),
        HumanSentError::NotFound => resource_not_found(request_id),
        HumanSentError::InvalidPageSize => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_sent_request",
            "the Human sent-directory request is invalid",
            false,
            request_id,
        ),
        HumanSentError::InvalidCursor => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_sent_cursor",
            "the Human sent-directory cursor is invalid",
            false,
            request_id,
        ),
        HumanSentError::Conflict => api_error(
            StatusCode::CONFLICT,
            "human_sent_conflict",
            "the sent-directory state or authorization changed",
            false,
            request_id,
        ),
        HumanSentError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_sent_unavailable",
            "the Human sent directory is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn resource_not_found(request_id: &str) -> Response {
    api_error(
        StatusCode::NOT_FOUND,
        "resource_not_found",
        "the requested resource does not exist",
        false,
        request_id,
    )
}

pub(crate) const fn mailbox_kind_wire(kind: MailboxKind) -> &'static str {
    match kind {
        MailboxKind::Human => "human",
        MailboxKind::Agent => "agent",
        MailboxKind::Shared => "shared",
        MailboxKind::Group => "group",
    }
}

pub(crate) fn agent_inspector_api_error(error: AgentInspectorError, request_id: &str) -> Response {
    match error {
        AgentInspectorError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_inspector_request",
            "the Agent Inspector request is invalid",
            false,
            request_id,
        ),
        AgentInspectorError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_inspector_forbidden",
            "an active Human with identity.manage is required to inspect this Agent",
            false,
            request_id,
        ),
        // Target absence, cross-tenant targets, non-Agent principals, and a
        // missing profile intentionally share one indistinguishable response.
        AgentInspectorError::NotFound => resource_not_found(request_id),
        AgentInspectorError::Unavailable | AgentInspectorError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_inspector_unavailable",
            "Agent Inspector state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn agent_mail_submission_evidence_api_error(
    error: AgentMailSubmissionEvidenceError,
    request_id: &str,
) -> Response {
    match error {
        AgentMailSubmissionEvidenceError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_submission_evidence_request",
            "the Agent mail submission evidence request is invalid",
            false,
            request_id,
        ),
        AgentMailSubmissionEvidenceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "agent_mail_submission_evidence_forbidden",
            "an active Human with identity.manage and audit.read is required to inspect Agent submission evidence",
            false,
            request_id,
        ),
        // Missing, cross-tenant, non-Agent, Agent/submission-mismatched, and
        // non-production-admission targets are intentionally indistinguishable.
        AgentMailSubmissionEvidenceError::NotFound => resource_not_found(request_id),
        AgentMailSubmissionEvidenceError::Unavailable
        | AgentMailSubmissionEvidenceError::Store(_) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent_mail_submission_evidence_unavailable",
            "Agent mail submission evidence is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn compliance_api_error(
    error: copilotmail_application::ComplianceGovernanceError,
    request_id: &str,
) -> Response {
    match error {
        copilotmail_application::ComplianceGovernanceError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_compliance_request",
            "the compliance request violates a domain invariant",
            false,
            request_id,
        ),
        copilotmail_application::ComplianceGovernanceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "compliance_operation_forbidden",
            "the compliance operation is not authorized",
            false,
            request_id,
        ),
        copilotmail_application::ComplianceGovernanceError::NotFound => {
            resource_not_found(request_id)
        }
        copilotmail_application::ComplianceGovernanceError::Conflict
        | copilotmail_application::ComplianceGovernanceError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => legal_hold_conflict(request_id),
        copilotmail_application::ComplianceGovernanceError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        copilotmail_application::ComplianceGovernanceError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => compliance_store_unavailable(request_id),
    }
}

pub(crate) fn legal_hold_conflict(request_id: &str) -> Response {
    api_error(
        StatusCode::CONFLICT,
        "legal_hold_conflict",
        "the compliance state, approval, or archive operation conflicts with this request",
        true,
        request_id,
    )
}

pub(crate) fn compliance_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "compliance_store_unavailable",
        "compliance state is temporarily unavailable",
        true,
        request_id,
    )
}

pub(crate) fn legal_hold_object_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "legal_hold_object_store_unavailable",
        "immutable archive verification is temporarily unavailable",
        true,
        request_id,
    )
}

pub(crate) fn webhook_subscription_response(
    subscription: WebhookSubscriptionSummary,
) -> WebhookSubscriptionResponse {
    WebhookSubscriptionResponse {
        subscription_id: subscription.subscription_id.to_string(),
        organization_id: subscription.organization_id.to_string(),
        principal_id: subscription.principal_id.to_string(),
        endpoint_url: subscription.endpoint_url,
        event_types: subscription
            .event_types
            .into_iter()
            .map(|event_type| event_type.as_str().to_owned())
            .collect(),
        signing_key_reference: subscription.signing_key_reference,
        enabled: subscription.enabled,
        version: subscription.version,
        created_at: subscription.created_at.to_rfc3339(),
        updated_at: subscription.updated_at.to_rfc3339(),
    }
}

pub(crate) fn webhook_dead_letter_response(
    dead_letter: WebhookDeadLetterSummary,
) -> WebhookDeadLetterResponse {
    WebhookDeadLetterResponse {
        delivery_id: dead_letter.delivery_id.to_string(),
        subscription_id: dead_letter.subscription_id.to_string(),
        principal_id: dead_letter.principal_id.to_string(),
        event_id: dead_letter.event_id.to_string(),
        event_type: dead_letter.event_type,
        attempt: dead_letter.attempt,
        response_status: dead_letter.response_status,
        error_code: dead_letter.error_code,
        failed_at: dead_letter.failed_at.to_rfc3339(),
    }
}

pub(crate) fn agent_event_envelope_response(event: AgentCursorEvent) -> AgentEventEnvelopeResponse {
    AgentEventEnvelopeResponse {
        id: event.event_id.to_string(),
        organization_id: event.organization_id.to_string(),
        event_type: event.event_type.as_str().into(),
        schema_version: event.schema_version,
        occurred_at: event.occurred_at.to_rfc3339(),
        aggregate: AgentEventAggregateResponse {
            aggregate_type: event.aggregate_type,
            id: event.aggregate_id,
        },
        data: event.payload,
    }
}

pub(crate) fn webhook_subscription_query_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "webhook_subscription_unavailable",
        "webhook subscription state is temporarily unavailable",
        true,
        request_id,
    )
}

pub(crate) fn agent_event_cursor_api_error(
    error: AgentEventCursorError,
    request_id: &str,
) -> Response {
    match error {
        AgentEventCursorError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied Agent event cursor access",
            false,
            request_id,
        ),
        AgentEventCursorError::NotFound => resource_not_found(request_id),
        AgentEventCursorError::InvalidCursor => api_error(
            StatusCode::GONE,
            "event_cursor_expired_or_invalid",
            "the event cursor is unavailable or is not part of this subscription",
            false,
            request_id,
        ),
        AgentEventCursorError::InvalidPageSize
        | AgentEventCursorError::InvalidQuota
        | AgentEventCursorError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_event_cursor_request",
            "the Agent event cursor request is invalid",
            false,
            request_id,
        ),
        AgentEventCursorError::TenantCapacityReached => api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "agent_event_stream_tenant_capacity_reached",
            "the tenant Agent event stream capacity is reached",
            true,
            request_id,
        ),
        AgentEventCursorError::PrincipalCapacityReached => api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "agent_event_stream_principal_capacity_reached",
            "the Agent event stream capacity is reached",
            true,
            request_id,
        ),
        AgentEventCursorError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        AgentEventCursorError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => webhook_subscription_query_unavailable(request_id),
        AgentEventCursorError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "event_cursor_conflict",
            "the subscription event feed changed while the page was read",
            true,
            request_id,
        ),
    }
}

pub(crate) fn webhook_dead_letter_api_error(
    error: ReplayWebhookDeadLetterError,
    request_id: &str,
) -> Response {
    match error {
        ReplayWebhookDeadLetterError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_webhook_replay",
            "the webhook replay request is invalid",
            false,
            request_id,
        ),
        ReplayWebhookDeadLetterError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied manual webhook replay",
            false,
            request_id,
        ),
        ReplayWebhookDeadLetterError::NotReplayable => api_error(
            StatusCode::CONFLICT,
            "webhook_delivery_not_replayable",
            "the delivery is not the latest dead letter for an enabled subscription",
            false,
            request_id,
        ),
        ReplayWebhookDeadLetterError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        ReplayWebhookDeadLetterError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => webhook_subscription_query_unavailable(request_id),
        ReplayWebhookDeadLetterError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "webhook_replay_conflict",
            "the delivery changed before replay could be scheduled",
            true,
            request_id,
        ),
    }
}

pub(crate) fn audit_event_search_api_error(
    error: AuditEventSearchError,
    request_id: &str,
) -> Response {
    match error {
        AuditEventSearchError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "audit_event_search_forbidden",
            "the authorization context denied audit event search",
            false,
            request_id,
        ),
        AuditEventSearchError::InvalidFilter | AuditEventSearchError::Domain(_) => {
            invalid_audit_event_query(request_id)
        }
        AuditEventSearchError::InvalidCursor => api_error(
            StatusCode::GONE,
            "audit_cursor_expired_or_invalid",
            "the audit cursor is unavailable or does not match the active filters",
            false,
            request_id,
        ),
        AuditEventSearchError::Store(copilotmail_application::StoreError::Conflict) => api_error(
            StatusCode::CONFLICT,
            "audit_event_search_conflict",
            "the audit event search conflicted with current state",
            true,
            request_id,
        ),
        AuditEventSearchError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        AuditEventSearchError::Store(
            copilotmail_application::StoreError::MissingReference
            | copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "audit_event_search_unavailable",
            "audit event search is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn webhook_subscription_api_error(
    error: WebhookSubscriptionError,
    request_id: &str,
) -> Response {
    match error {
        WebhookSubscriptionError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_webhook_subscription",
            "the webhook endpoint, event types, key reference, or state transition is invalid",
            false,
            request_id,
        ),
        WebhookSubscriptionError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied the webhook subscription operation",
            false,
            request_id,
        ),
        WebhookSubscriptionError::NotFound
        | WebhookSubscriptionError::Store(copilotmail_application::StoreError::MissingReference) => {
            resource_not_found(request_id)
        }
        WebhookSubscriptionError::Conflict
        | WebhookSubscriptionError::Store(copilotmail_application::StoreError::Conflict) => {
            api_error(
                StatusCode::CONFLICT,
                "webhook_subscription_conflict",
                "the webhook subscription changed concurrently",
                true,
                request_id,
            )
        }
        WebhookSubscriptionError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        WebhookSubscriptionError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => webhook_subscription_query_unavailable(request_id),
    }
}

pub(crate) fn mail_domain_api_error(
    error: MailDomainGovernanceError,
    request_id: &str,
) -> Response {
    match error {
        MailDomainGovernanceError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_mail_domain_operation",
            "the mail domain input or lifecycle transition is invalid",
            false,
            request_id,
        ),
        MailDomainGovernanceError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "permission_denied",
            "the authorization context denied the mail domain operation",
            false,
            request_id,
        ),
        MailDomainGovernanceError::NotFound
        | MailDomainGovernanceError::Store(copilotmail_application::StoreError::MissingReference) => {
            api_error(
                StatusCode::NOT_FOUND,
                "resource_not_found",
                "the requested mail domain does not exist",
                false,
                request_id,
            )
        }
        MailDomainGovernanceError::Conflict
        | MailDomainGovernanceError::Store(copilotmail_application::StoreError::Conflict) => {
            api_error(
                StatusCode::CONFLICT,
                "mail_domain_conflict",
                "the mail domain or route changed concurrently",
                true,
                request_id,
            )
        }
        MailDomainGovernanceError::DnsEvidenceRequired => api_error(
            StatusCode::CONFLICT,
            "dns_evidence_required",
            "fresh complete DNS evidence is required before activation",
            false,
            request_id,
        ),
        MailDomainGovernanceError::ChallengeRotationNotAllowed => api_error(
            StatusCode::CONFLICT,
            "challenge_rotation_not_allowed",
            "active domains must be suspended before challenge rotation; retired domains cannot rotate",
            false,
            request_id,
        ),
        MailDomainGovernanceError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        MailDomainGovernanceError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_domain_unavailable",
            "mail domain state is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn mail_domain_query_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "mail_domain_query_unavailable",
        "mail domain read state is temporarily unavailable",
        true,
        request_id,
    )
}

pub(crate) fn mail_domain_response(domain: MailDomainSummary) -> MailDomainResponse {
    MailDomainResponse {
        mail_domain_id: domain.mail_domain_id.to_string(),
        organization_id: domain.organization_id.to_string(),
        name: domain.name.to_string(),
        status: mail_domain_status_wire(domain.status).into(),
        route_mode: match domain.route_mode {
            MailDomainRouteMode::DirectMx => "direct_mx",
            MailDomainRouteMode::SmartHost => "smart_host",
        }
        .into(),
        ownership_challenge_version: domain.ownership_challenge_version,
        route_version: domain.route_version,
        inbound_enabled: domain.inbound_enabled,
        outbound_enabled: domain.outbound_enabled,
        smart_host_ref: domain.smart_host_ref,
        dkim_key_ref: domain.dkim_key_ref,
        mail_engine_domain_ref: domain.mail_engine_domain_ref,
        mail_engine_projection_status: match domain.mail_engine_projection_status {
            MailEngineProjectionStatus::NotProjected => "not_projected",
            MailEngineProjectionStatus::Pending => "pending",
            MailEngineProjectionStatus::Ready => "ready",
            MailEngineProjectionStatus::Failed => "failed",
        }
        .into(),
        mail_engine_projected_enabled: domain.mail_engine_projected_enabled,
        mail_engine_projection_last_attempt_at: domain
            .mail_engine_projection_last_attempt_at
            .map(|value| value.to_rfc3339()),
        mail_engine_projection_succeeded_at: domain
            .mail_engine_projection_succeeded_at
            .map(|value| value.to_rfc3339()),
        mail_engine_projection_error_code: domain.mail_engine_projection_error_code,
        latest_dns_observed_at: domain
            .latest_dns_observed_at
            .map(|value| value.to_rfc3339()),
        latest_dns_valid_until: domain
            .latest_dns_valid_until
            .map(|value| value.to_rfc3339()),
        latest_dns_activation_ready: domain.latest_dns_activation_ready,
        latest_dns_failure_codes: domain.latest_dns_failure_codes,
    }
}

pub(crate) fn dns_api_error(error: DnsError, request_id: &str) -> Response {
    match error {
        DnsError::InvalidConfiguration
        | DnsError::InvalidOwnershipToken
        | DnsError::InvalidAnswer
        | DnsError::RecordLimitExceeded
        | DnsError::ExpiredAnswer
        | DnsError::InvalidEvidence(_) => api_error(
            StatusCode::BAD_REQUEST,
            "dns_verification_invalid",
            "the DNS verification request or answer is invalid",
            false,
            request_id,
        ),
        DnsError::ResolverInitialization | DnsError::Unavailable | DnsError::Timeout => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "dns_verification_unavailable",
            "DNS verification is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) const fn mail_domain_status_wire(status: MailDomainStatus) -> &'static str {
    match status {
        MailDomainStatus::Draft => "draft",
        MailDomainStatus::PendingDns => "pending_dns",
        MailDomainStatus::Verified => "verified",
        MailDomainStatus::Active => "active",
        MailDomainStatus::Suspended => "suspended",
        MailDomainStatus::Retired => "retired",
    }
}

pub(crate) fn ambiguous_authentication(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "ambiguous_authentication",
        "send either a Human browser session or a bearer credential, not both",
        false,
        request_id,
    )
}

pub(crate) fn authentication_invalid(request_id: &str) -> Response {
    api_error(
        StatusCode::UNAUTHORIZED,
        "authentication_invalid",
        "the request authentication expired or was revoked before the work was accepted; re-authenticate and retry",
        false,
        request_id,
    )
}

pub(crate) fn invalid_access_token(request_id: &str) -> Response {
    api_error(
        StatusCode::UNAUTHORIZED,
        "invalid_access_token",
        "the bearer access token is invalid",
        false,
        request_id,
    )
}

pub(crate) fn credential_selection_api_error(
    error: CredentialSelectionError,
    request_id: &str,
) -> Response {
    match error {
        CredentialSelectionError::Ambiguous => ambiguous_authentication(request_id),
        CredentialSelectionError::InvalidBrowserSession => invalid_human_session_cookie(request_id),
        CredentialSelectionError::InvalidBearer => invalid_access_token(request_id),
    }
}

pub(crate) fn human_client_route_denied(request_id: &str) -> Response {
    api_error(
        StatusCode::FORBIDDEN,
        "human_client_route_denied",
        "the Human browser client is not permitted to access this route",
        false,
        request_id,
    )
}

pub(crate) fn invalid_human_session_cookie(request_id: &str) -> Response {
    let mut response = api_error(
        StatusCode::UNAUTHORIZED,
        "invalid_human_session",
        "the Human browser session is missing, expired, or invalid",
        false,
        request_id,
    );
    response.headers_mut().remove(header::WWW_AUTHENTICATE);
    response
}

pub(crate) fn human_session_api_error(error: HumanSessionError, request_id: &str) -> Response {
    match error {
        HumanSessionError::InvalidRequest => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_human_session_request",
            "the Human login or session request is invalid",
            false,
            request_id,
        ),
        HumanSessionError::NotFound => invalid_human_session_cookie(request_id),
        HumanSessionError::Conflict => api_error(
            StatusCode::CONFLICT,
            "human_session_conflict",
            "the Human login transaction or session was already consumed",
            false,
            request_id,
        ),
        HumanSessionError::RateLimited {
            retry_after_seconds,
        } => {
            let retry_after_seconds = retry_after_seconds.clamp(1, 60);
            let mut response = api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "human_login_rate_limited",
                "too many Human login attempts",
                true,
                request_id,
            );
            if let Ok(value) = header::HeaderValue::from_str(&retry_after_seconds.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            response
        }
        HumanSessionError::Unavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "human_session_unavailable",
            "Human browser session persistence is temporarily unavailable",
            true,
            request_id,
        ),
    }
}

pub(crate) fn oidc_flow_api_error(error: OidcFlowError, request_id: &str) -> Response {
    match error {
        OidcFlowError::ProviderUnavailable | OidcFlowError::EntropyUnavailable => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "oidc_login_unavailable",
            "the enterprise identity provider is temporarily unavailable",
            true,
            request_id,
        ),
        OidcFlowError::InvalidAuthorizationResponse
        | OidcFlowError::AuthorizationRejected
        | OidcFlowError::TokenEndpointRejected
        | OidcFlowError::InvalidTokenResponse
        | OidcFlowError::InvalidAccessToken => {
            let mut response = api_error(
                StatusCode::UNAUTHORIZED,
                "oidc_login_rejected",
                "the enterprise identity provider rejected the login",
                false,
                request_id,
            );
            response.headers_mut().remove(header::WWW_AUTHENTICATE);
            response
        }
    }
}

pub(crate) fn api_error(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
    request_id: &str,
) -> Response {
    api_error_with_details(
        status,
        code,
        message,
        retryable,
        serde_json::Value::Null,
        request_id,
    )
}

pub(crate) fn api_error_with_details(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retryable: bool,
    details: serde_json::Value,
    request_id: &str,
) -> Response {
    let mut response = (
        status,
        Json(ErrorResponse {
            code: code.into(),
            message: message.into(),
            request_id: request_id.into(),
            trace_id: request_id.into(),
            retryable,
            details,
        }),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap());
    }
    response
}
