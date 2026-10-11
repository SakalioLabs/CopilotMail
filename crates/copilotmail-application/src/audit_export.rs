use crate::{
    ApprovalProof, ApprovalQueryStore, ArchivedObjectRecord, AuditArchiveRecord,
    AuditExportArtifact, AuditExportDownloadRecord, AuditExportQueryStore, ComplianceQueryStore,
    ControlPlaneStore, PendingEvent, StoreError,
};
use chrono::{DateTime, Duration, Utc};
use copilotmail_domain::{
    ApprovalAction, AuditActor, AuditArchiveId, AuditContext, AuditEvent, AuditExportDownloadId,
    AuditExportGrant, AuditExportId, AuditOutcome, AuditResource, AuthorizationContext,
    DomainError, Permission,
};
use serde_json::json;

#[derive(Debug, Clone)]
pub struct AuthorizeAuditExportCommand {
    pub context: AuthorizationContext,
    pub archive_id: AuditArchiveId,
    pub approval: ApprovalProof,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuditExportAuthorizationError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("audit export authorization is forbidden")]
    Forbidden,
    #[error("audit archive is not available")]
    NotFound,
    #[error("audit export approval conflicts with an existing authorization")]
    Conflict,
    #[error(transparent)]
    Store(#[from] StoreError),
}

pub struct AuditExportAuthorizationService<S> {
    store: S,
}

impl<S> AuditExportAuthorizationService<S>
where
    S: ControlPlaneStore
        + ApprovalQueryStore
        + AuditExportQueryStore
        + ComplianceQueryStore
        + Clone,
{
    pub const GRANT_LIFETIME: Duration = Duration::minutes(15);

    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn authorize(
        &self,
        command: AuthorizeAuditExportCommand,
    ) -> Result<AuditExportGrant, AuditExportAuthorizationError> {
        let organization_id = command.context.organization_id;
        let requested_by = command.context.principal_id;
        let expected_resource_id = command.archive_id.to_string();
        if !command
            .context
            .permits(organization_id, Permission::AuditExport)
            || command.approval.organization_id != organization_id
            || command.approval.requested_by != requested_by
            || command.approval.action != ApprovalAction::AuditExport
            || command.approval.resource_kind != "audit.archive"
            || command.approval.resource_id != expected_resource_id
            || command.approval.now != command.now
        {
            return Err(AuditExportAuthorizationError::Forbidden);
        }

        if let Some(existing) = self
            .store
            .get_audit_export_grant_by_approval(organization_id, command.approval.request_id)
            .await?
        {
            // Idempotency preserves an existing result, not stale permission.
            // This read-only transaction is dropped without committing writes.
            let mut transaction = self.store.begin_authorized(&command.context).await?;
            if !transaction
                .authorize_human_permission(organization_id, requested_by, Permission::AuditExport)
                .await?
            {
                return Err(AuditExportAuthorizationError::Forbidden);
            }
            return ensure_exact_replay(
                existing,
                organization_id,
                command.archive_id,
                requested_by,
                command.approval.request_id,
            );
        }

        let approval = self
            .store
            .approved_request_evidence(&command.approval)
            .await?
            .ok_or(AuditExportAuthorizationError::Forbidden)?;
        if approval.required_approvals < 2 || approval.approval_count < approval.required_approvals
        {
            return Err(AuditExportAuthorizationError::Forbidden);
        }
        let expires_at = command
            .now
            .checked_add_signed(Self::GRANT_LIFETIME)
            .ok_or(DomainError::InvalidAuditExport)?
            .min(approval.expires_at);
        if expires_at <= command.now {
            return Err(AuditExportAuthorizationError::Forbidden);
        }
        let archive = self
            .store
            .get_audit_archive(organization_id, command.archive_id)
            .await?
            .ok_or(AuditExportAuthorizationError::NotFound)?;
        if archive.organization_id != organization_id || archive.archive_id != command.archive_id {
            return Err(AuditExportAuthorizationError::NotFound);
        }

        let candidate = AuditExportGrant::new(
            organization_id,
            command.archive_id,
            command.approval.request_id,
            requested_by,
            command.now,
            expires_at,
        )?;
        let candidate_id = candidate.id();
        let mut transaction = self.store.begin_authorized(&command.context).await?;
        if !transaction
            .authorize_human_permission(organization_id, requested_by, Permission::AuditExport)
            .await?
        {
            return Err(AuditExportAuthorizationError::Forbidden);
        }
        let grant = transaction.authorize_audit_export_grant(&candidate).await?;
        ensure_exact_replay(
            grant.clone(),
            organization_id,
            command.archive_id,
            requested_by,
            command.approval.request_id,
        )?;

        if grant.id() == candidate_id {
            if grant.expires_at() > candidate.expires_at() {
                return Err(AuditExportAuthorizationError::Conflict);
            }
            let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
            audit_context.subject_principal_id = Some(requested_by);
            audit_context.approval_request_id = Some(command.approval.request_id);
            let audit = AuditEvent::new(
                organization_id,
                AuditActor {
                    principal_id: requested_by,
                    on_behalf_of: None,
                    credential_id: None,
                },
                "audit.export.authorized",
                AuditResource {
                    kind: "audit.export_grant".into(),
                    id: grant.id().to_string(),
                },
                AuditOutcome::Success,
                command.trace_id,
                json!({
                    "archive_id": grant.archive_id(),
                    "checkpoint_id": archive.checkpoint_id,
                    "expires_at": grant.expires_at(),
                }),
                grant.created_at(),
            )?
            .with_context(audit_context)?;
            transaction.append_audit_event(&audit).await?;
            transaction
                .enqueue_event(&PendingEvent::new(
                    "audit.export.authorized.v1",
                    "audit.export_grant",
                    grant.id().to_string(),
                    json!({
                        "organization_id": organization_id,
                        "archive_id": grant.archive_id(),
                    }),
                    grant.created_at(),
                ))
                .await?;
        }
        transaction.commit().await?;
        Ok(grant)
    }
}

/// One exact archive object that an active grant authorizes the requesting
/// Human to retrieve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedAuditExportObject {
    pub grant: AuditExportGrant,
    pub archive_id: AuditArchiveId,
    pub artifact: AuditExportArtifact,
    pub object: ArchivedObjectRecord,
    pub bucket: String,
}

#[derive(Debug, Clone)]
pub struct ResolveAuditExportObjectCommand {
    pub context: AuthorizationContext,
    pub grant_id: AuditExportId,
    pub artifact: AuditExportArtifact,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct RecordAuditExportDownloadCommand {
    pub context: AuthorizationContext,
    pub authorized: AuthorizedAuditExportObject,
    /// Evidence produced by independently verifying the retrieved object
    /// version against the digest and size recorded when the archive was made.
    pub retrieved: ArchivedObjectRecord,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuditExportDownloadError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("audit export download is forbidden")]
    Forbidden,
    #[error("audit export grant or archive is not available")]
    NotFound,
    #[error("retrieved object does not match the archived evidence")]
    Conflict,
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Serves WORM archive artifacts strictly under an active export grant and
/// records every retrieval in an append-only ledger.
///
/// Remote object I/O is intentionally left to the caller so that no network
/// round trip is ever held open inside a database transaction. The service
/// therefore runs in two steps: [`Self::resolve`] decides exactly what may be
/// retrieved, and [`Self::record`] commits the evidence of what was served.
pub struct AuditExportDownloadService<S> {
    store: S,
}

impl<S> AuditExportDownloadService<S>
where
    S: ControlPlaneStore + AuditExportQueryStore + ComplianceQueryStore + Clone,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Resolves the single object version the caller is allowed to retrieve.
    pub async fn resolve(
        &self,
        command: ResolveAuditExportObjectCommand,
    ) -> Result<AuthorizedAuditExportObject, AuditExportDownloadError> {
        let organization_id = command.context.organization_id;
        if !command
            .context
            .permits(organization_id, Permission::AuditExport)
        {
            return Err(AuditExportDownloadError::Forbidden);
        }
        let grant = self
            .store
            .get_audit_export_grant(organization_id, command.grant_id)
            .await?
            .ok_or(AuditExportDownloadError::NotFound)?;
        if grant.organization_id() != organization_id
            || grant.id() != command.grant_id
            || grant.requested_by() != command.context.principal_id
        {
            return Err(AuditExportDownloadError::NotFound);
        }
        if !self
            .store
            .audit_export_grant_is_active(organization_id, command.grant_id)
            .await?
        {
            return Err(AuditExportDownloadError::Forbidden);
        }
        let archive = self
            .store
            .get_audit_archive(organization_id, grant.archive_id())
            .await?
            .ok_or(AuditExportDownloadError::NotFound)?;
        if archive.organization_id != organization_id || archive.archive_id != grant.archive_id() {
            return Err(AuditExportDownloadError::NotFound);
        }
        Ok(AuthorizedAuditExportObject {
            archive_id: archive.archive_id,
            artifact: command.artifact,
            object: select_artifact(&archive, command.artifact).clone(),
            bucket: archive.bucket,
            grant,
        })
    }

    /// Commits append-only proof that the exact authorized object was served.
    pub async fn record(
        &self,
        command: RecordAuditExportDownloadCommand,
    ) -> Result<AuditExportDownloadRecord, AuditExportDownloadError> {
        let organization_id = command.context.organization_id;
        let authorized = command.authorized;
        if !command
            .context
            .permits(organization_id, Permission::AuditExport)
            || authorized.grant.organization_id() != organization_id
            || authorized.grant.requested_by() != command.context.principal_id
        {
            return Err(AuditExportDownloadError::Forbidden);
        }
        if !self
            .store
            .audit_export_grant_is_active(organization_id, authorized.grant.id())
            .await?
        {
            return Err(AuditExportDownloadError::Forbidden);
        }
        if authorized.archive_id != authorized.grant.archive_id()
            || command.retrieved != authorized.object
        {
            return Err(AuditExportDownloadError::Conflict);
        }

        let download = AuditExportDownloadRecord {
            id: AuditExportDownloadId::new(),
            organization_id,
            grant_id: authorized.grant.id(),
            archive_id: authorized.archive_id,
            downloaded_by: command.context.principal_id,
            artifact: authorized.artifact,
            object: command.retrieved,
            downloaded_at: command.now,
        };

        let mut transaction = self.store.begin_authorized(&command.context).await?;
        if !transaction
            .authorize_human_permission(
                organization_id,
                command.context.principal_id,
                Permission::AuditExport,
            )
            .await?
        {
            return Err(AuditExportDownloadError::Forbidden);
        }
        let persisted = transaction.record_audit_export_download(&download).await?;
        // Only the database owns download time; every other proof field must
        // match the exact verified object. Never audit an adapter substitution.
        let mut expected = download;
        expected.downloaded_at = persisted.downloaded_at;
        if expected != persisted {
            return Err(AuditExportDownloadError::Conflict);
        }
        if !authorized.grant.is_active(persisted.downloaded_at) {
            return Err(AuditExportDownloadError::Forbidden);
        }
        let download = persisted;

        let mut audit_context = AuditContext::for_request(command.trace_id.clone())?;
        audit_context.subject_principal_id = Some(command.context.principal_id);
        audit_context.approval_request_id = Some(authorized.grant.approval_request_id());
        let audit = AuditEvent::new(
            organization_id,
            AuditActor {
                principal_id: command.context.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "audit.export.downloaded",
            AuditResource {
                kind: "audit.export_grant".into(),
                id: download.grant_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "download_id": download.id,
                "archive_id": download.archive_id,
                "artifact": download.artifact.as_str(),
                "object_key": download.object.key,
                "object_version_id": download.object.version_id,
                "sha256": download.object.sha256_hex,
                "size_bytes": download.object.size_bytes,
            }),
            download.downloaded_at,
        )?
        .with_context(audit_context)?;
        transaction.append_audit_event(&audit).await?;
        transaction
            .enqueue_event(&PendingEvent::new(
                "audit.export.downloaded.v1",
                "audit.export_grant",
                download.grant_id.to_string(),
                json!({
                    "organization_id": organization_id,
                    "archive_id": download.archive_id,
                    "artifact": download.artifact.as_str(),
                }),
                download.downloaded_at,
            ))
            .await?;
        transaction.commit().await?;
        Ok(download)
    }
}

fn select_artifact(
    archive: &AuditArchiveRecord,
    artifact: AuditExportArtifact,
) -> &ArchivedObjectRecord {
    match artifact {
        AuditExportArtifact::Events => &archive.events,
        AuditExportArtifact::Manifest => &archive.manifest,
    }
}

fn ensure_exact_replay(
    grant: AuditExportGrant,
    organization_id: copilotmail_domain::OrganizationId,
    archive_id: AuditArchiveId,
    requested_by: copilotmail_domain::PrincipalId,
    approval_request_id: copilotmail_domain::ApprovalRequestId,
) -> Result<AuditExportGrant, AuditExportAuthorizationError> {
    if grant.organization_id() != organization_id
        || grant.archive_id() != archive_id
        || grant.requested_by() != requested_by
        || grant.approval_request_id() != approval_request_id
    {
        return Err(AuditExportAuthorizationError::Conflict);
    }
    Ok(grant)
}

#[cfg(test)]
mod service_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use copilotmail_domain::{ApprovalRequestId, OrganizationId, PrincipalId};

    #[test]
    fn approval_replay_cannot_change_archive_or_requester() {
        let now = Utc::now();
        let archive_id = AuditArchiveId::new();
        let approval_id = ApprovalRequestId::new();
        let requested_by = PrincipalId::new();
        let grant = AuditExportGrant::new(
            OrganizationId::new(),
            archive_id,
            approval_id,
            requested_by,
            now,
            now + Duration::minutes(15),
        )
        .unwrap();

        assert!(
            ensure_exact_replay(
                grant.clone(),
                grant.organization_id(),
                archive_id,
                requested_by,
                approval_id
            )
            .is_ok()
        );
        assert!(matches!(
            ensure_exact_replay(
                grant.clone(),
                grant.organization_id(),
                AuditArchiveId::new(),
                requested_by,
                approval_id
            ),
            Err(AuditExportAuthorizationError::Conflict)
        ));
        assert!(matches!(
            ensure_exact_replay(
                grant.clone(),
                grant.organization_id(),
                archive_id,
                PrincipalId::new(),
                approval_id
            ),
            Err(AuditExportAuthorizationError::Conflict)
        ));
    }
}
