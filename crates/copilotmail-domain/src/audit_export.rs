use crate::{
    ApprovalRequestId, AuditArchiveId, AuditExportId, DomainError, OrganizationId, PrincipalId,
};
use chrono::{DateTime, Duration, Utc};

/// Immutable, short-lived authorization to retrieve one exact WORM archive.
///
/// The approval request is single-use across all export grants. The database
/// independently proves that it is an approved `audit.export` request bound to
/// the same archive and requesting Human.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditExportGrant {
    id: AuditExportId,
    organization_id: OrganizationId,
    archive_id: AuditArchiveId,
    approval_request_id: ApprovalRequestId,
    requested_by: PrincipalId,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl AuditExportGrant {
    pub const MAX_LIFETIME: Duration = Duration::minutes(30);

    pub fn new(
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
        approval_request_id: ApprovalRequestId,
        requested_by: PrincipalId,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        Self::restore(
            AuditExportId::new(),
            organization_id,
            archive_id,
            approval_request_id,
            requested_by,
            created_at,
            expires_at,
        )
    }

    pub fn restore(
        id: AuditExportId,
        organization_id: OrganizationId,
        archive_id: AuditArchiveId,
        approval_request_id: ApprovalRequestId,
        requested_by: PrincipalId,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let latest_expiry = created_at
            .checked_add_signed(Self::MAX_LIFETIME)
            .ok_or(DomainError::InvalidAuditExport)?;
        if expires_at <= created_at || expires_at > latest_expiry {
            return Err(DomainError::InvalidAuditExport);
        }
        Ok(Self {
            id,
            organization_id,
            archive_id,
            approval_request_id,
            requested_by,
            created_at,
            expires_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> AuditExportId {
        self.id
    }
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub const fn archive_id(&self) -> AuditArchiveId {
        self.archive_id
    }
    #[must_use]
    pub const fn approval_request_id(&self) -> ApprovalRequestId {
        self.approval_request_id
    }
    #[must_use]
    pub const fn requested_by(&self) -> PrincipalId {
        self.requested_by
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    #[must_use]
    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        now >= self.created_at && now < self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_grant_is_short_lived_and_time_bounded() {
        let now = Utc::now();
        let grant = AuditExportGrant::new(
            OrganizationId::new(),
            AuditArchiveId::new(),
            ApprovalRequestId::new(),
            PrincipalId::new(),
            now,
            now + Duration::minutes(15),
        )
        .unwrap();
        assert!(grant.is_active(now));
        assert!(!grant.is_active(grant.expires_at()));
        assert_eq!(
            AuditExportGrant::new(
                OrganizationId::new(),
                AuditArchiveId::new(),
                ApprovalRequestId::new(),
                PrincipalId::new(),
                now,
                now + Duration::minutes(31),
            ),
            Err(DomainError::InvalidAuditExport)
        );
    }
}
