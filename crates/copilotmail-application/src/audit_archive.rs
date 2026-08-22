use crate::{AuditArchiveRecord, AuthorizationStore, ControlPlaneStore, PendingEvent, StoreError};
use copilotmail_domain::{
    AuditActor, AuditArchiveId, AuditEvent, AuditOutcome, AuditResource, OrganizationId,
    Permission, PrincipalId,
};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct RecordAuditArchiveCommand {
    pub archive: AuditArchiveRecord,
    pub actor_principal_id: PrincipalId,
    pub trace_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct RecordAuditArchiveResult {
    pub archive_id: AuditArchiveId,
    pub organization_id: OrganizationId,
    pub checkpoint_id: copilotmail_domain::AuditCheckpointId,
    pub bucket: String,
    pub events_key: String,
    pub events_version_id: String,
    pub manifest_key: String,
    pub manifest_version_id: String,
    pub retention_mode: String,
    pub retain_until: chrono::DateTime<chrono::Utc>,
    pub verified_at: chrono::DateTime<chrono::Utc>,
}

pub struct RecordAuditArchiveService<S> {
    store: S,
}

#[derive(Debug, thiserror::Error)]
pub enum RecordAuditArchiveError {
    #[error("actor is not authorized to archive tenant audit data")]
    Forbidden,
    #[error("verified archive evidence is invalid")]
    InvalidEvidence,
    #[error(transparent)]
    Domain(#[from] copilotmail_domain::DomainError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl<S> RecordAuditArchiveService<S>
where
    S: ControlPlaneStore + AuthorizationStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn execute(
        &self,
        command: RecordAuditArchiveCommand,
    ) -> Result<RecordAuditArchiveResult, RecordAuditArchiveError> {
        validate(&command)?;
        let authorization = self
            .store
            .resolve_authorization(command.actor_principal_id, command.archive.created_at)
            .await?
            .ok_or(RecordAuditArchiveError::Forbidden)?;
        if authorization.principal_id != command.actor_principal_id
            || authorization.organization_id != command.archive.organization_id
            || !authorization.permits(command.archive.organization_id, Permission::AuditExport)
        {
            return Err(RecordAuditArchiveError::Forbidden);
        }

        if let Some(existing) = find_existing(&self.store, &command).await? {
            return exact_result(&existing, &command.archive);
        }

        let archive = &command.archive;
        let audit = AuditEvent::new(
            archive.organization_id,
            AuditActor {
                principal_id: command.actor_principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "audit.archive.completed",
            AuditResource {
                kind: "audit.archive".into(),
                id: archive.archive_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "checkpoint_id": archive.checkpoint_id,
                "provider": archive.provider,
                "bucket": archive.bucket,
                "events_key": archive.events.key,
                "events_version_id": archive.events.version_id,
                "events_sha256": archive.events.sha256_hex,
                "manifest_key": archive.manifest.key,
                "manifest_version_id": archive.manifest.version_id,
                "manifest_sha256": archive.manifest.sha256_hex,
                "retention_mode": archive.retention_mode,
                "retain_until": archive.retain_until,
                "verified_at": archive.verified_at,
            }),
            archive.created_at,
        )?;
        let outbox = PendingEvent::new(
            "audit.archive.completed.v1",
            "audit.archive",
            archive.archive_id.to_string(),
            json!({
                "organization_id": archive.organization_id,
                "checkpoint_id": archive.checkpoint_id,
                "archive_id": archive.archive_id,
                "bucket": archive.bucket,
                "events_key": archive.events.key,
                "events_version_id": archive.events.version_id,
                "manifest_key": archive.manifest.key,
                "manifest_version_id": archive.manifest.version_id,
                "retention_mode": archive.retention_mode,
                "retain_until": archive.retain_until,
            }),
            archive.created_at,
        );

        let mut transaction = self.store.begin().await?;
        if !transaction.insert_audit_archive(archive).await? {
            drop(transaction);
            let existing = find_existing(&self.store, &command)
                .await?
                .ok_or(StoreError::Conflict)?;
            return exact_result(&existing, archive);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&outbox).await?;
        transaction.commit().await?;
        Ok(result(archive))
    }
}

async fn find_existing<S: ControlPlaneStore>(
    store: &S,
    command: &RecordAuditArchiveCommand,
) -> Result<Option<AuditArchiveRecord>, StoreError> {
    let mut transaction = store.begin().await?;
    transaction
        .find_audit_archive(command.archive.organization_id, command.archive.archive_id)
        .await
}

fn exact_result(
    existing: &AuditArchiveRecord,
    requested: &AuditArchiveRecord,
) -> Result<RecordAuditArchiveResult, RecordAuditArchiveError> {
    if existing.archive_id != requested.archive_id
        || existing.organization_id != requested.organization_id
        || existing.checkpoint_id != requested.checkpoint_id
        || existing.provider != requested.provider
        || existing.bucket != requested.bucket
        || existing.events != requested.events
        || existing.manifest != requested.manifest
        || existing.retention_mode != requested.retention_mode
        || existing.retain_until != requested.retain_until
        || existing.created_by != requested.created_by
    {
        return Err(StoreError::Conflict.into());
    }
    // audit-coverage: no-op-idempotent-replay
    Ok(result(existing))
}

fn result(archive: &AuditArchiveRecord) -> RecordAuditArchiveResult {
    RecordAuditArchiveResult {
        archive_id: archive.archive_id,
        organization_id: archive.organization_id,
        checkpoint_id: archive.checkpoint_id,
        bucket: archive.bucket.clone(),
        events_key: archive.events.key.clone(),
        events_version_id: archive.events.version_id.clone(),
        manifest_key: archive.manifest.key.clone(),
        manifest_version_id: archive.manifest.version_id.clone(),
        retention_mode: archive.retention_mode.clone(),
        retain_until: archive.retain_until,
        verified_at: archive.verified_at,
    }
}

fn validate(command: &RecordAuditArchiveCommand) -> Result<(), RecordAuditArchiveError> {
    let archive = &command.archive;
    let valid_text = |value: &str, max: usize| {
        !value.is_empty()
            && value.len() <= max
            && value.trim() == value
            && !value.chars().any(char::is_control)
    };
    let valid_object = |object: &crate::ArchivedObjectRecord| {
        valid_text(&object.key, 1_024)
            && valid_text(&object.version_id, 1_024)
            && object.size_bytes > 0
            && object.sha256_hex.len() == 64
            && object
                .sha256_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    if command.actor_principal_id != archive.created_by
        || !valid_text(&command.trace_id, 200)
        || archive.provider != "s3"
        || !valid_text(&archive.bucket, 255)
        || !matches!(archive.retention_mode.as_str(), "governance" | "compliance")
        || archive.retain_until <= archive.verified_at
        || archive.verified_at > archive.created_at
        || archive.events.key == archive.manifest.key
        || !valid_object(&archive.events)
        || !valid_object(&archive.manifest)
    {
        return Err(RecordAuditArchiveError::InvalidEvidence);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArchivedObjectRecord;
    use chrono::{Duration, Utc};

    #[test]
    fn rejects_mismatched_actor_and_non_future_retention() {
        let now = Utc::now();
        let object = ArchivedObjectRecord {
            key: "audit/events.jsonl".into(),
            version_id: "v1".into(),
            sha256_hex: "a".repeat(64),
            size_bytes: 1,
        };
        let command = RecordAuditArchiveCommand {
            archive: AuditArchiveRecord {
                archive_id: AuditArchiveId::new(),
                organization_id: OrganizationId::new(),
                checkpoint_id: copilotmail_domain::AuditCheckpointId::new(),
                provider: "s3".into(),
                bucket: "audit".into(),
                events: object.clone(),
                manifest: ArchivedObjectRecord {
                    key: "audit/manifest.json".into(),
                    ..object
                },
                retention_mode: "compliance".into(),
                retain_until: now - Duration::seconds(1),
                verified_at: now,
                created_by: PrincipalId::new(),
                created_at: now,
            },
            actor_principal_id: PrincipalId::new(),
            trace_id: "trace".into(),
        };
        assert!(matches!(
            validate(&command),
            Err(RecordAuditArchiveError::InvalidEvidence)
        ));
    }
}
