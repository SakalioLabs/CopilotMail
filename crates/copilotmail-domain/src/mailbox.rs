use crate::{DomainError, MailboxId, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A normalized ASCII mailbox address.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailboxAddress(String);

impl MailboxAddress {
    /// Parses and normalizes an address supported by the first enterprise release.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, DomainError> {
        let value = value.as_ref().trim();
        if value.is_empty()
            || value.len() > 254
            || value.chars().any(char::is_control)
            || value.matches('@').count() != 1
        {
            return Err(DomainError::InvalidMailboxAddress);
        }

        let (local, domain) = value
            .split_once('@')
            .ok_or(DomainError::InvalidMailboxAddress)?;
        if !valid_local_part(local) || !valid_domain(domain) {
            return Err(DomainError::InvalidMailboxAddress);
        }
        Ok(Self(format!(
            "{}@{}",
            local.to_ascii_lowercase(),
            domain.to_ascii_lowercase()
        )))
    }

    /// Returns the normalized address text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MailboxAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for MailboxAddress {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// Business purpose of a mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxKind {
    /// Personal mailbox primarily owned by a human.
    Human,
    /// Mailbox primarily owned and processed by an Agent.
    Agent,
    /// Shared mailbox with explicit membership and delegation.
    Shared,
    /// Distribution or collaborative group mailbox.
    Group,
}

/// Role granted to a principal on a mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxMembershipRole {
    /// Full owner of the mailbox and its delegation policy.
    Owner,
    /// Normal member permitted by mailbox policy.
    Member,
    /// Time-bounded or scope-bounded delegated access.
    Delegate,
}

/// A mailbox managed by the CopilotMail control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mailbox {
    /// Stable mailbox identifier.
    pub id: MailboxId,
    /// Owning organization.
    pub organization_id: OrganizationId,
    /// Mailbox purpose.
    pub kind: MailboxKind,
    /// Primary routable address.
    pub primary_address: MailboxAddress,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

impl Mailbox {
    /// Creates a mailbox with a validated primary address.
    #[must_use]
    pub fn new(
        organization_id: OrganizationId,
        kind: MailboxKind,
        primary_address: MailboxAddress,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: MailboxId::new(),
            organization_id,
            kind,
            primary_address,
            created_at: now,
        }
    }
}

fn valid_local_part(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value.is_ascii()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'.' | b'!'
                        | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'/'
                        | b'='
                        | b'?'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'{'
                        | b'|'
                        | b'}'
                        | b'~'
                )
        })
}

fn valid_domain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.is_ascii()
        && value.contains('.')
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_supported_address() {
        let address = MailboxAddress::parse(" Agent.One@Example.COM ").unwrap();
        assert_eq!(address.as_str(), "agent.one@example.com");
    }

    #[test]
    fn rejects_header_and_command_injection() {
        for value in [
            "agent\r\nBcc:victim@example.com@example.com",
            "agent name@example.com",
            ".agent@example.com",
            "agent..one@example.com",
            "agent@-example.com",
            "agent@example",
        ] {
            assert!(MailboxAddress::parse(value).is_err(), "{value:?}");
        }
    }
}
