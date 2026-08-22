//! S3-compatible Object Lock adapter for immutable audit evidence.

#![forbid(unsafe_code)]

use aws_config::{BehaviorVersion, Region};
use aws_sdk_s3::{
    Client,
    config::Builder as S3ConfigBuilder,
    primitives::{ByteStream, DateTime as AwsDateTime},
    types::{
        BucketVersioningStatus, ChecksumAlgorithm, ChecksumMode, ObjectLockEnabled,
        ObjectLockLegalHold, ObjectLockLegalHoldStatus, ObjectLockMode,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WormRetentionMode {
    Governance,
    Compliance,
}

impl WormRetentionMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Governance => "governance",
            Self::Compliance => "compliance",
        }
    }

    const fn as_aws(self) -> ObjectLockMode {
        match self {
            Self::Governance => ObjectLockMode::Governance,
            Self::Compliance => ObjectLockMode::Compliance,
        }
    }
}

#[derive(Clone, Debug)]
pub struct S3WormArchiveConfig {
    pub endpoint_url: Option<String>,
    pub region: String,
    pub bucket: String,
    pub force_path_style: bool,
    pub allow_insecure_http: bool,
}

#[derive(Clone, Debug)]
pub struct WormObjectSource {
    pub key: String,
    pub content_type: String,
    pub path: PathBuf,
    pub sha256_hex: String,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArchivedObjectEvidence {
    pub bucket: String,
    pub key: String,
    pub version_id: String,
    pub sha256_hex: String,
    pub size_bytes: u64,
    pub retention_mode: WormRetentionMode,
    pub retain_until: DateTime<Utc>,
}

/// One exact immutable object version retrieved and independently verified
/// against the evidence recorded when the archive was created.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetrievedObject {
    pub key: String,
    pub version_id: String,
    pub sha256_hex: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LegalHoldEvidence {
    pub bucket: String,
    pub key: String,
    pub version_id: String,
    pub enabled: bool,
    pub verified_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct S3WormArchive {
    client: Client,
    bucket: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WormArchiveError {
    #[error("S3 WORM archive configuration is invalid")]
    InvalidConfiguration,
    #[error("archive object source is invalid")]
    InvalidObject,
    #[error("archive bucket does not have enabled versioning and Object Lock")]
    BucketNotImmutable,
    #[error("archive object conflicts with existing content or metadata")]
    Conflict,
    #[error("archive service operation failed")]
    Remote,
    #[error("archive upload was not confirmed with exact retention and checksum")]
    Verification,
}

impl S3WormArchiveConfig {
    pub fn validate(&self) -> Result<(), WormArchiveError> {
        if !valid_region(&self.region) || !valid_bucket(&self.bucket) {
            return Err(WormArchiveError::InvalidConfiguration);
        }
        if let Some(endpoint) = &self.endpoint_url {
            let endpoint =
                Url::parse(endpoint).map_err(|_| WormArchiveError::InvalidConfiguration)?;
            let valid_scheme = endpoint.scheme() == "https"
                || (endpoint.scheme() == "http" && self.allow_insecure_http);
            if !valid_scheme
                || endpoint.host_str().is_none()
                || endpoint.username() != ""
                || endpoint.password().is_some()
                || endpoint.query().is_some()
                || endpoint.fragment().is_some()
            {
                return Err(WormArchiveError::InvalidConfiguration);
            }
        }
        Ok(())
    }
}

impl S3WormArchive {
    pub async fn connect(config: S3WormArchiveConfig) -> Result<Self, WormArchiveError> {
        config.validate()?;
        let mut loader = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(config.region.clone()));
        if let Some(endpoint) = &config.endpoint_url {
            loader = loader.endpoint_url(endpoint);
        }
        let shared = loader.load().await;
        let s3_config = S3ConfigBuilder::from(&shared)
            .force_path_style(config.force_path_style)
            .build();
        Ok(Self {
            client: Client::from_conf(s3_config),
            bucket: config.bucket,
        })
    }

    /// Fails closed unless the remote bucket confirms both required controls.
    pub async fn verify_bucket_is_immutable(&self) -> Result<(), WormArchiveError> {
        let versioning = self
            .client
            .get_bucket_versioning()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|_| WormArchiveError::Remote)?;
        if versioning.status() != Some(&BucketVersioningStatus::Enabled) {
            return Err(WormArchiveError::BucketNotImmutable);
        }
        let lock = self
            .client
            .get_object_lock_configuration()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|_| WormArchiveError::Remote)?;
        let enabled = lock
            .object_lock_configuration()
            .and_then(|configuration| configuration.object_lock_enabled());
        if enabled != Some(&ObjectLockEnabled::Enabled) {
            return Err(WormArchiveError::BucketNotImmutable);
        }
        Ok(())
    }

    /// Conditionally uploads one object and independently reads the exact
    /// version metadata back before returning WORM evidence.
    pub async fn put_immutable(
        &self,
        source: &WormObjectSource,
        retention_mode: WormRetentionMode,
        retain_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<ArchivedObjectEvidence, WormArchiveError> {
        validate_source(source, retain_until, now).await?;
        self.verify_bucket_is_immutable().await?;

        if let Some(existing) = self.head_current(source).await? {
            return verify_head(&self.bucket, source, retention_mode, retain_until, existing);
        }

        let checksum = sha256_base64(&source.sha256_hex)?;
        let body = ByteStream::from_path(&source.path)
            .await
            .map_err(|_| WormArchiveError::InvalidObject)?;
        let result = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&source.key)
            .body(body)
            .content_type(&source.content_type)
            .checksum_algorithm(ChecksumAlgorithm::Sha256)
            .checksum_sha256(&checksum)
            .set_metadata(Some(source.metadata.clone().into_iter().collect()))
            .object_lock_mode(retention_mode.as_aws())
            .object_lock_retain_until_date(AwsDateTime::from_millis(
                retain_until.timestamp_millis(),
            ))
            .if_none_match("*")
            .send()
            .await;

        let version_id = match result {
            Ok(result) => result
                .version_id()
                .map(str::to_owned)
                .ok_or(WormArchiveError::Verification)?,
            Err(_) => {
                let existing = self
                    .head_current(source)
                    .await?
                    .ok_or(WormArchiveError::Remote)?;
                return verify_head(&self.bucket, source, retention_mode, retain_until, existing);
            }
        };
        let head = self.head_version(source, &version_id).await?;
        verify_head(&self.bucket, source, retention_mode, retain_until, head)
    }

    /// Downloads one exact immutable object version and verifies its length
    /// and SHA-256 against the evidence recorded at archive time.
    ///
    /// The caller supplies the expected digest and size, so a substituted,
    /// truncated, or re-uploaded object can never be returned as evidence.
    pub async fn get_verified_version(
        &self,
        key: &str,
        version_id: &str,
        expected_sha256_hex: &str,
        expected_size_bytes: u64,
        max_size_bytes: u64,
    ) -> Result<RetrievedObject, WormArchiveError> {
        if !valid_object_key(key)
            || !valid_version_id(version_id)
            || !valid_sha256_hex(expected_sha256_hex)
            || expected_size_bytes == 0
            || expected_size_bytes > max_size_bytes
        {
            return Err(WormArchiveError::InvalidObject);
        }

        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .version_id(version_id)
            .checksum_mode(ChecksumMode::Enabled)
            .send()
            .await
            .map_err(|_| WormArchiveError::Remote)?;

        let remote_version = output
            .version_id()
            .filter(|value| !value.is_empty() && *value != "null")
            .ok_or(WormArchiveError::Verification)?
            .to_owned();
        let remote_size = output
            .content_length()
            .and_then(|value| u64::try_from(value).ok())
            .ok_or(WormArchiveError::Verification)?;
        let content_type = output.content_type().unwrap_or_default().to_owned();
        if remote_version != version_id || remote_size != expected_size_bytes {
            return Err(WormArchiveError::Verification);
        }

        let bytes = output
            .body
            .collect()
            .await
            .map_err(|_| WormArchiveError::Remote)?
            .to_vec();
        let actual_size = u64::try_from(bytes.len()).map_err(|_| WormArchiveError::Verification)?;
        let actual_sha256 = hex_lower(Sha256::digest(&bytes).as_slice());
        if actual_size != expected_size_bytes || actual_sha256 != expected_sha256_hex {
            return Err(WormArchiveError::Verification);
        }

        Ok(RetrievedObject {
            key: key.into(),
            version_id: remote_version,
            sha256_hex: actual_sha256,
            size_bytes: actual_size,
            content_type,
            bytes,
        })
    }

    /// Applies or releases Legal Hold on an exact immutable object version and
    /// independently reads the remote state back before returning evidence.
    pub async fn set_legal_hold(
        &self,
        key: &str,
        version_id: &str,
        enabled: bool,
    ) -> Result<LegalHoldEvidence, WormArchiveError> {
        if !valid_object_key(key) || !valid_version_id(version_id) {
            return Err(WormArchiveError::InvalidObject);
        }
        self.verify_bucket_is_immutable().await?;
        let desired = if enabled {
            ObjectLockLegalHoldStatus::On
        } else {
            ObjectLockLegalHoldStatus::Off
        };
        if self.get_legal_hold_status(key, version_id).await?.as_ref() == Some(&desired) {
            return Ok(LegalHoldEvidence {
                bucket: self.bucket.clone(),
                key: key.into(),
                version_id: version_id.into(),
                enabled,
                verified_at: Utc::now(),
            });
        }
        self.client
            .put_object_legal_hold()
            .bucket(&self.bucket)
            .key(key)
            .version_id(version_id)
            .legal_hold(
                ObjectLockLegalHold::builder()
                    .status(desired.clone())
                    .build(),
            )
            .send()
            .await
            .map_err(|_| WormArchiveError::Remote)?;
        if self.get_legal_hold_status(key, version_id).await?.as_ref() != Some(&desired) {
            return Err(WormArchiveError::Verification);
        }
        Ok(LegalHoldEvidence {
            bucket: self.bucket.clone(),
            key: key.into(),
            version_id: version_id.into(),
            enabled,
            verified_at: Utc::now(),
        })
    }

    async fn get_legal_hold_status(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<Option<ObjectLockLegalHoldStatus>, WormArchiveError> {
        self.client
            .get_object_legal_hold()
            .bucket(&self.bucket)
            .key(key)
            .version_id(version_id)
            .send()
            .await
            .map(|output| output.legal_hold().and_then(|hold| hold.status()).cloned())
            .map_err(|_| WormArchiveError::Remote)
    }

    async fn head_current(
        &self,
        source: &WormObjectSource,
    ) -> Result<Option<aws_sdk_s3::operation::head_object::HeadObjectOutput>, WormArchiveError>
    {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&source.key)
            .checksum_mode(ChecksumMode::Enabled)
            .send()
            .await
        {
            Ok(output) => Ok(Some(output)),
            Err(error)
                if error
                    .as_service_error()
                    .is_some_and(|service| service.is_not_found()) =>
            {
                Ok(None)
            }
            Err(_) => Err(WormArchiveError::Remote),
        }
    }

    async fn head_version(
        &self,
        source: &WormObjectSource,
        version_id: &str,
    ) -> Result<aws_sdk_s3::operation::head_object::HeadObjectOutput, WormArchiveError> {
        self.client
            .head_object()
            .bucket(&self.bucket)
            .key(&source.key)
            .version_id(version_id)
            .checksum_mode(ChecksumMode::Enabled)
            .send()
            .await
            .map_err(|_| WormArchiveError::Remote)
    }
}

fn verify_head(
    bucket: &str,
    source: &WormObjectSource,
    retention_mode: WormRetentionMode,
    minimum_retain_until: DateTime<Utc>,
    head: aws_sdk_s3::operation::head_object::HeadObjectOutput,
) -> Result<ArchivedObjectEvidence, WormArchiveError> {
    let expected_checksum = sha256_base64(&source.sha256_hex)?;
    let expected_size = std::fs::metadata(&source.path)
        .map_err(|_| WormArchiveError::InvalidObject)?
        .len();
    let version_id = head
        .version_id()
        .filter(|value| !value.is_empty() && *value != "null")
        .ok_or(WormArchiveError::Verification)?;
    let remote_size = head
        .content_length()
        .and_then(|value| u64::try_from(value).ok())
        .ok_or(WormArchiveError::Verification)?;
    let remote_retain_until = head
        .object_lock_retain_until_date()
        .and_then(|value| value.to_millis().ok())
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .ok_or(WormArchiveError::Verification)?;
    let metadata_matches = source
        .metadata
        .iter()
        .all(|(key, value)| head.metadata().and_then(|metadata| metadata.get(key)) == Some(value));
    if head.checksum_sha256() != Some(expected_checksum.as_str())
        || remote_size != expected_size
        || head.object_lock_mode() != Some(&retention_mode.as_aws())
        || remote_retain_until < minimum_retain_until
        || !metadata_matches
    {
        return Err(WormArchiveError::Conflict);
    }
    Ok(ArchivedObjectEvidence {
        bucket: bucket.into(),
        key: source.key.clone(),
        version_id: version_id.into(),
        sha256_hex: source.sha256_hex.clone(),
        size_bytes: expected_size,
        retention_mode,
        retain_until: remote_retain_until,
    })
}

async fn validate_source(
    source: &WormObjectSource,
    retain_until: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(), WormArchiveError> {
    let metadata = tokio::fs::metadata(&source.path)
        .await
        .map_err(|_| WormArchiveError::InvalidObject)?;
    if !metadata.is_file()
        || metadata.len() == 0
        || !valid_object_key(&source.key)
        || !valid_content_type(&source.content_type)
        || !valid_sha256_hex(&source.sha256_hex)
        || retain_until <= now
        || source.metadata.len() > 20
        || source
            .metadata
            .iter()
            .any(|(key, value)| !valid_metadata(key, value))
    {
        return Err(WormArchiveError::InvalidObject);
    }
    Ok(())
}

fn valid_region(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_bucket(value: &str) -> bool {
    (3..=63).contains(&value.len())
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_object_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1_024
        && value.trim() == value
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn valid_version_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1_024
        && value.trim() == value
        && !value.chars().any(char::is_control)
        && value != "null"
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble is a hex digit"));
        encoded.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("nibble is a hex digit"));
    }
    encoded
}

fn valid_content_type(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.trim() == value
        && value.is_ascii()
        && !value.chars().any(char::is_control)
}

fn valid_metadata(key: &str, value: &str) -> bool {
    !key.is_empty()
        && key.len() <= 100
        && key
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.is_empty()
        && value.len() <= 1_000
        && value.is_ascii()
        && !value.chars().any(char::is_control)
}

fn valid_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_base64(value: &str) -> Result<String, WormArchiveError> {
    if !valid_sha256_hex(value) {
        return Err(WormArchiveError::InvalidObject);
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_value(pair[0]).ok_or(WormArchiveError::InvalidObject)?;
            let low = hex_value(pair[1]).ok_or(WormArchiveError::InvalidObject)?;
            Ok((high << 4) | low)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(STANDARD.encode(bytes))
}

const fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_rejects_insecure_or_ambiguous_endpoints() {
        let valid = S3WormArchiveConfig {
            endpoint_url: Some("http://127.0.0.1:9000".into()),
            region: "us-east-1".into(),
            bucket: "copilotmail-audit".into(),
            force_path_style: true,
            allow_insecure_http: true,
        };
        assert!(valid.validate().is_ok());
        for endpoint in [
            "http://127.0.0.1:9000",
            "ftp://storage.example.com",
            "https://user:password@storage.example.com",
            "https://storage.example.com?secret=value",
        ] {
            let mut invalid = valid.clone();
            invalid.endpoint_url = Some(endpoint.into());
            invalid.allow_insecure_http = false;
            assert!(invalid.validate().is_err(), "{endpoint} must be rejected");
        }
    }

    #[test]
    fn object_names_metadata_and_checksums_are_bounded() {
        assert!(valid_object_key("audit/tenant/checkpoint/events.jsonl"));
        assert!(!valid_object_key("../events.jsonl"));
        assert!(!valid_object_key("audit//events.jsonl"));
        assert!(valid_metadata(
            "checkpoint-id",
            "018f0000-0000-7000-8000-000000000000"
        ));
        assert!(!valid_metadata("Checkpoint_ID", "value"));
        let digest = "00".repeat(32);
        assert_eq!(
            sha256_base64(&digest).unwrap(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );
        assert!(sha256_base64("not-a-digest").is_err());
    }

    #[test]
    fn retrieval_rejects_ambiguous_versions_and_unbounded_sizes() {
        assert!(valid_version_id(
            "3sL4kqtJlcpXroDTDmJ+rmSpXd3dIbrHY+MTRCxf3vjVBH40Nr8X8gdRQBpUMLUo"
        ));
        for invalid in ["", "null", " leading-space", "trailing-space "] {
            assert!(!valid_version_id(invalid), "{invalid} must be rejected");
        }
        assert!(!valid_version_id(&"v".repeat(1_025)));
        assert!(!valid_version_id("version\u{7f}id"));
    }

    #[test]
    fn hex_encoding_round_trips_through_the_base64_checksum_encoder() {
        let digest = Sha256::digest(b"immutable audit evidence");
        let hex = hex_lower(digest.as_slice());
        assert_eq!(hex.len(), 64);
        assert!(valid_sha256_hex(&hex));
        assert_eq!(sha256_base64(&hex).unwrap(), STANDARD.encode(digest));
    }
}
