use crate::{
    ApprovalRequestId, AuditArchiveId, ComplianceCaseId, DomainError, LegalHoldId, OrganizationId,
    PrincipalId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComplianceCaseStatus {
    Open,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplianceCase {
    id: ComplianceCaseId,
    organization_id: OrganizationId,
    external_reference: String,
    title: String,
    purpose: String,
    status: ComplianceCaseStatus,
    created_by: PrincipalId,
    created_at: DateTime<Utc>,
    closed_by: Option<PrincipalId>,
    closed_at: Option<DateTime<Utc>>,
}

impl ComplianceCase {
    pub fn new(
        organization_id: OrganizationId,
        external_reference: String,
        title: String,
        purpose: String,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if !valid_text(&external_reference, 200)
            || !valid_text(&title, 200)
            || !valid_text(&purpose, 2_000)
        {
            return Err(DomainError::InvalidComplianceCase);
        }
        Ok(Self {
            id: ComplianceCaseId::new(),
            organization_id,
            external_reference,
            title,
            purpose,
            status: ComplianceCaseStatus::Open,
            created_by,
            created_at,
            closed_by: None,
            closed_at: None,
        })
    }

    pub fn close(
        &mut self,
        closed_by: PrincipalId,
        closed_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        if self.status != ComplianceCaseStatus::Open || closed_at < self.created_at {
            return Err(DomainError::InvalidComplianceCase);
        }
        self.status = ComplianceCaseStatus::Closed;
        self.closed_by = Some(closed_by);
        self.closed_at = Some(closed_at);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        id: ComplianceCaseId,
        organization_id: OrganizationId,
        external_reference: String,
        title: String,
        purpose: String,
        status: ComplianceCaseStatus,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
        closed_by: Option<PrincipalId>,
        closed_at: Option<DateTime<Utc>>,
    ) -> Result<Self, DomainError> {
        let lifecycle_valid = match status {
            ComplianceCaseStatus::Open => closed_by.is_none() && closed_at.is_none(),
            ComplianceCaseStatus::Closed => {
                closed_by.is_some() && closed_at.is_some_and(|value| value >= created_at)
            }
        };
        if !valid_text(&external_reference, 200)
            || !valid_text(&title, 200)
            || !valid_text(&purpose, 2_000)
            || !lifecycle_valid
        {
            return Err(DomainError::InvalidComplianceCase);
        }
        Ok(Self {
            id,
            organization_id,
            external_reference,
            title,
            purpose,
            status,
            created_by,
            created_at,
            closed_by,
            closed_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> ComplianceCaseId {
        self.id
    }
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub fn external_reference(&self) -> &str {
        &self.external_reference
    }
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }
    #[must_use]
    pub const fn status(&self) -> ComplianceCaseStatus {
        self.status
    }
    #[must_use]
    pub const fn created_by(&self) -> PrincipalId {
        self.created_by
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub const fn closed_by(&self) -> Option<PrincipalId> {
        self.closed_by
    }
    #[must_use]
    pub const fn closed_at(&self) -> Option<DateTime<Utc>> {
        self.closed_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegalHoldStatus {
    Active,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegalHold {
    id: LegalHoldId,
    organization_id: OrganizationId,
    case_id: ComplianceCaseId,
    archive_id: AuditArchiveId,
    reason: String,
    status: LegalHoldStatus,
    apply_approval_request_id: ApprovalRequestId,
    created_by: PrincipalId,
    created_at: DateTime<Utc>,
    release_approval_request_id: Option<ApprovalRequestId>,
    released_by: Option<PrincipalId>,
    released_at: Option<DateTime<Utc>>,
}

impl LegalHold {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
        archive_id: AuditArchiveId,
        reason: String,
        apply_approval_request_id: ApprovalRequestId,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if !valid_text(&reason, 2_000) {
            return Err(DomainError::InvalidLegalHold);
        }
        Ok(Self {
            id: LegalHoldId::new(),
            organization_id,
            case_id,
            archive_id,
            reason,
            status: LegalHoldStatus::Active,
            apply_approval_request_id,
            created_by,
            created_at,
            release_approval_request_id: None,
            released_by: None,
            released_at: None,
        })
    }

    pub fn release(
        &mut self,
        approval_request_id: ApprovalRequestId,
        released_by: PrincipalId,
        released_at: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        if self.status != LegalHoldStatus::Active
            || approval_request_id == self.apply_approval_request_id
            || released_at < self.created_at
        {
            return Err(DomainError::InvalidLegalHold);
        }
        self.status = LegalHoldStatus::Released;
        self.release_approval_request_id = Some(approval_request_id);
        self.released_by = Some(released_by);
        self.released_at = Some(released_at);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        id: LegalHoldId,
        organization_id: OrganizationId,
        case_id: ComplianceCaseId,
        archive_id: AuditArchiveId,
        reason: String,
        status: LegalHoldStatus,
        apply_approval_request_id: ApprovalRequestId,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
        release_approval_request_id: Option<ApprovalRequestId>,
        released_by: Option<PrincipalId>,
        released_at: Option<DateTime<Utc>>,
    ) -> Result<Self, DomainError> {
        let lifecycle_valid = match status {
            LegalHoldStatus::Active => {
                release_approval_request_id.is_none()
                    && released_by.is_none()
                    && released_at.is_none()
            }
            LegalHoldStatus::Released => {
                release_approval_request_id.is_some_and(|value| value != apply_approval_request_id)
                    && released_by.is_some()
                    && released_at.is_some_and(|value| value >= created_at)
            }
        };
        if !valid_text(&reason, 2_000) || !lifecycle_valid {
            return Err(DomainError::InvalidLegalHold);
        }
        Ok(Self {
            id,
            organization_id,
            case_id,
            archive_id,
            reason,
            status,
            apply_approval_request_id,
            created_by,
            created_at,
            release_approval_request_id,
            released_by,
            released_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> LegalHoldId {
        self.id
    }
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub const fn case_id(&self) -> ComplianceCaseId {
        self.case_id
    }
    #[must_use]
    pub const fn archive_id(&self) -> AuditArchiveId {
        self.archive_id
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub const fn status(&self) -> LegalHoldStatus {
        self.status
    }
    #[must_use]
    pub const fn apply_approval_request_id(&self) -> ApprovalRequestId {
        self.apply_approval_request_id
    }
    #[must_use]
    pub const fn created_by(&self) -> PrincipalId {
        self.created_by
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub const fn release_approval_request_id(&self) -> Option<ApprovalRequestId> {
        self.release_approval_request_id
    }
    #[must_use]
    pub const fn released_by(&self) -> Option<PrincipalId> {
        self.released_by
    }
    #[must_use]
    pub const fn released_at(&self) -> Option<DateTime<Utc>> {
        self.released_at
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_and_hold_lifecycles_are_scoped_and_irreversible() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let mut case = ComplianceCase::new(
            organization_id,
            "LEGAL-2026-001".into(),
            "Investigation".into(),
            "Preserve signed audit evidence".into(),
            actor,
            now,
        )
        .unwrap();
        let apply_approval = ApprovalRequestId::new();
        let mut hold = LegalHold::new(
            organization_id,
            case.id(),
            AuditArchiveId::new(),
            "Counsel preservation notice".into(),
            apply_approval,
            actor,
            now,
        )
        .unwrap();

        assert_eq!(hold.status(), LegalHoldStatus::Active);
        assert_eq!(
            hold.release(apply_approval, actor, now),
            Err(DomainError::InvalidLegalHold)
        );
        let release_approval = ApprovalRequestId::new();
        hold.release(release_approval, actor, now).unwrap();
        assert_eq!(hold.status(), LegalHoldStatus::Released);
        assert_eq!(
            hold.release(ApprovalRequestId::new(), actor, now),
            Err(DomainError::InvalidLegalHold)
        );

        case.close(actor, now).unwrap();
        assert_eq!(case.status(), ComplianceCaseStatus::Closed);
        assert_eq!(
            case.close(actor, now),
            Err(DomainError::InvalidComplianceCase)
        );
    }

    #[test]
    fn compliance_text_rejects_controls_whitespace_and_empty_values() {
        let now = Utc::now();
        for invalid in ["", " leading", "trailing ", "line\nbreak"] {
            assert_eq!(
                ComplianceCase::new(
                    OrganizationId::new(),
                    invalid.into(),
                    "Title".into(),
                    "Purpose".into(),
                    PrincipalId::new(),
                    now,
                ),
                Err(DomainError::InvalidComplianceCase)
            );
        }
    }
}
