//! Bounded RSA encoding and key validation shared by identity, audit and DNS.
//!
//! AWS-LC owns parsing, key validation and key generation. This crate does not
//! implement RSA arithmetic or ASN.1. Errors never contain supplied key bytes.
#![forbid(unsafe_code)]

use aws_lc_rs::{
    encoding::{AsDer, Pkcs8V1Der, PublicKeyX509Der},
    rsa::PublicKey as BackendPublicKey,
    signature::{KeyPair, RsaKeyPair},
};
use zeroize::Zeroizing;

const MAX_DER_BYTES: usize = 16 * 1024;
const MAX_PEM_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("RSA key encoding or policy is invalid")]
pub struct KeyError;

#[derive(Clone, Debug)]
pub struct RsaPublicKey(BackendPublicKey);

impl RsaPublicKey {
    /// Accepts the exact SubjectPublicKeyInfo PEM format already used by the
    /// public API. A PKCS#1 key hidden under a PUBLIC KEY label is rejected.
    pub fn from_spki_pem(pem: &[u8]) -> Result<Self, KeyError> {
        let der = decode_pem(pem, "PUBLIC KEY")?;
        let key = Self::from_der(&der)?;
        if key.spki_der()?.as_ref() != der.as_slice() {
            return Err(KeyError);
        }
        Ok(key)
    }

    /// DKIM also permits PKCS#1 DER. Canonical round-trip checks reject trailing
    /// bytes, algorithm substitutions and multiple concatenated objects.
    pub fn from_der(der: &[u8]) -> Result<Self, KeyError> {
        if der.is_empty() || der.len() > MAX_DER_BYTES {
            return Err(KeyError);
        }
        let key = Self(BackendPublicKey::from_der(der).map_err(|_| KeyError)?);
        if key.bits() > 8192 || key.exponent().len() > 4 {
            return Err(KeyError);
        }
        if key.pkcs1_der() != der && key.spki_der()?.as_ref() != der {
            return Err(KeyError);
        }
        Ok(key)
    }

    #[must_use]
    pub fn modulus(&self) -> &[u8] {
        self.0.modulus().big_endian_without_leading_zero()
    }
    #[must_use]
    pub fn exponent(&self) -> &[u8] {
        self.0.exponent().big_endian_without_leading_zero()
    }
    #[must_use]
    pub fn bits(&self) -> usize {
        let n = self.modulus();
        n.len() * 8 - n.first().map_or(0, |byte| byte.leading_zeros() as usize)
    }
    #[must_use]
    pub fn permits_signatures(&self) -> bool {
        (2048..=8192).contains(&self.bits())
    }
    #[must_use]
    pub fn pkcs1_der(&self) -> &[u8] {
        self.0.as_ref()
    }
    pub fn spki_der(&self) -> Result<PublicKeyX509Der<'static>, KeyError> {
        self.0.as_der().map_err(|_| KeyError)
    }
    pub fn spki_pem(&self) -> Result<String, KeyError> {
        pem_rfc7468::encode_string(
            "PUBLIC KEY",
            pem_rfc7468::LineEnding::LF,
            self.spki_der()?.as_ref(),
        )
        .map_err(|_| KeyError)
    }
}

/// No Debug/Display implementation: private bytes cannot accidentally enter
/// diagnostics. Secret decoding buffers are zeroized after backend import.
pub struct RsaPrivateKey(RsaKeyPair);

impl RsaPrivateKey {
    pub fn from_pkcs8_pem(pem: &[u8]) -> Result<Self, KeyError> {
        let der = decode_pem(pem, "PRIVATE KEY")?;
        let key = Self(RsaKeyPair::from_pkcs8(&der).map_err(|_| KeyError)?);
        if !key.public_key().permits_signatures() {
            return Err(KeyError);
        }
        Ok(key)
    }
    #[must_use]
    pub fn public_key(&self) -> RsaPublicKey {
        RsaPublicKey(self.0.public_key().clone())
    }
    #[must_use]
    pub fn matches(&self, public: &RsaPublicKey) -> bool {
        self.0.public_key().as_ref() == public.pkcs1_der()
    }
    pub fn pkcs8_der(&self) -> Result<Pkcs8V1Der<'static>, KeyError> {
        self.0.as_der().map_err(|_| KeyError)
    }

    /// Fixtures use the same maintained backend, not the vulnerable rsa crate.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn generate_for_testing() -> Result<Self, KeyError> {
        RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
            .map(Self)
            .map_err(|_| KeyError)
    }
    #[cfg(any(test, feature = "test-utils"))]
    pub fn pkcs8_pem_for_testing(&self) -> Result<Zeroizing<String>, KeyError> {
        pem_rfc7468::encode_string(
            "PRIVATE KEY",
            pem_rfc7468::LineEnding::LF,
            self.pkcs8_der()?.as_ref(),
        )
        .map(Zeroizing::new)
        .map_err(|_| KeyError)
    }
}

fn decode_pem(pem: &[u8], expected: &str) -> Result<Zeroizing<Vec<u8>>, KeyError> {
    if pem.is_empty() || pem.len() > MAX_PEM_BYTES {
        return Err(KeyError);
    }
    let (label, bytes) = pem_rfc7468::decode_vec(pem).map_err(|_| KeyError)?;
    let bytes = Zeroizing::new(bytes);
    if label != expected || bytes.is_empty() || bytes.len() > MAX_DER_BYTES {
        return Err(KeyError);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_public_formats_and_private_binding_round_trip() {
        let private = RsaPrivateKey::generate_for_testing().unwrap();
        let public = private.public_key();
        assert_eq!(public.bits(), 2048);
        assert_eq!(public.exponent(), [1, 0, 1]);
        assert!(public.permits_signatures());
        for bytes in [public.spki_der().unwrap().as_ref(), public.pkcs1_der()] {
            assert_eq!(
                RsaPublicKey::from_der(bytes).unwrap().pkcs1_der(),
                public.pkcs1_der()
            );
        }
        let from_pem = RsaPublicKey::from_spki_pem(public.spki_pem().unwrap().as_bytes()).unwrap();
        let loaded =
            RsaPrivateKey::from_pkcs8_pem(private.pkcs8_pem_for_testing().unwrap().as_bytes())
                .unwrap();
        assert!(loaded.matches(&from_pem));
        assert!(
            !RsaPrivateKey::generate_for_testing()
                .unwrap()
                .matches(&public)
        );
    }

    #[test]
    fn weak_public_fixture_is_parseable_but_not_signature_eligible() {
        let weak = RsaPublicKey::from_spki_pem(include_bytes!("../tests/public-1024.pem")).unwrap();
        assert_eq!(weak.bits(), 1024);
        assert!(!weak.permits_signatures());
    }

    #[test]
    fn wrong_labels_trailing_der_empty_and_oversized_inputs_are_rejected() {
        let key = RsaPrivateKey::generate_for_testing().unwrap();
        let public = key.public_key();
        assert!(RsaPrivateKey::from_pkcs8_pem(public.spki_pem().unwrap().as_bytes()).is_err());
        assert!(
            RsaPublicKey::from_spki_pem(key.pkcs8_pem_for_testing().unwrap().as_bytes()).is_err()
        );
        for bytes in [vec![], vec![0; MAX_DER_BYTES + 1], vec![0x30, 0x80]] {
            assert!(RsaPublicKey::from_der(&bytes).is_err());
        }
        assert!(decode_pem(&vec![b'A'; MAX_PEM_BYTES + 1], "PUBLIC KEY").is_err());
        let mut trailing = public.spki_der().unwrap().as_ref().to_vec();
        trailing.push(0);
        assert!(RsaPublicKey::from_der(&trailing).is_err());
        let mislabeled = pem_rfc7468::encode_string(
            "PUBLIC KEY",
            pem_rfc7468::LineEnding::LF,
            public.pkcs1_der(),
        )
        .unwrap();
        assert!(RsaPublicKey::from_spki_pem(mislabeled.as_bytes()).is_err());
    }

    #[test]
    fn checkpoint_signature_bytes_match_across_ring_and_aws_lc() {
        let private = RsaPrivateKey::generate_for_testing().unwrap();
        let ring =
            ring::signature::RsaKeyPair::from_pkcs8(private.pkcs8_der().unwrap().as_ref()).unwrap();
        let mut left = vec![0; ring.public().modulus_len()];
        let mut right = left.clone();
        ring.sign(
            &ring::signature::RSA_PKCS1_SHA256,
            &ring::rand::SystemRandom::new(),
            b"checkpoint-format-compatibility",
            &mut left,
        )
        .unwrap();
        private
            .0
            .sign(
                &aws_lc_rs::signature::RSA_PKCS1_SHA256,
                &aws_lc_rs::rand::SystemRandom::new(),
                b"checkpoint-format-compatibility",
                &mut right,
            )
            .unwrap();
        assert_eq!(left, right);
    }
}
