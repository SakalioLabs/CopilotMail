//! Human-only Agent inspection HTTP adapters. Business rules live in application.

use super::*;

/// Reads one Human-admin-only, evidence-backed control-plane snapshot for an
/// Agent. This does not expose a runtime heartbeat, credential material,
/// mailbox grants, delegations, or mail data.
pub(super) async fn get_agent_inspector(
    State(state): State<Arc<AppState>>,
    // Keep the resource identifier opaque until authentication and Human/RBAC
    // checks complete. Axum's typed path extraction would otherwise return a
    // framework 400 for malformed IDs before the route's explicit 401 guard.
    Path(agent_id): Path<String>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "Agent Inspector requires an authenticated Human Principal",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::IdentityManage,
        "identity.manage permission is required to inspect an Agent",
        &request_id,
    )?;
    let agent_id = agent_id.parse::<PrincipalId>().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_inspector_request",
            "agent_id must be a valid UUID",
            false,
            &request_id,
        )
    })?;

    let snapshot = AgentInspectorService::new(state.store.clone())
        .get(GetAgentInspectorCommand {
            context: authorization.clone(),
            organization_id: authorization.organization_id,
            agent_id,
            trace_id: request_id.clone(),
        })
        .await
        .map_err(|error| agent_inspector_api_error(error, &request_id))?;
    let mut response = Json(agent_inspector_response(snapshot)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("referrer-policy"),
        header::HeaderValue::from_static("no-referrer"),
    );
    Ok(response)
}

pub(super) fn agent_inspector_response(snapshot: AgentInspectorSnapshot) -> AgentInspectorResponse {
    // Preserve a stable wire order independent of the domain enum declaration.
    // It makes equivalent scope sets deterministic for API clients and audit
    // comparison without exposing any per-credential scope list.
    let mut effective_scopes = snapshot
        .token_access
        .effective_scopes
        .iter()
        .map(|permission| permission.as_str().to_owned())
        .collect::<Vec<_>>();
    effective_scopes.sort_unstable();
    AgentInspectorResponse {
        agent_id: snapshot.agent.principal_id.to_string(),
        display_name: snapshot.agent.display_name,
        principal_status: principal_status_wire(snapshot.agent.principal_status).into(),
        owner: AgentInspectorOwnerResponse {
            principal_id: snapshot.owner.principal_id.to_string(),
            display_name: snapshot.owner.display_name,
            principal_status: principal_status_wire(snapshot.owner.principal_status).into(),
        },
        profile: AgentInspectorProfileResponse {
            environment: agent_environment_wire(snapshot.profile.environment).into(),
            risk_level: agent_risk_level_wire(snapshot.profile.risk_level).into(),
            purpose: snapshot.profile.purpose,
            review_due_at: snapshot.profile.review_due_at.to_rfc3339(),
            review_state: snapshot.profile.review_state.as_str().into(),
        },
        token_access: AgentInspectorTokenAccessResponse {
            state: snapshot.token_access.state.as_str().into(),
            effective_scopes,
            active_credential_count: snapshot.token_access.active_credential_count,
            next_credential_expiry_at: snapshot
                .token_access
                .next_credential_expiry_at
                .map(|value| value.to_rfc3339()),
            last_authenticated_at: snapshot
                .token_access
                .last_authenticated_at
                .map(|value| value.to_rfc3339()),
        },
        control_plane: AgentInspectorControlPlaneResponse {
            mail_admission_state: snapshot.control_plane.mail_admission_state.as_str().into(),
            active_processing_subscription_count: snapshot
                .control_plane
                .active_processing_subscription_count,
            paused_processing_subscription_count: snapshot
                .control_plane
                .paused_processing_subscription_count,
            retired_processing_subscription_count: snapshot
                .control_plane
                .retired_processing_subscription_count,
            last_processing_activity_at: snapshot
                .control_plane
                .last_processing_activity_at
                .map(|value| value.to_rfc3339()),
        },
        observed_at: snapshot.observed_at.to_rfc3339(),
    }
}

/// Reads the bounded, exact production-mail evidence chain for one Agent and
/// one submission. This route intentionally offers no list, runtime health,
/// execution, delegation, mailbox, recipient, provider, SMTP, or content
/// disclosure.
pub(super) async fn get_agent_mail_submission_evidence(
    State(state): State<Arc<AppState>>,
    // Keep both opaque resource identifiers as strings until authentication
    // and Human/RBAC checks complete. Typed extraction would otherwise expose
    // malformed-path parsing before the route's explicit 401 boundary.
    Path((agent_id, submission_id)): Path<(String, String)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (identity, authorization) =
        authenticate_api_request(&state, &headers, connection, &request_id).await?;
    if identity.principal_kind != AuthenticatedPrincipalKind::Human {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "Agent submission evidence requires an authenticated Human Principal",
            false,
            &request_id,
        ));
    }
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::IdentityManage,
        "identity.manage permission is required to inspect Agent submission evidence",
        &request_id,
    )?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuditRead,
        "audit.read permission is required to inspect Agent submission evidence",
        &request_id,
    )?;
    let agent_id_wire = agent_id;
    let agent_id = agent_id_wire.parse::<PrincipalId>().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_submission_evidence_request",
            "agent_id must be a canonical UUID",
            false,
            &request_id,
        )
    })?;
    if agent_id.to_string() != agent_id_wire {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_submission_evidence_request",
            "agent_id must be a canonical UUID",
            false,
            &request_id,
        ));
    }
    let submission_id_wire = submission_id;
    let submission_id = submission_id_wire
        .parse::<MailSubmissionId>()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_agent_mail_submission_evidence_request",
                "submission_id must be a canonical UUID",
                false,
                &request_id,
            )
        })?;
    if submission_id.to_string() != submission_id_wire {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "invalid_agent_mail_submission_evidence_request",
            "submission_id must be a canonical UUID",
            false,
            &request_id,
        ));
    }

    let evidence = AgentMailSubmissionEvidenceService::new(state.store.clone())
        .get(GetAgentMailSubmissionEvidenceCommand {
            context: authorization.clone(),
            organization_id: authorization.organization_id,
            agent_id,
            submission_id,
            trace_id: request_id.clone(),
        })
        .await
        .map_err(|error| agent_mail_submission_evidence_api_error(error, &request_id))?;
    let mut response = Json(agent_mail_submission_evidence_response(evidence)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("referrer-policy"),
        header::HeaderValue::from_static("no-referrer"),
    );
    Ok(response)
}

pub(super) fn agent_mail_submission_evidence_response(
    evidence: AgentMailSubmissionEvidence,
) -> AgentMailSubmissionEvidenceResponse {
    AgentMailSubmissionEvidenceResponse {
        agent_id: evidence.agent_id.to_string(),
        submission: AgentMailSubmissionEvidenceSubmissionResponse {
            submission_id: evidence.submission_id.to_string(),
            status: evidence.submission_status.as_str().into(),
            created_at: evidence.submission_created_at.to_rfc3339(),
            updated_at: evidence.submission_updated_at.to_rfc3339(),
        },
        admission: AgentMailSubmissionEvidenceAdmissionResponse {
            admission_id: evidence.admission.admission_id.to_string(),
            admitted_at: evidence.admission.admitted_at.to_rfc3339(),
            policy: AgentMailSubmissionEvidencePolicyResponse {
                activation_id: evidence.admission.policy_activation_id.to_string(),
                activated_at: evidence.admission.policy_activated_at.to_rfc3339(),
                version_id: evidence.admission.policy_version_id.to_string(),
                version: evidence.admission.policy_version,
                approval_request_id: evidence
                    .admission
                    .policy_activation_approval_request_id
                    .to_string(),
            },
            recipient_count: evidence.admission.recipient_count,
            first_contact_count: evidence.admission.first_contact_count,
            first_contact_approval_request_id: evidence
                .admission
                .first_contact_approval_request_id
                .map(|request_id| request_id.to_string()),
        },
        delivery: evidence
            .delivery
            .map(|delivery| AgentMailSubmissionEvidenceDeliveryResponse {
                observed_at: delivery.observed_at.to_rfc3339(),
                terminal: delivery.terminal,
                recipient_count: delivery.recipient_count,
                delivered_count: delivery.delivered_count,
                failed_count: delivery.failed_count,
                queued_count: delivery.queued_count,
                unknown_count: delivery.unknown_count,
            }),
        audit_references: evidence
            .audit_references
            .into_iter()
            .map(
                |reference| AgentMailSubmissionEvidenceAuditReferenceResponse {
                    event_id: reference.event_id.to_string(),
                    action: reference.action,
                    occurred_at: reference.occurred_at.to_rfc3339(),
                },
            )
            .collect(),
        observed_at: evidence.observed_at.to_rfc3339(),
    }
}
