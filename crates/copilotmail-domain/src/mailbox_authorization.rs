use crate::{
    DelegationId, DomainError, MailboxId, MailboxMembershipRole, OrganizationId, PrincipalId,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Resource-level capabilities that may be granted on one mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxCapability {
    Read,
    Send,
    SendAs,
    ManageDelegations,
}

impl MailboxCapability {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "mailbox.read",
            Self::Send => "mailbox.send",
            Self::SendAs => "mailbox.send_as",
            Self::ManageDelegations => "mailbox.manage_delegations",
        }
    }

    pub const DELEGABLE: [Self; 3] = [Self::Read, Self::Send, Self::SendAs];
}

/// Time-bounded direct membership on a mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxMembership {
    organization_id: OrganizationId,
    mailbox_id: MailboxId,
    principal_id: PrincipalId,
    role: MailboxMembershipRole,
    valid_from: DateTime<Utc>,
    valid_until: Option<DateTime<Utc>>,
    created_by: PrincipalId,
}

impl MailboxMembership {
    pub fn new(
        organization_id: OrganizationId,
        mailbox_id: MailboxId,
        principal_id: PrincipalId,
        role: MailboxMembershipRole,
        valid_from: DateTime<Utc>,
        valid_until: Option<DateTime<Utc>>,
        created_by: PrincipalId,
    ) -> Result<Self, DomainError> {
        if valid_until.is_some_and(|until| until <= valid_from) {
            return Err(DomainError::InvalidMailboxAuthorization);
        }
        Ok(Self {
            organization_id,
            mailbox_id,
            principal_id,
            role,
            valid_from,
            valid_until,
            created_by,
        })
    }

    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub const fn mailbox_id(&self) -> MailboxId {
        self.mailbox_id
    }
    #[must_use]
    pub const fn principal_id(&self) -> PrincipalId {
        self.principal_id
    }
    #[must_use]
    pub const fn role(&self) -> MailboxMembershipRole {
        self.role
    }
    #[must_use]
    pub const fn valid_from(&self) -> DateTime<Utc> {
        self.valid_from
    }
    #[must_use]
    pub const fn valid_until(&self) -> Option<DateTime<Utc>> {
        self.valid_until
    }
    #[must_use]
    pub const fn created_by(&self) -> PrincipalId {
        self.created_by
    }
}

/// Explicit, scope-bounded mailbox authority delegated by one Principal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailboxDelegation {
    id: DelegationId,
    organization_id: OrganizationId,
    grantor_principal_id: PrincipalId,
    grantee_principal_id: PrincipalId,
    mailbox_id: MailboxId,
    capabilities: BTreeSet<MailboxCapability>,
    reason: String,
    valid_from: DateTime<Utc>,
    valid_until: DateTime<Utc>,
    created_at: DateTime<Utc>,
}

impl MailboxDelegation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        organization_id: OrganizationId,
        grantor_principal_id: PrincipalId,
        grantee_principal_id: PrincipalId,
        mailbox_id: MailboxId,
        capabilities: impl IntoIterator<Item = MailboxCapability>,
        reason: impl Into<String>,
        valid_from: DateTime<Utc>,
        valid_until: DateTime<Utc>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let capabilities = capabilities.into_iter().collect::<BTreeSet<_>>();
        let reason = reason.into();
        if grantor_principal_id == grantee_principal_id
            || capabilities.is_empty()
            || !capabilities
                .iter()
                .all(|capability| MailboxCapability::DELEGABLE.contains(capability))
            || reason.trim() != reason
            || reason.is_empty()
            || reason.chars().count() > 1_000
            || reason.chars().any(char::is_control)
            || valid_from < created_at - Duration::minutes(5)
            || valid_until <= valid_from
            || valid_until > created_at + Duration::days(365)
        {
            return Err(DomainError::InvalidMailboxAuthorization);
        }
        Ok(Self {
            id: DelegationId::new(),
            organization_id,
            grantor_principal_id,
            grantee_principal_id,
            mailbox_id,
            capabilities,
            reason,
            valid_from,
            valid_until,
            created_at,
        })
    }

    #[must_use]
    pub const fn id(&self) -> DelegationId {
        self.id
    }
    #[must_use]
    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }
    #[must_use]
    pub const fn grantor_principal_id(&self) -> PrincipalId {
        self.grantor_principal_id
    }
    #[must_use]
    pub const fn grantee_principal_id(&self) -> PrincipalId {
        self.grantee_principal_id
    }
    #[must_use]
    pub const fn mailbox_id(&self) -> MailboxId {
        self.mailbox_id
    }
    pub fn capabilities(&self) -> impl Iterator<Item = MailboxCapability> + '_ {
        self.capabilities.iter().copied()
    }
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    #[must_use]
    pub const fn valid_from(&self) -> DateTime<Utc> {
        self.valid_from
    }
    #[must_use]
    pub const fn valid_until(&self) -> DateTime<Utc> {
        self.valid_until
    }
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

/// Database projection used by the pure policy evaluator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMailboxDelegation {
    pub delegation_id: DelegationId,
    pub grantor_principal_id: PrincipalId,
    pub capabilities: BTreeSet<MailboxCapability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxAccessSnapshot {
    pub organization_id: OrganizationId,
    pub mailbox_id: MailboxId,
    pub membership: Option<MailboxMembershipRole>,
    pub delegations: Vec<ResolvedMailboxDelegation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailboxGrantSource {
    Membership(MailboxMembershipRole),
    Delegation {
        delegation_id: DelegationId,
        grantor_principal_id: PrincipalId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailboxAuthorizationDecision {
    pub source: MailboxGrantSource,
    pub on_behalf_of: Option<PrincipalId>,
}

impl MailboxAccessSnapshot {
    /// Evaluates resource ABAC after the caller has already passed the RBAC
    /// `mailbox.use` ceiling. Missing or mismatched context always denies.
    #[must_use]
    pub fn authorize(
        &self,
        target_organization_id: OrganizationId,
        target_mailbox_id: MailboxId,
        capability: MailboxCapability,
        on_behalf_of: Option<PrincipalId>,
    ) -> Option<MailboxAuthorizationDecision> {
        if self.organization_id != target_organization_id || self.mailbox_id != target_mailbox_id {
            return None;
        }
        if let Some(role) = self.membership {
            let permitted = match role {
                MailboxMembershipRole::Owner => true,
                MailboxMembershipRole::Member => {
                    matches!(
                        capability,
                        MailboxCapability::Read | MailboxCapability::Send
                    )
                }
                MailboxMembershipRole::Delegate => false,
            };
            if permitted && on_behalf_of.is_none() {
                return Some(MailboxAuthorizationDecision {
                    source: MailboxGrantSource::Membership(role),
                    on_behalf_of: None,
                });
            }
        }
        self.delegations.iter().find_map(|delegation| {
            if !delegation.capabilities.contains(&capability)
                || (capability == MailboxCapability::SendAs
                    && on_behalf_of != Some(delegation.grantor_principal_id))
                || (capability != MailboxCapability::SendAs && on_behalf_of.is_some())
            {
                return None;
            }
            Some(MailboxAuthorizationDecision {
                source: MailboxGrantSource::Delegation {
                    delegation_id: delegation.delegation_id,
                    grantor_principal_id: delegation.grantor_principal_id,
                },
                on_behalf_of,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailbox_policy_denies_cross_mailbox_and_requires_explicit_send_as_actor() {
        let organization_id = OrganizationId::new();
        let mailbox_id = MailboxId::new();
        let grantor = PrincipalId::new();
        let snapshot = MailboxAccessSnapshot {
            organization_id,
            mailbox_id,
            membership: None,
            delegations: vec![ResolvedMailboxDelegation {
                delegation_id: DelegationId::new(),
                grantor_principal_id: grantor,
                capabilities: [MailboxCapability::Read, MailboxCapability::SendAs]
                    .into_iter()
                    .collect(),
            }],
        };
        assert!(
            snapshot
                .authorize(organization_id, mailbox_id, MailboxCapability::Read, None)
                .is_some()
        );
        assert!(
            snapshot
                .authorize(
                    organization_id,
                    MailboxId::new(),
                    MailboxCapability::Read,
                    None
                )
                .is_none()
        );
        assert!(
            snapshot
                .authorize(organization_id, mailbox_id, MailboxCapability::SendAs, None)
                .is_none()
        );
        assert!(
            snapshot
                .authorize(
                    organization_id,
                    mailbox_id,
                    MailboxCapability::SendAs,
                    Some(grantor)
                )
                .is_some()
        );
    }
}
