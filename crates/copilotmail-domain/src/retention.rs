use crate::{
    DomainError, OrganizationId, PrincipalId, RetentionPolicyId, RetentionPolicyVersionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable resource families to which a retention policy can apply.
///
/// Audit evidence and mail content remain separate because their storage,
/// export, legal-hold, and eventual disposal mechanisms are different.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionResourceKind {
    AuditEvidence,
    MailContent,
}

impl RetentionResourceKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuditEvidence => "audit.evidence",
            Self::MailContent => "mail.content",
        }
    }
}

/// Immutable tenant policy identity. Operational settings only ever appear in
/// append-only versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicy {
    id: RetentionPolicyId,
    organization_id: OrganizationId,
    policy_key: String,
    display_name: String,
    resource_kind: RetentionResourceKind,
    created_by: PrincipalId,
    created_at: DateTime<Utc>,
}

impl RetentionPolicy {
    pub fn new(
        organization_id: OrganizationId,
        policy_key: String,
        display_name: String,
        resource_kind: RetentionResourceKind,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        Self::restore(
            RetentionPolicyId::new(),
            organization_id,
            policy_key,
            display_name,
            resource_kind,
            created_by,
            created_at,
        )
    }

    pub fn restore(
        id: RetentionPolicyId,
        organization_id: OrganizationId,
        policy_key: String,
        display_name: String,
        resource_kind: RetentionResourceKind,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if !valid_policy_key(&policy_key) || !valid_text(&display_name, 200) {
            return Err(DomainError::InvalidRetentionPolicy);
        }
        Ok(Self {
            id,
            organization_id,
            policy_key,
            display_name,
            resource_kind,
            created_by,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> RetentionPolicyId {
        self.id
    }
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub fn policy_key(&self) -> &str {
        &self.policy_key
    }
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    #[must_use]
    pub const fn resource_kind(&self) -> RetentionResourceKind {
        self.resource_kind
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

/// One immutable set of production retention controls.
///
/// `minimum_retention_days` is the earliest point at which disposal may ever
/// be considered. `archive_after_days` can move cold evidence earlier.
/// `delete_after_days = None` means indefinite retention. Legal Hold is not a
/// configurable flag: every disposal executor must check it and fail closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicyVersion {
    id: RetentionPolicyVersionId,
    policy_id: RetentionPolicyId,
    organization_id: OrganizationId,
    version: u32,
    minimum_retention_days: u32,
    archive_after_days: Option<u32>,
    delete_after_days: Option<u32>,
    rule: Value,
    created_by: PrincipalId,
    created_at: DateTime<Utc>,
}

impl RetentionPolicyVersion {
    pub const MAX_RETENTION_DAYS: u32 = 36_500;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        policy_id: RetentionPolicyId,
        organization_id: OrganizationId,
        version: u32,
        minimum_retention_days: u32,
        archive_after_days: Option<u32>,
        delete_after_days: Option<u32>,
        rule: Value,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        Self::restore(
            RetentionPolicyVersionId::new(),
            policy_id,
            organization_id,
            version,
            minimum_retention_days,
            archive_after_days,
            delete_after_days,
            rule,
            created_by,
            created_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: RetentionPolicyVersionId,
        policy_id: RetentionPolicyId,
        organization_id: OrganizationId,
        version: u32,
        minimum_retention_days: u32,
        archive_after_days: Option<u32>,
        delete_after_days: Option<u32>,
        rule: Value,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let retention_valid = (1..=Self::MAX_RETENTION_DAYS).contains(&minimum_retention_days);
        let archive_valid = archive_after_days.is_none_or(|days| {
            days > 0 && days <= minimum_retention_days && days <= Self::MAX_RETENTION_DAYS
        });
        let delete_valid = delete_after_days
            .is_none_or(|days| days >= minimum_retention_days && days <= Self::MAX_RETENTION_DAYS);
        if version == 0 || !retention_valid || !archive_valid || !delete_valid || !valid_rule(&rule)
        {
            return Err(DomainError::InvalidRetentionPolicy);
        }
        Ok(Self {
            id,
            policy_id,
            organization_id,
            version,
            minimum_retention_days,
            archive_after_days,
            delete_after_days,
            rule,
            created_by,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> RetentionPolicyVersionId {
        self.id
    }
    #[must_use]
    pub const fn policy_id(&self) -> RetentionPolicyId {
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
    pub const fn minimum_retention_days(&self) -> u32 {
        self.minimum_retention_days
    }
    #[must_use]
    pub const fn archive_after_days(&self) -> Option<u32> {
        self.archive_after_days
    }
    #[must_use]
    pub const fn delete_after_days(&self) -> Option<u32> {
        self.delete_after_days
    }
    #[must_use]
    pub const fn rule(&self) -> &Value {
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

fn valid_policy_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn valid_text(value: &str, max_chars: usize) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed == value
        && trimmed.chars().count() <= max_chars
        && !trimmed.chars().any(char::is_control)
}

fn valid_rule(rule: &Value) -> bool {
    rule.as_object().is_some_and(|object| {
        object.len() <= 32
            && serde_json::to_vec(rule).is_ok_and(|serialized| serialized.len() <= 16 * 1024)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn policy_identity_and_versions_are_bounded_and_legal_hold_safe() {
        let now = Utc::now();
        let organization_id = OrganizationId::new();
        let actor = PrincipalId::new();
        let policy = RetentionPolicy::new(
            organization_id,
            "audit.standard".into(),
            "Standard audit retention".into(),
            RetentionResourceKind::AuditEvidence,
            actor,
            now,
        )
        .unwrap();
        let version = RetentionPolicyVersion::new(
            policy.id(),
            organization_id,
            1,
            2_555,
            Some(30),
            Some(3_650),
            json!({"classification": ["standard"]}),
            actor,
            now,
        )
        .unwrap();
        assert_eq!(version.minimum_retention_days(), 2_555);
        assert_eq!(version.delete_after_days(), Some(3_650));

        assert_eq!(
            RetentionPolicyVersion::new(
                policy.id(),
                organization_id,
                2,
                365,
                Some(366),
                None,
                json!({}),
                actor,
                now,
            ),
            Err(DomainError::InvalidRetentionPolicy)
        );
        assert_eq!(
            RetentionPolicyVersion::new(
                policy.id(),
                organization_id,
                2,
                365,
                None,
                Some(364),
                json!({}),
                actor,
                now,
            ),
            Err(DomainError::InvalidRetentionPolicy)
        );
    }

    #[test]
    fn retention_policy_rejects_ambiguous_keys_and_unbounded_rules() {
        let now = Utc::now();
        for key in ["", ".audit", "audit.", "audit..standard", "Audit.standard"] {
            assert!(matches!(
                RetentionPolicy::new(
                    OrganizationId::new(),
                    key.into(),
                    "Policy".into(),
                    RetentionResourceKind::AuditEvidence,
                    PrincipalId::new(),
                    now,
                ),
                Err(DomainError::InvalidRetentionPolicy)
            ));
        }
        let mut oversized = serde_json::Map::new();
        for index in 0..33 {
            oversized.insert(format!("field{index}"), json!(true));
        }
        assert!(matches!(
            RetentionPolicyVersion::new(
                RetentionPolicyId::new(),
                OrganizationId::new(),
                1,
                365,
                None,
                None,
                Value::Object(oversized),
                PrincipalId::new(),
                now,
            ),
            Err(DomainError::InvalidRetentionPolicy)
        ));
    }
}
