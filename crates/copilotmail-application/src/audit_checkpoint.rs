use crate::{
    AuditCheckpointQueryStore, AuthorizationStore, ControlPlaneStore, ExistingAuditCheckpoint,
    PendingEvent, StoreError,
};
use chrono::{DateTime, Utc};
use copilotmail_audit::{AuditArtifactError, AuditCheckpointSigner};
use copilotmail_domain::{
    AuditActor, AuditCheckpointId, AuditEvent, AuditOutcome, AuditResource, AuthorizationContext,
    DomainError, OrganizationId, Permission, PrincipalId,
};
use serde_json::json;

#[derive(Clone, Debug)]
pub struct CreateAuditCheckpointCommand {
    pub checkpoint_id: AuditCheckpointId,
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub archive_object_key: String,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CreateAuditCheckpointResult {
    pub checkpoint_id: AuditCheckpointId,
    pub organization_id: OrganizationId,
    pub chain_sequence_end: i64,
    pub event_count: u64,
    pub root_hash: String,
    pub key_id: String,
    pub public_key_sha256: String,
}

pub struct CreateAuditCheckpointService<S, K> {
    store: S,
    signer: K,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateAuditCheckpointError {
    #[error("actor is not authorized to export tenant audit data")]
    Forbidden,
    #[error("tenant has no audit events to checkpoint")]
    EmptyChain,
    #[error("archive object key is invalid")]
    InvalidObjectKey,
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Artifact(#[from] AuditArtifactError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl<S, K> CreateAuditCheckpointService<S, K>
where
    S: ControlPlaneStore + AuthorizationStore + AuditCheckpointQueryStore,
    K: AuditCheckpointSigner,
{
    #[must_use]
    pub const fn new(store: S, signer: K) -> Self {
        Self { store, signer }
    }

    pub async fn execute(
        &self,
        command: CreateAuditCheckpointCommand,
    ) -> Result<CreateAuditCheckpointResult, CreateAuditCheckpointError> {
        validate_object_key(&command.archive_object_key)?;
        let authorization = self
            .store
            .resolve_authorization(command.actor_principal_id, command.now)
            .await?
            .ok_or(CreateAuditCheckpointError::Forbidden)?;
        authorize(&authorization, &command)?;
        if let Some(existing) = find_existing(&self.store, &command).await? {
            return existing_result(&existing, &command, &self.signer);
        }

        let candidate = self
            .store
            .audit_checkpoint_candidate(command.organization_id)
            .await?
            .ok_or(CreateAuditCheckpointError::EmptyChain)?;
        let checkpoint =
            self.signer
                .sign_candidate(&candidate, command.checkpoint_id, command.now)?;
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: command.actor_principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "audit.checkpoint.created",
            AuditResource {
                kind: "audit.checkpoint".into(),
                id: command.checkpoint_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id.clone(),
            json!({
                "chain_sequence_end": candidate.chain_sequence_end,
                "event_count": candidate.event_count,
                "root_hash": candidate.root_hash,
                "key_id": self.signer.key_id(),
                "public_key_sha256": self.signer.public_key_sha256(),
                "archive_object_key": command.archive_object_key,
            }),
            command.now,
        )?;
        let event = PendingEvent::new(
            "audit.checkpoint.created.v1",
            "audit.checkpoint",
            command.checkpoint_id.to_string(),
            json!({
                "organization_id": command.organization_id,
                "checkpoint_id": command.checkpoint_id,
                "chain_sequence_end": candidate.chain_sequence_end,
                "key_id": self.signer.key_id(),
            }),
            command.now,
        );

        let mut transaction = self.store.begin().await?;
        let inserted = transaction
            .insert_audit_checkpoint(&checkpoint, &command.archive_object_key)
            .await?;
        if !inserted {
            drop(transaction);
            let existing = find_existing(&self.store, &command)
                .await?
                .ok_or(StoreError::Conflict)?;
            return existing_result(&existing, &command, &self.signer);
        }
        transaction.append_audit_event(&audit).await?;
        transaction.enqueue_event(&event).await?;
        transaction.commit().await?;

        Ok(CreateAuditCheckpointResult {
            checkpoint_id: command.checkpoint_id,
            organization_id: command.organization_id,
            chain_sequence_end: candidate.chain_sequence_end,
            event_count: candidate.event_count,
            root_hash: candidate.root_hash,
            key_id: self.signer.key_id().into(),
            public_key_sha256: self.signer.public_key_sha256().into(),
        })
    }
}

async fn find_existing<S: ControlPlaneStore>(
    store: &S,
    command: &CreateAuditCheckpointCommand,
) -> Result<Option<ExistingAuditCheckpoint>, StoreError> {
    let mut transaction = store.begin().await?;
    transaction
        .find_audit_checkpoint(command.organization_id, command.checkpoint_id)
        .await
}

fn existing_result<K: AuditCheckpointSigner>(
    existing: &ExistingAuditCheckpoint,
    command: &CreateAuditCheckpointCommand,
    signer: &K,
) -> Result<CreateAuditCheckpointResult, CreateAuditCheckpointError> {
    let payload = &existing.checkpoint.payload;
    if payload.checkpoint_id != command.checkpoint_id
        || payload.organization_id != command.organization_id
        || payload.key_id != signer.key_id()
        || payload.public_key_sha256 != signer.public_key_sha256()
        || existing.archive_object_key != command.archive_object_key
    {
        return Err(StoreError::Conflict.into());
    }
    // audit-coverage: no-op-idempotent-replay
    Ok(CreateAuditCheckpointResult {
        checkpoint_id: payload.checkpoint_id,
        organization_id: payload.organization_id,
        chain_sequence_end: payload.chain_sequence_end,
        event_count: payload.event_count,
        root_hash: payload.root_hash.clone(),
        key_id: payload.key_id.clone(),
        public_key_sha256: payload.public_key_sha256.clone(),
    })
}

fn authorize(
    authorization: &AuthorizationContext,
    command: &CreateAuditCheckpointCommand,
) -> Result<(), CreateAuditCheckpointError> {
    if authorization.principal_id != command.actor_principal_id
        || authorization.organization_id != command.organization_id
        || !authorization.permits(command.organization_id, Permission::AuditExport)
    {
        return Err(CreateAuditCheckpointError::Forbidden);
    }
    Ok(())
}

fn validate_object_key(value: &str) -> Result<(), CreateAuditCheckpointError> {
    if value.is_empty()
        || value.len() > 1_024
        || value.trim() != value
        || value.chars().any(char::is_control)
        || value.starts_with('/')
        || value.contains('\\')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(CreateAuditCheckpointError::InvalidObjectKey);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn command(
        organization_id: OrganizationId,
        actor_principal_id: PrincipalId,
    ) -> CreateAuditCheckpointCommand {
        CreateAuditCheckpointCommand {
            checkpoint_id: AuditCheckpointId::new(),
            organization_id,
            actor_principal_id,
            archive_object_key: "audit/tenant/checkpoint/events.jsonl".into(),
            trace_id: "request-1".into(),
            now: Utc::now(),
        }
    }

    #[test]
    fn checkpoint_authorization_binds_actor_tenant_and_export_permission() {
        let organization_id = OrganizationId::new();
        let actor_principal_id = PrincipalId::new();
        let command = command(organization_id, actor_principal_id);
        let permitted = AuthorizationContext {
            principal_id: actor_principal_id,
            organization_id,
            permissions: [Permission::AuditExport].into_iter().collect(),
        };
        assert!(authorize(&permitted, &command).is_ok());

        let denied = AuthorizationContext {
            principal_id: actor_principal_id,
            organization_id,
            permissions: BTreeSet::new(),
        };
        assert!(authorize(&denied, &command).is_err());

        let cross_tenant = AuthorizationContext {
            principal_id: actor_principal_id,
            organization_id: OrganizationId::new(),
            permissions: [Permission::AuditExport].into_iter().collect(),
        };
        assert!(authorize(&cross_tenant, &command).is_err());
    }

    #[test]
    fn archive_object_key_rejects_traversal_absolute_and_control_values() {
        assert!(validate_object_key("audit/tenant/checkpoint/events.jsonl").is_ok());
        for invalid in [
            "",
            "/audit/events.jsonl",
            "../audit/events.jsonl",
            "audit/../events.jsonl",
            "audit\\events.jsonl",
            "audit//events.jsonl",
            "audit/\nevents.jsonl",
        ] {
            assert!(
                validate_object_key(invalid).is_err(),
                "{invalid:?} must be rejected"
            );
        }
    }
}
