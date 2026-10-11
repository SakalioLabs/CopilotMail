use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingAuditCheckpoint {
    pub checkpoint: SignedAuditCheckpoint,
    pub archive_object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArchivedObjectRecord {
    pub key: String,
    pub version_id: String,
    pub sha256_hex: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AuditArchiveRecord {
    pub archive_id: AuditArchiveId,
    pub organization_id: OrganizationId,
    pub checkpoint_id: AuditCheckpointId,
    pub provider: String,
    pub bucket: String,
    pub events: ArchivedObjectRecord,
    pub manifest: ArchivedObjectRecord,
    pub retention_mode: String,
    pub retain_until: chrono::DateTime<chrono::Utc>,
    pub verified_at: chrono::DateTime<chrono::Utc>,
    pub created_by: PrincipalId,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Names the exact artifact of a WORM archive that an export grant may serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditExportArtifact {
    Events,
    Manifest,
}

impl AuditExportArtifact {
    /// Returns the stable wire and storage representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Manifest => "manifest",
        }
    }
}

/// Append-only evidence that one exact archive object version was served
/// under one exact export grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditExportDownloadRecord {
    pub id: AuditExportDownloadId,
    pub organization_id: OrganizationId,
    pub grant_id: AuditExportId,
    pub archive_id: AuditArchiveId,
    pub downloaded_by: PrincipalId,
    pub artifact: AuditExportArtifact,
    pub object: ArchivedObjectRecord,
    pub downloaded_at: chrono::DateTime<chrono::Utc>,
}

/// Transactional retention-policy identity plus its activation/version
/// cursors. The latest version may remain a draft and therefore differ from
/// the active version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicyRecord {
    pub policy: RetentionPolicy,
    pub active_version_id: Option<RetentionPolicyVersionId>,
    pub latest_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicyDetail {
    pub policy: RetentionPolicy,
    pub active_version: Option<RetentionPolicyVersion>,
    pub latest_version: RetentionPolicyVersion,
}

/// Append-only proof that an exact retention-policy version was activated by
/// its requesting Human under an exact dual approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPolicyActivationRecord {
    pub id: RetentionPolicyActivationId,
    pub organization_id: OrganizationId,
    pub policy_id: RetentionPolicyId,
    pub policy_version_id: RetentionPolicyVersionId,
    pub approval_request_id: ApprovalRequestId,
    pub activated_by: PrincipalId,
    pub activated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegalHoldObjectEvidence {
    pub events_version_id: String,
    pub manifest_version_id: String,
    pub enabled: bool,
    pub verified_at: chrono::DateTime<chrono::Utc>,
}

/// Bounded, tenant-scoped filters for the append-only audit event read model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEventSearch {
    pub organization_id: OrganizationId,
    pub occurred_from: chrono::DateTime<chrono::Utc>,
    pub occurred_to: chrono::DateTime<chrono::Utc>,
    pub action: Option<String>,
    pub actor_principal_id: Option<PrincipalId>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub outcome: Option<AuditOutcome>,
    pub request_id: Option<String>,
    pub before_chain_sequence: Option<i64>,
    pub limit: i64,
}

/// One immutable audit row exposed to authorized tenant auditors.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEventSummary {
    pub event_id: uuid::Uuid,
    pub organization_id: OrganizationId,
    pub actor_principal_id: PrincipalId,
    pub actor_kind: String,
    pub on_behalf_of_principal_id: Option<PrincipalId>,
    pub credential_id: Option<String>,
    pub subject_principal_id: Option<PrincipalId>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub outcome: AuditOutcome,
    pub trace_id: String,
    pub request_id: String,
    pub source_ip: Option<String>,
    pub client_id: Option<String>,
    pub session_id: Option<String>,
    pub token_jti: Option<String>,
    pub policy_id: Option<PolicyId>,
    pub policy_version_id: Option<PolicyVersionId>,
    pub approval_request_id: Option<ApprovalRequestId>,
    pub ticket_reference: Option<String>,
    pub environment: Option<String>,
    pub region: Option<String>,
    pub details: serde_json::Value,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub received_at: chrono::DateTime<chrono::Utc>,
    pub chain_sequence: i64,
    pub hash_version: i16,
    pub previous_hash_hex: Option<String>,
    pub event_hash_hex: String,
    pub signature_batch_id: Option<uuid::Uuid>,
}
