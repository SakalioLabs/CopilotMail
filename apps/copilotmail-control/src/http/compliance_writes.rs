use crate::*;

pub(crate) async fn create_compliance_case(
    State(state): State<Arc<AppState>>,
    Path(organization_id): Path<OrganizationId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<ComplianceCaseCreateRequest>,
) -> Result<(StatusCode, Json<ComplianceCaseResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    if authorization.organization_id != organization_id {
        return Err(resource_not_found(&request_id));
    }
    let case = ComplianceGovernanceService::new(state.store.clone())
        .create_case(CreateComplianceCaseCommand {
            context: authorization,
            external_reference: body.external_reference,
            title: body.title,
            purpose: body.purpose,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| compliance_api_error(error, &request_id))?;
    Ok((StatusCode::CREATED, Json(compliance_case_response(case))))
}

pub(crate) async fn get_compliance_case(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<ComplianceCaseId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ComplianceCaseResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let case = state
        .store
        .get_compliance_case(authorization.organization_id, case_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::Case,
        case_id.to_string(),
        serde_json::json!({"operation": "get"}),
        &request_id,
    )
    .await?;
    Ok(Json(compliance_case_response(case)))
}

pub(crate) async fn close_compliance_case(
    State(state): State<Arc<AppState>>,
    Path(case_id): Path<ComplianceCaseId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<ComplianceCaseResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let case = ComplianceGovernanceService::new(state.store.clone())
        .close_case(CloseComplianceCaseCommand {
            context: authorization,
            case_id,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| compliance_api_error(error, &request_id))?;
    Ok(Json(compliance_case_response(case)))
}

pub(crate) async fn apply_audit_legal_hold(
    State(state): State<Arc<AppState>>,
    Path(archive_id): Path<AuditArchiveId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<LegalHoldApplyRequest>,
) -> Result<(StatusCode, Json<LegalHoldResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let case_id = body.case_id.parse::<ComplianceCaseId>().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "invalid_compliance_case_id",
            "case_id must be a UUID",
            false,
            &request_id,
        )
    })?;
    let approval_request_id = parse_legal_hold_approval_id(&body.approval_request_id)
        .ok_or_else(|| invalid_legal_hold_approval_id(&request_id))?;
    let approval = ApprovalProof {
        organization_id: authorization.organization_id,
        request_id: approval_request_id,
        requested_by: authorization.principal_id,
        action: ApprovalAction::LegalHold,
        resource_kind: "audit.archive".into(),
        resource_id: archive_id.to_string(),
        now: Utc::now(),
    };
    ensure_legal_hold_approval(&state, &approval, &request_id).await?;
    if let Some(existing) = state
        .store
        .get_legal_hold_by_apply_approval(authorization.organization_id, approval_request_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
    {
        if exact_active_hold_replay(&existing, case_id, archive_id, &body.reason) {
            return Ok((StatusCode::OK, Json(legal_hold_response(existing))));
        }
        return Err(legal_hold_conflict(&request_id));
    }
    let lock = state
        .store
        .try_acquire_audit_legal_hold_lock(authorization.organization_id, archive_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| legal_hold_conflict(&request_id))?;
    let result = async {
        if let Some(existing) = state
            .store
            .get_legal_hold_by_apply_approval(authorization.organization_id, approval_request_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
        {
            return if exact_active_hold_replay(&existing, case_id, archive_id, &body.reason) {
                Ok(existing)
            } else {
                Err(legal_hold_conflict(&request_id))
            };
        }
        let record = state
            .store
            .get_audit_archive(authorization.organization_id, archive_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
            .ok_or_else(|| resource_not_found(&request_id))?;
        let archive_settings = state
            .audit_archive
            .as_ref()
            .ok_or_else(|| legal_hold_object_store_unavailable(&request_id))?;
        let archive = connect_legal_hold_archive_settings(archive_settings, &record)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let events = archive
            .set_legal_hold(&record.events.key, &record.events.version_id, true)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let manifest = archive
            .set_legal_hold(&record.manifest.key, &record.manifest.version_id, true)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        ComplianceGovernanceService::new(state.store.clone())
            .apply_hold(ApplyLegalHoldCommand {
                context: authorization,
                case_id,
                archive_id,
                reason: body.reason,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: true,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: request_id.clone(),
                now: Utc::now(),
            })
            .await
            .map_err(|error| compliance_api_error(error, &request_id))
    }
    .await;
    lock.release()
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    let hold = result?;
    Ok((StatusCode::CREATED, Json(legal_hold_response(hold))))
}

pub(crate) async fn get_legal_hold(
    State(state): State<Arc<AppState>>,
    Path(hold_id): Path<LegalHoldId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<LegalHoldResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let hold = state
        .store
        .get_legal_hold(authorization.organization_id, hold_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    record_compliance_read(
        &state,
        authorization,
        ComplianceReadKind::LegalHold,
        hold_id.to_string(),
        serde_json::json!({"operation": "get"}),
        &request_id,
    )
    .await?;
    Ok(Json(legal_hold_response(hold)))
}

pub(crate) async fn release_audit_legal_hold(
    State(state): State<Arc<AppState>>,
    Path(hold_id): Path<LegalHoldId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<LegalHoldReleaseRequest>,
) -> Result<Json<LegalHoldResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_compliance_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = parse_legal_hold_approval_id(&body.approval_request_id)
        .ok_or_else(|| invalid_legal_hold_approval_id(&request_id))?;
    let approval = ApprovalProof {
        organization_id: authorization.organization_id,
        request_id: approval_request_id,
        requested_by: authorization.principal_id,
        action: ApprovalAction::LegalHold,
        resource_kind: "compliance.legal_hold".into(),
        resource_id: hold_id.to_string(),
        now: Utc::now(),
    };
    ensure_legal_hold_approval(&state, &approval, &request_id).await?;
    let current = state
        .store
        .get_legal_hold(authorization.organization_id, hold_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    if current.status() == LegalHoldStatus::Released {
        return if current.release_approval_request_id() == Some(approval_request_id) {
            Ok(Json(legal_hold_response(current)))
        } else {
            Err(legal_hold_conflict(&request_id))
        };
    }
    let archive_id = current.archive_id();
    let lock = state
        .store
        .try_acquire_audit_legal_hold_lock(authorization.organization_id, archive_id)
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?
        .ok_or_else(|| legal_hold_conflict(&request_id))?;
    let result = async {
        let current = state
            .store
            .get_legal_hold(authorization.organization_id, hold_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
            .ok_or_else(|| resource_not_found(&request_id))?;
        if current.status() == LegalHoldStatus::Released {
            return if current.release_approval_request_id() == Some(approval_request_id) {
                Ok(current)
            } else {
                Err(legal_hold_conflict(&request_id))
            };
        }
        let record = state
            .store
            .get_audit_archive(authorization.organization_id, archive_id)
            .await
            .map_err(|_| compliance_store_unavailable(&request_id))?
            .ok_or_else(|| resource_not_found(&request_id))?;
        let archive_settings = state
            .audit_archive
            .as_ref()
            .ok_or_else(|| legal_hold_object_store_unavailable(&request_id))?;
        let archive = connect_legal_hold_archive_settings(archive_settings, &record)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let manifest = archive
            .set_legal_hold(&record.manifest.key, &record.manifest.version_id, false)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        let events = archive
            .set_legal_hold(&record.events.key, &record.events.version_id, false)
            .await
            .map_err(|_| legal_hold_object_store_unavailable(&request_id))?;
        ComplianceGovernanceService::new(state.store.clone())
            .release_hold(ReleaseLegalHoldCommand {
                context: authorization,
                hold_id,
                approval,
                evidence: LegalHoldObjectEvidence {
                    events_version_id: events.version_id,
                    manifest_version_id: manifest.version_id,
                    enabled: false,
                    verified_at: events.verified_at.max(manifest.verified_at),
                },
                trace_id: request_id.clone(),
                now: Utc::now(),
            })
            .await
            .map_err(|error| compliance_api_error(error, &request_id))
    }
    .await;
    lock.release()
        .await
        .map_err(|_| compliance_store_unavailable(&request_id))?;
    Ok(Json(legal_hold_response(result?)))
}

pub(crate) async fn authenticate_compliance_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::LegalHoldManage,
        "legal_hold.manage permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| compliance_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "compliance operations require an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

pub(crate) async fn ensure_legal_hold_approval(
    state: &AppState,
    approval: &ApprovalProof,
    request_id: &str,
) -> Result<(), Response> {
    if !state
        .store
        .approval_permits(approval)
        .await
        .map_err(|_| compliance_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "legal_hold_approval_required",
            "an exact approved and unexpired Legal Hold request is required",
            false,
            request_id,
        ));
    }
    Ok(())
}

pub(crate) async fn record_compliance_read(
    state: &AppState,
    context: AuthorizationContext,
    kind: ComplianceReadKind,
    resource_id: String,
    details: serde_json::Value,
    request_id: &str,
) -> Result<(), Response> {
    ComplianceGovernanceService::new(state.store.clone())
        .record_read(RecordComplianceReadCommand {
            context,
            kind,
            resource_id,
            details,
            trace_id: request_id.into(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| compliance_api_error(error, request_id))
}

pub(crate) fn parse_legal_hold_approval_id(value: &str) -> Option<ApprovalRequestId> {
    value.parse().ok()
}

pub(crate) fn invalid_legal_hold_approval_id(request_id: &str) -> Response {
    api_error(
        StatusCode::BAD_REQUEST,
        "invalid_approval_request_id",
        "approval_request_id must be a UUID",
        false,
        request_id,
    )
}

pub(crate) fn exact_active_hold_replay(
    hold: &LegalHold,
    case_id: ComplianceCaseId,
    archive_id: AuditArchiveId,
    reason: &str,
) -> bool {
    hold.case_id() == case_id
        && hold.archive_id() == archive_id
        && hold.reason() == reason
        && hold.status() == LegalHoldStatus::Active
}

pub(crate) fn compliance_case_response(case: ComplianceCase) -> ComplianceCaseResponse {
    ComplianceCaseResponse {
        case_id: case.id().to_string(),
        organization_id: case.organization_id().to_string(),
        external_reference: case.external_reference().into(),
        title: case.title().into(),
        purpose: case.purpose().into(),
        status: match case.status() {
            ComplianceCaseStatus::Open => "open",
            ComplianceCaseStatus::Closed => "closed",
        }
        .into(),
        created_by: case.created_by().to_string(),
        created_at: case.created_at().to_rfc3339(),
        closed_by: case.closed_by().map(|value| value.to_string()),
        closed_at: case.closed_at().map(|value| value.to_rfc3339()),
    }
}

pub(crate) fn legal_hold_response(hold: LegalHold) -> LegalHoldResponse {
    LegalHoldResponse {
        legal_hold_id: hold.id().to_string(),
        organization_id: hold.organization_id().to_string(),
        case_id: hold.case_id().to_string(),
        archive_id: hold.archive_id().to_string(),
        reason: hold.reason().into(),
        status: match hold.status() {
            LegalHoldStatus::Active => "active",
            LegalHoldStatus::Released => "released",
        }
        .into(),
        apply_approval_request_id: hold.apply_approval_request_id().to_string(),
        created_by: hold.created_by().to_string(),
        created_at: hold.created_at().to_rfc3339(),
        release_approval_request_id: hold
            .release_approval_request_id()
            .map(|value| value.to_string()),
        released_by: hold.released_by().map(|value| value.to_string()),
        released_at: hold.released_at().map(|value| value.to_rfc3339()),
    }
}
