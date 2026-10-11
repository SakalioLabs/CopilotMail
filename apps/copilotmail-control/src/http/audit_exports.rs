use crate::*;

pub(crate) async fn authorize_audit_export(
    State(state): State<Arc<AppState>>,
    Path(archive_id): Path<AuditArchiveId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
    Json(body): Json<AuditExportGrantCreateRequest>,
) -> Result<(StatusCode, Json<AuditExportGrantResponse>), Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_audit_export_request(&state, &headers, connection, &request_id).await?;
    let approval_request_id = body
        .approval_request_id
        .parse::<ApprovalRequestId>()
        .map_err(|_| {
            api_error(
                StatusCode::BAD_REQUEST,
                "invalid_approval_request_id",
                "approval_request_id must be a UUID",
                false,
                &request_id,
            )
        })?;
    let now = Utc::now();
    let approval = ApprovalProof {
        organization_id: authorization.organization_id,
        request_id: approval_request_id,
        requested_by: authorization.principal_id,
        action: ApprovalAction::AuditExport,
        resource_kind: "audit.archive".into(),
        resource_id: archive_id.to_string(),
        now,
    };
    let grant = AuditExportAuthorizationService::new(state.store.clone())
        .authorize(AuthorizeAuditExportCommand {
            context: authorization,
            archive_id,
            approval,
            trace_id: request_id.clone(),
            now,
        })
        .await
        .map_err(|error| audit_export_authorization_api_error(error, &request_id))?;
    Ok((
        StatusCode::CREATED,
        Json(audit_export_grant_response(grant)),
    ))
}

pub(crate) async fn get_audit_export_grant(
    State(state): State<Arc<AppState>>,
    Path(grant_id): Path<AuditExportId>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AuditExportGrantResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_audit_export_request(&state, &headers, connection, &request_id).await?;
    let grant = state
        .store
        .get_audit_export_grant(authorization.organization_id, grant_id)
        .await
        .map_err(|_| audit_export_store_unavailable(&request_id))?
        .filter(|grant| grant.requested_by() == authorization.principal_id)
        .ok_or_else(|| resource_not_found(&request_id))?;
    Ok(Json(audit_export_grant_response(grant)))
}

/// Reconciles a possibly unknown export-authorization write using the exact
/// immutable approval identity supplied by the caller.
///
/// This is intentionally a read-only endpoint. It must never turn an unknown
/// client outcome into a retry: creation remains exclusively on the POST
/// endpoint, while this GET only reveals an already persisted exact grant to
/// its requesting Human.
pub(crate) async fn reconcile_audit_export_grant(
    State(state): State<Arc<AppState>>,
    Path((archive_id_wire, approval_request_id_wire)): Path<(String, String)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Json<AuditExportGrantResponse>, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_audit_export_request(&state, &headers, connection, &request_id).await?;

    // Parse after authentication so malformed route values cannot distinguish
    // an otherwise protected endpoint from a missing resource. Require the
    // standard UUID spelling as well, making this an exact reconciliation key.
    let archive_id = parse_canonical_audit_archive_id(&archive_id_wire)
        .ok_or_else(|| resource_not_found(&request_id))?;
    let approval_request_id = parse_canonical_approval_request_id(&approval_request_id_wire)
        .ok_or_else(|| resource_not_found(&request_id))?;
    let grant = state
        .store
        .get_audit_export_grant_by_approval(authorization.organization_id, approval_request_id)
        .await
        .map_err(|_| audit_export_store_unavailable(&request_id))?
        .filter(|grant| {
            audit_export_grant_matches_reconciliation(
                grant,
                &authorization,
                archive_id,
                approval_request_id,
            )
        })
        .ok_or_else(|| resource_not_found(&request_id))?;
    Ok(Json(audit_export_grant_response(grant)))
}

pub(crate) fn parse_canonical_audit_archive_id(value: &str) -> Option<AuditArchiveId> {
    value
        .parse::<AuditArchiveId>()
        .ok()
        .filter(|parsed| parsed.to_string() == value)
}

pub(crate) fn parse_canonical_approval_request_id(value: &str) -> Option<ApprovalRequestId> {
    value
        .parse::<ApprovalRequestId>()
        .ok()
        .filter(|parsed| parsed.to_string() == value)
}

pub(crate) fn audit_export_grant_matches_reconciliation(
    grant: &AuditExportGrant,
    authorization: &AuthorizationContext,
    archive_id: AuditArchiveId,
    approval_request_id: ApprovalRequestId,
) -> bool {
    grant.organization_id() == authorization.organization_id
        && grant.archive_id() == archive_id
        && grant.approval_request_id() == approval_request_id
        && grant.requested_by() == authorization.principal_id
}

/// Upper bound on a single served archive artifact. It keeps one request from
/// buffering an unbounded object into memory.
pub(crate) const MAX_AUDIT_EXPORT_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

/// Serves one exact WORM archive artifact under an active export grant.
///
/// The object is fetched and independently reverified against the digest and
/// size recorded when the archive was created, and the retrieval is committed
/// to the append-only download ledger before any byte reaches the caller.
pub(crate) async fn download_audit_export_artifact(
    State(state): State<Arc<AppState>>,
    Path((grant_id, artifact)): Path<(AuditExportId, String)>,
    Extension(request_id): Extension<RequestId>,
    ConnectionProof(connection): ConnectionProof,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let request_id = request_id_value(&request_id);
    let (_, authorization) =
        authenticate_audit_export_request(&state, &headers, connection, &request_id).await?;
    let artifact = match artifact.as_str() {
        "events" => AuditExportArtifact::Events,
        "manifest" => AuditExportArtifact::Manifest,
        _ => return Err(resource_not_found(&request_id)),
    };
    let service = AuditExportDownloadService::new(state.store.clone());
    let authorized = service
        .resolve(ResolveAuditExportObjectCommand {
            context: authorization.clone(),
            grant_id,
            artifact,
            now: Utc::now(),
        })
        .await
        .map_err(|error| audit_export_download_api_error(error, &request_id))?;

    let archive_record = state
        .store
        .get_audit_archive(authorization.organization_id, authorized.archive_id)
        .await
        .map_err(|_| audit_export_store_unavailable(&request_id))?
        .ok_or_else(|| resource_not_found(&request_id))?;
    let archive_settings = state
        .audit_archive
        .as_ref()
        .ok_or_else(|| audit_export_object_store_unavailable(&request_id))?;
    let archive = connect_legal_hold_archive_settings(archive_settings, &archive_record)
        .await
        .map_err(|_| audit_export_object_store_unavailable(&request_id))?;
    let retrieved = archive
        .get_verified_version(
            &authorized.object.key,
            &authorized.object.version_id,
            &authorized.object.sha256_hex,
            authorized.object.size_bytes,
            MAX_AUDIT_EXPORT_ARTIFACT_BYTES,
        )
        .await
        .map_err(|error| audit_export_object_store_error(error, &request_id))?;

    service
        .record(RecordAuditExportDownloadCommand {
            context: authorization,
            retrieved: ArchivedObjectRecord {
                key: retrieved.key,
                version_id: retrieved.version_id,
                sha256_hex: retrieved.sha256_hex.clone(),
                size_bytes: retrieved.size_bytes,
            },
            authorized,
            trace_id: request_id.clone(),
            now: Utc::now(),
        })
        .await
        .map_err(|error| audit_export_download_api_error(error, &request_id))?;

    let extension = match artifact {
        AuditExportArtifact::Events => "jsonl",
        AuditExportArtifact::Manifest => "json",
    };
    let file_name = format!("{grant_id}-{}.{}", artifact.as_str(), extension);
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            (header::CONTENT_LENGTH, retrieved.size_bytes.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{file_name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (
                HeaderName::from_static("x-copilotmail-artifact-sha256"),
                retrieved.sha256_hex,
            ),
        ],
        retrieved.bytes,
    )
        .into_response())
}

pub(crate) async fn authenticate_audit_export_request(
    state: &AppState,
    headers: &HeaderMap,
    connection: Option<TlsConnectionInfo>,
    request_id: &str,
) -> Result<(AuthenticatedIdentity, AuthorizationContext), Response> {
    let (identity, authorization) =
        authenticate_api_request(state, headers, connection, request_id).await?;
    enforce_audit_export_human_principal(&identity, request_id)?;
    enforce_permission_and_scope(
        &identity,
        &authorization,
        Permission::AuditExport,
        "audit.export permission is required",
        request_id,
    )?;
    if !state
        .store
        .active_human_compliance_actor_exists(
            authorization.organization_id,
            authorization.principal_id,
        )
        .await
        .map_err(|_| audit_export_store_unavailable(request_id))?
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "human_principal_required",
            "audit export authorization requires an active Human Principal",
            false,
            request_id,
        ));
    }
    Ok((identity, authorization))
}

#[allow(clippy::result_large_err)]
pub(crate) fn enforce_audit_export_human_principal(
    identity: &AuthenticatedIdentity,
    request_id: &str,
) -> Result<(), Response> {
    if identity.principal_kind == AuthenticatedPrincipalKind::Human {
        return Ok(());
    }
    Err(api_error(
        StatusCode::FORBIDDEN,
        "human_principal_required",
        "audit export authorization requires an active Human Principal",
        false,
        request_id,
    ))
}

pub(crate) fn audit_export_grant_response(grant: AuditExportGrant) -> AuditExportGrantResponse {
    AuditExportGrantResponse {
        audit_export_grant_id: grant.id().to_string(),
        organization_id: grant.organization_id().to_string(),
        archive_id: grant.archive_id().to_string(),
        approval_request_id: grant.approval_request_id().to_string(),
        requested_by: grant.requested_by().to_string(),
        created_at: grant.created_at().to_rfc3339(),
        expires_at: grant.expires_at().to_rfc3339(),
    }
}

pub(crate) fn audit_export_authorization_api_error(
    error: AuditExportAuthorizationError,
    request_id: &str,
) -> Response {
    match error {
        AuditExportAuthorizationError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "audit_export_approval_required",
            "an exact unexpired dual approval by the requesting Human is required",
            false,
            request_id,
        ),
        AuditExportAuthorizationError::NotFound => resource_not_found(request_id),
        AuditExportAuthorizationError::Conflict
        | AuditExportAuthorizationError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "audit_export_authorization_conflict",
            "the approval was already consumed by a different export authorization",
            false,
            request_id,
        ),
        AuditExportAuthorizationError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_audit_export_authorization",
            "the export authorization violates its validity constraints",
            false,
            request_id,
        ),
        AuditExportAuthorizationError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        AuditExportAuthorizationError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => audit_export_store_unavailable(request_id),
    }
}

pub(crate) fn audit_export_download_api_error(
    error: AuditExportDownloadError,
    request_id: &str,
) -> Response {
    match error {
        AuditExportDownloadError::Forbidden => api_error(
            StatusCode::FORBIDDEN,
            "audit_export_grant_required",
            "an active export grant held by the requesting Human is required",
            false,
            request_id,
        ),
        AuditExportDownloadError::NotFound => resource_not_found(request_id),
        AuditExportDownloadError::Conflict
        | AuditExportDownloadError::Store(
            copilotmail_application::StoreError::Conflict
            | copilotmail_application::StoreError::MissingReference,
        ) => api_error(
            StatusCode::CONFLICT,
            "audit_export_download_conflict",
            "the retrieved object does not match the recorded archive evidence",
            false,
            request_id,
        ),
        AuditExportDownloadError::Domain(_) => api_error(
            StatusCode::BAD_REQUEST,
            "invalid_audit_export_download",
            "the export download violates its validity constraints",
            false,
            request_id,
        ),
        AuditExportDownloadError::Store(
            copilotmail_application::StoreError::AuthenticationInvalid,
        ) => authentication_invalid(request_id),
        AuditExportDownloadError::Store(
            copilotmail_application::StoreError::Unavailable
            | copilotmail_application::StoreError::AgentMailAdmission(_),
        ) => audit_export_store_unavailable(request_id),
    }
}

pub(crate) fn audit_export_object_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "audit_export_object_store_unavailable",
        "the immutable archive object could not be retrieved and verified",
        true,
        request_id,
    )
}

pub(crate) fn audit_export_object_store_error(
    error: WormArchiveError,
    request_id: &str,
) -> Response {
    match error {
        WormArchiveError::Verification
        | WormArchiveError::InvalidObject
        | WormArchiveError::BucketNotImmutable
        | WormArchiveError::Conflict => api_error(
            StatusCode::CONFLICT,
            "audit_export_object_verification_failed",
            "the immutable archive object does not match its recorded evidence",
            false,
            request_id,
        ),
        WormArchiveError::Remote | WormArchiveError::InvalidConfiguration => {
            audit_export_object_store_unavailable(request_id)
        }
    }
}

pub(crate) fn audit_export_store_unavailable(request_id: &str) -> Response {
    api_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "audit_export_store_unavailable",
        "audit export authorization state is temporarily unavailable",
        true,
        request_id,
    )
}
