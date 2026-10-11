use crate::{DnsVerificationId, DomainError, MailDomainId, OrganizationId};
use chrono::{DateTime, Utc};
use idna::{AsciiDenyList, domain_to_ascii_cow};
use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr, str::FromStr};

/// Canonical ASCII DNS name used for mail routing and policy.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MailDomainName(String);

impl MailDomainName {
    /// Normalizes an internationalized domain to lowercase ASCII/Punycode.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, DomainError> {
        let value = value.as_ref().trim();
        if value.ends_with("..") {
            return Err(DomainError::InvalidMailDomainName);
        }
        let value = value.strip_suffix('.').unwrap_or(value);
        if value.is_empty()
            || value.len() > 253
            || value.chars().any(crate::is_unicode_confusion_mark)
            || value.parse::<IpAddr>().is_ok()
        {
            return Err(DomainError::InvalidMailDomainName);
        }
        let ascii = domain_to_ascii_cow(value.as_bytes(), AsciiDenyList::EMPTY)
            .map_err(|_| DomainError::InvalidMailDomainName)?
            .to_ascii_lowercase();
        if ascii.len() > 253
            || !ascii.contains('.')
            || ascii.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
        {
            return Err(DomainError::InvalidMailDomainName);
        }
        Ok(Self(ascii))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MailDomainName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for MailDomainName {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

/// Lifecycle state controlled by explicit governance transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailDomainStatus {
    Draft,
    PendingDns,
    Verified,
    Active,
    Suspended,
    Retired,
}

/// Outbound delivery strategy selected without coupling to one engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailDomainRouteMode {
    DirectMx,
    SmartHost,
}

/// Eventual-consistency state of the replaceable mail-engine projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailEngineProjectionStatus {
    NotProjected,
    Pending,
    Ready,
    Failed,
}

/// Versioned routing projection. Both directions are disabled at creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomainRoute {
    pub mail_domain_id: MailDomainId,
    pub organization_id: OrganizationId,
    pub version: i64,
    pub inbound_enabled: bool,
    pub outbound_enabled: bool,
    pub smart_host_ref: Option<String>,
    pub dkim_key_ref: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl MailDomainRoute {
    #[must_use]
    pub fn disabled(
        mail_domain_id: MailDomainId,
        organization_id: OrganizationId,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            mail_domain_id,
            organization_id,
            version: 1,
            inbound_enabled: false,
            outbound_enabled: false,
            smart_host_ref: None,
            dkim_key_ref: None,
            updated_at: now,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        domain_status: MailDomainStatus,
        route_mode: MailDomainRouteMode,
        inbound_enabled: bool,
        outbound_enabled: bool,
        smart_host_ref: Option<String>,
        dkim_key_ref: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), DomainError> {
        let smart_host_ref = normalized_reference(smart_host_ref)?;
        let dkim_key_ref = normalized_reference(dkim_key_ref)?;
        if now < self.updated_at
            || (inbound_enabled || outbound_enabled) && domain_status != MailDomainStatus::Active
            || outbound_enabled && dkim_key_ref.is_none()
            || route_mode == MailDomainRouteMode::SmartHost
                && outbound_enabled
                && smart_host_ref.is_none()
            || route_mode == MailDomainRouteMode::DirectMx && smart_host_ref.is_some()
            || self.version == i64::MAX
        {
            return Err(DomainError::InvalidMailDomainRoute);
        }
        self.version += 1;
        self.inbound_enabled = inbound_enabled;
        self.outbound_enabled = outbound_enabled;
        self.smart_host_ref = smart_host_ref;
        self.dkim_key_ref = dkim_key_ref;
        self.updated_at = now;
        Ok(())
    }
}

fn normalized_reference(value: Option<String>) -> Result<Option<String>, DomainError> {
    value
        .map(|value| {
            let value = value.trim().to_owned();
            if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                Err(DomainError::InvalidMailDomainRoute)
            } else {
                Ok(value)
            }
        })
        .transpose()
}

/// Immutable result of one bounded DNS verification run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsVerificationEvidence {
    pub id: DnsVerificationId,
    pub ownership_challenge_sha256: [u8; 32],
    pub observed_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub ownership_txt: bool,
    pub mx_present: bool,
    pub mx_hosts_public: bool,
    pub spf_present: bool,
    pub dkim_present: bool,
    pub dmarc_present: bool,
}

impl DnsVerificationEvidence {
    pub fn new(
        ownership_challenge_sha256: [u8; 32],
        observed_at: DateTime<Utc>,
        valid_until: DateTime<Utc>,
        checks: DnsVerificationChecks,
    ) -> Result<Self, DomainError> {
        let evidence = Self {
            id: DnsVerificationId::new(),
            ownership_challenge_sha256,
            observed_at,
            valid_until,
            ownership_txt: checks.ownership_txt,
            mx_present: checks.mx_present,
            mx_hosts_public: checks.mx_hosts_public,
            spf_present: checks.spf_present,
            dkim_present: checks.dkim_present,
            dmarc_present: checks.dmarc_present,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.ownership_challenge_sha256 == [0; 32]
            || self.valid_until <= self.observed_at
            || self.valid_until > self.observed_at + chrono::Duration::hours(24)
        {
            return Err(DomainError::InvalidDnsVerificationEvidence);
        }
        Ok(())
    }

    #[must_use]
    pub fn activation_ready(&self, now: DateTime<Utc>) -> bool {
        now >= self.observed_at
            && now < self.valid_until
            && self.ownership_txt
            && self.mx_present
            && self.mx_hosts_public
            && self.spf_present
            && self.dkim_present
            && self.dmarc_present
    }
}

/// Boolean checks produced by the DNS adapter after recording detailed answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnsVerificationChecks {
    pub ownership_txt: bool,
    pub mx_present: bool,
    pub mx_hosts_public: bool,
    pub spf_present: bool,
    pub dkim_present: bool,
    pub dmarc_present: bool,
}

/// Tenant-owned mail domain aggregate. Network observation never activates it
/// directly: verification and activation remain separate auditable actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailDomain {
    pub id: MailDomainId,
    pub organization_id: OrganizationId,
    pub name: MailDomainName,
    pub status: MailDomainStatus,
    pub route_mode: MailDomainRouteMode,
    pub ownership_challenge_sha256: [u8; 32],
    /// Monotonically increasing public version of the secret ownership challenge.
    /// The digest itself must never be exposed through read models or audit events.
    pub ownership_challenge_version: i64,
    pub created_at: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub activated_at: Option<DateTime<Utc>>,
    pub suspended_at: Option<DateTime<Utc>>,
}

impl MailDomain {
    pub fn new(
        organization_id: OrganizationId,
        name: MailDomainName,
        route_mode: MailDomainRouteMode,
        ownership_challenge_sha256: [u8; 32],
        now: DateTime<Utc>,
    ) -> Result<Self, DomainError> {
        if ownership_challenge_sha256 == [0; 32] {
            return Err(DomainError::InvalidDnsVerificationEvidence);
        }
        Ok(Self {
            id: MailDomainId::new(),
            organization_id,
            name,
            status: MailDomainStatus::Draft,
            route_mode,
            ownership_challenge_sha256,
            ownership_challenge_version: 1,
            created_at: now,
            verified_at: None,
            activated_at: None,
            suspended_at: None,
        })
    }

    pub fn begin_dns_verification(&mut self) -> Result<(), DomainError> {
        if !matches!(
            self.status,
            MailDomainStatus::Draft | MailDomainStatus::Suspended
        ) {
            return Err(DomainError::InvalidMailDomainTransition);
        }
        self.status = MailDomainStatus::PendingDns;
        self.verified_at = None;
        Ok(())
    }

    /// Replaces the ownership proof while invalidating every observation bound to
    /// the previous digest. Active domains must first be explicitly suspended so
    /// challenge rotation cannot silently transfer control of live mail routing.
    pub fn rotate_ownership_challenge(
        &mut self,
        ownership_challenge_sha256: [u8; 32],
    ) -> Result<(), DomainError> {
        if ownership_challenge_sha256 == [0; 32]
            || ownership_challenge_sha256 == self.ownership_challenge_sha256
            || self.ownership_challenge_version == i64::MAX
            || matches!(
                self.status,
                MailDomainStatus::Active | MailDomainStatus::Retired
            )
        {
            return Err(DomainError::InvalidMailDomainTransition);
        }
        self.ownership_challenge_sha256 = ownership_challenge_sha256;
        self.ownership_challenge_version += 1;
        self.status = MailDomainStatus::PendingDns;
        self.verified_at = None;
        Ok(())
    }

    pub fn record_dns_evidence(
        &mut self,
        evidence: &DnsVerificationEvidence,
        now: DateTime<Utc>,
    ) -> Result<bool, DomainError> {
        if self.status != MailDomainStatus::PendingDns
            || self.ownership_challenge_sha256 != evidence.ownership_challenge_sha256
            || now < evidence.observed_at
            || now >= evidence.valid_until
        {
            return Err(DomainError::InvalidMailDomainTransition);
        }
        let ready = evidence.activation_ready(now);
        if ready {
            self.status = MailDomainStatus::Verified;
            self.verified_at = Some(evidence.observed_at);
        }
        Ok(ready)
    }

    pub fn activate(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        if self.status != MailDomainStatus::Verified
            || self.verified_at.is_none()
            || now < self.created_at
        {
            return Err(DomainError::InvalidMailDomainTransition);
        }
        self.status = MailDomainStatus::Active;
        self.activated_at = Some(now);
        self.suspended_at = None;
        Ok(())
    }

    pub fn suspend(&mut self, now: DateTime<Utc>) -> Result<(), DomainError> {
        if self.status != MailDomainStatus::Active || now < self.created_at {
            return Err(DomainError::InvalidMailDomainTransition);
        }
        self.status = MailDomainStatus::Suspended;
        self.suspended_at = Some(now);
        Ok(())
    }

    pub fn retire(&mut self) -> Result<(), DomainError> {
        if self.status == MailDomainStatus::Retired {
            return Err(DomainError::InvalidMailDomainTransition);
        }
        self.status = MailDomainStatus::Retired;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checks(ready: bool) -> DnsVerificationChecks {
        DnsVerificationChecks {
            ownership_txt: ready,
            mx_present: ready,
            mx_hosts_public: ready,
            spf_present: ready,
            dkim_present: ready,
            dmarc_present: ready,
        }
    }

    #[test]
    fn normalizes_idn_and_absolute_dns_name() {
        assert_eq!(
            MailDomainName::parse(" BÜCHER.example. ").unwrap().as_str(),
            "xn--bcher-kva.example"
        );
    }

    #[test]
    fn rejects_ip_single_label_and_unsafe_labels() {
        for value in [
            "127.0.0.1",
            "::1",
            "localhost",
            "-mail.example",
            "mail_.example",
        ] {
            assert!(MailDomainName::parse(value).is_err(), "{value}");
        }
    }

    #[test]
    fn rejects_injection_and_confusion_marks_without_echo() {
        let overlong = "a".repeat(254);
        for value in [
            "ex\r\nample.com",
            "../example.com",
            "example.com/../jmap",
            "\u{202e}example.com",
            &overlong,
            "';drop.example",
            "drop table.example",
        ] {
            let error = MailDomainName::parse(value).unwrap_err();
            assert_eq!(error, DomainError::InvalidMailDomainName);
            let rendered = format!("{error} {error:?}");
            assert_eq!(
                rendered,
                "invalid managed mail domain name InvalidMailDomainName"
            );
            assert!(!rendered.contains(value), "{value:?}");
            assert!(!rendered.contains("DROP"));
            assert!(!rendered.contains('\u{202e}'));
        }
        for value in ["drop.example", "email.example"] {
            let parsed = MailDomainName::parse(value).unwrap();
            assert_eq!(parsed.as_str(), value);
            assert!(!parsed.as_str().contains('\u{202e}'));
        }
    }

    #[test]
    fn activation_requires_fresh_complete_evidence_and_explicit_action() {
        let now = Utc::now();
        let mut domain = MailDomain::new(
            OrganizationId::new(),
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        let evidence = DnsVerificationEvidence::new(
            [7; 32],
            now,
            now + chrono::Duration::hours(1),
            checks(true),
        )
        .unwrap();
        assert!(domain.record_dns_evidence(&evidence, now).unwrap());
        assert_eq!(domain.status, MailDomainStatus::Verified);
        domain.activate(now).unwrap();
        assert_eq!(domain.status, MailDomainStatus::Active);
    }

    #[test]
    fn suspension_requires_reverification_before_resume() {
        let now = Utc::now();
        let mut domain = MailDomain::new(
            OrganizationId::new(),
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::SmartHost,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        let evidence = DnsVerificationEvidence::new(
            [7; 32],
            now,
            now + chrono::Duration::hours(1),
            checks(true),
        )
        .unwrap();
        domain.record_dns_evidence(&evidence, now).unwrap();
        domain.activate(now).unwrap();
        domain.suspend(now).unwrap();
        domain.begin_dns_verification().unwrap();
        assert_eq!(domain.status, MailDomainStatus::PendingDns);
        assert!(domain.activate(now).is_err());
    }

    #[test]
    fn incomplete_dns_observation_never_verifies() {
        let now = Utc::now();
        let mut domain = MailDomain::new(
            OrganizationId::new(),
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        let evidence = DnsVerificationEvidence::new(
            [7; 32],
            now,
            now + chrono::Duration::minutes(5),
            checks(false),
        )
        .unwrap();
        assert!(!domain.record_dns_evidence(&evidence, now).unwrap());
        assert_eq!(domain.status, MailDomainStatus::PendingDns);
    }

    #[test]
    fn evidence_from_an_old_ownership_challenge_is_rejected() {
        let now = Utc::now();
        let mut domain = MailDomain::new(
            OrganizationId::new(),
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        let evidence = DnsVerificationEvidence::new(
            [8; 32],
            now,
            now + chrono::Duration::minutes(5),
            checks(true),
        )
        .unwrap();
        assert_eq!(
            domain.record_dns_evidence(&evidence, now),
            Err(DomainError::InvalidMailDomainTransition)
        );
    }

    #[test]
    fn challenge_rotation_invalidates_old_evidence_and_increments_version() {
        let now = Utc::now();
        let mut domain = MailDomain::new(
            OrganizationId::new(),
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        domain
            .record_dns_evidence(
                &DnsVerificationEvidence::new(
                    [7; 32],
                    now,
                    now + chrono::Duration::minutes(5),
                    checks(true),
                )
                .unwrap(),
                now,
            )
            .unwrap();

        domain.rotate_ownership_challenge([8; 32]).unwrap();

        assert_eq!(domain.ownership_challenge_version, 2);
        assert_eq!(domain.status, MailDomainStatus::PendingDns);
        assert!(domain.verified_at.is_none());
        assert_eq!(
            domain.record_dns_evidence(
                &DnsVerificationEvidence::new(
                    [7; 32],
                    now,
                    now + chrono::Duration::minutes(5),
                    checks(true),
                )
                .unwrap(),
                now,
            ),
            Err(DomainError::InvalidMailDomainTransition)
        );
    }

    #[test]
    fn active_domain_challenge_cannot_be_rotated() {
        let now = Utc::now();
        let mut domain = MailDomain::new(
            OrganizationId::new(),
            MailDomainName::parse("example.test").unwrap(),
            MailDomainRouteMode::DirectMx,
            [7; 32],
            now,
        )
        .unwrap();
        domain.begin_dns_verification().unwrap();
        domain
            .record_dns_evidence(
                &DnsVerificationEvidence::new(
                    [7; 32],
                    now,
                    now + chrono::Duration::minutes(5),
                    checks(true),
                )
                .unwrap(),
                now,
            )
            .unwrap();
        domain.activate(now).unwrap();

        assert_eq!(
            domain.rotate_ownership_challenge([8; 32]),
            Err(DomainError::InvalidMailDomainTransition)
        );
        assert_eq!(domain.ownership_challenge_version, 1);
    }

    #[test]
    fn route_enablement_requires_active_domain_and_dkim() {
        let now = Utc::now();
        let mut route = MailDomainRoute::disabled(MailDomainId::new(), OrganizationId::new(), now);
        assert!(
            route
                .update(
                    MailDomainStatus::Verified,
                    MailDomainRouteMode::DirectMx,
                    true,
                    true,
                    None,
                    Some("kms://dkim/key".into()),
                    now,
                )
                .is_err()
        );
        assert!(
            route
                .update(
                    MailDomainStatus::Active,
                    MailDomainRouteMode::DirectMx,
                    true,
                    true,
                    None,
                    None,
                    now,
                )
                .is_err()
        );
        route
            .update(
                MailDomainStatus::Active,
                MailDomainRouteMode::DirectMx,
                true,
                true,
                None,
                Some("kms://dkim/key".into()),
                now,
            )
            .unwrap();
        assert_eq!(route.version, 2);
    }

    #[test]
    fn smart_host_and_direct_mx_references_cannot_be_confused() {
        let now = Utc::now();
        let mut route = MailDomainRoute::disabled(MailDomainId::new(), OrganizationId::new(), now);
        assert!(
            route
                .update(
                    MailDomainStatus::Active,
                    MailDomainRouteMode::SmartHost,
                    false,
                    true,
                    None,
                    Some("kms://dkim/key".into()),
                    now,
                )
                .is_err()
        );
        assert!(
            route
                .update(
                    MailDomainStatus::Active,
                    MailDomainRouteMode::DirectMx,
                    false,
                    false,
                    Some("relay-a".into()),
                    None,
                    now,
                )
                .is_err()
        );
    }
}
