use crate::{
    ApprovalRequestId, ApprovalStepId, DomainError, OrganizationId, PolicyId, PolicyVersionId,
    PrincipalId,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

/// High-risk operations that may be narrowed by an approval policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalAction {
    SendAs,
    SendOnBehalfOf,
    ContentReview,
    AuditExport,
    RetentionChange,
    PrivilegeElevation,
    LegalHold,
    BreakGlass,
    BulkDelete,
    ProductionAgentMail,
    AgentFirstContact,
}

impl ApprovalAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SendAs => "mail.send_as",
            Self::SendOnBehalfOf => "mail.send_on_behalf_of",
            Self::ContentReview => "compliance.content_review",
            Self::AuditExport => "audit.export",
            Self::RetentionChange => "compliance.retention_change",
            Self::PrivilegeElevation => "authorization.privilege_elevation",
            Self::LegalHold => "compliance.legal_hold",
            Self::BreakGlass => "security.break_glass",
            Self::BulkDelete => "mail.bulk_delete",
            Self::ProductionAgentMail => "agent.production_mail",
            Self::AgentFirstContact => "agent.first_contact",
        }
    }
}

/// One immutable, auditable policy version. Editing creates a new version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalPolicyVersion {
    id: PolicyVersionId,
    policy_id: PolicyId,
    organization_id: OrganizationId,
    version: u32,
    action: ApprovalAction,
    required_approvals: u8,
    rule: Value,
    created_by: PrincipalId,
    created_at: DateTime<Utc>,
}

impl ApprovalPolicyVersion {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        policy_id: PolicyId,
        organization_id: OrganizationId,
        version: u32,
        action: ApprovalAction,
        required_approvals: u8,
        rule: Value,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if version == 0 || !(1..=5).contains(&required_approvals) || !rule.is_object() {
            return Err(DomainError::InvalidApproval);
        }
        Ok(Self {
            id: PolicyVersionId::new(),
            policy_id,
            organization_id,
            version,
            action,
            required_approvals,
            rule,
            created_by,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> PolicyVersionId {
        self.id
    }
    #[must_use]
    pub const fn policy_id(&self) -> PolicyId {
        self.policy_id
    }
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }
    #[must_use]
    pub const fn action(&self) -> ApprovalAction {
        self.action
    }
    #[must_use]
    pub const fn required_approvals(&self) -> u8 {
        self.required_approvals
    }
    #[must_use]
    pub fn rule(&self) -> &Value {
        &self.rule
    }
    #[must_use]
    pub const fn created_by(&self) -> PrincipalId {
        self.created_by
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalStep {
    id: ApprovalStepId,
    approver_principal_id: PrincipalId,
    decision: ApprovalDecision,
    reason: String,
    decided_at: DateTime<Utc>,
}

impl ApprovalStep {
    #[must_use]
    pub const fn id(&self) -> ApprovalStepId {
        self.id
    }
    #[must_use]
    pub const fn approver_principal_id(&self) -> PrincipalId {
        self.approver_principal_id
    }
    #[must_use]
    pub const fn decision(&self) -> ApprovalDecision {
        self.decision
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub const fn decided_at(&self) -> DateTime<Utc> {
        self.decided_at
    }
}

/// Aggregate for one policy-version-bound approval request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    id: ApprovalRequestId,
    organization_id: OrganizationId,
    policy_version_id: PolicyVersionId,
    action: ApprovalAction,
    resource_kind: String,
    resource_id: String,
    requested_by: PrincipalId,
    reason: String,
    ticket_reference: Option<String>,
    required_approvals: u8,
    status: ApprovalStatus,
    steps: Vec<ApprovalStep>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl ApprovalRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        policy_version_id: PolicyVersionId,
        action: ApprovalAction,
        resource_kind: impl Into<String>,
        resource_id: impl Into<String>,
        requested_by: PrincipalId,
        reason: impl Into<String>,
        ticket_reference: Option<String>,
        required_approvals: u8,
        created_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let resource_kind = resource_kind.into();
        let resource_id = resource_id.into();
        let reason = reason.into();
        if !valid_text(&resource_kind, 100)
            || !valid_text(&resource_id, 500)
            || !valid_text(&reason, 1_000)
            || ticket_reference
                .as_deref()
                .is_some_and(|ticket| !valid_text(ticket, 500))
            || !(1..=5).contains(&required_approvals)
            || expires_at <= created_at
            || expires_at > created_at + Duration::days(30)
        {
            return Err(DomainError::InvalidApproval);
        }
        Ok(Self {
            id: ApprovalRequestId::new(),
            organization_id,
            policy_version_id,
            action,
            resource_kind,
            resource_id,
            requested_by,
            reason,
            ticket_reference,
            required_approvals,
            status: ApprovalStatus::Pending,
            steps: Vec::new(),
            created_at,
            expires_at,
        })
    }

    pub fn decide(
        &mut self,
        approver: PrincipalId,
        decision: ApprovalDecision,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<ApprovalStepId, DomainError> {
        let reason = reason.into();
        if self.effective_status(now) != ApprovalStatus::Pending
            || approver == self.requested_by
            || self
                .steps
                .iter()
                .any(|step| step.approver_principal_id == approver)
            || !valid_text(&reason, 1_000)
            || now < self.created_at
        {
            return Err(DomainError::InvalidApproval);
        }
        let step = ApprovalStep {
            id: ApprovalStepId::new(),
            approver_principal_id: approver,
            decision,
            reason,
            decided_at: now,
        };
        let step_id = step.id;
        self.steps.push(step);
        if decision == ApprovalDecision::Rejected {
            self.status = ApprovalStatus::Rejected;
        } else {
            let approvals = self
                .steps
                .iter()
                .filter(|step| step.decision == ApprovalDecision::Approved)
                .map(|step| step.approver_principal_id)
                .collect::<BTreeSet<_>>();
            if approvals.len() >= usize::from(self.required_approvals) {
                self.status = ApprovalStatus::Approved;
            }
        }
        Ok(step_id)
    }

    /// Withdraws a still-pending request. Approval cancellation is deliberately
    /// requester-bound and cannot rewrite an approved or rejected decision.
    pub fn cancel(
        &mut self,
        requested_by: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        if requested_by != self.requested_by
            || self.effective_status(now) != ApprovalStatus::Pending
            || now < self.created_at
        {
            return Err(DomainError::InvalidApproval);
        }
        self.status = ApprovalStatus::Cancelled;
        Ok(())
    }

    #[must_use]
    pub fn effective_status(&self, now: DateTime<Utc>) -> ApprovalStatus {
        if matches!(
            self.status,
            ApprovalStatus::Pending | ApprovalStatus::Approved
        ) && now >= self.expires_at
        {
            ApprovalStatus::Expired
        } else {
            self.status
        }
    }

    /// Approval can only narrow an already-authorized operation; callers must
    /// still evaluate identity, tenant, RBAC, and resource authorization.
    #[must_use]
    pub fn permits(
        &self,
        organization_id: OrganizationId,
        action: ApprovalAction,
        resource_kind: &str,
        resource_id: &str,
        now: DateTime<Utc>,
    ) -> bool {
        self.organization_id == organization_id
            && self.action == action
            && self.resource_kind == resource_kind
            && self.resource_id == resource_id
            && self.effective_status(now) == ApprovalStatus::Approved
    }

    #[must_use]
    pub const fn id(&self) -> ApprovalRequestId {
        self.id
    }
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub const fn policy_version_id(&self) -> PolicyVersionId {
        self.policy_version_id
    }
    #[must_use]
    pub const fn action(&self) -> ApprovalAction {
        self.action
    }
    #[must_use]
    pub fn resource_kind(&self) -> &str {
        &self.resource_kind
    }
    #[must_use]
    pub fn resource_id(&self) -> &str {
        &self.resource_id
    }
    #[must_use]
    pub const fn requested_by(&self) -> PrincipalId {
        self.requested_by
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub fn ticket_reference(&self) -> Option<&str> {
        self.ticket_reference.as_deref()
    }
    #[must_use]
    pub const fn required_approvals(&self) -> u8 {
        self.required_approvals
    }
    #[must_use]
    pub const fn status(&self) -> ApprovalStatus {
        self.status
    }
    pub fn steps(&self) -> impl Iterator<Item = &ApprovalStep> {
        self.steps.iter()
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
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
    fn dual_approval_enforces_separation_of_duty_scope_and_expiry() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let requester = PrincipalId::new();
        let mut request = ApprovalRequest::new(
            organization_id,
            PolicyVersionId::new(),
            ApprovalAction::SendAs,
            "mailbox",
            "mailbox-a",
            requester,
            "Send approved customer response",
            Some("SEC-123".into()),
            2,
            now,
            now + Duration::hours(1),
        )
        .unwrap();
        assert!(
            request
                .decide(requester, ApprovalDecision::Approved, "self approve", now,)
                .is_err()
        );
        let first = PrincipalId::new();
        request
            .decide(first, ApprovalDecision::Approved, "reviewed", now)
            .unwrap();
        assert_eq!(request.effective_status(now), ApprovalStatus::Pending);
        assert!(
            request
                .decide(first, ApprovalDecision::Approved, "again", now)
                .is_err()
        );
        request
            .decide(
                PrincipalId::new(),
                ApprovalDecision::Approved,
                "independent review",
                now,
            )
            .unwrap();
        assert!(request.permits(
            organization_id,
            ApprovalAction::SendAs,
            "mailbox",
            "mailbox-a",
            now,
        ));
        assert!(!request.permits(
            organization_id,
            ApprovalAction::SendAs,
            "mailbox",
            "mailbox-b",
            now,
        ));
        assert_eq!(
            request.effective_status(now + Duration::hours(2)),
            ApprovalStatus::Expired
        );
    }

    #[test]
    fn one_rejection_is_terminal() {
        let now = Utc::now();
        let mut request = ApprovalRequest::new(
            OrganizationId::new(),
            PolicyVersionId::new(),
            ApprovalAction::AuditExport,
            "audit_query",
            "query-1",
            PrincipalId::new(),
            "Export evidence",
            Some("GRC-9".into()),
            2,
            now,
            now + Duration::hours(1),
        )
        .unwrap();
        request
            .decide(
                PrincipalId::new(),
                ApprovalDecision::Rejected,
                "scope too broad",
                now,
            )
            .unwrap();
        assert_eq!(request.effective_status(now), ApprovalStatus::Rejected);
        assert!(
            request
                .decide(
                    PrincipalId::new(),
                    ApprovalDecision::Approved,
                    "late approval",
                    now,
                )
                .is_err()
        );
    }

    #[test]
    fn only_requester_can_cancel_a_live_pending_request() {
        let now = Utc::now();
        let requester = PrincipalId::new();
        let mut request = ApprovalRequest::new(
            OrganizationId::new(),
            PolicyVersionId::new(),
            ApprovalAction::LegalHold,
            "compliance.legal_hold",
            "hold-1",
            requester,
            "Preserve evidence",
            None,
            2,
            now,
            now + Duration::hours(1),
        )
        .unwrap();
        assert!(request.cancel(PrincipalId::new(), now).is_err());
        request.cancel(requester, now).unwrap();
        assert_eq!(request.effective_status(now), ApprovalStatus::Cancelled);
        assert!(
            request
                .decide(
                    PrincipalId::new(),
                    ApprovalDecision::Approved,
                    "too late",
                    now,
                )
                .is_err()
        );
    }
}
