//! Versioned audit checkpoint signatures and portable export verification.

#![forbid(unsafe_code)]

use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use copilotmail_domain::{AuditCheckpointId, OrganizationId};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey},
    signature::{SignatureEncoding, Signer, Verifier},
    traits::PublicKeyParts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write;
use uuid::Uuid;

pub const CHECKPOINT_FORMAT_VERSION: u16 = 1;
pub const EXPORT_FORMAT_VERSION: u16 = 1;
pub const AUDIT_HASH_VERSION: i16 = 1;
pub const SIGNATURE_ALGORITHM: &str = "RSASSA-PKCS1-v1_5-SHA256";
pub const EVENTS_FILE_NAME: &str = "events.jsonl";
const SHA256_HEX_LENGTH: usize = 64;

/// Verified database evidence used to build a signed checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditCheckpointCandidate {
    pub organization_id: OrganizationId,
    pub chain_sequence_end: i64,
    pub event_count: u64,
    pub first_event_id: Uuid,
    pub last_event_id: Uuid,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub root_hash: String,
    pub hash_version: i16,
}

/// Immutable, signed statement anchoring a prefix of one tenant audit chain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditCheckpointPayload {
    pub format_version: u16,
    pub checkpoint_id: AuditCheckpointId,
    pub organization_id: OrganizationId,
    pub chain_sequence_start: i64,
    pub chain_sequence_end: i64,
    pub event_count: u64,
    pub first_event_id: Uuid,
    pub last_event_id: Uuid,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub root_hash: String,
    pub hash_version: i16,
    pub key_id: String,
    pub public_key_sha256: String,
    pub created_at: DateTime<Utc>,
}

/// Payload plus detached signature, suitable for durable database storage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedAuditCheckpoint {
    pub payload: AuditCheckpointPayload,
    pub signature_algorithm: String,
    pub signature: String,
}

/// One portable audit-chain row. The signed checkpoint binds its final hash.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditExportRecord {
    pub event_id: Uuid,
    pub organization_id: OrganizationId,
    pub chain_sequence: i64,
    pub hash_version: i16,
    pub hash_payload: String,
    pub previous_hash: Option<String>,
    pub event_hash: String,
}

/// Portable export metadata. The event-file digest detects transport damage;
/// the signed chain root provides authenticity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditExportManifest {
    pub format_version: u16,
    pub checkpoint: SignedAuditCheckpoint,
    pub events_file: String,
    pub events_sha256: String,
    pub exported_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuditExportVerificationReport {
    pub checkpoint_id: AuditCheckpointId,
    pub organization_id: OrganizationId,
    pub event_count: u64,
    pub chain_head: String,
    pub key_id: String,
    pub public_key_sha256: String,
    pub checkpoint_created_at: DateTime<Utc>,
}

/// Stateful verifier so large JSONL exports can be checked with bounded memory.
pub struct AuditExportVerifier {
    checkpoint: AuditCheckpointPayload,
    expected_sequence: i64,
    previous_hash: Option<Vec<u8>>,
    event_count: u64,
    first_event_id: Option<Uuid>,
    last_event_id: Option<Uuid>,
}

/// Validated local RSA checkpoint signer.
pub struct RsaAuditCheckpointSigner {
    key_id: String,
    public_key_sha256: String,
    signing_key: SigningKey<Sha256>,
}

pub trait AuditCheckpointSigner: Send + Sync {
    fn key_id(&self) -> &str;
    fn public_key_sha256(&self) -> &str;
    fn sign_candidate(
        &self,
        candidate: &AuditCheckpointCandidate,
        checkpoint_id: AuditCheckpointId,
        created_at: DateTime<Utc>,
    ) -> Result<SignedAuditCheckpoint, AuditArtifactError>;
}

impl<T> AuditCheckpointSigner for &T
where
    T: AuditCheckpointSigner + ?Sized,
{
    fn key_id(&self) -> &str {
        (*self).key_id()
    }

    fn public_key_sha256(&self) -> &str {
        (*self).public_key_sha256()
    }

    fn sign_candidate(
        &self,
        candidate: &AuditCheckpointCandidate,
        checkpoint_id: AuditCheckpointId,
        created_at: DateTime<Utc>,
    ) -> Result<SignedAuditCheckpoint, AuditArtifactError> {
        (*self).sign_candidate(candidate, checkpoint_id, created_at)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuditArtifactError {
    #[error("audit checkpoint or export format is invalid")]
    InvalidFormat,
    #[error("audit checkpoint key is invalid or does not match")]
    InvalidKey,
    #[error("audit checkpoint signature is invalid")]
    InvalidSignature,
    #[error("audit export integrity violation at sequence {sequence}: {reason}")]
    Integrity { sequence: i64, reason: String },
    #[error("serialize audit artifact: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl AuditCheckpointPayload {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        checkpoint_id: AuditCheckpointId,
        organization_id: OrganizationId,
        chain_sequence_end: i64,
        first_event_id: Uuid,
        last_event_id: Uuid,
        period_start: DateTime<Utc>,
        period_end: DateTime<Utc>,
        root_hash: String,
        key_id: String,
        public_key_sha256: String,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AuditArtifactError> {
        let event_count =
            u64::try_from(chain_sequence_end).map_err(|_| AuditArtifactError::InvalidFormat)?;
        let payload = Self {
            format_version: CHECKPOINT_FORMAT_VERSION,
            checkpoint_id,
            organization_id,
            chain_sequence_start: 1,
            chain_sequence_end,
            event_count,
            first_event_id,
            last_event_id,
            period_start,
            period_end,
            root_hash,
            hash_version: AUDIT_HASH_VERSION,
            key_id,
            public_key_sha256,
            created_at,
        };
        payload.validate()?;
        Ok(payload)
    }

    pub fn validate(&self) -> Result<(), AuditArtifactError> {
        let expected_count = self
            .chain_sequence_end
            .checked_sub(self.chain_sequence_start)
            .and_then(|value| value.checked_add(1))
            .and_then(|value| u64::try_from(value).ok());
        if self.format_version != CHECKPOINT_FORMAT_VERSION
            || self.chain_sequence_start != 1
            || self.chain_sequence_end < self.chain_sequence_start
            || expected_count != Some(self.event_count)
            || self.event_count == 0
            || self.period_end < self.period_start
            || self.created_at < self.period_end
            || self.hash_version != AUDIT_HASH_VERSION
            || !valid_key_id(&self.key_id)
            || !valid_sha256_hex(&self.root_hash)
            || !valid_sha256_hex(&self.public_key_sha256)
        {
            return Err(AuditArtifactError::InvalidFormat);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AuditArtifactError> {
        self.validate()?;
        Ok(serde_json::to_vec(self)?)
    }
}

impl SignedAuditCheckpoint {
    pub fn from_storage(
        signed_payload: &str,
        signature_algorithm: String,
        signature: &[u8],
    ) -> Result<Self, AuditArtifactError> {
        let payload: AuditCheckpointPayload = serde_json::from_str(signed_payload)?;
        if payload.canonical_bytes()? != signed_payload.as_bytes() {
            return Err(AuditArtifactError::InvalidFormat);
        }
        let checkpoint = Self {
            payload,
            signature_algorithm,
            signature: STANDARD.encode(signature),
        };
        if checkpoint.signature_algorithm != SIGNATURE_ALGORITHM || signature.is_empty() {
            return Err(AuditArtifactError::InvalidFormat);
        }
        Ok(checkpoint)
    }

    pub fn signature_bytes(&self) -> Result<Vec<u8>, AuditArtifactError> {
        STANDARD
            .decode(&self.signature)
            .map_err(|_| AuditArtifactError::InvalidSignature)
    }

    pub fn verify(&self, public_key_pem: &[u8]) -> Result<(), AuditArtifactError> {
        self.payload.validate()?;
        if self.signature_algorithm != SIGNATURE_ALGORITHM {
            return Err(AuditArtifactError::InvalidFormat);
        }
        let public_key = parse_public_key(public_key_pem)?;
        if rsa_public_key_fingerprint(&public_key)? != self.payload.public_key_sha256 {
            return Err(AuditArtifactError::InvalidKey);
        }
        let signature = STANDARD
            .decode(&self.signature)
            .map_err(|_| AuditArtifactError::InvalidSignature)?;
        let signature = Signature::try_from(signature.as_slice())
            .map_err(|_| AuditArtifactError::InvalidSignature)?;
        VerifyingKey::<Sha256>::new(public_key)
            .verify(&self.payload.canonical_bytes()?, &signature)
            .map_err(|_| AuditArtifactError::InvalidSignature)
    }
}

impl AuditExportRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn from_hash_bytes(
        event_id: Uuid,
        organization_id: OrganizationId,
        chain_sequence: i64,
        hash_version: i16,
        hash_payload: String,
        previous_hash: Option<&[u8]>,
        event_hash: &[u8],
    ) -> Result<Self, AuditArtifactError> {
        if chain_sequence <= 0
            || hash_version != AUDIT_HASH_VERSION
            || event_hash.len() != 32
            || previous_hash.is_some_and(|value| value.len() != 32)
        {
            return Err(AuditArtifactError::InvalidFormat);
        }
        Ok(Self {
            event_id,
            organization_id,
            chain_sequence,
            hash_version,
            hash_payload,
            previous_hash: previous_hash.map(encode_hex),
            event_hash: encode_hex(event_hash),
        })
    }
}

impl RsaAuditCheckpointSigner {
    pub fn new(
        key_id: String,
        private_key_pem: &[u8],
        public_key_pem: &[u8],
    ) -> Result<Self, AuditArtifactError> {
        if !valid_key_id(&key_id) {
            return Err(AuditArtifactError::InvalidKey);
        }
        let private_key = RsaPrivateKey::from_pkcs8_pem(
            std::str::from_utf8(private_key_pem).map_err(|_| AuditArtifactError::InvalidKey)?,
        )
        .map_err(|_| AuditArtifactError::InvalidKey)?;
        let public_key = parse_public_key(public_key_pem)?;
        if private_key.n() != public_key.n() || private_key.e() != public_key.e() {
            return Err(AuditArtifactError::InvalidKey);
        }
        let public_key_sha256 = rsa_public_key_fingerprint(&public_key)?;
        Ok(Self {
            key_id,
            public_key_sha256,
            signing_key: SigningKey::<Sha256>::new(private_key),
        })
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    #[must_use]
    pub fn public_key_sha256(&self) -> &str {
        &self.public_key_sha256
    }

    pub fn sign(
        &self,
        payload: AuditCheckpointPayload,
    ) -> Result<SignedAuditCheckpoint, AuditArtifactError> {
        if payload.key_id != self.key_id || payload.public_key_sha256 != self.public_key_sha256 {
            return Err(AuditArtifactError::InvalidKey);
        }
        let signature = self.signing_key.sign(&payload.canonical_bytes()?);
        Ok(SignedAuditCheckpoint {
            payload,
            signature_algorithm: SIGNATURE_ALGORITHM.into(),
            signature: STANDARD.encode(signature.to_bytes()),
        })
    }

    pub fn sign_candidate(
        &self,
        candidate: &AuditCheckpointCandidate,
        checkpoint_id: AuditCheckpointId,
        created_at: DateTime<Utc>,
    ) -> Result<SignedAuditCheckpoint, AuditArtifactError> {
        if candidate.event_count
            != u64::try_from(candidate.chain_sequence_end)
                .map_err(|_| AuditArtifactError::InvalidFormat)?
            || candidate.hash_version != AUDIT_HASH_VERSION
        {
            return Err(AuditArtifactError::InvalidFormat);
        }
        self.sign(AuditCheckpointPayload::new(
            checkpoint_id,
            candidate.organization_id,
            candidate.chain_sequence_end,
            candidate.first_event_id,
            candidate.last_event_id,
            candidate.period_start,
            candidate.period_end,
            candidate.root_hash.clone(),
            self.key_id.clone(),
            self.public_key_sha256.clone(),
            created_at,
        )?)
    }
}

impl AuditCheckpointSigner for RsaAuditCheckpointSigner {
    fn key_id(&self) -> &str {
        RsaAuditCheckpointSigner::key_id(self)
    }

    fn public_key_sha256(&self) -> &str {
        RsaAuditCheckpointSigner::public_key_sha256(self)
    }

    fn sign_candidate(
        &self,
        candidate: &AuditCheckpointCandidate,
        checkpoint_id: AuditCheckpointId,
        created_at: DateTime<Utc>,
    ) -> Result<SignedAuditCheckpoint, AuditArtifactError> {
        RsaAuditCheckpointSigner::sign_candidate(self, candidate, checkpoint_id, created_at)
    }
}

impl AuditExportManifest {
    pub fn new(
        checkpoint: SignedAuditCheckpoint,
        events_sha256: String,
        exported_at: DateTime<Utc>,
    ) -> Result<Self, AuditArtifactError> {
        let manifest = Self {
            format_version: EXPORT_FORMAT_VERSION,
            checkpoint,
            events_file: EVENTS_FILE_NAME.into(),
            events_sha256,
            exported_at,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), AuditArtifactError> {
        if self.format_version != EXPORT_FORMAT_VERSION
            || self.events_file != EVENTS_FILE_NAME
            || !valid_sha256_hex(&self.events_sha256)
            || self.exported_at < self.checkpoint.payload.created_at
        {
            return Err(AuditArtifactError::InvalidFormat);
        }
        self.checkpoint.payload.validate()
    }
}

impl AuditExportVerifier {
    pub fn new(
        manifest: &AuditExportManifest,
        public_key_pem: &[u8],
    ) -> Result<Self, AuditArtifactError> {
        manifest.validate()?;
        manifest.checkpoint.verify(public_key_pem)?;
        Ok(Self {
            checkpoint: manifest.checkpoint.payload.clone(),
            expected_sequence: 1,
            previous_hash: None,
            event_count: 0,
            first_event_id: None,
            last_event_id: None,
        })
    }

    pub fn push(&mut self, record: &AuditExportRecord) -> Result<(), AuditArtifactError> {
        let violation = |reason: &str| AuditArtifactError::Integrity {
            sequence: record.chain_sequence,
            reason: reason.into(),
        };
        if record.organization_id != self.checkpoint.organization_id {
            return Err(violation("event belongs to another organization"));
        }
        if record.chain_sequence != self.expected_sequence {
            return Err(violation("chain sequence is missing or out of order"));
        }
        if record.hash_version != self.checkpoint.hash_version {
            return Err(violation("hash version does not match checkpoint"));
        }
        let previous_hash = record
            .previous_hash
            .as_deref()
            .map(decode_sha256_hex)
            .transpose()
            .map_err(|_| violation("previous hash is malformed"))?;
        if previous_hash != self.previous_hash {
            return Err(violation("previous hash does not match prior event"));
        }
        let event_hash = decode_sha256_hex(&record.event_hash)
            .map_err(|_| violation("event hash is malformed"))?;
        let calculated = calculate_event_hash(
            self.previous_hash.as_deref(),
            record.hash_payload.as_bytes(),
        );
        if event_hash != calculated {
            return Err(violation("event hash does not match hash payload"));
        }

        self.first_event_id.get_or_insert(record.event_id);
        self.last_event_id = Some(record.event_id);
        self.previous_hash = Some(event_hash);
        self.event_count = self
            .event_count
            .checked_add(1)
            .ok_or(AuditArtifactError::InvalidFormat)?;
        self.expected_sequence = self
            .expected_sequence
            .checked_add(1)
            .ok_or(AuditArtifactError::InvalidFormat)?;
        Ok(())
    }

    pub fn finish(
        self,
        manifest: &AuditExportManifest,
        actual_events_sha256: &str,
    ) -> Result<AuditExportVerificationReport, AuditArtifactError> {
        if !valid_sha256_hex(actual_events_sha256)
            || actual_events_sha256 != manifest.events_sha256
            || self.event_count != self.checkpoint.event_count
            || self.first_event_id != Some(self.checkpoint.first_event_id)
            || self.last_event_id != Some(self.checkpoint.last_event_id)
            || self.previous_hash.as_deref().map(encode_hex).as_deref()
                != Some(self.checkpoint.root_hash.as_str())
            || self.expected_sequence - 1 != self.checkpoint.chain_sequence_end
        {
            return Err(AuditArtifactError::Integrity {
                sequence: self.expected_sequence.saturating_sub(1),
                reason: "export does not match signed checkpoint or event-file digest".into(),
            });
        }
        Ok(AuditExportVerificationReport {
            checkpoint_id: self.checkpoint.checkpoint_id,
            organization_id: self.checkpoint.organization_id,
            event_count: self.event_count,
            chain_head: self.checkpoint.root_hash,
            key_id: self.checkpoint.key_id,
            public_key_sha256: self.checkpoint.public_key_sha256,
            checkpoint_created_at: self.checkpoint.created_at,
        })
    }
}

#[must_use]
pub fn sha256_hex(value: &[u8]) -> String {
    encode_hex(&Sha256::digest(value))
}

pub fn decode_sha256(value: &str) -> Result<Vec<u8>, AuditArtifactError> {
    decode_sha256_hex(value)
}

fn calculate_event_hash(previous_hash: Option<&[u8]>, payload: &[u8]) -> Vec<u8> {
    let mut digest = Sha256::new();
    if let Some(previous_hash) = previous_hash {
        digest.update(previous_hash);
    }
    digest.update(payload);
    digest.finalize().to_vec()
}

fn parse_public_key(public_key_pem: &[u8]) -> Result<RsaPublicKey, AuditArtifactError> {
    let pem = std::str::from_utf8(public_key_pem).map_err(|_| AuditArtifactError::InvalidKey)?;
    let key = RsaPublicKey::from_public_key_pem(pem).map_err(|_| AuditArtifactError::InvalidKey)?;
    if !(2_048..=8_192).contains(&key.n().bits()) || key.e().bits() > 32 {
        return Err(AuditArtifactError::InvalidKey);
    }
    Ok(key)
}

fn rsa_public_key_fingerprint(key: &RsaPublicKey) -> Result<String, AuditArtifactError> {
    let der = key
        .to_public_key_der()
        .map_err(|_| AuditArtifactError::InvalidKey)?;
    Ok(sha256_hex(der.as_ref()))
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn decode_sha256_hex(value: &str) -> Result<Vec<u8>, AuditArtifactError> {
    if !valid_sha256_hex(value) {
        return Err(AuditArtifactError::InvalidFormat);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_value(pair[0]).ok_or(AuditArtifactError::InvalidFormat)?;
            let low = hex_value(pair[1]).ok_or(AuditArtifactError::InvalidFormat)?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn encode_hex(value: &[u8]) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core_06::OsRng;
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};

    fn signer() -> (RsaAuditCheckpointSigner, Vec<u8>) {
        let private = RsaPrivateKey::new(&mut OsRng, 2_048).unwrap();
        let public = RsaPublicKey::from(&private);
        let private_pem = private.to_pkcs8_pem(LineEnding::LF).unwrap();
        let public_pem = public.to_public_key_pem(LineEnding::LF).unwrap();
        (
            RsaAuditCheckpointSigner::new(
                "audit-2026-01".into(),
                private_pem.as_bytes(),
                public_pem.as_bytes(),
            )
            .unwrap(),
            public_pem.into_bytes(),
        )
    }

    fn records(organization_id: OrganizationId) -> Vec<AuditExportRecord> {
        let mut prior: Option<Vec<u8>> = None;
        ["{\"event\":1}", "{\"event\":2}"]
            .into_iter()
            .enumerate()
            .map(|(index, payload)| {
                let hash = calculate_event_hash(prior.as_deref(), payload.as_bytes());
                let record = AuditExportRecord {
                    event_id: Uuid::now_v7(),
                    organization_id,
                    chain_sequence: i64::try_from(index + 1).unwrap(),
                    hash_version: AUDIT_HASH_VERSION,
                    hash_payload: payload.into(),
                    previous_hash: prior.as_deref().map(encode_hex),
                    event_hash: encode_hex(&hash),
                };
                prior = Some(hash);
                record
            })
            .collect()
    }

    #[test]
    fn signed_export_verifies_without_database() {
        let (signer, public_key) = signer();
        let organization_id = OrganizationId::new();
        let records = records(organization_id);
        let now = Utc::now();
        let payload = AuditCheckpointPayload::new(
            AuditCheckpointId::new(),
            organization_id,
            2,
            records[0].event_id,
            records[1].event_id,
            now,
            now,
            records[1].event_hash.clone(),
            signer.key_id().into(),
            signer.public_key_sha256().into(),
            now,
        )
        .unwrap();
        let checkpoint = signer.sign(payload).unwrap();
        let bytes = records
            .iter()
            .flat_map(|record| {
                let mut line = serde_json::to_vec(record).unwrap();
                line.push(b'\n');
                line
            })
            .collect::<Vec<_>>();
        let manifest = AuditExportManifest::new(checkpoint, sha256_hex(&bytes), now).unwrap();
        let mut verifier = AuditExportVerifier::new(&manifest, &public_key).unwrap();
        for record in &records {
            verifier.push(record).unwrap();
        }
        let report = verifier.finish(&manifest, &sha256_hex(&bytes)).unwrap();
        assert_eq!(report.event_count, 2);
        assert_eq!(report.organization_id, organization_id);
    }

    #[test]
    fn signature_and_chain_tampering_fail_closed() {
        let (signer, public_key) = signer();
        let organization_id = OrganizationId::new();
        let records = records(organization_id);
        let now = Utc::now();
        let payload = AuditCheckpointPayload::new(
            AuditCheckpointId::new(),
            organization_id,
            2,
            records[0].event_id,
            records[1].event_id,
            now,
            now,
            records[1].event_hash.clone(),
            signer.key_id().into(),
            signer.public_key_sha256().into(),
            now,
        )
        .unwrap();
        let mut checkpoint = signer.sign(payload).unwrap();
        checkpoint.payload.event_count = 1;
        assert!(checkpoint.verify(&public_key).is_err());

        checkpoint = signer
            .sign({
                let mut payload = checkpoint.payload;
                payload.event_count = 2;
                payload
            })
            .unwrap();
        let manifest = AuditExportManifest::new(checkpoint, sha256_hex(b"events"), now).unwrap();
        let mut verifier = AuditExportVerifier::new(&manifest, &public_key).unwrap();
        let mut tampered = records[0].clone();
        tampered.hash_payload.push(' ');
        assert!(verifier.push(&tampered).is_err());
    }

    #[test]
    fn signer_port_signs_a_verified_candidate() {
        let (signer, public_key) = signer();
        let organization_id = OrganizationId::new();
        let records = records(organization_id);
        let now = Utc::now();
        let candidate = AuditCheckpointCandidate {
            organization_id,
            chain_sequence_end: 2,
            event_count: 2,
            first_event_id: records[0].event_id,
            last_event_id: records[1].event_id,
            period_start: now,
            period_end: now,
            root_hash: records[1].event_hash.clone(),
            hash_version: AUDIT_HASH_VERSION,
        };

        let checkpoint = AuditCheckpointSigner::sign_candidate(
            &signer,
            &candidate,
            AuditCheckpointId::new(),
            now,
        )
        .unwrap();

        checkpoint.verify(&public_key).unwrap();
    }
}
