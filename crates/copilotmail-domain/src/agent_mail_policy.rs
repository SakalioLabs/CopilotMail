use crate::{
    AgentMailPolicyActivationId, ApprovalRequestId, DomainError, OrganizationId, PrincipalId,
};
use chrono::{DateTime, Utc};
use idna::domain_to_ascii;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One immutable candidate policy for production Agent outbound mail.
///
/// This type deliberately does not make a policy active. Activation and each
/// send admission require separate approval and ledger evidence. Hard maxima
/// constrain configuration mistakes without pretending to choose the
/// company's actual business quotas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMailPolicyVersion {
    organization_id: OrganizationId,
    agent_id: PrincipalId,
    version: u32,
    allowed_recipient_domains: BTreeSet<String>,
    max_recipients_per_message: u16,
    max_messages_per_hour: u32,
    max_messages_per_recipient_per_day: u16,
    require_first_contact_approval: bool,
    max_auto_replies_per_thread_per_day: u8,
}

/// Append-only proof that an exact production Agent policy version became the
/// current candidate for future send admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMailPolicyActivation {
    id: AgentMailPolicyActivationId,
    organization_id: OrganizationId,
    agent_id: PrincipalId,
    policy_version: u32,
    approval_request_id: ApprovalRequestId,
    activated_by: PrincipalId,
    activated_at: DateTime<Utc>,
}

impl AgentMailPolicyActivation {
    pub fn new(
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        policy_version: u32,
        approval_request_id: ApprovalRequestId,
        activated_by: PrincipalId,
        activated_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        Self::restore(
            AgentMailPolicyActivationId::new(),
            organization_id,
            agent_id,
            policy_version,
            approval_request_id,
            activated_by,
            activated_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: AgentMailPolicyActivationId,
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        policy_version: u32,
        approval_request_id: ApprovalRequestId,
        activated_by: PrincipalId,
        activated_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if policy_version == 0 {
            return Err(DomainError::InvalidAgentMailPolicy);
        }
        Ok(Self {
            id,
            organization_id,
            agent_id,
            policy_version,
            approval_request_id,
            activated_by,
            activated_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> AgentMailPolicyActivationId {
        self.id
    }

    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    #[must_use]
    pub const fn agent_id(&self) -> PrincipalId {
        self.agent_id
    }

    #[must_use]
    pub const fn policy_version(&self) -> u32 {
        self.policy_version
    }

    #[must_use]
    pub const fn approval_request_id(&self) -> ApprovalRequestId {
        self.approval_request_id
    }

    #[must_use]
    pub const fn activated_by(&self) -> PrincipalId {
        self.activated_by
    }

    #[must_use]
    pub const fn activated_at(&self) -> DateTime<Utc> {
        self.activated_at
    }
}

impl AgentMailPolicyVersion {
    pub const MAX_ALLOWED_DOMAINS: usize = 100;
    pub const MAX_RECIPIENTS_PER_MESSAGE: u16 = 100;
    pub const MAX_MESSAGES_PER_HOUR: u32 = 10_000;
    pub const MAX_MESSAGES_PER_RECIPIENT_PER_DAY: u16 = 1_000;
    pub const MAX_AUTO_REPLIES_PER_THREAD_PER_DAY: u8 = 10;

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        agent_id: PrincipalId,
        version: u32,
        allowed_recipient_domains: impl IntoIterator<Item = String>,
        max_recipients_per_message: u16,
        max_messages_per_hour: u32,
        max_messages_per_recipient_per_day: u16,
        require_first_contact_approval: bool,
        max_auto_replies_per_thread_per_day: u8,
    ) -> Result<Self, DomainError> {
        let domains = allowed_recipient_domains
            .into_iter()
            .map(|domain| normalize_domain(&domain))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if version == 0
            || domains.is_empty()
            || domains.len() > Self::MAX_ALLOWED_DOMAINS
            || !(1..=Self::MAX_RECIPIENTS_PER_MESSAGE).contains(&max_recipients_per_message)
            || !(1..=Self::MAX_MESSAGES_PER_HOUR).contains(&max_messages_per_hour)
            || !(1..=Self::MAX_MESSAGES_PER_RECIPIENT_PER_DAY)
                .contains(&max_messages_per_recipient_per_day)
            || !require_first_contact_approval
            || max_auto_replies_per_thread_per_day > Self::MAX_AUTO_REPLIES_PER_THREAD_PER_DAY
        {
            return Err(DomainError::InvalidAgentMailPolicy);
        }
        Ok(Self {
            organization_id,
            agent_id,
            version,
            allowed_recipient_domains: domains,
            max_recipients_per_message,
            max_messages_per_hour,
            max_messages_per_recipient_per_day,
            require_first_contact_approval,
            max_auto_replies_per_thread_per_day,
        })
    }

    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    #[must_use]
    pub const fn agent_id(&self) -> PrincipalId {
        self.agent_id
    }

    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub fn allowed_recipient_domains(&self) -> &BTreeSet<String> {
        &self.allowed_recipient_domains
    }

    #[must_use]
    pub const fn max_recipients_per_message(&self) -> u16 {
        self.max_recipients_per_message
    }

    #[must_use]
    pub const fn max_messages_per_hour(&self) -> u32 {
        self.max_messages_per_hour
    }

    #[must_use]
    pub const fn max_messages_per_recipient_per_day(&self) -> u16 {
        self.max_messages_per_recipient_per_day
    }

    #[must_use]
    pub const fn require_first_contact_approval(&self) -> bool {
        self.require_first_contact_approval
    }

    #[must_use]
    pub const fn max_auto_replies_per_thread_per_day(&self) -> u8 {
        self.max_auto_replies_per_thread_per_day
    }
}

fn normalize_domain(value: &str) -> Result<String, DomainError> {
    if value.is_empty()
        || value != value.trim()
        || value.ends_with('.')
        || value.chars().any(char::is_control)
    {
        return Err(DomainError::InvalidAgentMailPolicy);
    }
    let ascii = domain_to_ascii(value).map_err(|_| DomainError::InvalidAgentMailPolicy)?;
    let normalized = ascii.to_ascii_lowercase();
    if normalized.len() > 253
        || !normalized.contains('.')
        || normalized.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                || !label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                || !label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
    {
        return Err(DomainError::InvalidAgentMailPolicy);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_normalizes_domains_and_preserves_explicit_safe_limits() {
        let policy = AgentMailPolicyVersion::new(
            OrganizationId::new(),
            PrincipalId::new(),
            1,
            ["BÜCHER.example".into(), "internal.example".into()],
            10,
            100,
            5,
            true,
            2,
        )
        .unwrap();
        assert!(
            policy
                .allowed_recipient_domains()
                .contains("xn--bcher-kva.example")
        );
        assert_eq!(policy.max_messages_per_hour(), 100);
    }

    #[test]
    fn production_policy_cannot_disable_first_contact_approval() {
        let result = AgentMailPolicyVersion::new(
            OrganizationId::new(),
            PrincipalId::new(),
            1,
            ["example.test".into()],
            1,
            1,
            1,
            false,
            0,
        );
        assert_eq!(result, Err(DomainError::InvalidAgentMailPolicy));
    }

    #[test]
    fn policy_rejects_empty_allowlists_and_unbounded_or_ambiguous_values() {
        let invalid = [
            AgentMailPolicyVersion::new(
                OrganizationId::new(),
                PrincipalId::new(),
                0,
                ["example.test".into()],
                1,
                1,
                1,
                true,
                0,
            ),
            AgentMailPolicyVersion::new(
                OrganizationId::new(),
                PrincipalId::new(),
                1,
                Vec::new(),
                1,
                1,
                1,
                true,
                0,
            ),
            AgentMailPolicyVersion::new(
                OrganizationId::new(),
                PrincipalId::new(),
                1,
                ["example.test.".into()],
                1,
                1,
                1,
                true,
                0,
            ),
            AgentMailPolicyVersion::new(
                OrganizationId::new(),
                PrincipalId::new(),
                1,
                ["_service.example".into()],
                1,
                1,
                1,
                true,
                0,
            ),
            AgentMailPolicyVersion::new(
                OrganizationId::new(),
                PrincipalId::new(),
                1,
                ["example.test".into()],
                101,
                1,
                1,
                true,
                0,
            ),
        ];
        assert!(
            invalid
                .into_iter()
                .all(|result| result == Err(DomainError::InvalidAgentMailPolicy))
        );
    }

    #[test]
    fn activation_requires_a_positive_exact_policy_version() {
        let result = AgentMailPolicyActivation::new(
            OrganizationId::new(),
            PrincipalId::new(),
            0,
            ApprovalRequestId::new(),
            PrincipalId::new(),
            Utc::now(),
        );
        assert_eq!(result, Err(DomainError::InvalidAgentMailPolicy));
    }
}
