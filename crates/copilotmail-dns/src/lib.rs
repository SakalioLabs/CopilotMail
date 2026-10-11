//! Bounded DNS verification adapter for managed mail domains.
//!
//! DNS answers are untrusted network input. This crate only produces immutable
//! candidate evidence; it cannot activate a domain or enable a mail route.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use copilotmail_crypto::RsaPublicKey;
use copilotmail_domain::{
    DnsVerificationChecks, DnsVerificationEvidence, DomainError, MailDomainName,
};
use hickory_resolver::{TokioResolver, config::ResolveHosts, lookup::Lookup, proto::rr::RData};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    cmp::min,
    collections::BTreeSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::{Duration, Instant},
};

const MAX_RECORDS_PER_LOOKUP: usize = 64;
const MAX_MX_HOSTS: usize = 16;
const MAX_TXT_BYTES: usize = 16 * 1024;
const NEGATIVE_TTL: Duration = Duration::from_secs(60);
const MAX_EVIDENCE_TTL: Duration = Duration::from_secs(15 * 60);

/// One DNS answer set with a bounded cache deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsRecordSet<T> {
    pub values: Vec<T>,
    pub valid_until: DateTime<Utc>,
}

/// Normalized MX answer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct MxRecord {
    pub preference: u16,
    pub exchange: String,
}

/// Replaceable DNS query port used by the verifier.
#[async_trait]
pub trait DnsLookup: Send + Sync {
    async fn txt(&self, name: &str) -> Result<DnsRecordSet<String>, DnsError>;
    async fn mx(&self, name: &str) -> Result<DnsRecordSet<MxRecord>, DnsError>;
    async fn ip(&self, name: &str) -> Result<DnsRecordSet<IpAddr>, DnsError>;
}

/// Sanitized persistence payload plus strongly typed activation checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DnsVerificationReport {
    pub evidence: DnsVerificationEvidence,
    pub mx_records: Vec<MxRecord>,
    pub mx_addresses: Vec<IpAddr>,
    pub failure_codes: Vec<String>,
    pub raw_answers: serde_json::Value,
}

/// Configuration that is independent of a concrete recursive resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsVerifierConfig {
    pub dkim_selector: String,
    pub timeout: Duration,
}

impl DnsVerifierConfig {
    pub fn new(dkim_selector: impl Into<String>, timeout: Duration) -> Result<Self, DnsError> {
        let dkim_selector = dkim_selector.into().to_ascii_lowercase();
        if dkim_selector.is_empty()
            || dkim_selector.len() > 63
            || dkim_selector.starts_with('-')
            || dkim_selector.ends_with('-')
            || !dkim_selector
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !(Duration::from_secs(1)..=Duration::from_secs(30)).contains(&timeout)
        {
            return Err(DnsError::InvalidConfiguration);
        }
        Ok(Self {
            dkim_selector,
            timeout,
        })
    }
}

/// Orchestrates bounded queries and evaluates mail-specific DNS policy.
pub struct DnsVerifier<L> {
    lookup: L,
    config: DnsVerifierConfig,
}

impl<L> DnsVerifier<L>
where
    L: DnsLookup,
{
    #[must_use]
    pub const fn new(lookup: L, config: DnsVerifierConfig) -> Self {
        Self { lookup, config }
    }

    pub async fn verify(
        &self,
        domain: &MailDomainName,
        ownership_token: &str,
        now: DateTime<Utc>,
    ) -> Result<DnsVerificationReport, DnsError> {
        validate_ownership_token(ownership_token)?;
        tokio::time::timeout(
            self.config.timeout,
            self.verify_inner(domain, ownership_token, now),
        )
        .await
        .map_err(|_| DnsError::Timeout)?
    }

    async fn verify_inner(
        &self,
        domain: &MailDomainName,
        ownership_token: &str,
        now: DateTime<Utc>,
    ) -> Result<DnsVerificationReport, DnsError> {
        let domain_fqdn = format!("{}.", domain.as_str());
        let ownership_name = format!("_copilotmail-verification.{domain_fqdn}");
        let dkim_name = format!("{}._domainkey.{domain_fqdn}", self.config.dkim_selector);
        let dmarc_name = format!("_dmarc.{domain_fqdn}");
        let (ownership, root_txt, dkim, dmarc, mx) = tokio::try_join!(
            self.lookup.txt(&ownership_name),
            self.lookup.txt(&domain_fqdn),
            self.lookup.txt(&dkim_name),
            self.lookup.txt(&dmarc_name),
            self.lookup.mx(&domain_fqdn),
        )?;
        enforce_record_limit(&ownership)?;
        enforce_record_limit(&root_txt)?;
        enforce_record_limit(&dkim)?;
        enforce_record_limit(&dmarc)?;
        enforce_record_limit(&mx)?;
        if mx.values.len() > MAX_MX_HOSTS {
            return Err(DnsError::RecordLimitExceeded);
        }
        enforce_txt_size(&ownership)?;
        enforce_txt_size(&root_txt)?;
        enforce_txt_size(&dkim)?;
        enforce_txt_size(&dmarc)?;

        let ownership_txt = ownership
            .values
            .iter()
            .any(|value| value.trim() == ownership_token);
        let spf_present = exactly_one_prefixed(&root_txt.values, "v=spf1");
        let dkim_present = exactly_one_valid_dkim(&dkim.values);
        let dmarc_present = exactly_one_valid_dmarc(&dmarc.values);
        let mx_records = normalized_mx(mx.values)?;
        let mx_present = !mx_records.is_empty();

        let mut mx_addresses = BTreeSet::new();
        let mut mx_hosts_public = mx_present;
        let mut valid_until = min_deadline(
            now,
            [
                ownership.valid_until,
                root_txt.valid_until,
                dkim.valid_until,
                dmarc.valid_until,
                mx.valid_until,
            ],
        );
        for record in &mx_records {
            let addresses = self.lookup.ip(&format!("{}.", record.exchange)).await?;
            enforce_record_limit(&addresses)?;
            valid_until = min(valid_until, addresses.valid_until);
            if addresses.values.is_empty()
                || addresses
                    .values
                    .iter()
                    .any(|address| !is_public_mail_address(*address))
            {
                mx_hosts_public = false;
            }
            mx_addresses.extend(addresses.values);
        }

        let valid_until = min(
            valid_until,
            now + chrono::Duration::from_std(MAX_EVIDENCE_TTL).expect("constant duration"),
        );
        if valid_until <= now {
            return Err(DnsError::ExpiredAnswer);
        }
        let checks = DnsVerificationChecks {
            ownership_txt,
            mx_present,
            mx_hosts_public,
            spf_present,
            dkim_present,
            dmarc_present,
        };
        let challenge_digest: [u8; 32] = Sha256::digest(ownership_token.as_bytes()).into();
        let evidence = DnsVerificationEvidence::new(challenge_digest, now, valid_until, checks)
            .map_err(DnsError::InvalidEvidence)?;
        let failure_codes = failure_codes(checks);
        let mx_addresses: Vec<_> = mx_addresses.into_iter().collect();
        let raw_answers = serde_json::json!({
            "ownership_match_count": ownership.values.iter()
                .filter(|value| value.trim() == ownership_token)
                .count(),
            "spf_records": redact_policy_records(&root_txt.values, "v=spf1"),
            "dkim_selector": self.config.dkim_selector,
            "dkim_record_count": dkim.values.len(),
            "dmarc_records": redact_policy_records(&dmarc.values, "v=dmarc1"),
            "mx": mx_records,
            "mx_addresses": mx_addresses,
        });
        Ok(DnsVerificationReport {
            evidence,
            mx_records,
            mx_addresses,
            failure_codes,
            raw_answers,
        })
    }
}

/// System-configured Hickory resolver with host-file lookup disabled.
#[derive(Clone)]
pub struct HickoryDnsLookup {
    resolver: TokioResolver,
}

impl HickoryDnsLookup {
    pub fn from_system() -> Result<Self, DnsError> {
        let mut builder =
            TokioResolver::builder_tokio().map_err(|_| DnsError::ResolverInitialization)?;
        let options = builder.options_mut();
        options.timeout = Duration::from_secs(3);
        options.attempts = 2;
        options.cache_size = 128;
        options.use_hosts_file = ResolveHosts::Never;
        Ok(Self {
            resolver: builder
                .build()
                .map_err(|_| DnsError::ResolverInitialization)?,
        })
    }
}

#[async_trait]
impl DnsLookup for HickoryDnsLookup {
    async fn txt(&self, name: &str) -> Result<DnsRecordSet<String>, DnsError> {
        match self.resolver.txt_lookup(name).await {
            Ok(lookup) => {
                let valid_until = instant_deadline(lookup.valid_until());
                let values = hickory_txt_values(&lookup)?;
                Ok(DnsRecordSet {
                    values,
                    valid_until,
                })
            }
            Err(error) if error.is_no_records_found() => Ok(negative_record_set()),
            Err(_) => Err(DnsError::Unavailable),
        }
    }

    async fn mx(&self, name: &str) -> Result<DnsRecordSet<MxRecord>, DnsError> {
        match self.resolver.mx_lookup(name).await {
            Ok(lookup) => Ok(DnsRecordSet {
                valid_until: instant_deadline(lookup.valid_until()),
                values: hickory_mx_values(&lookup),
            }),
            Err(error) if error.is_no_records_found() => Ok(negative_record_set()),
            Err(_) => Err(DnsError::Unavailable),
        }
    }

    async fn ip(&self, name: &str) -> Result<DnsRecordSet<IpAddr>, DnsError> {
        match self.resolver.lookup_ip(name).await {
            Ok(lookup) => Ok(DnsRecordSet {
                valid_until: instant_deadline(lookup.valid_until()),
                values: lookup.iter().collect(),
            }),
            Err(error) if error.is_no_records_found() => Ok(negative_record_set()),
            Err(_) => Err(DnsError::Unavailable),
        }
    }
}

fn validate_ownership_token(value: &str) -> Result<(), DnsError> {
    if value.len() < 32
        || value.len() > 512
        || value.chars().any(char::is_whitespace)
        || !value.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(DnsError::InvalidOwnershipToken);
    }
    Ok(())
}

fn normalized_mx(records: Vec<MxRecord>) -> Result<Vec<MxRecord>, DnsError> {
    if records.len() == 1 && records[0].preference == 0 && records[0].exchange.trim() == "." {
        return Ok(Vec::new());
    }
    let mut records: Vec<_> = records
        .into_iter()
        .map(|mut record| {
            record.exchange = record
                .exchange
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase();
            record
        })
        .collect();
    if records.iter().any(|record| {
        record.exchange.is_empty()
            || record.exchange.len() > 253
            || !record.exchange.contains('.')
            || record.exchange.parse::<IpAddr>().is_ok()
            || record.exchange.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
    }) {
        return Err(DnsError::InvalidAnswer);
    }
    records.sort();
    records.dedup();
    Ok(records)
}

fn exactly_one_prefixed(records: &[String], prefix: &str) -> bool {
    records
        .iter()
        .filter(|record| {
            let record = record.trim_start().to_ascii_lowercase();
            record == prefix
                || record
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with(char::is_whitespace))
        })
        .count()
        == 1
}

fn exactly_one_valid_dmarc(records: &[String]) -> bool {
    let matching: Vec<_> = records
        .iter()
        .filter(|record| record_version_matches(record, "v=dmarc1"))
        .collect();
    matching.len() == 1
        && tag_value(matching[0], "p").is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "none" | "quarantine" | "reject"
            )
        })
}

fn exactly_one_valid_dkim(records: &[String]) -> bool {
    let matching: Vec<_> = records
        .iter()
        .filter(|record| record_version_matches(record, "v=dkim1"))
        .collect();
    if matching.len() != 1 {
        return false;
    }
    let Some(public_key) = tag_value(matching[0], "p") else {
        return false;
    };
    let Ok(decoded) = STANDARD.decode(public_key.as_bytes()) else {
        return false;
    };
    RsaPublicKey::from_der(&decoded).is_ok_and(|key| key.permits_signatures())
}

fn tag_value(record: &str, name: &str) -> Option<String> {
    record.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn record_version_matches(record: &str, prefix: &str) -> bool {
    let record = record.trim_start().to_ascii_lowercase();
    record == prefix
        || record
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(char::is_whitespace) || rest.starts_with(';'))
}

fn redact_policy_records(records: &[String], prefix: &str) -> Vec<String> {
    records
        .iter()
        .filter(|record| record.trim_start().to_ascii_lowercase().starts_with(prefix))
        .map(|record| record.trim().to_owned())
        .collect()
}

fn failure_codes(checks: DnsVerificationChecks) -> Vec<String> {
    [
        (!checks.ownership_txt).then_some("ownership_txt_missing"),
        (!checks.mx_present).then_some("mx_missing"),
        (!checks.mx_hosts_public).then_some("mx_host_not_public"),
        (!checks.spf_present).then_some("spf_missing_or_ambiguous"),
        (!checks.dkim_present).then_some("dkim_missing_or_weak"),
        (!checks.dmarc_present).then_some("dmarc_missing_or_invalid"),
    ]
    .into_iter()
    .flatten()
    .map(str::to_owned)
    .collect()
}

fn enforce_record_limit<T>(records: &DnsRecordSet<T>) -> Result<(), DnsError> {
    if records.values.len() > MAX_RECORDS_PER_LOOKUP {
        Err(DnsError::RecordLimitExceeded)
    } else {
        Ok(())
    }
}

fn enforce_txt_size(records: &DnsRecordSet<String>) -> Result<(), DnsError> {
    if records.values.iter().map(String::len).sum::<usize>() > MAX_TXT_BYTES {
        Err(DnsError::RecordLimitExceeded)
    } else {
        Ok(())
    }
}

fn min_deadline<const N: usize>(
    now: DateTime<Utc>,
    deadlines: [DateTime<Utc>; N],
) -> DateTime<Utc> {
    deadlines.into_iter().fold(
        now + chrono::Duration::from_std(MAX_EVIDENCE_TTL).expect("constant duration"),
        min,
    )
}

// Hickory 0.26 returns a mixed answer section rather than typed lookup
// iterators. Keep record selection separate from network I/O and test it.
fn hickory_txt_values(lookup: &Lookup) -> Result<Vec<String>, DnsError> {
    lookup
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
            RData::TXT(txt) => Some(txt),
            _ => None,
        })
        .map(|record| {
            let bytes = record
                .txt_data
                .iter()
                .flat_map(|part| part.iter().copied())
                .collect();
            String::from_utf8(bytes).map_err(|_| DnsError::InvalidAnswer)
        })
        .collect()
}

fn hickory_mx_values(lookup: &Lookup) -> Vec<MxRecord> {
    lookup
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
            RData::MX(mx) => Some(MxRecord {
                preference: mx.preference,
                exchange: mx.exchange.to_utf8().to_ascii_lowercase(),
            }),
            _ => None,
        })
        .collect()
}

fn instant_deadline(deadline: Instant) -> DateTime<Utc> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    Utc::now()
        + chrono::Duration::from_std(min(remaining, MAX_EVIDENCE_TTL))
            .expect("bounded duration converts to chrono")
}

fn negative_record_set<T>() -> DnsRecordSet<T> {
    DnsRecordSet {
        values: Vec::new(),
        valid_until: Utc::now()
            + chrono::Duration::from_std(NEGATIVE_TTL).expect("constant duration"),
    }
}

/// Returns true only for addresses suitable as public MX delivery targets.
#[must_use]
pub fn is_public_mail_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => public_v4(address),
        IpAddr::V6(address) => public_v6(address),
    }
}

fn public_v4(address: Ipv4Addr) -> bool {
    let value = u32::from(address);
    ![
        (0x0000_0000, 8),
        (0x0a00_0000, 8),
        (0x6440_0000, 10),
        (0x7f00_0000, 8),
        (0xa9fe_0000, 16),
        (0xac10_0000, 12),
        (0xc000_0000, 24),
        (0xc000_0200, 24),
        (0xc058_6300, 24),
        (0xc0a8_0000, 16),
        (0xc612_0000, 15),
        (0xc633_6400, 24),
        (0xcb00_7100, 24),
        (0xe000_0000, 4),
        (0xf000_0000, 4),
    ]
    .into_iter()
    .any(|(network, prefix)| in_v4_prefix(value, network, prefix))
}

fn in_v4_prefix(value: u32, network: u32, prefix: u32) -> bool {
    let mask = u32::MAX << (32 - prefix);
    value & mask == network
}

fn public_v6(address: Ipv6Addr) -> bool {
    if address.to_ipv4().is_some() {
        return false;
    }
    let value = u128::from(address);
    if !in_v6_prefix(value, 0x2000_u128 << 112, 3) {
        return false;
    }
    ![
        (0x2001_0000_u128 << 96, 23), // IETF protocol assignments
        (0x2001_0db8_u128 << 96, 32), // documentation
        (0x2002_u128 << 112, 16),     // 6to4 transition
        (0x3fff_u128 << 112, 20),     // documentation
    ]
    .into_iter()
    .any(|(network, prefix)| in_v6_prefix(value, network, prefix))
}

fn in_v6_prefix(value: u128, network: u128, prefix: u32) -> bool {
    let mask = u128::MAX << (128 - prefix);
    value & mask == network
}

#[derive(Debug, thiserror::Error)]
pub enum DnsError {
    #[error("DNS verifier configuration is invalid")]
    InvalidConfiguration,
    #[error("ownership token is invalid")]
    InvalidOwnershipToken,
    #[error("system DNS resolver could not be initialized")]
    ResolverInitialization,
    #[error("DNS service is unavailable")]
    Unavailable,
    #[error("DNS verification timed out")]
    Timeout,
    #[error("DNS answer is invalid")]
    InvalidAnswer,
    #[error("DNS answer exceeded a safety limit")]
    RecordLimitExceeded,
    #[error("DNS answer expired before evidence could be created")]
    ExpiredAnswer,
    #[error("DNS evidence violates domain invariants: {0}")]
    InvalidEvidence(#[source] DomainError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hickory_mixed_answers_keep_txt_segments_and_mx_identity_separate() {
        use hickory_resolver::proto::{
            op::Query,
            rr::{
                Name, Record, RecordType,
                rdata::{MX, TXT},
            },
        };
        let name = Name::from_ascii("EXAMPLE.TEST.").unwrap();
        let lookup = Lookup::new_with_max_ttl(
            Query::query(name.clone(), RecordType::TXT),
            [
                Record::from_rdata(
                    name.clone(),
                    60,
                    RData::TXT(TXT::new(vec!["first".into(), "second".into()])),
                ),
                Record::from_rdata(name.clone(), 60, RData::MX(MX::new(10, name.clone()))),
            ],
        );
        assert_eq!(hickory_txt_values(&lookup).unwrap(), vec!["firstsecond"]);
        assert_eq!(
            hickory_mx_values(&lookup),
            vec![MxRecord {
                preference: 10,
                exchange: "example.test.".into()
            }]
        );
    }

    #[test]
    fn hickory_txt_rejects_invalid_utf8_instead_of_silently_changing_evidence() {
        use hickory_resolver::proto::{
            op::Query,
            rr::{Name, RecordType, rdata::TXT},
        };
        let lookup = Lookup::from_rdata(
            Query::query(Name::from_ascii("example.test.").unwrap(), RecordType::TXT),
            RData::TXT(TXT::from_bytes(vec![b"\xff"])),
        );
        assert!(matches!(
            hickory_txt_values(&lookup),
            Err(DnsError::InvalidAnswer)
        ));
        assert!(hickory_mx_values(&lookup).is_empty());
    }
    use copilotmail_crypto::RsaPrivateKey;
    use std::{collections::BTreeMap, str::FromStr};

    #[derive(Default)]
    struct MemoryLookup {
        txt: BTreeMap<String, DnsRecordSet<String>>,
        mx: BTreeMap<String, DnsRecordSet<MxRecord>>,
        ip: BTreeMap<String, DnsRecordSet<IpAddr>>,
    }

    #[async_trait]
    impl DnsLookup for MemoryLookup {
        async fn txt(&self, name: &str) -> Result<DnsRecordSet<String>, DnsError> {
            Ok(self.txt.get(name).cloned().unwrap_or_else(empty_record_set))
        }

        async fn mx(&self, name: &str) -> Result<DnsRecordSet<MxRecord>, DnsError> {
            Ok(self.mx.get(name).cloned().unwrap_or_else(empty_record_set))
        }

        async fn ip(&self, name: &str) -> Result<DnsRecordSet<IpAddr>, DnsError> {
            Ok(self.ip.get(name).cloned().unwrap_or_else(empty_record_set))
        }
    }

    struct SlowLookup;

    #[async_trait]
    impl DnsLookup for SlowLookup {
        async fn txt(&self, _: &str) -> Result<DnsRecordSet<String>, DnsError> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(empty_record_set())
        }

        async fn mx(&self, _: &str) -> Result<DnsRecordSet<MxRecord>, DnsError> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(empty_record_set())
        }

        async fn ip(&self, _: &str) -> Result<DnsRecordSet<IpAddr>, DnsError> {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(empty_record_set())
        }
    }

    fn empty_record_set<T>() -> DnsRecordSet<T> {
        records(Vec::new())
    }

    fn records<T>(values: Vec<T>) -> DnsRecordSet<T> {
        DnsRecordSet {
            values,
            valid_until: Utc::now() + chrono::Duration::minutes(5),
        }
    }

    fn dkim_record(bits: usize) -> String {
        let key = match bits {
            2048 => RsaPrivateKey::generate_for_testing().unwrap().public_key(),
            1024 => RsaPublicKey::from_spki_pem(include_bytes!(
                "../../copilotmail-crypto/tests/public-1024.pem"
            ))
            .unwrap(),
            _ => panic!("fixture only supports explicit tested strengths"),
        };
        let der = key.spki_der().unwrap();
        format!("v=DKIM1; k=rsa; p={}", STANDARD.encode(der.as_ref()))
    }

    fn ready_lookup(token: &str, mx_address: IpAddr) -> MemoryLookup {
        let mut lookup = MemoryLookup::default();
        lookup.txt.insert(
            "_copilotmail-verification.example.test.".into(),
            records(vec![token.into()]),
        );
        lookup.txt.insert(
            "example.test.".into(),
            records(vec!["v=spf1 mx -all".into()]),
        );
        lookup.txt.insert(
            "mail._domainkey.example.test.".into(),
            records(vec![dkim_record(2048)]),
        );
        lookup.txt.insert(
            "_dmarc.example.test.".into(),
            records(vec![
                "v=DMARC1; p=reject; rua=mailto:dmarc@example.test".into(),
            ]),
        );
        lookup.mx.insert(
            "example.test.".into(),
            records(vec![MxRecord {
                preference: 10,
                exchange: "mx.example.net.".into(),
            }]),
        );
        lookup
            .ip
            .insert("mx.example.net.".into(), records(vec![mx_address]));
        lookup
    }

    #[tokio::test]
    async fn complete_public_dns_produces_activation_ready_evidence() {
        let token = "copilotmail-verification-token-1234567890";
        let now = Utc::now();
        let verifier = DnsVerifier::new(
            ready_lookup(token, IpAddr::from_str("8.8.8.8").unwrap()),
            DnsVerifierConfig::new("mail", Duration::from_secs(2)).unwrap(),
        );
        let report = verifier
            .verify(&MailDomainName::parse("example.test").unwrap(), token, now)
            .await
            .unwrap();
        assert!(report.evidence.activation_ready(now));
        assert!(report.failure_codes.is_empty());
        assert_eq!(
            report.evidence.ownership_challenge_sha256,
            <[u8; 32]>::from(Sha256::digest(token.as_bytes()))
        );
        assert!(
            !serde_json::to_string(&report.raw_answers)
                .unwrap()
                .contains(token)
        );
    }

    #[tokio::test]
    async fn private_mx_address_is_evidence_but_never_activation_ready() {
        let token = "copilotmail-verification-token-1234567890";
        let now = Utc::now();
        let verifier = DnsVerifier::new(
            ready_lookup(token, IpAddr::from_str("10.1.2.3").unwrap()),
            DnsVerifierConfig::new("mail", Duration::from_secs(2)).unwrap(),
        );
        let report = verifier
            .verify(&MailDomainName::parse("example.test").unwrap(), token, now)
            .await
            .unwrap();
        assert!(!report.evidence.activation_ready(now));
        assert!(
            report
                .failure_codes
                .contains(&"mx_host_not_public".to_owned())
        );
    }

    #[tokio::test]
    async fn duplicate_spf_and_weak_dkim_are_rejected() {
        let token = "copilotmail-verification-token-1234567890";
        let now = Utc::now();
        let mut lookup = ready_lookup(token, IpAddr::from_str("8.8.8.8").unwrap());
        lookup.txt.insert(
            "example.test.".into(),
            records(vec!["v=spf1 -all".into(), "v=spf1 mx -all".into()]),
        );
        lookup.txt.insert(
            "mail._domainkey.example.test.".into(),
            records(vec![dkim_record(1024)]),
        );
        let report = DnsVerifier::new(
            lookup,
            DnsVerifierConfig::new("mail", Duration::from_secs(2)).unwrap(),
        )
        .verify(&MailDomainName::parse("example.test").unwrap(), token, now)
        .await
        .unwrap();
        assert!(!report.evidence.spf_present);
        assert!(!report.evidence.dkim_present);
    }

    #[test]
    fn rejects_non_public_ipv4_and_ipv6_ranges() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "::",
            "::1",
            "::ffff:8.8.8.8",
            "64:ff9b::10.0.0.1",
            "100::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "3fff::1",
            "5f00::1",
            "ff02::1",
        ] {
            let address = IpAddr::from_str(address).unwrap();
            assert!(!is_public_mail_address(address), "{address}");
        }
        assert!(is_public_mail_address(IpAddr::from_str("8.8.8.8").unwrap()));
        assert!(is_public_mail_address(
            IpAddr::from_str("2606:4700:4700::1111").unwrap()
        ));
    }

    #[tokio::test]
    async fn null_mx_is_treated_as_explicitly_not_accepting_mail() {
        let token = "copilotmail-verification-token-1234567890";
        let now = Utc::now();
        let mut lookup = ready_lookup(token, IpAddr::from_str("8.8.8.8").unwrap());
        lookup.mx.insert(
            "example.test.".into(),
            records(vec![MxRecord {
                preference: 0,
                exchange: ".".into(),
            }]),
        );
        let report = DnsVerifier::new(
            lookup,
            DnsVerifierConfig::new("mail", Duration::from_secs(2)).unwrap(),
        )
        .verify(&MailDomainName::parse("example.test").unwrap(), token, now)
        .await
        .unwrap();
        assert!(!report.evidence.mx_present);
        assert!(!report.evidence.mx_hosts_public);
    }

    #[tokio::test]
    async fn total_verification_timeout_is_enforced() {
        let verifier = DnsVerifier::new(
            SlowLookup,
            DnsVerifierConfig {
                dkim_selector: "mail".into(),
                timeout: Duration::from_millis(10),
            },
        );
        let result = verifier
            .verify(
                &MailDomainName::parse("example.test").unwrap(),
                "copilotmail-verification-token-1234567890",
                Utc::now(),
            )
            .await;
        assert!(matches!(result, Err(DnsError::Timeout)));
    }

    #[tokio::test]
    async fn excessive_answers_fail_instead_of_being_truncated() {
        let token = "copilotmail-verification-token-1234567890";
        let mut lookup = ready_lookup(token, IpAddr::from_str("8.8.8.8").unwrap());
        lookup.txt.insert(
            "_copilotmail-verification.example.test.".into(),
            records((0..=MAX_RECORDS_PER_LOOKUP).map(|_| token.into()).collect()),
        );
        let result = DnsVerifier::new(
            lookup,
            DnsVerifierConfig::new("mail", Duration::from_secs(2)).unwrap(),
        )
        .verify(
            &MailDomainName::parse("example.test").unwrap(),
            token,
            Utc::now(),
        )
        .await;
        assert!(matches!(result, Err(DnsError::RecordLimitExceeded)));
    }

    #[tokio::test]
    async fn expired_dns_deadline_cannot_create_evidence() {
        let token = "copilotmail-verification-token-1234567890";
        let now = Utc::now();
        let mut lookup = ready_lookup(token, IpAddr::from_str("8.8.8.8").unwrap());
        lookup.txt.get_mut("example.test.").unwrap().valid_until =
            now - chrono::Duration::seconds(1);
        let result = DnsVerifier::new(
            lookup,
            DnsVerifierConfig::new("mail", Duration::from_secs(2)).unwrap(),
        )
        .verify(&MailDomainName::parse("example.test").unwrap(), token, now)
        .await;
        assert!(matches!(result, Err(DnsError::ExpiredAnswer)));
    }
}
