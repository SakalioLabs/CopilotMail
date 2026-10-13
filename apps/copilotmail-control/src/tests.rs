use super::*;
use axum::{body::Body, http::Request};
use copilotmail_application::MailDraftMetadataStore;
use opentelemetry::{propagation::TextMapPropagator, trace::TraceContextExt};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

#[test]
fn fingerprint_compatibility_is_selected_only_for_the_fixed_legacy_plane() {
    let legacy = MailDataPlaneId::from_uuid(Uuid::from_u128(LEGACY_MAIL_DATA_PLANE_ID));
    let other =
        MailDataPlaneId::from_uuid(Uuid::from_u128(0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2102));

    assert!(matches!(
        mail_event_fingerprint_scheme(legacy).unwrap(),
        MailEventFingerprintScheme::LegacySource(_)
    ));
    assert_eq!(
        mail_event_fingerprint_scheme(other).unwrap(),
        MailEventFingerprintScheme::DataPlaneV2
    );
    assert_eq!(
        mail_data_plane_fingerprint_scheme(legacy),
        MailDataPlaneFingerprintScheme::LegacyV1
    );
    assert_eq!(
        mail_data_plane_fingerprint_scheme(other),
        MailDataPlaneFingerprintScheme::DataPlaneV2
    );
}

#[tokio::test]
async fn shutdown_signal_selector_reports_ctrl_c_without_process_signal() {
    let reason = select_shutdown_reason(
        std::future::ready(Ok(())),
        std::future::pending::<std::io::Result<()>>(),
    )
    .await
    .unwrap();

    assert_eq!(reason, ShutdownReason::CtrlC);
    assert_eq!(reason.as_str(), "ctrl_c");
}

#[tokio::test]
async fn shutdown_signal_selector_reports_sigterm_without_process_signal() {
    let reason = select_shutdown_reason(
        std::future::pending::<std::io::Result<()>>(),
        std::future::ready(Ok(())),
    )
    .await
    .unwrap();

    assert_eq!(reason, ShutdownReason::Terminate);
    assert_eq!(reason.as_str(), "sigterm");
}

#[tokio::test]
async fn pinned_shutdown_signal_survives_a_work_branch_winning_first() {
    let (signal_sender, signal_receiver) = tokio::sync::oneshot::channel::<()>();
    let simulated_ctrl_c = async move {
        signal_receiver.await.map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "simulated signal sender closed",
            )
        })
    };
    let shutdown = select_shutdown_reason(
        simulated_ctrl_c,
        std::future::pending::<std::io::Result<()>>(),
    );
    tokio::pin!(shutdown);

    let work_branch_won = tokio::select! {
        _ = &mut shutdown => false,
        _ = std::future::ready(()) => true,
    };
    assert!(work_branch_won);

    signal_sender.send(()).unwrap();
    let reason = tokio::select! {
        result = &mut shutdown => result.unwrap(),
        _ = tokio::time::sleep(Duration::from_secs(1)) => {
            panic!("pinned shutdown future did not complete in a later select")
        }
    };
    assert_eq!(reason, ShutdownReason::CtrlC);
}

#[tokio::test]
async fn liveness_does_not_depend_on_postgresql() {
    let response = health().await.0;
    assert_eq!(response.status, "ok");
    assert_eq!(response.service, SERVICE);
}

#[tokio::test]
async fn system_info_declares_v2_principal_model() {
    let response = system_info().await.0;
    assert_eq!(response.api_version, "v2");
    assert_eq!(response.principal_model, "human_agent_service");
}

#[test]
fn worker_error_backoff_is_exponential_bounded_and_resettable() {
    assert_eq!(worker_error_backoff(0), Duration::ZERO);
    assert_eq!(worker_error_backoff(1), Duration::from_secs(1));
    assert_eq!(worker_error_backoff(2), Duration::from_secs(2));
    assert_eq!(worker_error_backoff(5), Duration::from_secs(16));
    assert_eq!(worker_error_backoff(6), WORKER_ERROR_BACKOFF_MAX);
    assert_eq!(worker_error_backoff(u32::MAX), WORKER_ERROR_BACKOFF_MAX);
}

#[test]
fn pkce_protection_is_authenticated_and_state_bound() {
    let protector = PkceProtector::new(&[7; 32]).unwrap();
    let verifier = "v".repeat(64);
    let state_sha256 = [3; 32];
    let protected = protector.protect(&verifier, &state_sha256).unwrap();
    assert_ne!(&protected[12..protected.len() - 16], verifier.as_bytes());
    assert_eq!(
        protector.unprotect(&protected, &state_sha256).unwrap(),
        verifier
    );

    let mut tampered = protected.clone();
    tampered[20] ^= 1;
    assert!(protector.unprotect(&tampered, &state_sha256).is_err());
    assert!(protector.unprotect(&protected, &[4; 32]).is_err());
}

#[test]
fn browser_cookies_are_host_only_http_only_and_ambiguity_rejecting() {
    let token = URL_SAFE_NO_PAD.encode([9; 32]);
    assert_eq!(
        human_login_cookie_name(HumanSessionClient::Admin, true),
        "__Host-copilotmail-admin-login"
    );
    assert_eq!(
        human_session_cookie_name(HumanSessionClient::Mail, false),
        "copilotmail-mail-session"
    );
    let session_name = human_session_cookie_name(HumanSessionClient::Admin, true);
    let session_cookie = browser_cookie_header(session_name, &token, 900, true).unwrap();
    let session_cookie = session_cookie.to_str().unwrap();
    assert!(session_cookie.contains("; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=900"));
    assert!(!session_cookie.to_ascii_lowercase().contains("domain="));

    let login_name = human_login_cookie_name(HumanSessionClient::Admin, true);
    let login_cookie = browser_cookie_header(login_name, &token, 600, true).unwrap();
    let login_cookie = login_cookie.to_str().unwrap();
    assert!(login_cookie.contains("; Secure; SameSite=Lax; Max-Age=600"));

    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("theme=dark; {session_name}={token}")
            .parse()
            .unwrap(),
    );
    assert_eq!(
        browser_cookie_value(&headers, session_name).unwrap(),
        Some(token.clone())
    );
    headers.append(
        header::COOKIE,
        format!("{session_name}={token}").parse().unwrap(),
    );
    assert!(browser_cookie_value(&headers, session_name).is_err());
}

#[test]
fn human_session_cookies_and_routes_are_strictly_client_partitioned() {
    let admin_token = URL_SAFE_NO_PAD.encode([7; 32]);
    let mail_token = URL_SAFE_NO_PAD.encode([8; 32]);
    let admin_name = human_session_cookie_name(HumanSessionClient::Admin, true);
    let mail_name = human_session_cookie_name(HumanSessionClient::Mail, true);

    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        format!("{admin_name}={admin_token}").parse().unwrap(),
    );
    assert!(matches!(
        select_presented_credential(&headers, Some(true)),
        Ok(PresentedCredential::BrowserSession {
            client: HumanSessionClient::Admin,
            ..
        })
    ));

    headers.insert(
        header::COOKIE,
        format!("{admin_name}={admin_token}; {mail_name}={mail_token}")
            .parse()
            .unwrap(),
    );
    assert!(matches!(
        select_presented_credential(&headers, Some(true)),
        Err(CredentialSelectionError::Ambiguous)
    ));

    let mail_routes = [
        (Method::GET, "/api/v2/mailboxes"),
        (Method::GET, "/api/v2/draft-attempts"),
        (Method::HEAD, "/api/v2/draft-attempts"),
        (Method::GET, "/api/v2/mailboxes/m1/messages"),
        (Method::POST, "/api/v2/mailboxes/m1/messages/search"),
        (Method::GET, "/api/v2/mailboxes/m1/messages/msg1"),
        (Method::PATCH, "/api/v2/mailboxes/m1/messages/msg1"),
        (Method::GET, "/api/v2/mailboxes/m1/drafts"),
        (Method::POST, "/api/v2/mailboxes/m1/drafts"),
        (Method::POST, "/api/v2/mailboxes/m1/drafts/d1/restore"),
        (Method::GET, "/api/v2/mailboxes/m1/submissions"),
        (Method::HEAD, "/api/v2/mailboxes/m1/submissions"),
        (Method::POST, "/api/v2/mailboxes/m1/submissions"),
        (Method::GET, "/api/v2/mail-submissions/s1"),
    ];
    for (method, path) in mail_routes {
        assert!(human_client_route_permitted(
            HumanSessionClient::Mail,
            &method,
            path
        ));
        assert!(
            !human_client_route_permitted(HumanSessionClient::Admin, &method, path),
            "Admin browser sessions must not read mail content or compose mail"
        );
    }

    let admin_management_route = "/api/v2/organizations/o1/role-assignments";
    assert!(human_client_route_permitted(
        HumanSessionClient::Admin,
        &Method::POST,
        admin_management_route
    ));
    assert!(
        !human_client_route_permitted(
            HumanSessionClient::Mail,
            &Method::POST,
            admin_management_route
        ),
        "the client boundary runs before and cannot be overridden by admin RBAC"
    );
    assert!(human_client_route_permitted(
        HumanSessionClient::Admin,
        &Method::GET,
        "/api/v2/auth/admin/session"
    ));
    assert!(!human_client_route_permitted(
        HumanSessionClient::Mail,
        &Method::GET,
        "/api/v2/auth/admin/session"
    ));
    assert!(human_client_route_permitted(
        HumanSessionClient::Mail,
        &Method::POST,
        "/api/v2/auth/mail/logout"
    ));
    assert!(human_client_route_permitted(
        HumanSessionClient::Mail,
        &Method::GET,
        "/api/v2/auth/mail/sessions"
    ));
    assert!(!human_client_route_permitted(
        HumanSessionClient::Admin,
        &Method::GET,
        "/api/v2/auth/mail/sessions"
    ));
    assert!(human_client_route_permitted(
        HumanSessionClient::Admin,
        &Method::GET,
        "/api/v2/auth/admin/sessions"
    ));
    assert!(!human_client_route_permitted(
        HumanSessionClient::Mail,
        &Method::GET,
        "/api/v2/auth/admin/sessions"
    ));
    assert!(human_client_route_permitted(
        HumanSessionClient::Admin,
        &Method::POST,
        "/api/v2/auth/admin/logout-all"
    ));
    assert!(!human_client_route_permitted(
        HumanSessionClient::Mail,
        &Method::POST,
        "/api/v2/auth/admin/logout-all"
    ));
    assert!(human_client_route_permitted(
        HumanSessionClient::Mail,
        &Method::POST,
        "/api/v2/auth/mail/logout-all"
    ));
    assert!(!human_client_route_permitted(
        HumanSessionClient::Admin,
        &Method::POST,
        "/api/v2/auth/mail/logout-all"
    ));
    let revocation_route = format!(
        "/api/v2/organizations/{}/principals/{}/browser-session-revocations",
        OrganizationId::new(),
        PrincipalId::new()
    );
    assert!(human_client_route_permitted(
        HumanSessionClient::Admin,
        &Method::POST,
        &revocation_route
    ));
    assert!(
        !human_client_route_permitted(HumanSessionClient::Mail, &Method::POST, &revocation_route),
        "Mail browser sessions cannot revoke another principal"
    );
    assert!(!human_client_route_permitted(
        HumanSessionClient::Mail,
        &Method::GET,
        "/api/v2/mailboxes/m1/messages/"
    ));

    let mut admin_headers = HeaderMap::new();
    admin_headers.insert(
        header::COOKIE,
        format!("{admin_name}={admin_token}").parse().unwrap(),
    );
    let denied = require_human_mail_browser_session(&admin_headers, true, "request-1").unwrap_err();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);

    let mut mail_headers = HeaderMap::new();
    mail_headers.insert(
        header::COOKIE,
        format!("{mail_name}={mail_token}").parse().unwrap(),
    );
    assert!(require_human_mail_browser_session(&mail_headers, true, "request-1").is_ok());

    let mut bearer_headers = HeaderMap::new();
    bearer_headers.insert(header::AUTHORIZATION, "Bearer agent-token".parse().unwrap());
    let denied =
        require_human_mail_browser_session(&bearer_headers, true, "request-1").unwrap_err();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert!(!denied.headers().contains_key(header::WWW_AUTHENTICATE));
}

#[test]
fn successful_login_sets_active_session_and_clears_the_other_client_cookie() {
    let token = URL_SAFE_NO_PAD.encode([7; 32]);
    let [active_cookie, cleared_cookie] =
        browser_session_cookie_headers(HumanSessionClient::Admin, &token, 900, true).unwrap();
    let mut headers = HeaderMap::new();
    headers.append(header::SET_COOKIE, active_cookie);
    headers.append(header::SET_COOKIE, cleared_cookie);

    let cookies = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(cookies.len(), 2);
    assert!(cookies[0].starts_with(&format!("__Host-copilotmail-admin-session={token};")));
    assert!(cookies[0].contains("SameSite=Strict; Max-Age=900"));
    assert!(cookies[1].starts_with("__Host-copilotmail-mail-session=;"));
    assert!(cookies[1].contains("SameSite=Strict; Max-Age=0"));
}

#[tokio::test]
async fn human_session_routes_fail_closed_without_configuration_or_storage_access() {
    let session = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri("/api/v2/auth/admin/session")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(session.status(), StatusCode::UNAUTHORIZED);
    assert!(!session.headers().contains_key(header::WWW_AUTHENTICATE));
    assert_eq!(session.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(session.headers()["referrer-policy"], "no-referrer");

    let login = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri("/api/v2/auth/admin/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(login.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(login.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(login.headers()["referrer-policy"], "no-referrer");

    let logout = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v2/auth/admin/logout")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::UNAUTHORIZED);
    assert!(!logout.headers().contains_key(header::WWW_AUTHENTICATE));
    assert_eq!(logout.headers()[header::CACHE_CONTROL], "no-store");

    for uri in [
        "/api/v2/auth/admin/logout-all",
        "/api/v2/auth/mail/logout-all",
    ] {
        let logout_all = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logout_all.status(), StatusCode::UNAUTHORIZED);
        assert!(!logout_all.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(logout_all.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(logout_all.headers()["referrer-policy"], "no-referrer");
    }

    let missing_target = format!(
        "/api/v2/organizations/{}/principals/{}/browser-session-revocations",
        OrganizationId::new(),
        PrincipalId::new()
    );
    let foreign_target = format!(
        "/api/v2/organizations/{}/principals/{}/browser-session-revocations",
        OrganizationId::new(),
        PrincipalId::new()
    );
    let mut revocation_errors = Vec::new();
    for uri in [missing_target, foreign_target] {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
        assert!(!error.message.contains("revoked_session_count"));
        revocation_errors.push((error.code, error.message, error.retryable));
    }
    assert_eq!(revocation_errors[0], revocation_errors[1]);
}

#[test]
fn human_login_rate_limit_response_is_bounded_and_retryable() {
    let response = human_session_api_error(
        HumanSessionError::RateLimited {
            retry_after_seconds: 600,
        },
        "request-1",
    );
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "60");
}

#[test]
fn delivery_response_exposes_counts_without_recipient_addresses_or_engine_ids() {
    let response = mail_delivery_response(copilotmail_application::MailDeliverySnapshotRecord {
        snapshot_id: Uuid::now_v7(),
        organization_id: OrganizationId::new(),
        submission_id: MailSubmissionId::new(),
        source_state: "private-engine-state".into(),
        source_sha256: [7; 32],
        recipients: vec![
            copilotmail_application::MailDeliveryRecipientWrite {
                recipient_ref: [1; 32],
                state: MailDeliveryRecipientState::Delivered,
                smtp_reply: "250 2.0.0 Accepted".into(),
            },
            copilotmail_application::MailDeliveryRecipientWrite {
                recipient_ref: [2; 32],
                state: MailDeliveryRecipientState::PermanentFailure,
                smtp_reply: "550 5.1.1 Unknown".into(),
            },
        ],
        terminal: true,
        observed_at: Utc::now(),
    });
    assert_eq!(response.recipient_count, 2);
    assert_eq!(response.delivered_count, 1);
    assert_eq!(response.failed_count, 1);
    assert_eq!(response.recipients[0].recipient_ref.len(), 64);
    let serialized = serde_json::to_string(&response).unwrap();
    assert!(!serialized.contains('@'));
    assert!(!serialized.contains("private-engine-state"));
}

#[test]
fn request_headers_accept_only_valid_w3c_remote_trace_context() {
    let propagator = TraceContextPropagator::new();
    let mut headers = HeaderMap::new();
    headers.insert(
        "traceparent",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
            .parse()
            .unwrap(),
    );
    let context = propagator.extract(&RequestHeaderExtractor(&headers));
    let span = context.span();
    let span_context = span.span_context();
    assert!(span_context.is_valid());
    assert!(span_context.is_remote());
    assert_eq!(
        span_context.trace_id().to_string(),
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );

    headers.insert("traceparent", "not-a-trace-context".parse().unwrap());
    let invalid = propagator.extract(&RequestHeaderExtractor(&headers));
    assert!(!invalid.span().span_context().is_valid());
}

#[test]
fn legal_hold_approval_uses_the_dedicated_permission() {
    assert_eq!(
        approval_request_permission(ApprovalAction::LegalHold),
        Permission::LegalHoldManage
    );
}

#[test]
fn compliance_cursors_are_versioned_kind_and_tenant_scoped() {
    let organization_id = OrganizationId::new();
    let id = Uuid::now_v7();
    let cursor = encode_compliance_cursor("case", organization_id, id);
    assert_eq!(
        decode_compliance_cursor(&cursor, "case", organization_id),
        Ok(id)
    );
    assert!(decode_compliance_cursor(&cursor, "hold", organization_id).is_err());
    assert!(decode_compliance_cursor(&cursor, "case", OrganizationId::new()).is_err());
    assert!(decode_compliance_cursor(&format!("{cursor}.extra"), "case", organization_id).is_err());
    assert_eq!(bounded_compliance_limit(None), Some(50));
    assert_eq!(bounded_compliance_limit(Some(100)), Some(100));
    assert_eq!(bounded_compliance_limit(Some(101)), None);
    assert_eq!(
        parse_compliance_case_filter(Some("open")),
        Some(Some(ComplianceCaseStatus::Open))
    );
    assert_eq!(parse_legal_hold_filter(Some("unknown")), None);
}

#[test]
fn approval_cursors_bind_tenant_view_and_status() {
    let organization_id = OrganizationId::new();
    let id = Uuid::now_v7();
    let cursor = encode_approval_cursor(
        organization_id,
        ApprovalRequestView::Requested,
        Some(ApprovalStatus::Pending),
        id,
    );
    assert_eq!(
        decode_approval_cursor(
            &cursor,
            organization_id,
            ApprovalRequestView::Requested,
            Some(ApprovalStatus::Pending),
        ),
        Ok(id)
    );
    assert!(
        decode_approval_cursor(
            &cursor,
            organization_id,
            ApprovalRequestView::Actionable,
            Some(ApprovalStatus::Pending),
        )
        .is_err()
    );
    assert!(
        decode_approval_cursor(
            &cursor,
            organization_id,
            ApprovalRequestView::Requested,
            Some(ApprovalStatus::Approved),
        )
        .is_err()
    );
    assert!(
        decode_approval_cursor(
            &cursor,
            OrganizationId::new(),
            ApprovalRequestView::Requested,
            Some(ApprovalStatus::Pending),
        )
        .is_err()
    );
}

#[test]
fn role_assignment_cursors_bind_tenant_principal_and_activity_filter() {
    let organization_id = OrganizationId::new();
    let principal_id = PrincipalId::new();
    let assignment_id = RoleAssignmentId::new();
    let cursor = encode_role_assignment_cursor(
        organization_id,
        Some(principal_id),
        false,
        assignment_id.as_uuid(),
    );
    assert_eq!(
        decode_role_assignment_cursor(&cursor, organization_id, Some(principal_id), false,),
        Ok(assignment_id.as_uuid())
    );
    assert!(decode_role_assignment_cursor(&cursor, organization_id, None, false).is_err());
    assert!(
        decode_role_assignment_cursor(&cursor, organization_id, Some(principal_id), true,).is_err()
    );
    assert!(
        decode_role_assignment_cursor(&cursor, OrganizationId::new(), Some(principal_id), false,)
            .is_err()
    );
}

#[tokio::test]
async fn errors_include_and_propagate_request_id() {
    let request_id_header = HeaderName::from_static("x-request-id");
    let app = Router::new()
        .fallback(not_found)
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/missing")
                .header("x-request-id", "test-request-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()["x-request-id"], "test-request-123");
    let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let error: copilotmail_contracts::ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "route_not_found");
    assert_eq!(error.request_id, "test-request-123");
    assert_eq!(error.trace_id, "test-request-123");
    assert!(!error.retryable);
}

#[test]
fn bearer_parser_rejects_ambiguous_or_malformed_credentials() {
    let mut headers = HeaderMap::new();
    assert_eq!(bearer_token(&headers), Ok(None));
    headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
    assert!(bearer_token(&headers).is_err());
    headers.insert(header::AUTHORIZATION, "Bearer ".parse().unwrap());
    assert!(bearer_token(&headers).is_err());
    headers.insert(
        header::AUTHORIZATION,
        "Bearer token with spaces".parse().unwrap(),
    );
    assert!(bearer_token(&headers).is_err());
    headers.insert(
        header::AUTHORIZATION,
        "bEaReR compact.token".parse().unwrap(),
    );
    assert_eq!(bearer_token(&headers), Ok(Some("compact.token")));
    headers.append(
        header::AUTHORIZATION,
        "Bearer second.token".parse().unwrap(),
    );
    assert!(bearer_token(&headers).is_err());
}

#[test]
fn any_authorization_header_with_a_session_cookie_is_ambiguous() {
    let token = URL_SAFE_NO_PAD.encode([5; 32]);
    let cookie_name = human_session_cookie_name(HumanSessionClient::Mail, true);
    for authorization in [
        "Bearer valid.token",
        "bearer lower.case",
        "Bearer ",
        "Basic client-credential",
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("{cookie_name}={token}").parse().unwrap(),
        );
        headers.insert(header::AUTHORIZATION, authorization.parse().unwrap());
        assert!(matches!(
            select_presented_credential(&headers, Some(true)),
            Err(CredentialSelectionError::Ambiguous)
        ));
    }

    let mut duplicate = HeaderMap::new();
    duplicate.insert(
        header::COOKIE,
        format!("{cookie_name}={token}").parse().unwrap(),
    );
    duplicate.append(header::AUTHORIZATION, "Bearer first".parse().unwrap());
    duplicate.append(header::AUTHORIZATION, "Basic second".parse().unwrap());
    assert!(matches!(
        select_presented_credential(&duplicate, Some(true)),
        Err(CredentialSelectionError::Ambiguous)
    ));

    let mut malformed_without_cookie = HeaderMap::new();
    malformed_without_cookie.insert(header::AUTHORIZATION, "Basic only".parse().unwrap());
    assert!(matches!(
        select_presented_credential(&malformed_without_cookie, Some(true)),
        Err(CredentialSelectionError::InvalidBearer)
    ));
}

#[test]
fn mail_engine_projection_errors_use_stable_non_secret_codes() {
    let cases = [
        (
            MailEngineError::InvalidConfiguration,
            "mail_engine_invalid_configuration",
        ),
        (
            MailEngineError::CredentialUnavailable,
            "mail_engine_credential_unavailable",
        ),
        (MailEngineError::Unavailable, "mail_engine_unavailable"),
        (
            MailEngineError::InvalidSession,
            "mail_engine_invalid_session",
        ),
        (
            MailEngineError::InvalidResponse,
            "mail_engine_invalid_response",
        ),
        (MailEngineError::Rejected, "mail_engine_rejected"),
        (
            MailEngineError::ProjectionConflict,
            "mail_engine_projection_conflict",
        ),
    ];
    for (error, expected) in cases {
        let code = mail_engine_error_code(error);
        assert_eq!(code, expected);
        assert!(code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        }));
    }
}

#[test]
fn webhook_retry_delay_is_exponential_and_bounded() {
    assert_eq!(webhook_retry_delay(0), chrono::Duration::seconds(5));
    assert_eq!(webhook_retry_delay(1), chrono::Duration::seconds(10));
    assert_eq!(webhook_retry_delay(8), chrono::Duration::seconds(1_280));
    assert_eq!(webhook_retry_delay(100), chrono::Duration::seconds(1_280));
}

fn disconnected_store() -> PgControlPlaneStore {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    PgControlPlaneStore::from_pool(pool)
}

fn router_with_test_ingress(settings: MailEventIngressSettings) -> Router {
    router_with_state(
        AppState {
            store: disconnected_store(),
            authenticator: Arc::new(UnconfiguredAuthenticator),
            authentication_ready: false,
            human_auth_required: false,
            human_session: None,
            human_session_required: false,
            agent_token_service: None,
            agent_authentication_ready: false,
            agent_auth_required: false,
            agent_event_stream_tenant_limit: 100,
            agent_event_stream_principal_limit: 5,
            agent_ingress_tenant_requests_per_minute: 6_000,
            agent_ingress_principal_requests_per_minute: 600,
            agent_event_stream_metrics: AgentEventStreamMetrics::default(),
            mail_data_plane_id: Some(MailDataPlaneId::from_uuid(Uuid::from_u128(
                0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2101,
            ))),
            mail_engine: None,
            mail_engine_required: false,
            mail_content: None,
            mail_content_required: false,
            mail_drafts: None,
            mail_draft_content: None,
            mail_drafts_required: false,
            mail_event_ingress: Some(settings),
            mail_event_ingress_required: true,
            audit_archive: None,
        },
        None,
    )
}

/// A deliberately narrow authenticated HTTP fixture. It is only used by
/// the ignored PostgreSQL acceptance test below: production authenticators
/// remain the OIDC and Agent OAuth adapters configured by the process.
#[derive(Clone)]
struct InspectorHttpTestAuthenticator {
    identities: BTreeMap<&'static str, AuthenticatedIdentity>,
}

#[async_trait]
impl AccessTokenAuthenticator for InspectorHttpTestAuthenticator {
    async fn authenticate(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedIdentity, AuthenticationError> {
        self.identities
            .get(bearer_token)
            .cloned()
            .ok_or(AuthenticationError::InvalidCredential)
    }
}

struct AgentInspectorHttpFixture {
    store: PgControlPlaneStore,
    organization_id: OrganizationId,
    owner_id: PrincipalId,
    owner_admin_session_token: String,
    agent_id: PrincipalId,
    unprivileged_human_admin_session_token: String,
    foreign_agent_id: PrincipalId,
}

struct AgentMailSubmissionEvidenceHttpFixture {
    store: PgControlPlaneStore,
    organization_id: OrganizationId,
    owner_id: PrincipalId,
    owner_admin_session_token: String,
    agent_id: PrincipalId,
    submission_id: MailSubmissionId,
    identity_only_admin_session_token: String,
    audit_only_admin_session_token: String,
    foreign_agent_id: PrincipalId,
}

async fn migrated_inspector_http_store() -> PgControlPlaneStore {
    assert_eq!(
        std::env::var("COPILOTMAIL_ALLOW_AGENT_INSPECTOR_HTTP_TEST_WRITES").unwrap_or_default(),
        "1",
        "refusing Agent Inspector HTTP fixture writes without explicit disposable-database opt-in"
    );
    let database_url =
        std::env::var("COPILOTMAIL_DATABASE_URL").expect("COPILOTMAIL_DATABASE_URL is required");
    let store = PgControlPlaneStore::connect(&database_url, 5)
        .await
        .expect("connect Agent Inspector HTTP integration database");
    store
        .migrate()
        .await
        .expect("apply Agent Inspector HTTP integration migrations");
    store
}

fn inspector_bootstrap_command(slug: String, now: DateTime<Utc>) -> BootstrapOrganizationCommand {
    BootstrapOrganizationCommand {
        slug,
        display_name: "Agent Inspector HTTP Integration Organization".into(),
        root_principal_name: "Agent Inspector HTTP Bootstrap".into(),
        trace_id: Uuid::now_v7().to_string(),
        now,
    }
}

async fn bind_inspector_http_owner(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    label: &str,
    now: DateTime<Utc>,
) -> PrincipalId {
    BindOidcOwnerService::new(store.clone())
        .execute(BindOidcOwnerCommand {
            organization_id,
            issuer: format!("https://inspector-{}.example.test", Uuid::new_v4().simple()),
            subject: format!("{label}-{}", Uuid::new_v4().simple()),
            display_name: format!("{label} Owner"),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("bind Agent Inspector HTTP owner")
        .principal_id
}

async fn insert_inspector_http_agent(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    owner_id: PrincipalId,
    display_name: &str,
    now: DateTime<Utc>,
    assign_agent_operator_role: bool,
) -> PrincipalId {
    let agent_id = PrincipalId::new();
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        )
        VALUES ($1, $2, 'agent', 'active', $3, $4, $4)
        "#,
    )
    .bind(agent_id.as_uuid())
    .bind(organization_id.as_uuid())
    .bind(display_name)
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert Agent Inspector HTTP target principal");
    sqlx::query(
        r#"
        INSERT INTO agent_profiles(
            principal_id, owner_principal_id, environment, risk_level, purpose, review_due_at
        )
        VALUES ($1, $2, 'testing', 'medium',
                'Verify the authenticated Agent Inspector HTTP boundary', $3)
        "#,
    )
    .bind(agent_id.as_uuid())
    .bind(owner_id.as_uuid())
    .bind(now + chrono::Duration::days(30))
    .execute(store.pool())
    .await
    .expect("insert Agent Inspector HTTP target profile");

    if assign_agent_operator_role {
        let assignment = sqlx::query(
            r#"
            INSERT INTO role_assignments(
                id, organization_id, principal_id, role_id,
                created_by, valid_from, valid_until, created_at
            )
            SELECT $1, $2, $3, role.id, $4, $5, NULL, $5
              FROM roles AS role
             WHERE role.organization_id = $2
               AND role.system_key = 'agent_operator'
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(organization_id.as_uuid())
        .bind(agent_id.as_uuid())
        .bind(owner_id.as_uuid())
        .bind(now)
        .execute(store.pool())
        .await
        .expect("assign Agent Inspector HTTP target the machine runtime role");
        assert_eq!(assignment.rows_affected(), 1);
    }
    agent_id
}

async fn insert_inspector_http_unprivileged_human(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    owner_id: PrincipalId,
    now: DateTime<Utc>,
) -> PrincipalId {
    let principal_id = PrincipalId::new();
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        )
        VALUES ($1, $2, 'human', 'active', 'Agent Inspector HTTP Employee', $3, $3)
        "#,
    )
    .bind(principal_id.as_uuid())
    .bind(organization_id.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert unprivileged Agent Inspector HTTP Human");
    sqlx::query(
        r#"
        INSERT INTO human_profiles(principal_id, identity_provider, identity_subject)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(principal_id.as_uuid())
    .bind(format!(
        "https://inspector-{}.example.test",
        Uuid::new_v4().simple()
    ))
    .bind(format!("employee-{}", Uuid::new_v4().simple()))
    .execute(store.pool())
    .await
    .expect("insert unprivileged Agent Inspector HTTP Human profile");
    let assignment = sqlx::query(
        r#"
        INSERT INTO role_assignments(
            id, organization_id, principal_id, role_id,
            created_by, valid_from, valid_until, created_at
        )
        SELECT $1, $2, $3, role.id, $4, $5, NULL, $5
          FROM roles AS role
         WHERE role.organization_id = $2
           AND role.system_key = 'employee'
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(organization_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(owner_id.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("assign unprivileged Agent Inspector HTTP Human the employee role");
    assert_eq!(assignment.rows_affected(), 1);
    principal_id
}

async fn grant_inspector_http_test_role(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    owner_id: PrincipalId,
    system_key: &str,
    now: DateTime<Utc>,
) {
    let assignment = sqlx::query(
        r#"
        INSERT INTO role_assignments(
            id, organization_id, principal_id, role_id,
            created_by, valid_from, valid_until, created_at
        )
        SELECT $1, $2, $3, role.id, $4, $5, NULL, $5
          FROM roles AS role
         WHERE role.organization_id = $2
           AND role.system_key = $6
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(organization_id.as_uuid())
    .bind(principal_id.as_uuid())
    .bind(owner_id.as_uuid())
    .bind(now)
    .bind(system_key)
    .execute(store.pool())
    .await
    .expect("assign Agent evidence HTTP test role");
    assert_eq!(
        assignment.rows_affected(),
        1,
        "role {system_key} must exist once"
    );
}

async fn insert_agent_mail_evidence_http_target(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    owner_id: PrincipalId,
    now: DateTime<Utc>,
) -> PrincipalId {
    let agent_id = PrincipalId::new();
    sqlx::query(
        r#"
        INSERT INTO principals(
            id, organization_id, kind, status, display_name, created_at, updated_at
        )
        VALUES ($1, $2, 'agent', 'active', 'Production Agent evidence HTTP target', $3, $3)
        "#,
    )
    .bind(agent_id.as_uuid())
    .bind(organization_id.as_uuid())
    .bind(now)
    .execute(store.pool())
    .await
    .expect("insert production Agent evidence HTTP target");
    sqlx::query(
        r#"
        INSERT INTO agent_profiles(
            principal_id, owner_principal_id, environment, risk_level, purpose, review_due_at
        )
        VALUES ($1, $2, 'production', 'high',
                'Verify the production Agent submission-evidence HTTP boundary', $3)
        "#,
    )
    .bind(agent_id.as_uuid())
    .bind(owner_id.as_uuid())
    .bind(now + chrono::Duration::days(30))
    .execute(store.pool())
    .await
    .expect("insert production Agent evidence HTTP profile");
    grant_inspector_http_test_role(
        store,
        organization_id,
        agent_id,
        owner_id,
        "agent_operator",
        now,
    )
    .await;
    agent_id
}

/// Creates the same persisted browser-session evidence that the BFF
/// resolves in production. The fixture does not impersonate an OIDC
/// provider: OIDC validation belongs to the login endpoint, while this
/// acceptance test exercises the already-established Admin session cookie
/// boundary through the public session service.
async fn create_inspector_http_admin_session(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    principal_id: PrincipalId,
    now: DateTime<Utc>,
) -> String {
    let login_now = now;
    let state = format!("state-{}", Uuid::new_v4().simple());
    let browser_nonce = format!("nonce-{}", Uuid::new_v4().simple());
    let session_service = HumanSessionService::new(store.clone());
    session_service
        .begin_login(BeginHumanLoginCommand {
            client_key: HumanSessionClient::Admin,
            return_to: "/".into(),
            state: state.clone(),
            browser_nonce: browser_nonce.clone(),
            encrypted_pkce_verifier: vec![0x5a; 96],
            admission_limit: 100_000,
            cleanup_limit: 100,
            now: login_now,
            expires_at: login_now + chrono::Duration::minutes(5),
        })
        .await
        .expect("begin Agent Inspector HTTP Admin browser login");
    let consumed = session_service
        .consume_login(ConsumeHumanLoginCommand {
            state,
            browser_nonce,
            now: login_now + chrono::Duration::seconds(1),
        })
        .await
        .expect("consume Agent Inspector HTTP Admin browser login");
    let session_now = login_now + chrono::Duration::seconds(2);
    // The cookie parser accepts only the production shape: 32 bytes encoded
    // as 43 unpadded URL-safe characters. A longer label is rejected as an
    // invalid session before RBAC or CSRF.
    let session_token = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    session_service
        .create_session(CreateHumanSessionCommand {
            organization_id,
            principal_id,
            login_transaction_id: consumed.login_transaction_id,
            client_key: consumed.client_key,
            session_token: session_token.clone(),
            oidc_access_token: format!("header.{}.signature", Uuid::new_v4().simple()),
            oidc_jti: None,
            upstream_expires_at: session_now + chrono::Duration::hours(2),
            expires_at: session_now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now: session_now,
        })
        .await
        .expect("create Agent Inspector HTTP Admin browser session");
    session_token
}

fn inspector_http_admin_session_runtime() -> HumanSessionRuntime {
    HumanSessionRuntime {
        // Inspector GET does not initiate OIDC or issue a callback, so a
        // real OIDC RP would be unrelated external test infrastructure.
        // The configured-session allowlist is enough for the persisted
        // cookie resolver and is derived from the same production config.
        clients: Arc::new(BTreeMap::new()),
        session_clients: Arc::new(vec![HumanSessionClient::Admin]),
        protector: PkceProtector::new(&[0x3a; 32])
            .expect("create Agent Inspector HTTP session protector"),
        failure_source_hasher: LoginFailureSourceHasher::new(&[0x3a; 32])
            .expect("create Agent Inspector HTTP login failure hasher"),
        maximum_lifetime: std::time::Duration::from_secs(60 * 60),
        login_requests_per_minute: 100,
        secure_cookie: false,
    }
}

const AGENT_EVIDENCE_ADMIN_ORIGIN: &str = "http://admin-agent-evidence.example.test";
const AGENT_EVIDENCE_CSRF_TEST_MODULUS: &str = "rGT458ynts63IDw-ypZps1tzC-T_nawKfmonIyeUpdw6zJDJYN3G2W1fxbr7fhRFtDlRRBTRHMIl5BiTZo68UrP4aa-eBUD_IMZDCEfIp6hNJI4MNjytRr3VM1rN7DGYZ14TSottWcYBsuapuDPJCgj0Wut_3iNUzXs4LrSUQI0UCbFKXR9eudvNjwYbQbjv8FjiCQRh7ylIzuzh93l_oFXLBDCYe5I9yu_XzZ68eAkdthoD2qaB_xOthCT8AjWm4eTKE3_de_XeBw2azDJaO_gDwjDhBHQRa9ACole_ZWZ0WRJZ6dDd5fvzM8onzqCvN7h6d9BRoSRSI1--R8Z6wQ";

/// The unsafe-method CSRF middleware requires a configured browser
/// client, including its exact public origin. Build one from a loopback
/// discovery document so the test exercises the production runtime shape
/// without reaching an external identity provider.
async fn agent_evidence_csrf_admin_session_runtime(
    store: PgControlPlaneStore,
) -> (HumanSessionRuntime, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback OIDC discovery provider for CSRF test");
    let issuer = format!(
        "http://{}",
        listener.local_addr().expect("read OIDC listener")
    );
    let discovery = serde_json::json!({
        "issuer": issuer,
        "jwks_uri": format!("{issuer}/jwks"),
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "authorization_response_iss_parameter_supported": true,
        "id_token_signing_alg_values_supported": ["RS256"]
    });
    let jwks = serde_json::json!({
        "keys": [{
            "kty": "RSA",
            "kid": "agent-evidence-csrf-test-key",
            "use": "sig",
            "alg": "RS256",
            "n": AGENT_EVIDENCE_CSRF_TEST_MODULUS,
            "e": "AQAB"
        }]
    });
    let provider = tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || {
                    let discovery = discovery.clone();
                    async move { Json(discovery) }
                }),
            )
            .route(
                "/jwks",
                get(move || {
                    let jwks = jwks.clone();
                    async move { Json(jwks) }
                }),
            );
        let _ = axum::serve(listener, app).await;
    });
    let relying_party = OidcRelyingParty::discover(
        OidcConfig::new(&issuer, "agent-evidence-csrf-test", true)
            .expect("configure loopback OIDC issuer for CSRF test"),
        OidcPublicClientConfig::new(
            "agent-evidence-csrf-admin",
            format!("{AGENT_EVIDENCE_ADMIN_ORIGIN}/api/v2/auth/admin/callback"),
            ["openid"],
            true,
        )
        .expect("configure loopback Admin OIDC client for CSRF test"),
        store,
    )
    .await
    .expect("discover loopback OIDC provider for CSRF test");
    (
        HumanSessionRuntime {
            clients: Arc::new(BTreeMap::from([(
                HumanSessionClient::Admin.as_str().to_owned(),
                HumanBrowserClientRuntime {
                    client_key: HumanSessionClient::Admin,
                    public_origin: AGENT_EVIDENCE_ADMIN_ORIGIN.into(),
                    relying_party: Arc::new(relying_party),
                },
            )])),
            session_clients: Arc::new(vec![HumanSessionClient::Admin]),
            protector: PkceProtector::new(&[0x3a; 32])
                .expect("create Agent evidence CSRF session protector"),
            failure_source_hasher: LoginFailureSourceHasher::new(&[0x3a; 32])
                .expect("create Agent evidence CSRF login failure hasher"),
            maximum_lifetime: std::time::Duration::from_secs(60 * 60),
            login_requests_per_minute: 100,
            secure_cookie: false,
        },
        provider,
    )
}

/// Builds one real, pending production-Agent submission through the same
/// approval, policy, mailbox, draft, and admission services used in
/// production. The HTTP test below must never synthesize an evidence
/// response: success is meaningful only if the evidence reader can prove
/// the complete immutable chain against PostgreSQL.
async fn create_agent_mail_submission_evidence_http_target(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
    owner_id: PrincipalId,
    now: DateTime<Utc>,
) -> (PrincipalId, MailSubmissionId) {
    let owner_context = store
        .resolve_authorization(owner_id, now)
        .await
        .expect("resolve Agent evidence HTTP owner authorization")
        .expect("Agent evidence HTTP owner remains active");
    let agent_id =
        insert_agent_mail_evidence_http_target(store, organization_id, owner_id, now).await;
    let agent_context = store
        .resolve_authorization(agent_id, now)
        .await
        .expect("resolve production Agent authorization")
        .expect("production Agent remains active");

    let policy_version = AgentMailPolicyService::new(store.clone())
        .create_version(CreateAgentMailPolicyVersionCommand {
            context: owner_context.clone(),
            agent_id,
            allowed_recipient_domains: BTreeSet::from(["example.test".into()]),
            max_recipients_per_message: 2,
            max_messages_per_hour: 1,
            max_messages_per_recipient_per_day: 1,
            require_first_contact_approval: true,
            max_auto_replies_per_thread_per_day: 1,
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create production Agent evidence policy");

    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id,
            policy_key: "agent.evidence.production.activation".into(),
            display_name: "Agent evidence production activation".into(),
            action: ApprovalAction::ProductionAgentMail,
            required_approvals: 2,
            rule: json!({"resource_scope": "exact_policy_version"}),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create Agent evidence production activation approval policy");
    let workflow = ApprovalWorkflowService::new(store.clone());
    let activation_approval_id = workflow
        .request(RequestApprovalCommand {
            context: owner_context.clone(),
            policy_key: "agent.evidence.production.activation".into(),
            action: ApprovalAction::ProductionAgentMail,
            resource_kind: "agent.mail_policy_version".into(),
            resource_id: policy_version.id.to_string(),
            reason: "Approve bounded production Agent evidence mail".into(),
            ticket_reference: Some("AGENT-EVIDENCE-HTTP-1".into()),
            expires_at: now + chrono::Duration::hours(1),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("request Agent evidence policy activation");
    for _ in 0..2 {
        let approver =
            insert_inspector_http_unprivileged_human(store, organization_id, owner_id, now).await;
        grant_inspector_http_test_role(
            store,
            organization_id,
            approver,
            owner_id,
            "org_owner",
            now,
        )
        .await;
        let approver_context = store
            .resolve_authorization(approver, now)
            .await
            .expect("resolve Agent evidence activation approver")
            .expect("Agent evidence activation approver remains active");
        workflow
            .decide(DecideApprovalCommand {
                context: approver_context,
                request_id: activation_approval_id,
                decision: ApprovalDecision::Approved,
                reason: "independent production policy review".into(),
                trace_id: Uuid::now_v7().to_string(),
                now,
            })
            .await
            .expect("approve Agent evidence policy activation");
    }
    AgentMailPolicyService::new(store.clone())
        .activate(ActivateAgentMailPolicyCommand {
            context: owner_context.clone(),
            agent_id,
            policy_version_id: policy_version.id,
            approval: ApprovalProof {
                organization_id,
                request_id: activation_approval_id,
                requested_by: owner_id,
                action: ApprovalAction::ProductionAgentMail,
                resource_kind: "agent.mail_policy_version".into(),
                resource_id: policy_version.id.to_string(),
                now,
            },
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("activate Agent evidence policy");

    let mailbox = MailboxManagementService::new(store.clone())
        .create(CreateMailboxCommand {
            organization_id,
            kind: MailboxKind::Agent,
            primary_address: format!("agent-evidence-{}@example.test", Uuid::new_v4().simple()),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create production Agent evidence mailbox");
    MailboxDelegationService::new(store.clone())
        .grant_authorized(
            &owner_context,
            GrantMailboxDelegationCommand {
                organization_id,
                mailbox_id: mailbox.mailbox_id,
                grantee_principal_id: agent_id,
                capabilities: BTreeSet::from([MailboxCapability::Send]),
                reason: "production Agent owns the evidence send workflow".into(),
                valid_from: now,
                valid_until: now + chrono::Duration::days(30),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("delegate production Agent evidence send");
    let data_plane_id = MailDataPlaneId::from_uuid(Uuid::from_u128(LEGACY_MAIL_DATA_PLANE_ID));
    let engine_account_ref = ((Uuid::new_v4().as_u128() % 4_000_000_000_u128) + 1).to_string();
    MailEngineMailboxBindingService::try_new(store.clone(), data_plane_id, "stalwart")
        .expect("construct Agent evidence mailbox binding service")
        .bind_authorized(
            &owner_context,
            BindMailEngineMailboxCommand {
                organization_id,
                mailbox_id: mailbox.mailbox_id,
                account_ref: engine_account_ref.clone(),
                expected_version: 0,
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("bind production Agent evidence mailbox");

    ApprovalPolicyService::new(store.clone())
        .create(CreateApprovalPolicyCommand {
            organization_id,
            policy_key: "agent.evidence.first.contact".into(),
            display_name: "Agent evidence first contact".into(),
            action: ApprovalAction::AgentFirstContact,
            required_approvals: 1,
            rule: json!({"resource_scope": "exact_draft_version"}),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("create Agent evidence first-contact approval policy");
    let draft_id = DraftId::new();
    let creation_attempt_id = copilotmail_domain::DraftCreationAttemptId::new();
    store
        .record_draft_creation_attempt(&copilotmail_application::DraftCreationAttemptWrite {
            attempt_id: creation_attempt_id,
            organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: agent_id,
            draft_id,
            data_plane_id,
        })
        .await
        .expect("record Agent evidence draft creation attempt");
    store
        .record_mail_draft(copilotmail_application::MailDraftWrite {
            draft_id,
            organization_id,
            mailbox_id: mailbox.mailbox_id,
            created_by: agent_id,
            data_plane_id,
            creation_attempt_id,
            engine_account_ref: engine_account_ref.clone(),
            engine_email_ref: "agent-evidence-draft-1".into(),
            engine_thread_ref: "agent-evidence-thread-1".into(),
            reply_to_message_id: None,
            reply_to_automatic: false,
            body_ref: [0x71; 32],
            content_sha256: [0x72; 32],
            recipients: vec![copilotmail_application::MailDraftRecipientEvidence {
                recipient_ref: [0x73; 32],
                domain: "example.test".into(),
            }],
            expected_version: None,
            now,
        })
        .await
        .expect("record production Agent evidence draft");
    let contact_approval_id = workflow
        .request(RequestApprovalCommand {
            context: agent_context.clone(),
            policy_key: "agent.evidence.first.contact".into(),
            action: ApprovalAction::AgentFirstContact,
            resource_kind: "agent.mail_draft".into(),
            resource_id: format!("{draft_id}:1"),
            reason: "First business contact is expected".into(),
            ticket_reference: Some("AGENT-EVIDENCE-HTTP-CONTACT-1".into()),
            expires_at: now + chrono::Duration::minutes(30),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("request Agent evidence first-contact approval");
    let contact_approver =
        insert_inspector_http_unprivileged_human(store, organization_id, owner_id, now).await;
    grant_inspector_http_test_role(
        store,
        organization_id,
        contact_approver,
        owner_id,
        "org_owner",
        now,
    )
    .await;
    workflow
        .decide(DecideApprovalCommand {
            context: store
                .resolve_authorization(contact_approver, now)
                .await
                .expect("resolve Agent evidence first-contact approver")
                .expect("Agent evidence first-contact approver remains active"),
            request_id: contact_approval_id,
            decision: ApprovalDecision::Approved,
            reason: "first-contact purpose reviewed".into(),
            trace_id: Uuid::now_v7().to_string(),
            now,
        })
        .await
        .expect("approve Agent evidence first contact");
    let intent = MailSubmissionService::new(store.clone())
        .request(
            &agent_context,
            RequestMailSubmissionCommand {
                mailbox_id: mailbox.mailbox_id,
                draft_id,
                expected_draft_version: 1,
                idempotency_key: "agent-evidence-http-submission-000001".into(),
                first_contact_approval_request_id: Some(contact_approval_id),
                trace_id: Uuid::now_v7().to_string(),
                now,
            },
        )
        .await
        .expect("admit production Agent evidence submission");
    (agent_id, intent.submission_id)
}

async fn agent_inspector_http_fixture() -> AgentInspectorHttpFixture {
    let store = migrated_inspector_http_store().await;
    // Keep each persisted event in the past so the real resolver can
    // safely require `created_at <= Utc::now()` without a timing race.
    let now = Utc::now() - chrono::Duration::seconds(5);
    let bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(inspector_bootstrap_command(
            format!("agent-inspector-http-{}", Uuid::new_v4().simple()),
            now,
        ))
        .await
        .expect("bootstrap Agent Inspector HTTP organization");
    let owner_id =
        bind_inspector_http_owner(&store, bootstrap.organization_id, "Primary", now).await;
    let agent_id = insert_inspector_http_agent(
        &store,
        bootstrap.organization_id,
        owner_id,
        "Agent Inspector HTTP Target",
        now,
        true,
    )
    .await;
    let unprivileged_human_id =
        insert_inspector_http_unprivileged_human(&store, bootstrap.organization_id, owner_id, now)
            .await;
    let owner_admin_session_token =
        create_inspector_http_admin_session(&store, bootstrap.organization_id, owner_id, now).await;
    let unprivileged_human_admin_session_token = create_inspector_http_admin_session(
        &store,
        bootstrap.organization_id,
        unprivileged_human_id,
        now,
    )
    .await;

    // A real Agent in another tenant proves the 404 boundary does not
    // distinguish "exists elsewhere" from "does not exist".
    let foreign_bootstrap = BootstrapOrganizationService::new(store.clone())
        .execute(inspector_bootstrap_command(
            format!("agent-inspector-http-foreign-{}", Uuid::new_v4().simple()),
            now,
        ))
        .await
        .expect("bootstrap foreign Agent Inspector HTTP organization");
    let foreign_owner_id =
        bind_inspector_http_owner(&store, foreign_bootstrap.organization_id, "Foreign", now).await;
    let foreign_agent_id = insert_inspector_http_agent(
        &store,
        foreign_bootstrap.organization_id,
        foreign_owner_id,
        "Foreign Agent Inspector HTTP Target",
        now,
        false,
    )
    .await;

    AgentInspectorHttpFixture {
        store,
        organization_id: bootstrap.organization_id,
        owner_id,
        owner_admin_session_token,
        agent_id,
        unprivileged_human_admin_session_token,
        foreign_agent_id,
    }
}

async fn agent_mail_submission_evidence_http_fixture() -> AgentMailSubmissionEvidenceHttpFixture {
    let inspector = agent_inspector_http_fixture().await;
    // Keep authorization, approval, and admission timestamps in the past
    // relative to the database clock used by the read transaction.
    let now = Utc::now() - chrono::Duration::seconds(5);
    let (agent_id, submission_id) = create_agent_mail_submission_evidence_http_target(
        &inspector.store,
        inspector.organization_id,
        inspector.owner_id,
        now,
    )
    .await;

    // `helpdesk` has identity.manage but no audit.read; the compliance
    // auditor is the inverse. Both retain employee only as a harmless
    // baseline role so this asserts each missing permission precisely.
    let identity_only_principal = insert_inspector_http_unprivileged_human(
        &inspector.store,
        inspector.organization_id,
        inspector.owner_id,
        now,
    )
    .await;
    grant_inspector_http_test_role(
        &inspector.store,
        inspector.organization_id,
        identity_only_principal,
        inspector.owner_id,
        "helpdesk",
        now,
    )
    .await;
    let audit_only_principal = insert_inspector_http_unprivileged_human(
        &inspector.store,
        inspector.organization_id,
        inspector.owner_id,
        now,
    )
    .await;
    grant_inspector_http_test_role(
        &inspector.store,
        inspector.organization_id,
        audit_only_principal,
        inspector.owner_id,
        "compliance_auditor",
        now,
    )
    .await;
    let identity_only_admin_session_token = create_inspector_http_admin_session(
        &inspector.store,
        inspector.organization_id,
        identity_only_principal,
        now,
    )
    .await;
    let audit_only_admin_session_token = create_inspector_http_admin_session(
        &inspector.store,
        inspector.organization_id,
        audit_only_principal,
        now,
    )
    .await;

    AgentMailSubmissionEvidenceHttpFixture {
        store: inspector.store,
        organization_id: inspector.organization_id,
        owner_id: inspector.owner_id,
        owner_admin_session_token: inspector.owner_admin_session_token,
        agent_id,
        submission_id,
        identity_only_admin_session_token,
        audit_only_admin_session_token,
        foreign_agent_id: inspector.foreign_agent_id,
    }
}

fn agent_inspector_http_router(
    store: PgControlPlaneStore,
    authenticator: InspectorHttpTestAuthenticator,
) -> Router {
    agent_inspector_http_router_with_human_session_requirement(store, authenticator, true)
}

/// The production-facing fixture requires the BFF Admin cookie. The
/// bearer-only variant below exists solely to prove the handler enforces
/// *both* token scopes when the API is deployed in a bearer-enabled mode.
fn agent_inspector_http_bearer_router(
    store: PgControlPlaneStore,
    authenticator: InspectorHttpTestAuthenticator,
) -> Router {
    agent_inspector_http_router_with_human_session_requirement(store, authenticator, false)
}

fn agent_inspector_http_router_with_human_session_requirement(
    store: PgControlPlaneStore,
    authenticator: InspectorHttpTestAuthenticator,
    human_session_required: bool,
) -> Router {
    let human_session = human_session_required.then(inspector_http_admin_session_runtime);
    agent_inspector_http_router_with_session_runtime(
        store,
        authenticator,
        human_session,
        human_session_required,
    )
}

fn agent_inspector_http_router_with_session_runtime(
    store: PgControlPlaneStore,
    authenticator: InspectorHttpTestAuthenticator,
    human_session: Option<HumanSessionRuntime>,
    human_session_required: bool,
) -> Router {
    router_with_state(
        AppState {
            store,
            authenticator: Arc::new(authenticator),
            authentication_ready: true,
            human_auth_required: human_session_required,
            human_session,
            human_session_required,
            agent_token_service: None,
            agent_authentication_ready: true,
            agent_auth_required: true,
            agent_event_stream_tenant_limit: 100,
            agent_event_stream_principal_limit: 5,
            agent_ingress_tenant_requests_per_minute: 6_000,
            agent_ingress_principal_requests_per_minute: 600,
            agent_event_stream_metrics: AgentEventStreamMetrics::default(),
            mail_data_plane_id: None,
            mail_engine: None,
            mail_engine_required: false,
            mail_content: None,
            mail_content_required: false,
            mail_drafts: None,
            mail_draft_content: None,
            mail_drafts_required: false,
            mail_event_ingress: None,
            mail_event_ingress_required: false,
            audit_archive: None,
        },
        None,
    )
}

async fn agent_mail_submission_evidence_http_router(
    store: PgControlPlaneStore,
    authenticator: InspectorHttpTestAuthenticator,
) -> (Router, tokio::task::JoinHandle<()>) {
    let (human_session, oidc_provider) =
        agent_evidence_csrf_admin_session_runtime(store.clone()).await;
    (
        agent_inspector_http_router_with_session_runtime(
            store,
            authenticator,
            Some(human_session),
            true,
        ),
        oidc_provider,
    )
}

async fn get_agent_inspector_http_with_bearer(
    app: &Router,
    bearer_token: &'static str,
    agent_id: PrincipalId,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v2/agents/{agent_id}/inspector"))
                .header(header::AUTHORIZATION, format!("Bearer {bearer_token}"))
                .body(Body::empty())
                .expect("build authenticated Agent Inspector HTTP request"),
        )
        .await
        .expect("serve authenticated Agent Inspector HTTP request")
}

async fn get_agent_inspector_http_with_admin_session(
    app: &Router,
    session_token: &str,
    agent_id: PrincipalId,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v2/agents/{agent_id}/inspector"))
                .header(
                    header::COOKIE,
                    format!(
                        "{}={session_token}",
                        human_session_cookie_name(HumanSessionClient::Admin, false)
                    ),
                )
                .body(Body::empty())
                .expect("build Admin-session Agent Inspector HTTP request"),
        )
        .await
        .expect("serve Admin-session Agent Inspector HTTP request")
}

async fn post_agent_mail_submission_evidence_http_with_bearer(
    app: &Router,
    bearer_token: &'static str,
    agent_id: PrincipalId,
    submission_id: MailSubmissionId,
) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/api/v2/agents/{agent_id}/mail-submissions/{submission_id}/evidence"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {bearer_token}"))
                .body(Body::empty())
                .expect("build bearer Agent submission evidence HTTP request"),
        )
        .await
        .expect("serve bearer Agent submission evidence HTTP request")
}

async fn post_agent_mail_submission_evidence_http_with_admin_session(
    app: &Router,
    session_token: &str,
    agent_id: PrincipalId,
    submission_id: MailSubmissionId,
    origins: &[&str],
) -> Response {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(format!(
            "/api/v2/agents/{agent_id}/mail-submissions/{submission_id}/evidence"
        ))
        .header(
            header::COOKIE,
            format!(
                "{}={session_token}",
                human_session_cookie_name(HumanSessionClient::Admin, false)
            ),
        );
    for origin in origins {
        request = request.header(header::ORIGIN, *origin);
    }
    app.clone()
        .oneshot(
            request
                .body(Body::empty())
                .expect("build Admin-session Agent submission evidence HTTP request"),
        )
        .await
        .expect("serve Admin-session Agent submission evidence HTTP request")
}

async fn inspector_http_error(response: Response) -> ErrorResponse {
    let body = to_bytes(response.into_body(), 16 * 1024)
        .await
        .expect("read Agent Inspector HTTP error response");
    serde_json::from_slice(&body).expect("deserialize Agent Inspector HTTP error response")
}

async fn agent_mail_submission_evidence_write_counts(
    store: &PgControlPlaneStore,
    organization_id: OrganizationId,
) -> (i64, i64) {
    sqlx::query_as(
        r#"
        SELECT
            (SELECT count(*)
              FROM audit_events
             WHERE organization_id = $1
                AND action = 'agent.mail_submission_evidence.queried'
                AND resource_type = 'agent.mail_submission_evidence') AS audit_count,
            (SELECT count(*)
               FROM outbox_events
              WHERE event_type = 'agent.mail_submission_evidence.queried.v1'
                AND aggregate_type = 'agent.mail_submission_evidence'
                AND payload ->> 'organization_id' = $1::text) AS outbox_count
        "#,
    )
    .bind(organization_id.as_uuid())
    .fetch_one(store.pool())
    .await
    .expect("count Agent submission evidence audit and outbox writes")
}

#[tokio::test]
async fn agent_mail_submission_evidence_csrf_runtime_configures_the_admin_origin() {
    let (runtime, oidc_provider) =
        agent_evidence_csrf_admin_session_runtime(disconnected_store()).await;
    let client = runtime
        .client(HumanSessionClient::Admin.as_str())
        .expect("CSRF test runtime configures the Admin browser client");
    assert_eq!(client.client_key, HumanSessionClient::Admin);
    assert_eq!(client.public_origin, AGENT_EVIDENCE_ADMIN_ORIGIN);
    assert!(runtime.session_client_is_configured(HumanSessionClient::Admin));
    oidc_provider.abort();
}

#[tokio::test]
async fn mail_event_ingress_authenticates_before_body_and_verifies_exact_hmac() {
    let bearer = b"0123456789abcdef0123456789abcdef";
    let signature_key = b"abcdef0123456789abcdef0123456789";
    let prefix = std::env::temp_dir().join(format!("copilotmail-ingress-test-{}", Uuid::now_v7()));
    let bearer_file = prefix.with_extension("bearer");
    let signature_file = prefix.with_extension("hmac");
    tokio::fs::write(&bearer_file, [bearer.as_slice(), b"\n"].concat())
        .await
        .unwrap();
    tokio::fs::write(&signature_file, signature_key)
        .await
        .unwrap();
    let settings = MailEventIngressSettings {
        bearer_token_file: bearer_file.clone(),
        signature_key_file: signature_file.clone(),
    };

    let unauthenticated = router_with_test_ingress(settings.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/v2/mail-engine/stalwart/events")
                .body(Body::from(vec![b'x'; 2 * 1024 * 1024]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    let body = br#"{"events":[{"id":"event-1","createdAt":"2026-07-26T08:00:00Z","type":"delivery.delivered","data":{"accountId":42,"documentId":991,"mailboxId":[7]}}]}"#;
    let mut mac = Hmac::<Sha256>::new_from_slice(signature_key).unwrap();
    mac.update(body);
    let signature = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
    let verified_but_unsupported = router_with_test_ingress(settings.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/v2/mail-engine/stalwart/events")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", String::from_utf8_lossy(bearer)),
                )
                .header("x-signature", signature)
                .body(Body::from(body.as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(verified_but_unsupported.status(), StatusCode::BAD_REQUEST);

    let bad_signature = router_with_test_ingress(settings)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/internal/v2/mail-engine/stalwart/events")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", String::from_utf8_lossy(bearer)),
                )
                .header(
                    "x-signature",
                    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                )
                .body(Body::from(body.as_slice()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_signature.status(), StatusCode::UNAUTHORIZED);

    tokio::fs::remove_file(bearer_file).await.unwrap();
    tokio::fs::remove_file(signature_file).await.unwrap();
}

#[tokio::test]
async fn protected_organization_route_fails_closed_without_authentication() {
    let organization_id = OrganizationId::new();
    let app = router(disconnected_store());
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v2/organizations/{organization_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");

    let app = router(disconnected_store());
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v2/organizations/{organization_id}"))
                .header(header::AUTHORIZATION, "Bearer unverified-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v2/organizations/{organization_id}/audit-events"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri("/api/v2/agent/mail-policy")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let subscription_id = SubscriptionId::new();
    let message_id = Uuid::now_v7();
    for (method, uri, body) in [
        (
            "PUT",
            format!(
                "/api/v2/agent/mailboxes/{}/processing-subscription",
                MailboxId::new()
            ),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/agent/processing-subscriptions/{subscription_id}/claims"),
            r#"{"lease_seconds":60}"#.into(),
        ),
        (
            "PATCH",
            format!("/api/v2/agent/processing-subscriptions/{subscription_id}"),
            r#"{"expected_status":"active","status":"paused"}"#.into(),
        ),
        (
            "GET",
            format!("/api/v2/agent/processing-subscriptions/{subscription_id}/dead-letters"),
            String::new(),
        ),
        (
            "POST",
            format!(
                "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/lease-extension"
            ),
            format!(r#"{{"lease_id":"{}","lease_seconds":60}}"#, LeaseId::new()),
        ),
        (
            "POST",
            format!(
                "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/acknowledgement"
            ),
            format!(r#"{{"lease_id":"{}"}}"#, LeaseId::new()),
        ),
        (
            "POST",
            format!(
                "/api/v2/agent/processing-subscriptions/{subscription_id}/messages/{message_id}/nack"
            ),
            format!(
                r#"{{"lease_id":"{}","reason_code":"temporary_failure"}}"#,
                LeaseId::new()
            ),
        ),
    ] {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn approval_routes_fail_closed_before_touching_storage() {
    let organization_id = OrganizationId::new();
    let approval_request_id = ApprovalRequestId::new();
    let create = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v2/approval-requests")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{
                        "policy_key":"mail.send_as.external",
                        "action":"mail.send_as",
                        "resource_kind":"mailbox",
                        "resource_id":"mailbox-1",
                        "reason":"review",
                        "ticket_reference":"SEC-1",
                        "expires_at":"2026-07-26T00:00:00Z"
                    }"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::UNAUTHORIZED);

    let decide = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v2/approval-requests/{}/decisions",
                    ApprovalRequestId::new()
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"decision":"approved","reason":"reviewed"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(decide.status(), StatusCode::UNAUTHORIZED);

    for (method, uri, body) in [
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/approval-requests?view=actionable"),
            String::new(),
        ),
        (
            "GET",
            format!("/api/v2/approval-requests/{approval_request_id}"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/approval-requests/{approval_request_id}/cancellation"),
            "{}".into(),
        ),
    ] {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn role_governance_routes_fail_closed_before_touching_storage() {
    let organization_id = OrganizationId::new();
    let assignment_id = RoleAssignmentId::new();
    for (method, uri, body) in [
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/roles"),
            String::new(),
        ),
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/role-assignments"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/organizations/{organization_id}/role-assignments"),
            r#"{
                "principal_id":"018f13b9-1111-7bc8-965d-48a28866f073",
                "role_id":"018f13b9-2222-7bc8-965d-48a28866f073",
                "valid_until":"2026-08-01T00:00:00.000Z",
                "approval_request_id":"018f13b9-3333-7bc8-965d-48a28866f073"
            }"#
            .into(),
        ),
        (
            "POST",
            format!("/api/v2/role-assignments/{assignment_id}/revocation"),
            "{}".into(),
        ),
    ] {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn compliance_routes_fail_closed_before_database_or_object_store_access() {
    let organization_id = OrganizationId::new();
    let case_id = ComplianceCaseId::new();
    let archive_id = AuditArchiveId::new();
    let grant_id = AuditExportId::new();
    let retention_policy_id = RetentionPolicyId::new();
    let retention_version_id = RetentionPolicyVersionId::new();
    let agent_id = PrincipalId::new();
    let submission_id = MailSubmissionId::new();
    let agent_mail_policy_version_id = Uuid::now_v7();
    let hold_id = LegalHoldId::new();
    let approval_request_id = ApprovalRequestId::new();
    let requests = [
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/compliance-cases"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/organizations/{organization_id}/compliance-cases"),
            r#"{"external_reference":"CASE-1","title":"Evidence hold","purpose":"Investigation"}"#.to_owned(),
        ),
        (
            "GET",
            format!("/api/v2/compliance-cases/{case_id}"),
            String::new(),
        ),
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/legal-holds"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/compliance-cases/{case_id}/closure"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/audit-archives/{archive_id}/legal-holds"),
            format!(
                r#"{{"case_id":"{case_id}","approval_request_id":"{approval_request_id}","reason":"Evidence preservation"}}"#
            ),
        ),
        (
            "POST",
            format!("/api/v2/audit-archives/{archive_id}/exports"),
            format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
        ),
        (
            "GET",
            format!(
                "/api/v2/audit-archives/{archive_id}/export-grants/{approval_request_id}"
            ),
            String::new(),
        ),
        (
            "GET",
            format!("/api/v2/audit-export-grants/{grant_id}"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/audit-export-grants/{grant_id}/artifacts/events"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/audit-export-grants/{grant_id}/artifacts/manifest"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/organizations/{organization_id}/retention-policies"),
            r#"{"policy_key":"audit.standard","display_name":"Audit standard","resource_kind":"audit_evidence","minimum_retention_days":2555,"archive_after_days":30,"delete_after_days":3650,"rule":{}}"#.to_owned(),
        ),
        (
            "GET",
            format!("/api/v2/retention-policies/{retention_policy_id}"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/retention-policies/{retention_policy_id}/versions"),
            r#"{"minimum_retention_days":2555,"archive_after_days":30,"delete_after_days":3650,"rule":{}}"#.to_owned(),
        ),
        (
            "POST",
            format!("/api/v2/retention-policies/{retention_policy_id}/versions/{retention_version_id}/activation"),
            format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
        ),
        (
            "POST",
            format!("/api/v2/agents/{agent_id}/mail-policy-versions/{agent_mail_policy_version_id}/activation"),
            format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
        ),
        (
            "POST",
            format!("/api/v2/agents/{agent_id}/mail-policy-versions"),
            r#"{"allowed_recipient_domains":["internal.example"],"max_recipients_per_message":5,"max_messages_per_hour":100,"max_messages_per_recipient_per_day":10,"require_first_contact_approval":true,"max_auto_replies_per_thread_per_day":2}"#.to_owned(),
        ),
        (
            "GET",
            format!("/api/v2/agents/{agent_id}/mail-policy-versions/{agent_mail_policy_version_id}"),
            String::new(),
        ),
        (
            "GET",
            format!("/api/v2/agents/{agent_id}/inspector"),
            String::new(),
        ),
        (
            "POST",
            format!(
                "/api/v2/agents/{agent_id}/mail-submissions/{submission_id}/evidence"
            ),
            String::new(),
        ),
        (
            "POST",
            "/api/v2/agents/not-a-uuid/mail-submissions/not-a-uuid/evidence".to_owned(),
            String::new(),
        ),
        (
            "GET",
            "/api/v2/agents/not-a-uuid/inspector".to_owned(),
            String::new(),
        ),
        (
            "GET",
            format!("/api/v2/legal-holds/{hold_id}"),
            String::new(),
        ),
        (
            "POST",
            format!("/api/v2/legal-holds/{hold_id}/release"),
            format!(r#"{{"approval_request_id":"{approval_request_id}"}}"#),
        ),
    ];
    for (method, uri, body) in requests {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn agent_mail_submission_evidence_unauthenticated_paths_hide_target_and_parse_details() {
    let agent_id = PrincipalId::new();
    let submission_id = MailSubmissionId::new();
    let app = router(disconnected_store());
    let mut errors = Vec::new();
    for uri in [
        format!("/api/v2/agents/{agent_id}/mail-submissions/{submission_id}/evidence"),
        "/api/v2/agents/not-a-uuid/mail-submissions/not-a-uuid/evidence".into(),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("build unauthenticated Agent evidence request"),
            )
            .await
            .expect("serve unauthenticated Agent evidence request");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        errors.push(inspector_http_error(response).await);
    }
    assert_eq!(errors[0].code, errors[1].code);
    assert_eq!(errors[0].message, errors[1].message);
    assert_eq!(errors[0].retryable, errors[1].retryable);
    assert_eq!(errors[0].details, errors[1].details);
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn agent_inspector_http_route_enforces_bff_human_session_rbac_tenant_non_disclosure_and_sensitive_headers()
 {
    let fixture = agent_inspector_http_fixture().await;
    let app = agent_inspector_http_router(
        fixture.store.clone(),
        InspectorHttpTestAuthenticator {
            identities: BTreeMap::from([
                (
                    "owner",
                    AuthenticatedIdentity {
                        principal_id: fixture.owner_id,
                        principal_kind: AuthenticatedPrincipalKind::Human,
                        credential_id: Some("http-fixture-owner".into()),
                        scope_restriction: None,
                        authentication: AuthenticationBinding::HumanBearer {
                            expires_at: Utc::now(),
                        },
                    },
                ),
                (
                    "agent-machine",
                    AuthenticatedIdentity {
                        principal_id: fixture.agent_id,
                        principal_kind: AuthenticatedPrincipalKind::Agent,
                        credential_id: Some("http-fixture-agent".into()),
                        scope_restriction: None,
                        authentication: AuthenticationBinding::AgentCredential {
                            credential_id: CredentialId::new(),
                            expires_at: Utc::now(),
                            scopes: BTreeSet::new(),
                        },
                    },
                ),
            ]),
        },
    );

    // A verified machine identity is never allowed through the Human-only
    // Inspector boundary, even when its live Agent role resolves.
    let machine =
        get_agent_inspector_http_with_bearer(&app, "agent-machine", fixture.agent_id).await;
    assert_eq!(machine.status(), StatusCode::FORBIDDEN);
    let machine_error = inspector_http_error(machine).await;
    assert_eq!(machine_error.code, "human_principal_required");
    assert!(!machine_error.retryable);

    // The BFF policy is mandatory for Humans: a verified Human bearer is
    // not a substitute for the persisted Admin browser session cookie.
    let human_bearer = get_agent_inspector_http_with_bearer(&app, "owner", fixture.agent_id).await;
    assert_eq!(human_bearer.status(), StatusCode::UNAUTHORIZED);
    let human_bearer_error = inspector_http_error(human_bearer).await;
    assert_eq!(human_bearer_error.code, "human_session_required");
    assert!(!human_bearer_error.retryable);

    // An active Human with a real BFF session and tenant role still needs
    // identity.manage.
    let employee = get_agent_inspector_http_with_admin_session(
        &app,
        &fixture.unprivileged_human_admin_session_token,
        fixture.agent_id,
    )
    .await;
    assert_eq!(employee.status(), StatusCode::FORBIDDEN);
    let employee_error = inspector_http_error(employee).await;
    assert_eq!(employee_error.code, "permission_denied");
    assert!(!employee_error.retryable);

    // A target that exists in another tenant must be indistinguishable
    // from an absent target to an otherwise authorized Human reader.
    let missing = get_agent_inspector_http_with_admin_session(
        &app,
        &fixture.owner_admin_session_token,
        PrincipalId::new(),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing_error = inspector_http_error(missing).await;
    let foreign = get_agent_inspector_http_with_admin_session(
        &app,
        &fixture.owner_admin_session_token,
        fixture.foreign_agent_id,
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    let foreign_error = inspector_http_error(foreign).await;
    assert_eq!(missing_error.code, "resource_not_found");
    assert_eq!(foreign_error.code, missing_error.code);
    assert_eq!(foreign_error.message, missing_error.message);
    assert_eq!(foreign_error.retryable, missing_error.retryable);
    assert_eq!(foreign_error.details, missing_error.details);

    let success = get_agent_inspector_http_with_admin_session(
        &app,
        &fixture.owner_admin_session_token,
        fixture.agent_id,
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(success.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(success.headers()["referrer-policy"], "no-referrer");
    let body = to_bytes(success.into_body(), 16 * 1024)
        .await
        .expect("read successful Agent Inspector HTTP response");
    let snapshot: AgentInspectorResponse =
        serde_json::from_slice(&body).expect("deserialize successful Agent Inspector response");
    assert_eq!(snapshot.agent_id, fixture.agent_id.to_string());
    assert_eq!(snapshot.owner.principal_id, fixture.owner_id.to_string());
    assert_eq!(snapshot.token_access.state, "no_active_credential");
    assert!(snapshot.token_access.effective_scopes.is_empty());
    assert_eq!(snapshot.control_plane.mail_admission_state, "not_required");

    let successful_reads: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*)
          FROM audit_events
         WHERE organization_id = (
                   SELECT organization_id
                     FROM principals
                    WHERE id = $1
               )
           AND action = 'identity.agent.queried'
           AND resource_id = $2
        "#,
    )
    .bind(fixture.agent_id.as_uuid())
    .bind(fixture.agent_id.to_string())
    .fetch_one(fixture.store.pool())
    .await
    .expect("count successful Agent Inspector HTTP audit evidence");
    assert_eq!(successful_reads, 1);
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn agent_mail_submission_evidence_http_post_route_enforces_human_rbac_scope_csrf_non_disclosure_and_sensitive_headers()
 {
    let fixture = agent_mail_submission_evidence_http_fixture().await;
    let identities = BTreeMap::from([
        (
            "evidence-owner-full",
            AuthenticatedIdentity {
                principal_id: fixture.owner_id,
                principal_kind: AuthenticatedPrincipalKind::Human,
                credential_id: Some("http-evidence-owner".into()),
                scope_restriction: None,
                authentication: AuthenticationBinding::HumanBearer {
                    expires_at: Utc::now(),
                },
            },
        ),
        (
            "evidence-owner-identity-only",
            AuthenticatedIdentity {
                principal_id: fixture.owner_id,
                principal_kind: AuthenticatedPrincipalKind::Human,
                credential_id: Some("http-evidence-owner-identity".into()),
                scope_restriction: Some([Permission::IdentityManage].into_iter().collect()),
                authentication: AuthenticationBinding::HumanBearer {
                    expires_at: Utc::now(),
                },
            },
        ),
        (
            "evidence-owner-audit-only",
            AuthenticatedIdentity {
                principal_id: fixture.owner_id,
                principal_kind: AuthenticatedPrincipalKind::Human,
                credential_id: Some("http-evidence-owner-audit".into()),
                scope_restriction: Some([Permission::AuditRead].into_iter().collect()),
                authentication: AuthenticationBinding::HumanBearer {
                    expires_at: Utc::now(),
                },
            },
        ),
        (
            "evidence-agent-machine",
            AuthenticatedIdentity {
                principal_id: fixture.agent_id,
                principal_kind: AuthenticatedPrincipalKind::Agent,
                credential_id: Some("http-evidence-agent".into()),
                scope_restriction: None,
                authentication: AuthenticationBinding::AgentCredential {
                    credential_id: CredentialId::new(),
                    expires_at: Utc::now(),
                    scopes: BTreeSet::new(),
                },
            },
        ),
    ]);
    let (session_app, oidc_provider) = agent_mail_submission_evidence_http_router(
        fixture.store.clone(),
        InspectorHttpTestAuthenticator {
            identities: identities.clone(),
        },
    )
    .await;

    assert_eq!(
        agent_mail_submission_evidence_write_counts(&fixture.store, fixture.organization_id).await,
        (0, 0)
    );

    // This read creates an Audit/Outbox fact only after a successful
    // lookup, so cookie-authenticated POSTs must satisfy the exact BFF
    // Origin check before they can reach the handler.
    let missing_origin = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.owner_admin_session_token,
        fixture.agent_id,
        fixture.submission_id,
        &[],
    )
    .await;
    assert_eq!(missing_origin.status(), StatusCode::FORBIDDEN);
    let missing_origin_error = inspector_http_error(missing_origin).await;
    assert_eq!(missing_origin_error.code, "csrf_origin_rejected");
    assert!(!missing_origin_error.retryable);

    let wrong_origin = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.owner_admin_session_token,
        fixture.agent_id,
        fixture.submission_id,
        &["https://attacker.example.test"],
    )
    .await;
    assert_eq!(wrong_origin.status(), StatusCode::FORBIDDEN);
    let wrong_origin_error = inspector_http_error(wrong_origin).await;
    assert_eq!(wrong_origin_error.code, "csrf_origin_rejected");
    assert!(!wrong_origin_error.retryable);

    let duplicate_origin = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.owner_admin_session_token,
        fixture.agent_id,
        fixture.submission_id,
        &[AGENT_EVIDENCE_ADMIN_ORIGIN, AGENT_EVIDENCE_ADMIN_ORIGIN],
    )
    .await;
    assert_eq!(duplicate_origin.status(), StatusCode::FORBIDDEN);
    let duplicate_origin_error = inspector_http_error(duplicate_origin).await;
    assert_eq!(duplicate_origin_error.code, "csrf_origin_rejected");
    assert!(!duplicate_origin_error.retryable);
    assert_eq!(
        agent_mail_submission_evidence_write_counts(&fixture.store, fixture.organization_id).await,
        (0, 0),
        "rejected browser requests must not record evidence reads"
    );

    // An authenticated machine Principal is never permitted to read the
    // Human-only evidence chain, regardless of its live Agent role.
    let machine = post_agent_mail_submission_evidence_http_with_bearer(
        &session_app,
        "evidence-agent-machine",
        fixture.agent_id,
        fixture.submission_id,
    )
    .await;
    assert_eq!(machine.status(), StatusCode::FORBIDDEN);
    let machine_error = inspector_http_error(machine).await;
    assert_eq!(machine_error.code, "human_principal_required");
    assert!(!machine_error.retryable);

    // In the Admin BFF deployment, every Human bearer is rejected before
    // target parsing or authorization regardless of its declared scopes.
    for token in [
        "evidence-owner-full",
        "evidence-owner-identity-only",
        "evidence-owner-audit-only",
    ] {
        let response = post_agent_mail_submission_evidence_http_with_bearer(
            &session_app,
            token,
            fixture.agent_id,
            fixture.submission_id,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let error = inspector_http_error(response).await;
        assert_eq!(error.code, "human_session_required");
        assert!(!error.retryable);
    }

    // Browser sessions resolve the persisted principal and must possess
    // both independently checked RBAC permissions.
    let missing_identity = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.audit_only_admin_session_token,
        fixture.agent_id,
        fixture.submission_id,
        &[AGENT_EVIDENCE_ADMIN_ORIGIN],
    )
    .await;
    assert_eq!(missing_identity.status(), StatusCode::FORBIDDEN);
    let missing_identity_error = inspector_http_error(missing_identity).await;
    assert_eq!(missing_identity_error.code, "permission_denied");
    let missing_audit = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.identity_only_admin_session_token,
        fixture.agent_id,
        fixture.submission_id,
        &[AGENT_EVIDENCE_ADMIN_ORIGIN],
    )
    .await;
    assert_eq!(missing_audit.status(), StatusCode::FORBIDDEN);
    let missing_audit_error = inspector_http_error(missing_audit).await;
    assert_eq!(missing_audit_error.code, "permission_denied");

    // The handler may also be mounted for a bearer client. It repeats the
    // same independent scope intersection rather than trusting RBAC alone.
    let bearer_app = agent_inspector_http_bearer_router(
        fixture.store.clone(),
        InspectorHttpTestAuthenticator { identities },
    );
    for token in ["evidence-owner-identity-only", "evidence-owner-audit-only"] {
        let response = post_agent_mail_submission_evidence_http_with_bearer(
            &bearer_app,
            token,
            fixture.agent_id,
            fixture.submission_id,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let error = inspector_http_error(response).await;
        assert_eq!(error.code, "insufficient_scope");
        assert!(!error.retryable);
    }

    // An absent pair and a real Agent in another tenant are deliberately
    // indistinguishable to an otherwise authorized Human.
    let missing = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.owner_admin_session_token,
        PrincipalId::new(),
        MailSubmissionId::new(),
        &[AGENT_EVIDENCE_ADMIN_ORIGIN],
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing_error = inspector_http_error(missing).await;
    let foreign = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.owner_admin_session_token,
        fixture.foreign_agent_id,
        fixture.submission_id,
        &[AGENT_EVIDENCE_ADMIN_ORIGIN],
    )
    .await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    let foreign_error = inspector_http_error(foreign).await;
    assert_eq!(missing_error.code, "resource_not_found");
    assert_eq!(foreign_error.code, missing_error.code);
    assert_eq!(foreign_error.message, missing_error.message);
    assert_eq!(foreign_error.retryable, missing_error.retryable);
    assert_eq!(foreign_error.details, missing_error.details);
    assert_eq!(
        agent_mail_submission_evidence_write_counts(&fixture.store, fixture.organization_id).await,
        (0, 0),
        "rejected authentication, authorization, scope, and not-found requests must not record evidence reads"
    );

    let success = post_agent_mail_submission_evidence_http_with_admin_session(
        &session_app,
        &fixture.owner_admin_session_token,
        fixture.agent_id,
        fixture.submission_id,
        &[AGENT_EVIDENCE_ADMIN_ORIGIN],
    )
    .await;
    assert_eq!(success.status(), StatusCode::OK);
    assert_eq!(success.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(success.headers()["referrer-policy"], "no-referrer");
    let body = to_bytes(success.into_body(), 16 * 1024)
        .await
        .expect("read successful Agent submission evidence response");
    let evidence: AgentMailSubmissionEvidenceResponse = serde_json::from_slice(&body)
        .expect("deserialize successful Agent submission evidence response");
    assert_eq!(evidence.agent_id, fixture.agent_id.to_string());
    assert_eq!(
        evidence.submission.submission_id,
        fixture.submission_id.to_string()
    );
    assert_eq!(evidence.submission.status, "pending");
    assert!(evidence.delivery.is_none());
    assert_eq!(
        evidence
            .audit_references
            .iter()
            .map(|reference| reference.action.as_str())
            .collect::<Vec<_>>(),
        vec!["mail.submission.requested", "agent.mail.admitted"]
    );

    let successful_reads: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*)
          FROM audit_events
         WHERE organization_id = $1
           AND actor_principal_id = $2
           AND action = 'agent.mail_submission_evidence.queried'
           AND resource_type = 'agent.mail_submission_evidence'
           AND resource_id = $3
        "#,
    )
    .bind(fixture.organization_id.as_uuid())
    .bind(fixture.owner_id.as_uuid())
    .bind(fixture.submission_id.to_string())
    .fetch_one(fixture.store.pool())
    .await
    .expect("count successful Agent submission evidence reads");
    assert_eq!(successful_reads, 1);
    assert_eq!(
        agent_mail_submission_evidence_write_counts(&fixture.store, fixture.organization_id).await,
        (1, 1)
    );
    oidc_provider.abort();
}

#[tokio::test]
async fn mailbox_resource_route_fails_closed_without_authentication() {
    let mailbox_id = MailboxId::new();
    let message_id = Uuid::now_v7();
    let submission_id = MailSubmissionId::new();
    for uri in [
        "/api/v2/mailboxes".to_owned(),
        "/api/v2/draft-attempts".to_owned(),
        format!("/api/v2/mailboxes/{mailbox_id}"),
        format!("/api/v2/mailboxes/{mailbox_id}/messages"),
        format!("/api/v2/mailboxes/{mailbox_id}/messages/{message_id}"),
        format!("/api/v2/mailboxes/{mailbox_id}/drafts"),
        format!("/api/v2/mailboxes/{mailbox_id}/submissions"),
        format!(
            "/api/v2/agent/mailboxes/{mailbox_id}/messages/{}/content",
            Uuid::now_v7()
        ),
        format!("/api/v2/mail-submissions/{submission_id}"),
    ] {
        let human_mail = human_mail_route(&Method::GET, &uri);
        let response = router(disconnected_store())
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        if human_mail {
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
    }
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri("/api/v2/mailboxes")
                .header(header::AUTHORIZATION, "Bearer unverified-agent-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!response.headers().contains_key(header::WWW_AUTHENTICATE));
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri("/api/v2/mailboxes?limit=not-a-number&provider=forbidden")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "authentication must fail closed before parsing directory filters or touching storage"
    );
    for request in [
        Request::builder()
            .uri("/api/v2/mailboxes/not-a-uuid/submissions?limit=not-a-number")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/api/v2/mailboxes/not-a-uuid/submissions?limit=not-a-number")
            .header(header::AUTHORIZATION, "Bearer unverified-agent-token")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method(Method::HEAD)
            .uri("/api/v2/mailboxes/not-a-uuid/submissions?limit=not-a-number")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .method(Method::POST)
            .uri("/api/v2/mailboxes/not-a-uuid/submissions")
            .header(header::CONTENT_TYPE, "application/json")
            .header("idempotency-key", "test-submission-idempotency-key")
            .body(Body::from(format!(
                r#"{{"draft_id":"{}","expected_draft_version":1}}"#,
                DraftId::new()
            )))
            .unwrap(),
    ] {
        let response = router(disconnected_store()).oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "authentication must precede submissions-route path/query parsing"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    }
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/api/v2/mailboxes/{mailbox_id}/messages/{message_id}"
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"is_read":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/mailboxes/{mailbox_id}/messages/search"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"text":"quarterly report","cursor":null,"page_size":30}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/mailboxes/{mailbox_id}/drafts"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"draft_id":null,"expected_version":null,"to":["recipient@example.test"],"cc":[],"bcc":[],"subject":"Status","text_body":"Ready."}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let draft_id = DraftId::new();
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v2/mailboxes/{mailbox_id}/drafts/{draft_id}/restore"
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"expected_version":"not-an-integer"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "authentication must fail before parsing a draft restore body"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/mailboxes/{mailbox_id}/submissions"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "test-submission-key-000001")
                .body(Body::from(format!(
                    r#"{{"draft_id":"{}","expected_draft_version":2}}"#,
                    copilotmail_domain::DraftId::new()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v2/agent/mailboxes/{mailbox_id}/sandbox-submissions"
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "test-sandbox-key-00000001")
                .body(Body::from(format!(
                    r#"{{"draft_id":"{}","expected_draft_version":2}}"#,
                    copilotmail_domain::DraftId::new()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn inbox_cursor_is_mailbox_bound_and_bounded() {
    let mailbox_id = MailboxId::new();
    let cursor = encode_inbox_cursor(mailbox_id, 50);
    assert_eq!(decode_inbox_cursor(&cursor, mailbox_id), Ok(50));
    assert_eq!(decode_inbox_cursor(&cursor, MailboxId::new()), Err(()));
    assert_eq!(decode_inbox_cursor("", mailbox_id), Err(()));
    assert_eq!(
        decode_inbox_cursor(
            &URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&serde_json::json!({
                    "v": 2,
                    "mailbox_id": mailbox_id,
                    "position": 10_001
                }))
                .unwrap()
            ),
            mailbox_id
        ),
        Err(())
    );

    let digest = inbox_search_digest("quarterly report");
    let search_cursor = encode_inbox_search_cursor(mailbox_id, 30, &digest);
    assert_eq!(
        decode_inbox_search_cursor(&search_cursor, mailbox_id, &digest),
        Ok(30)
    );
    assert_eq!(
        decode_inbox_search_cursor(
            &search_cursor,
            mailbox_id,
            &inbox_search_digest("different query")
        ),
        Err(())
    );
    assert_eq!(
        decode_inbox_search_cursor(&search_cursor, MailboxId::new(), &digest),
        Err(())
    );
    let decoded = URL_SAFE_NO_PAD.decode(search_cursor).unwrap();
    let decoded = String::from_utf8(decoded).unwrap();
    assert!(!decoded.contains("quarterly report"));
    assert!(!decoded.contains("query_state"));
}

#[tokio::test]
async fn mailbox_management_routes_fail_closed_without_authentication() {
    let organization_id = OrganizationId::new();
    let mailbox_id = MailboxId::new();
    let create = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/organizations/{organization_id}/mailboxes"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"kind":"shared","primary_address":"support@example.test"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::UNAUTHORIZED);

    let grant = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/mailboxes/{mailbox_id}/delegations"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{
                        "grantee_principal_id":"{}",
                        "capabilities":["mailbox.read"],
                        "reason":"support rotation",
                        "valid_until":"2026-07-26T00:00:00Z"
                    }}"#,
                    PrincipalId::new()
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(grant.status(), StatusCode::UNAUTHORIZED);

    let revoke = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/v2/mailbox-delegations/{}",
                    DelegationId::new()
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(revoke.status(), StatusCode::UNAUTHORIZED);

    let binding = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding"
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"account_ref":"42","expected_version":0}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(binding.status(), StatusCode::UNAUTHORIZED);

    let malformed_binding = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding"
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed_binding.status(), StatusCode::UNAUTHORIZED);

    let oversized_binding = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!(
                    "/api/v2/mailboxes/{mailbox_id}/mail-engine-binding"
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(vec![b'x'; 16 * 1024 + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(oversized_binding.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn mail_domain_routes_fail_closed_before_dns_or_storage_access() {
    let organization_id = OrganizationId::new();
    let mail_domain_id = MailDomainId::new();
    let requests = [
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/mail-domains"),
            "",
        ),
        ("GET", format!("/api/v2/mail-domains/{mail_domain_id}"), ""),
        (
            "POST",
            format!("/api/v2/organizations/{organization_id}/mail-domains"),
            r#"{"name":"example.test","route_mode":"direct_mx"}"#,
        ),
        (
            "POST",
            format!("/api/v2/mail-domains/{mail_domain_id}/dns-verifications"),
            r#"{"domain":"example.test","ownership_token":"cmv1_abcdefghijklmnopqrstuvwxyz123456","dkim_selector":"mail"}"#,
        ),
        (
            "POST",
            format!("/api/v2/mail-domains/{mail_domain_id}/ownership-challenge"),
            r#"{"expected_version":1}"#,
        ),
        (
            "POST",
            format!("/api/v2/mail-domains/{mail_domain_id}/activation"),
            "{}",
        ),
        (
            "POST",
            format!("/api/v2/mail-domains/{mail_domain_id}/suspension"),
            r#"{"reason_code":"security_incident"}"#,
        ),
        (
            "PUT",
            format!("/api/v2/mail-domains/{mail_domain_id}/route"),
            r#"{"expected_version":1,"inbound_enabled":false,"outbound_enabled":false,"smart_host_ref":null,"dkim_key_ref":null}"#,
        ),
    ];
    for (method, uri, body) in requests {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn webhook_subscription_routes_fail_closed_before_storage_access() {
    let organization_id = OrganizationId::new();
    let subscription_id = WebhookSubscriptionId::new();
    let event_id = Uuid::now_v7();
    let requests = [
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/webhook-subscriptions"),
            "",
        ),
        (
            "POST",
            format!("/api/v2/organizations/{organization_id}/webhook-subscriptions"),
            r#"{"principal_id":"00000000-0000-0000-0000-000000000001","endpoint_url":"https://hooks.example.test/events","event_types":["message.received.v1"],"signing_key_reference":"agent-hook-1"}"#,
        ),
        (
            "GET",
            format!("/api/v2/webhook-subscriptions/{subscription_id}"),
            "",
        ),
        (
            "PUT",
            format!("/api/v2/webhook-subscriptions/{subscription_id}"),
            r#"{"expected_version":1,"endpoint_url":"https://hooks.example.test/events","event_types":["message.received.v1"],"signing_key_reference":"agent-hook-1","enabled":false}"#,
        ),
        (
            "GET",
            format!("/api/v2/webhook-subscriptions/{subscription_id}/events?limit=50"),
            "",
        ),
        (
            "GET",
            format!("/api/v2/webhook-subscriptions/{subscription_id}/stream"),
            "",
        ),
        (
            "GET",
            format!("/api/v2/organizations/{organization_id}/webhook-dead-letters"),
            "",
        ),
        (
            "POST",
            format!(
                "/api/v2/webhook-subscriptions/{subscription_id}/dead-letters/{event_id}/replay"
            ),
            "{}",
        ),
    ];
    for (method, uri, body) in requests {
        let response = router(disconnected_store())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[test]
fn agent_event_cursor_is_versioned_and_subscription_scoped() {
    let subscription_id = WebhookSubscriptionId::new();
    let event_id = Uuid::now_v7();
    let cursor = format_agent_event_cursor(subscription_id, event_id);
    assert_eq!(
        parse_agent_event_cursor(&cursor, subscription_id).unwrap(),
        event_id
    );
    assert!(parse_agent_event_cursor(&cursor, WebhookSubscriptionId::new()).is_err());
    assert!(parse_agent_event_cursor("v2.invalid", subscription_id).is_err());
}

#[test]
fn agent_processing_dead_letter_cursor_is_versioned_and_subscription_scoped() {
    let subscription_id = SubscriptionId::new();
    let message_id = Uuid::now_v7();
    let cursor = format_agent_processing_dead_letter_cursor(subscription_id, message_id);
    assert_eq!(
        parse_agent_processing_dead_letter_cursor(&cursor, subscription_id),
        Ok(message_id)
    );
    assert!(parse_agent_processing_dead_letter_cursor(&cursor, SubscriptionId::new()).is_err());
    assert!(
        parse_agent_processing_dead_letter_cursor(&format!("{cursor}.extra"), subscription_id)
            .is_err()
    );
    assert_eq!(
        parse_agent_processing_subscription_status("paused", "request-id").unwrap(),
        AgentProcessingSubscriptionStatus::Paused
    );
    assert!(parse_agent_processing_subscription_status("deleted", "request-id").is_err());
}

#[test]
fn agent_event_stream_resume_sources_must_match_and_remain_subscription_scoped() {
    let subscription_id = WebhookSubscriptionId::new();
    let event_id = Uuid::now_v7();
    let cursor = format_agent_event_cursor(subscription_id, event_id);
    let header = header::HeaderValue::from_str(&cursor).unwrap();
    assert_eq!(
        resolve_agent_stream_cursor(Some(&cursor), Some(&header), subscription_id).unwrap(),
        Some(event_id)
    );
    assert_eq!(
        resolve_agent_stream_cursor(None, Some(&header), subscription_id).unwrap(),
        Some(event_id)
    );
    let other_cursor = format_agent_event_cursor(subscription_id, Uuid::now_v7());
    let other_header = header::HeaderValue::from_str(&other_cursor).unwrap();
    assert_eq!(
        resolve_agent_stream_cursor(Some(&cursor), Some(&other_header), subscription_id),
        Err(AgentEventCursorParseError::ConflictingResumeSources)
    );
    assert_eq!(
        resolve_agent_stream_cursor(Some(&cursor), None, WebhookSubscriptionId::new()),
        Err(AgentEventCursorParseError::ScopeMismatch)
    );
}

#[test]
fn audit_event_cursor_is_positive_versioned_and_tenant_scoped() {
    let organization_id = OrganizationId::new();
    let cursor = format_audit_event_cursor(organization_id, 42);
    assert_eq!(
        parse_audit_event_cursor(&cursor, organization_id).unwrap(),
        42
    );
    assert_eq!(
        parse_audit_event_cursor(&cursor, OrganizationId::new()),
        Err(AuditEventCursorParseError::ScopeMismatch)
    );
    for invalid in [
        "v2.invalid.1",
        "v1.00000000-0000-0000-0000-000000000000.0",
        "v1.00000000-0000-0000-0000-000000000000.-1",
        "v1.00000000-0000-0000-0000-000000000000.1.extra",
    ] {
        assert!(parse_audit_event_cursor(invalid, organization_id).is_err());
    }
}

#[test]
fn audit_maintenance_ids_are_stable_scoped_and_uuid_v8() {
    let organization_id = OrganizationId::new().to_string();
    let first = stable_maintenance_uuid(
        "checkpoint",
        &[organization_id.as_bytes(), b"42", b"audit-key-1"],
    );
    let replay = stable_maintenance_uuid(
        "checkpoint",
        &[organization_id.as_bytes(), b"42", b"audit-key-1"],
    );
    let another_head = stable_maintenance_uuid(
        "checkpoint",
        &[organization_id.as_bytes(), b"43", b"audit-key-1"],
    );
    let archive = stable_maintenance_uuid(
        "archive",
        &[organization_id.as_bytes(), b"42", b"audit-key-1"],
    );
    assert_eq!(first, replay);
    assert_ne!(first, another_head);
    assert_ne!(first, archive);
    assert_eq!(first.get_version_num(), 8);
}

#[test]
fn audit_archive_metadata_carries_stable_artifact_trace_without_sensitive_fields() {
    let organization_id = OrganizationId::new();
    let checkpoint_id = AuditCheckpointId::new();
    let archive_id = AuditArchiveId::new();
    let trace_id = stable_audit_artifact_trace_id(organization_id, checkpoint_id);
    assert_eq!(
        trace_id,
        stable_audit_artifact_trace_id(organization_id, checkpoint_id)
    );
    assert_ne!(
        trace_id,
        stable_audit_artifact_trace_id(organization_id, AuditCheckpointId::new())
    );
    assert_eq!(trace_id.parse::<Uuid>().unwrap().get_version_num(), 8);
    let metadata =
        audit_archive_object_metadata(organization_id, checkpoint_id, archive_id, 1, &trace_id);

    assert_eq!(metadata["organization-id"], organization_id.to_string());
    assert_eq!(metadata["checkpoint-id"], checkpoint_id.to_string());
    assert_eq!(metadata["archive-id"], archive_id.to_string());
    assert_eq!(metadata["trace-id"], trace_id);
    assert_eq!(metadata.len(), 5);
    assert!(
        metadata
            .keys()
            .all(|key| !key.contains("secret") && !key.contains("content"))
    );
}

#[tokio::test]
async fn audit_maintenance_metrics_expose_alertable_fail_closed_state() {
    let organization_id = OrganizationId::new();
    let metrics = AuditMaintenanceMetrics::with_limits(organization_id, 2, 60, 30);
    assert!(metrics.readiness().is_ok());

    metrics.cycle_started();
    metrics.failed();
    assert!(metrics.readiness().is_ok());
    metrics.cycle_started();
    metrics.failed();
    assert_eq!(metrics.readiness(), Err("failure_threshold_exceeded"));

    let rendered = metrics.render();
    assert!(rendered.contains("copilotmail_audit_maintenance_failures_total"));
    assert!(rendered.contains(&format!("organization_id=\"{organization_id}\"")));
    assert!(rendered.contains("copilotmail_audit_maintenance_consecutive_failures"));
    let metrics_response = audit_maintenance_observability_router(metrics.clone())
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(metrics_response.status(), StatusCode::OK);
    assert_eq!(
        metrics_response.headers()[header::CONTENT_TYPE],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let readiness_response = audit_maintenance_observability_router(metrics)
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(readiness_response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let pending = AuditMaintenanceMetrics::with_limits(organization_id, 3, 60, 30);
    pending.state.lock().unwrap().pending_checkpoint_age_seconds = 61;
    assert_eq!(pending.readiness(), Err("pending_checkpoint_overdue"));

    let slow = AuditMaintenanceMetrics::with_limits(organization_id, 3, 60, 30);
    slow.cycle_started();
    slow.state.lock().unwrap().cycle_started_at = Utc::now().timestamp() - 31;
    assert_eq!(slow.readiness(), Err("cycle_duration_exceeded"));

    let passive = AuditMaintenanceMetrics::with_limits(organization_id, 3, 60, 30);
    passive.cycle_started();
    passive.lock_conflict();
    assert!(passive.readiness().is_ok());
    assert!(
        passive
            .render()
            .contains("copilotmail_audit_maintenance_lock_conflicts_total")
    );
}

#[tokio::test]
async fn agent_token_endpoint_fails_closed_and_disables_caching_when_unconfigured() {
    let app = router(disconnected_store());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/oauth2/token")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    "grant_type=client_credentials&client_id=invalid&\
                     client_assertion_type=invalid&client_assertion=invalid",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::PRAGMA], "no-cache");
}

#[tokio::test]
async fn agent_jwks_endpoint_fails_closed_when_authorization_server_is_unconfigured() {
    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri("/oauth2/jwks.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn malformed_agent_token_form_uses_oauth_error_and_disables_caching() {
    let app = router(disconnected_store());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/oauth2/token")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::PRAGMA], "no-cache");
}

#[test]
fn agent_event_stream_metrics_are_bounded_and_content_free() {
    let metrics = AgentEventStreamMetrics::default();
    metrics.opened();
    metrics
        .tenant_rejected_total
        .fetch_add(2, Ordering::Relaxed);
    metrics
        .principal_rejected_total
        .fetch_add(3, Ordering::Relaxed);
    metrics.stream_errors_total.fetch_add(1, Ordering::Relaxed);
    metrics
        .ingress_allowed_total
        .fetch_add(4, Ordering::Relaxed);
    metrics
        .ingress_principal_rejected_total
        .fetch_add(5, Ordering::Relaxed);
    let rendered = metrics.render();
    assert!(rendered.contains("copilotmail_agent_event_stream_active 1"));
    assert!(rendered.contains("scope=\"tenant\"} 2"));
    assert!(rendered.contains("scope=\"principal\"} 3"));
    assert!(rendered.contains("outcome=\"allowed\"} 4"));
    assert!(rendered.contains("outcome=\"principal_limited\"} 5"));
    assert!(!rendered.contains("organization_id"));
    assert!(!rendered.contains("principal_id"));
    metrics.closed();
    assert!(
        metrics
            .render()
            .contains("copilotmail_agent_event_stream_active 0")
    );
}

#[test]
fn agent_ingress_rejection_has_bounded_retry_after() {
    let response = agent_ingress_limited_response(
        "agent_ingress_principal_rate_limited",
        "Agent event ingress rate is exhausted",
        600,
        "request-id",
    );
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "60");
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
}

#[test]
fn agent_mail_submission_requires_agent_operate_in_rbac_and_token_scope() {
    let organization_id = OrganizationId::new();
    let principal_id = PrincipalId::new();
    let mut authorization = AuthorizationContext {
        principal_id,
        organization_id,
        permissions: [Permission::MailboxUse].into_iter().collect(),
        authentication: None,
    };
    let mut identity = AuthenticatedIdentity {
        principal_id,
        principal_kind: AuthenticatedPrincipalKind::Agent,
        credential_id: Some("agent-credential".into()),
        scope_restriction: Some([Permission::MailboxUse].into_iter().collect()),
        authentication: AuthenticationBinding::AgentCredential {
            credential_id: CredentialId::new(),
            expires_at: Utc::now(),
            scopes: [Permission::MailboxUse].into_iter().collect(),
        },
    };
    assert_eq!(
        enforce_mail_submission_scopes(&identity, &authorization, "request")
            .expect_err("Agent RBAC must include agent.operate")
            .status(),
        StatusCode::FORBIDDEN
    );
    authorization.permissions.insert(Permission::AgentOperate);
    assert_eq!(
        enforce_mail_submission_scopes(&identity, &authorization, "request")
            .expect_err("Agent token scope must include agent.operate")
            .status(),
        StatusCode::FORBIDDEN
    );
    identity
        .scope_restriction
        .as_mut()
        .unwrap()
        .insert(Permission::AgentOperate);
    assert!(enforce_mail_submission_scopes(&identity, &authorization, "request").is_ok());
}

#[test]
fn audit_export_rejects_agent_principals_before_authorization_store_access() {
    let agent = AuthenticatedIdentity {
        principal_id: PrincipalId::new(),
        principal_kind: AuthenticatedPrincipalKind::Agent,
        credential_id: Some("agent-credential".into()),
        scope_restriction: Some([Permission::AuditExport].into_iter().collect()),
        authentication: AuthenticationBinding::AgentCredential {
            credential_id: CredentialId::new(),
            expires_at: Utc::now(),
            scopes: [Permission::AuditExport].into_iter().collect(),
        },
    };
    assert_eq!(
        enforce_audit_export_human_principal(&agent, "request")
            .expect_err("Agents must never use the Human audit export path")
            .status(),
        StatusCode::FORBIDDEN
    );

    let human = AuthenticatedIdentity {
        principal_kind: AuthenticatedPrincipalKind::Human,
        ..agent
    };
    assert!(enforce_audit_export_human_principal(&human, "request").is_ok());
}

#[test]
fn audit_export_reconciliation_returns_the_callers_exact_grant() {
    let organization_id = OrganizationId::new();
    let requested_by = PrincipalId::new();
    let archive_id = AuditArchiveId::new();
    let approval_request_id = ApprovalRequestId::new();
    let authorization = AuthorizationContext {
        principal_id: requested_by,
        organization_id,
        permissions: [Permission::AuditExport].into_iter().collect(),
        authentication: None,
    };
    let now = Utc::now();
    let grant = AuditExportGrant::new(
        organization_id,
        archive_id,
        approval_request_id,
        requested_by,
        now,
        now + chrono::Duration::minutes(15),
    )
    .expect("construct a valid short-lived export grant");

    assert!(audit_export_grant_matches_reconciliation(
        &grant,
        &authorization,
        archive_id,
        approval_request_id,
    ));
}

#[test]
fn audit_export_reconciliation_hides_grants_owned_by_another_human() {
    let organization_id = OrganizationId::new();
    let requested_by = PrincipalId::new();
    let archive_id = AuditArchiveId::new();
    let approval_request_id = ApprovalRequestId::new();
    let now = Utc::now();
    let grant = AuditExportGrant::new(
        organization_id,
        archive_id,
        approval_request_id,
        requested_by,
        now,
        now + chrono::Duration::minutes(15),
    )
    .expect("construct a valid short-lived export grant");
    let another_human = AuthorizationContext {
        principal_id: PrincipalId::new(),
        organization_id,
        permissions: [Permission::AuditExport].into_iter().collect(),
        authentication: None,
    };

    assert!(!audit_export_grant_matches_reconciliation(
        &grant,
        &another_human,
        archive_id,
        approval_request_id,
    ));
}

#[test]
fn audit_export_reconciliation_hides_route_identity_mismatches() {
    let organization_id = OrganizationId::new();
    let requested_by = PrincipalId::new();
    let archive_id = AuditArchiveId::new();
    let approval_request_id = ApprovalRequestId::new();
    let authorization = AuthorizationContext {
        principal_id: requested_by,
        organization_id,
        permissions: [Permission::AuditExport].into_iter().collect(),
        authentication: None,
    };
    let now = Utc::now();
    let grant = AuditExportGrant::new(
        organization_id,
        archive_id,
        approval_request_id,
        requested_by,
        now,
        now + chrono::Duration::minutes(15),
    )
    .expect("construct a valid short-lived export grant");

    assert!(!audit_export_grant_matches_reconciliation(
        &grant,
        &authorization,
        AuditArchiveId::new(),
        approval_request_id,
    ));
    assert!(!audit_export_grant_matches_reconciliation(
        &grant,
        &authorization,
        archive_id,
        ApprovalRequestId::new(),
    ));
    let foreign_tenant = AuthorizationContext {
        organization_id: OrganizationId::new(),
        ..authorization
    };
    assert!(!audit_export_grant_matches_reconciliation(
        &grant,
        &foreign_tenant,
        archive_id,
        approval_request_id,
    ));
}

#[test]
fn audit_export_reconciliation_requires_canonical_route_ids() {
    let archive_id = AuditArchiveId::new();
    let approval_request_id = ApprovalRequestId::new();

    assert_eq!(
        parse_canonical_audit_archive_id(&archive_id.to_string()),
        Some(archive_id)
    );
    assert_eq!(
        parse_canonical_approval_request_id(&approval_request_id.to_string()),
        Some(approval_request_id)
    );
    assert!(parse_canonical_audit_archive_id(&archive_id.to_string().to_uppercase()).is_none());
    assert!(
        parse_canonical_approval_request_id(&approval_request_id.to_string().replace('-', ""))
            .is_none()
    );
}

#[tokio::test]
async fn audit_export_object_verification_failures_are_non_retryable_conflicts() {
    let response = audit_export_object_store_error(WormArchiveError::Verification, "request");
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "audit_export_object_verification_failed");
    assert!(!error.retryable);

    let response = audit_export_object_store_error(WormArchiveError::Remote, "request");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "audit_export_object_store_unavailable");
    assert!(error.retryable);
}

#[tokio::test]
async fn agent_mail_admission_errors_are_stable_minimal_and_actionable() {
    let cases = [
        (
            AgentMailAdmissionFailure::RecipientPolicy,
            StatusCode::UNPROCESSABLE_ENTITY,
            "agent_mail_recipient_policy_denied",
            json!({"scope": "recipient_policy"}),
            None,
        ),
        (
            AgentMailAdmissionFailure::HourlyQuota,
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_hourly_quota_exceeded",
            json!({"scope": "agent_hour"}),
            Some("60"),
        ),
        (
            AgentMailAdmissionFailure::RecipientDailyQuota,
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_recipient_daily_quota_exceeded",
            json!({"scope": "recipient_day"}),
            Some("3600"),
        ),
        (
            AgentMailAdmissionFailure::ThreadCircuitBreaker,
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_thread_circuit_breaker_open",
            json!({"scope": "thread_day"}),
            Some("3600"),
        ),
        (
            AgentMailAdmissionFailure::AutomaticReplyLoop,
            StatusCode::CONFLICT,
            "agent_mail_automatic_reply_blocked",
            json!({"scope": "automatic_reply_loop"}),
            None,
        ),
        (
            AgentMailAdmissionFailure::DuplicateContentCircuitBreaker,
            StatusCode::TOO_MANY_REQUESTS,
            "agent_mail_duplicate_content_circuit_breaker_open",
            json!({"scope": "identical_body_day"}),
            Some("3600"),
        ),
        (
            AgentMailAdmissionFailure::SandboxRequired,
            StatusCode::CONFLICT,
            "agent_mail_sandbox_required",
            json!({"scope": "testing_environment"}),
            None,
        ),
    ];
    for (reason, status, code, details, retry_after) in cases {
        let response = agent_mail_admission_api_error(reason, "admission-request");
        assert_eq!(response.status(), status);
        assert_eq!(
            response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok()),
            retry_after
        );
        let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(error.code, code);
        assert_eq!(error.details, details);
        assert_eq!(error.retryable, retry_after.is_some());
        assert!(
            !body
                .windows(b"recipient@example.test".len())
                .any(|window| window == b"recipient@example.test")
        );
    }

    let approval = agent_mail_admission_api_error(
        AgentMailAdmissionFailure::FirstContactApproval,
        "approval-request",
    );
    assert_eq!(approval.status(), StatusCode::CONFLICT);
    let body = to_bytes(approval.into_body(), 16 * 1024).await.unwrap();
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "agent_first_contact_approval_required");
    assert_eq!(error.details["approval_action"], "agent.first_contact");
    assert_eq!(error.details["resource_kind"], "agent.mail_draft");
    assert!(
        !body
            .windows(b"recipient@example.test".len())
            .any(|window| window == b"recipient@example.test")
    );
}

#[test]
fn own_agent_mail_policy_response_is_aggregate_and_privacy_preserving() {
    let organization_id = OrganizationId::new();
    let agent_id = PrincipalId::new();
    let observed_at = "2026-07-27T02:15:00Z".parse::<DateTime<Utc>>().unwrap();
    let response = own_agent_mail_policy_response(
        AgentMailPolicyRuntimeSnapshot {
            organization_id,
            agent_id,
            environment: AgentEnvironment::Production,
            active_policy: Some(
                copilotmail_domain::AgentMailPolicyVersion::new(
                    organization_id,
                    agent_id,
                    4,
                    ["internal.example".into()],
                    10,
                    25,
                    3,
                    true,
                    2,
                )
                .unwrap(),
            ),
            observed_at,
            hourly_window_started_at: "2026-07-27T01:15:00Z".parse::<DateTime<Utc>>().unwrap(),
            admitted_messages_in_hour: 7,
        },
        vec!["agent.operate".into(), "mailbox.use".into()],
    );
    assert_eq!(response.admission_state, "active");
    assert_eq!(response.hourly_usage.remaining_messages, Some(18));
    let body = serde_json::to_string(&response).unwrap();
    for forbidden in [
        "recipient_ref",
        "recipient@example.test",
        "thread_ref",
        "approval_request_id",
        "message_id",
        "engine_",
    ] {
        assert!(!body.contains(forbidden), "{forbidden} leaked: {body}");
    }
}

#[test]
fn agent_inspector_response_is_control_plane_only_and_uses_stable_states() {
    let organization_id = OrganizationId::new();
    let agent_id = PrincipalId::new();
    let owner_id = PrincipalId::new();
    let observed_at = "2026-07-27T02:15:00Z".parse::<DateTime<Utc>>().unwrap();
    let response = agent_inspector_response(AgentInspectorSnapshot {
        organization_id,
        agent: copilotmail_application::AgentInspectorAgent {
            principal_id: agent_id,
            display_name: "Inbound triage".into(),
            principal_status: PrincipalStatus::Active,
        },
        owner: copilotmail_application::AgentInspectorOwner {
            principal_id: owner_id,
            display_name: "Owner".into(),
            principal_status: PrincipalStatus::Active,
        },
        profile: copilotmail_application::AgentInspectorProfile {
            environment: AgentEnvironment::Production,
            risk_level: AgentRiskLevel::Medium,
            purpose: "Triage inbound support mail".into(),
            review_due_at: "2026-08-01T00:00:00Z".parse().unwrap(),
            review_state: copilotmail_application::AgentInspectorReviewState::Scheduled,
        },
        token_access: copilotmail_application::AgentInspectorTokenAccess {
            state: copilotmail_application::AgentInspectorTokenAccessState::Effective,
            effective_scopes: [Permission::OrganizationRead, Permission::AgentOperate]
                .into_iter()
                .collect(),
            active_credential_count: 2,
            next_credential_expiry_at: Some("2026-08-10T00:00:00Z".parse().unwrap()),
            last_authenticated_at: Some("2026-07-27T02:14:00Z".parse().unwrap()),
        },
        control_plane: copilotmail_application::AgentInspectorControlPlane {
            mail_admission_state: copilotmail_application::AgentInspectorMailAdmissionState::Active,
            active_processing_subscription_count: 1,
            paused_processing_subscription_count: 0,
            retired_processing_subscription_count: 0,
            last_processing_activity_at: Some("2026-07-27T02:13:00Z".parse().unwrap()),
        },
        observed_at,
    });

    assert_eq!(response.profile.review_state, "scheduled");
    assert_eq!(response.control_plane.mail_admission_state, "active");
    assert_eq!(
        response.token_access.effective_scopes,
        ["agent.operate", "organization.read"]
    );
    let body = serde_json::to_string(&response).unwrap();
    for forbidden in [
        "credential_id",
        "fingerprint",
        "mailbox_id",
        "delegation",
        "message_id",
        "recipient",
        "source_ip",
    ] {
        assert!(!body.contains(forbidden), "{forbidden} leaked: {body}");
    }
}

#[tokio::test]
async fn agent_inspector_errors_hide_target_existence_and_preserve_retry_semantics() {
    let response = agent_inspector_api_error(AgentInspectorError::NotFound, "request-id");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "resource_not_found");
    assert!(!error.retryable);

    let response = agent_inspector_api_error(AgentInspectorError::Unavailable, "request-id");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "agent_inspector_unavailable");
    assert!(error.retryable);
}

#[test]
fn agent_mail_submission_evidence_response_is_minimized_and_not_a_delivery_claim() {
    let now = "2026-08-24T09:03:00Z".parse::<DateTime<Utc>>().unwrap();
    let evidence = AgentMailSubmissionEvidence {
        agent_id: PrincipalId::new(),
        submission_id: MailSubmissionId::new(),
        submission_status: copilotmail_application::AgentMailSubmissionStatusEvidence::Accepted,
        submission_created_at: "2026-08-24T09:00:00Z".parse().unwrap(),
        submission_updated_at: "2026-08-24T09:01:00Z".parse().unwrap(),
        admission: copilotmail_application::AgentMailSubmissionAdmissionEvidence {
            admission_id: Uuid::now_v7(),
            admitted_at: "2026-08-24T09:00:01Z".parse().unwrap(),
            policy_activation_id: Uuid::now_v7(),
            policy_activated_at: "2026-08-24T08:00:00Z".parse().unwrap(),
            policy_version_id: Uuid::now_v7(),
            policy_version: 2,
            policy_activation_approval_request_id: ApprovalRequestId::new(),
            policy_activation_approval_verified: true,
            recipient_count: 2,
            first_contact_count: 1,
            first_contact_approval_request_id: Some(ApprovalRequestId::new()),
            first_contact_approval_verified: true,
        },
        delivery: Some(
            copilotmail_application::AgentMailSubmissionDeliveryEvidence {
                observed_at: "2026-08-24T09:02:00Z".parse().unwrap(),
                terminal: false,
                recipient_count: 2,
                delivered_count: 1,
                failed_count: 0,
                queued_count: 1,
                unknown_count: 0,
            },
        ),
        audit_references: vec![
            copilotmail_application::AgentMailSubmissionAuditReference {
                event_id: Uuid::now_v7(),
                action: "mail.submission.requested".into(),
                occurred_at: "2026-08-24T09:00:00Z".parse().unwrap(),
            },
            copilotmail_application::AgentMailSubmissionAuditReference {
                event_id: Uuid::now_v7(),
                action: "agent.mail.admitted".into(),
                occurred_at: "2026-08-24T09:00:01Z".parse().unwrap(),
            },
            copilotmail_application::AgentMailSubmissionAuditReference {
                event_id: Uuid::now_v7(),
                action: "mail.submission.accepted".into(),
                occurred_at: "2026-08-24T09:01:00Z".parse().unwrap(),
            },
        ],
        observed_at: now,
    };
    let response = agent_mail_submission_evidence_response(evidence);
    assert_eq!(response.submission.status, "accepted");
    assert!(!response.delivery.as_ref().unwrap().terminal);
    assert_eq!(response.audit_references.len(), 3);
    let body = serde_json::to_string(&response).unwrap();
    for forbidden in [
        "mailbox_id",
        "draft_id",
        "recipient@example.test",
        "smtp",
        "provider",
        "execution",
        "snapshot_id",
        "engine_",
    ] {
        assert!(!body.contains(forbidden), "{forbidden} leaked: {body}");
    }
}

#[tokio::test]
async fn agent_mail_submission_evidence_errors_hide_target_existence_and_preserve_retry_semantics()
{
    let response = agent_mail_submission_evidence_api_error(
        AgentMailSubmissionEvidenceError::NotFound,
        "request-id",
    );
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "resource_not_found");
    assert!(!error.retryable);

    let response = agent_mail_submission_evidence_api_error(
        AgentMailSubmissionEvidenceError::Unavailable,
        "request-id",
    );
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let error: ErrorResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(error.code, "agent_mail_submission_evidence_unavailable");
    assert!(error.retryable);
}

#[tokio::test]
async fn signing_key_manifest_resolves_relative_secret_files() {
    let directory = std::env::temp_dir().join(format!("copilotmail-keyset-{}", Uuid::now_v7()));
    tokio::fs::create_dir(&directory).await.unwrap();
    tokio::fs::write(directory.join("active-private.pem"), b"private")
        .await
        .unwrap();
    tokio::fs::write(directory.join("active-public.pem"), b"public")
        .await
        .unwrap();
    tokio::fs::write(
        directory.join("keyset.json"),
        br#"{
            "active_key_id": "active-2026-07",
            "keys": [{
                "key_id": "active-2026-07",
                "private_key_file": "active-private.pem",
                "public_key_file": "active-public.pem"
            }]
        }"#,
    )
    .await
    .unwrap();
    let settings = AgentOAuthSettings {
        issuer: "https://mail.example.test".into(),
        audience: "https://mail.example.test/api/v2".into(),
        token_endpoint: "https://mail.example.test/oauth2/token".into(),
        signing_key_id: None,
        private_key_file: None,
        public_key_file: None,
        signing_key_set_file: Some(directory.join("keyset.json")),
        signing_key_reload_seconds: 30,
        access_token_ttl_seconds: 300,
        allow_insecure_http: false,
    };
    let loaded = load_agent_signing_key_set(&settings).await.unwrap();
    assert_eq!(loaded.active_key_id, "active-2026-07");
    assert_eq!(loaded.keys.len(), 1);
    assert_eq!(
        loaded.keys[0].private_key_pem.as_deref(),
        Some(b"private".as_slice())
    );
    assert_eq!(loaded.keys[0].public_key_pem, b"public");
    assert_eq!(loaded.digest.len(), 32);
    tokio::fs::remove_dir_all(directory).await.unwrap();
}

const CONTRACT_SENTINEL: &str = "CM-LEAK-\u{202e}'; DROP TABLE audit_events;--\r\nBcc: leak@example.test/../.well-known/jmap Email/set";

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn closed_query<T: serde::de::DeserializeOwned>(query: &str) -> Result<T, &'static str> {
    let uri: axum::http::Uri = if query.is_empty() {
        "https://mail.example.test/resource".parse().unwrap()
    } else {
        format!("https://mail.example.test/resource?{query}")
            .parse()
            .unwrap()
    };
    match Query::<T>::try_from_uri(&uri) {
        Ok(Query(value)) => Ok(value),
        Err(_) => Err("request payload is invalid"),
    }
}

fn assert_query_closed<T: serde::de::DeserializeOwned>(query: &str) {
    assert_eq!(
        closed_query::<T>(query).err(),
        Some("request payload is invalid")
    );
}

async fn assert_redacted_error(response: Response, status: StatusCode, code: &str, message: &str) {
    assert_eq!(response.status(), status);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("application/json"),
        "{content_type}"
    );
    let request_header = response
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    for needle in [
        "CM-LEAK",
        "DROP TABLE",
        "Bcc:",
        "Email/set",
        "Failed to deserialize",
        "Invalid URL:",
        "Expected request with",
    ] {
        assert!(!text.contains(needle), "{text}");
    }
    assert!(!text.contains('\u{202e}'), "{text}");
    let error: ErrorResponse = serde_json::from_str(&text).unwrap();
    assert_eq!(error.code, code);
    assert_eq!(error.message, message);
    assert!(!error.retryable);
    assert_eq!(error.request_id, request_header);
    assert_eq!(error.trace_id, request_header);
}

#[tokio::test]
async fn protected_contract_rejections_do_not_echo_the_payload() {
    let mailbox_id = "018f13b9-1111-7bc8-965d-48a28866f073";
    let app = router(disconnected_store());
    let payload = serde_json::json!({
        "text": "quarterly",
        "cursor": null,
        "page_size": CONTRACT_SENTINEL
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/mailboxes/{mailbox_id}/messages/search"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-request-id", "contract-b31")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_redacted_error(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "invalid_request",
        "request payload is invalid",
    )
    .await;

    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v2/mailboxes/{mailbox_id}/messages?page_size={}",
                    percent_encode(CONTRACT_SENTINEL)
                ))
                .header("x-request-id", "contract-query")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_redacted_error(
        response,
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "request payload is invalid",
    )
    .await;

    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v2/mailboxes/{mailbox_id}/messages?page_size=4294967296"
                ))
                .header("x-request-id", "contract-bound")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_redacted_error(
        response,
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "request payload is invalid",
    )
    .await;

    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v2/mailboxes/{}",
                    percent_encode(CONTRACT_SENTINEL)
                ))
                .header("x-request-id", "contract-path")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_redacted_error(
        response,
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "request payload is invalid",
    )
    .await;

    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v2/mailboxes/{mailbox_id}/messages/search"))
                .header("x-request-id", "contract-type")
                .body(Body::from(r#"{"text":"quarterly"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_redacted_error(
        response,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "invalid_content_type",
        "Content-Type is not accepted",
    )
    .await;

    let response = router(disconnected_store())
        .oneshot(
            Request::builder()
                .uri("/missing")
                .header("x-request-id", "contract-missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_redacted_error(
        response,
        StatusCode::NOT_FOUND,
        "route_not_found",
        "the requested API route does not exist",
    )
    .await;
}

#[tokio::test]
async fn unrelated_plain_text_and_scim_extractor_errors_keep_their_contracts() {
    let preserved = Router::new()
        .route(
            "/plain",
            get(|| async { (StatusCode::BAD_REQUEST, "policy denied") }),
        )
        .layer(middleware::from_fn(redact_extractor_disclosure));
    let response = preserved
        .oneshot(
            Request::builder()
                .uri("/plain")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(body.as_ref(), b"policy denied");

    let disclosed = Router::new()
        .route(
            "/scim/v2/Users",
            get(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Failed to deserialize query string: {CONTRACT_SENTINEL}"),
                )
                    .into_response()
            }),
        )
        .layer(middleware::from_fn(redact_extractor_disclosure));
    let response = disclosed
        .oneshot(
            Request::builder()
                .uri("/scim/v2/Users")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/scim+json"
    );
    let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("CM-LEAK"), "{text}");
    assert!(!text.contains("DROP TABLE"), "{text}");
    assert!(!text.contains('\u{202e}'), "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value["schemas"][0],
        "urn:ietf:params:scim:api:messages:2.0:Error"
    );
    assert_eq!(value["status"], "400");
    assert_eq!(value["scimType"], "invalidValue");
    assert_eq!(value["detail"], "request payload is invalid");
    assert!(value.get("request_id").is_none());
}

#[tokio::test]
async fn oauth_token_form_keeps_protocol_errors_and_ignores_extension_fields() {
    let missing_grant = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/oauth2/token")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "client_id={}",
                    percent_encode(CONTRACT_SENTINEL)
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_grant.status(), StatusCode::BAD_REQUEST);
    assert_eq!(missing_grant.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(missing_grant.headers()[header::PRAGMA], "no-cache");
    let body = to_bytes(missing_grant.into_body(), 16 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("CM-LEAK"), "{text}");
    assert!(!text.contains('\u{202e}'), "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["error"], "invalid_request");
    assert_eq!(
        value["error_description"],
        "the token request form is invalid"
    );

    let extension = router(disconnected_store())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/oauth2/token")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "grant_type=client_credentials&client_id=abc&resource={}",
                    percent_encode(CONTRACT_SENTINEL)
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(extension.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(extension.headers()[header::CACHE_CONTROL], "no-store");
    let body = to_bytes(extension.into_body(), 16 * 1024).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("CM-LEAK"), "{text}");
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["error"], "temporarily_unavailable");
    assert_eq!(value["error_description"], "Agent OAuth is not configured");
}

#[test]
fn protected_query_contracts_reject_unknown_fields_and_bad_limits() {
    let encoded = percent_encode(CONTRACT_SENTINEL);
    assert!(closed_query::<AgentEventCursorQuery>("").is_ok());
    assert_query_closed::<AgentEventCursorQuery>("injected=1");
    assert!(closed_query::<AgentEventCursorQuery>("limit=65535").is_ok());
    assert_query_closed::<AgentEventCursorQuery>("limit=65536");
    assert_query_closed::<AgentEventCursorQuery>("limit=-1");
    assert_query_closed::<AgentEventCursorQuery>(&format!("limit={encoded}"));

    assert!(closed_query::<AgentEventStreamQuery>("").is_ok());
    assert_query_closed::<AgentEventStreamQuery>("injected=1");
    assert!(closed_query::<AgentEventStreamQuery>(&format!("cursor={encoded}")).is_ok());

    assert!(closed_query::<HumanLoginQuery>("").is_ok());
    assert_query_closed::<HumanLoginQuery>("injected=1");
    let login = closed_query::<HumanLoginQuery>(&format!("return_to={encoded}")).unwrap();
    assert_eq!(login.return_to.as_deref(), Some(CONTRACT_SENTINEL));

    assert!(closed_query::<WebhookSubscriptionListQuery>("").is_ok());
    assert_query_closed::<WebhookSubscriptionListQuery>("injected=1");
    assert!(closed_query::<WebhookSubscriptionListQuery>("limit=65535").is_ok());
    assert_query_closed::<WebhookSubscriptionListQuery>("limit=65536");

    assert!(closed_query::<WebhookDeadLetterListQuery>("").is_ok());
    assert_query_closed::<WebhookDeadLetterListQuery>("injected=1");
    assert_query_closed::<WebhookDeadLetterListQuery>(&format!("limit={encoded}"));

    assert!(closed_query::<HumanMailboxDirectoryQuery>("").is_ok());
    assert_query_closed::<HumanMailboxDirectoryQuery>("injected=1");
    assert!(closed_query::<HumanMailboxDirectoryQuery>("limit=65535").is_ok());
    assert_query_closed::<HumanMailboxDirectoryQuery>("limit=65536");

    assert!(closed_query::<HumanDraftListQuery>("").is_ok());
    assert_query_closed::<HumanDraftListQuery>("injected=1");
    assert_query_closed::<HumanDraftListQuery>("limit=-1");

    assert!(closed_query::<HumanSentListQuery>("").is_ok());
    assert_query_closed::<HumanSentListQuery>("injected=1");
    assert_query_closed::<HumanSentListQuery>(&format!("cursor={encoded}&limit=65536"));

    assert!(closed_query::<RoleAssignmentHttpQuery>("").is_ok());
    assert_query_closed::<RoleAssignmentHttpQuery>("injected=1");
    assert_query_closed::<RoleAssignmentHttpQuery>("principal_id=not-a-uuid");
    assert_query_closed::<RoleAssignmentHttpQuery>("include_inactive=not-bool");
    assert!(
        closed_query::<RoleAssignmentHttpQuery>(
            "principal_id=018f13b9-1111-7bc8-965d-48a28866f073&include_inactive=true&limit=-1"
        )
        .is_ok()
    );
    assert_query_closed::<RoleAssignmentHttpQuery>("limit=9223372036854775808");

    assert!(closed_query::<ComplianceCaseListQuery>("").is_ok());
    assert_query_closed::<ComplianceCaseListQuery>("injected=1");
    assert!(closed_query::<ComplianceCaseListQuery>("limit=-1").is_ok());
    assert_query_closed::<ComplianceCaseListQuery>("limit=9223372036854775808");

    assert!(closed_query::<LegalHoldListQuery>("").is_ok());
    assert_query_closed::<LegalHoldListQuery>("injected=1");
    assert!(closed_query::<LegalHoldListQuery>(&format!("status={encoded}&limit=-1")).is_ok());
    assert!(closed_query::<LegalHoldListQuery>("limit=-1").is_ok());

    assert!(closed_query::<InboxListQuery>("").is_ok());
    assert_query_closed::<InboxListQuery>("injected=1");
    assert!(closed_query::<InboxListQuery>("page_size=0").is_ok());
    assert!(closed_query::<InboxListQuery>("page_size=4294967295").is_ok());
    assert_query_closed::<InboxListQuery>("page_size=4294967296");
    assert_query_closed::<InboxListQuery>("page_size=-1");
    assert_query_closed::<InboxListQuery>(&format!("page_size={encoded}"));

    assert!(closed_query::<AuditEventSearchQuery>("").is_ok());
    assert_query_closed::<AuditEventSearchQuery>("injected=1");
    assert!(closed_query::<AuditEventSearchQuery>("limit=65535").is_ok());
    assert_query_closed::<AuditEventSearchQuery>("limit=65536");

    assert!(closed_query::<MailDomainListQuery>("").is_ok());
    assert_query_closed::<MailDomainListQuery>("injected=1");
    assert_query_closed::<MailDomainListQuery>(&format!("after={encoded}&limit=65536"));

    assert!(closed_query::<AgentProcessingDeadLetterQueryParams>("").is_ok());
    assert_query_closed::<AgentProcessingDeadLetterQueryParams>("injected=1");
    assert!(closed_query::<AgentProcessingDeadLetterQueryParams>("limit=65535").is_ok());
    assert_query_closed::<AgentProcessingDeadLetterQueryParams>("limit=65536");

    assert!(closed_query::<ApprovalRequestListQuery>("").is_ok());
    assert_query_closed::<ApprovalRequestListQuery>("injected=1");
    assert!(closed_query::<ApprovalRequestListQuery>("limit=-1").is_ok());
    assert_query_closed::<ApprovalRequestListQuery>("limit=9223372036854775808");
    assert!(closed_query::<ApprovalRequestListQuery>(&format!("view={encoded}")).is_ok());
}
