use crate::{DomainError, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Canonical, URL-safe organization key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrganizationSlug(String);

impl OrganizationSlug {
    /// Validates and normalizes an organization slug.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into().trim().to_ascii_lowercase();
        let valid = (3..=64).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && !value.starts_with('-')
            && !value.ends_with('-');
        if valid {
            Ok(Self(value))
        } else {
            Err(DomainError::InvalidOrganizationSlug)
        }
    }

    /// Returns the canonical slug.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OrganizationSlug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for OrganizationSlug {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// Enterprise tenant boundary for identities, policy, mailboxes, and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    id: OrganizationId,
    slug: OrganizationSlug,
    display_name: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl Organization {
    /// Creates an organization after enforcing canonical naming rules.
    pub fn new(
        slug: impl Into<String>,
        display_name: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        let display_name = display_name
            .into()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if display_name.is_empty() || display_name.chars().count() > 200 {
            return Err(DomainError::InvalidDisplayName);
        }
        Ok(Self {
            id: OrganizationId::new(),
            slug: OrganizationSlug::new(slug)?,
            display_name,
            created_at: now,
            updated_at: now,
        })
    }

    /// Returns the stable organization identifier.
    #[must_use]
    pub const fn id(&self) -> OrganizationId {
        self.id
    }

    /// Returns the canonical organization slug.
    #[must_use]
    pub const fn slug(&self) -> &OrganizationSlug {
        &self.slug
    }

    /// Returns the display name.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the creation timestamp.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Returns the most recent update timestamp.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn organization_normalizes_safe_slug_and_name() {
        let organization =
            Organization::new(" Example-Corp ", " Example   Corporation ", Utc::now()).unwrap();
        assert_eq!(organization.slug().as_str(), "example-corp");
        assert_eq!(organization.display_name(), "Example Corporation");
    }

    #[test]
    fn organization_rejects_path_like_slug() {
        assert_eq!(
            Organization::new("../corp", "Corp", Utc::now()).unwrap_err(),
            DomainError::InvalidOrganizationSlug
        );
    }
}
