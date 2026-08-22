//! OAuth 2.0 `private_key_jwt` authentication and short-lived Agent tokens.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, TimeZone, Utc};
use copilotmail_application::{
    AccessTokenAuthenticator, AgentCredentialStore, AuthenticatedIdentity,
    AuthenticatedPrincipalKind, AuthenticationContext, AuthenticationError, AuthorizationStore,
    StoreError,
};
use copilotmail_domain::{CredentialId, Permission, PrincipalId};
use jsonwebtoken_v2::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePublicKey},
    traits::PublicKeyParts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    sync::{Arc, RwLock},
};
use url::Url;
use uuid::Uuid;

pub const CLIENT_ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";
pub const CLIENT_CREDENTIALS_GRANT: &str = "client_credentials";
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_ASSERTION_LIFETIME_SECONDS: i64 = 300;

/// Validated authorization-server and signing configuration.
pub struct AgentOAuthConfig {
    issuer: String,
    audience: String,
    token_endpoint: String,
    signing_keys: AgentSigningKeyManager,
    access_token_ttl: Duration,
}

/// One key entry loaded by a secret-file adapter.
#[derive(Debug, Clone)]
pub struct AgentSigningKeyInput {
    pub key_id: String,
    pub private_key_pem: Option<Vec<u8>>,
    pub public_key_pem: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentJwks {
    pub keys: Vec<AgentJwk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentJwk {
    pub kty: String,
    #[serde(rename = "use")]
    pub usage: String,
    pub alg: String,
    pub kid: String,
    pub n: String,
    pub e: String,
}

#[derive(Clone)]
pub struct AgentSigningKeyManager {
    state: Arc<RwLock<SigningKeyRing>>,
    retention: Duration,
}

struct SigningKeyRing {
    active_key_id: String,
    keys: BTreeMap<String, ManagedSigningKey>,
}

struct ManagedSigningKey {
    encoding_key: Option<EncodingKey>,
    decoding_key: DecodingKey,
    jwk: AgentJwk,
    last_issued_at: Option<DateTime<Utc>>,
}

impl AgentOAuthConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        issuer: String,
        audience: String,
        token_endpoint: String,
        signing_key_id: String,
        private_key_pem: &[u8],
        public_key_pem: &[u8],
        access_token_ttl_seconds: i64,
        allow_insecure_http: bool,
    ) -> Result<Self, AgentOAuthConfigError> {
        Self::new_with_signing_keys(
            issuer,
            audience,
            token_endpoint,
            signing_key_id.clone(),
            vec![AgentSigningKeyInput {
                key_id: signing_key_id,
                private_key_pem: Some(private_key_pem.to_vec()),
                public_key_pem: public_key_pem.to_vec(),
            }],
            access_token_ttl_seconds,
            allow_insecure_http,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_signing_keys(
        issuer: String,
        audience: String,
        token_endpoint: String,
        active_key_id: String,
        signing_key_inputs: Vec<AgentSigningKeyInput>,
        access_token_ttl_seconds: i64,
        allow_insecure_http: bool,
    ) -> Result<Self, AgentOAuthConfigError> {
        for value in [&issuer, &audience, &token_endpoint, &active_key_id] {
            if value.is_empty()
                || value.trim() != value
                || value.len() > 2_048
                || value.chars().any(char::is_control)
            {
                return Err(AgentOAuthConfigError::Invalid);
            }
        }
        if !valid_endpoint_url(&issuer, allow_insecure_http)
            || !valid_endpoint_url(&token_endpoint, allow_insecure_http)
        {
            return Err(AgentOAuthConfigError::Invalid);
        }
        if !(60..=900).contains(&access_token_ttl_seconds) || active_key_id.len() > 128 {
            return Err(AgentOAuthConfigError::Invalid);
        }
        let access_token_ttl = Duration::seconds(access_token_ttl_seconds);
        let signing_keys = AgentSigningKeyManager::new(
            active_key_id,
            signing_key_inputs,
            access_token_ttl,
            Utc::now(),
        )?;
        Ok(Self {
            issuer,
            audience,
            token_endpoint,
            signing_keys,
            access_token_ttl,
        })
    }

    #[must_use]
    pub fn token_endpoint(&self) -> &str {
        &self.token_endpoint
    }
}

impl AgentSigningKeyManager {
    pub fn new(
        active_key_id: String,
        keys: Vec<AgentSigningKeyInput>,
        access_token_ttl: Duration,
        now: DateTime<Utc>,
    ) -> Result<Self, AgentOAuthConfigError> {
        let manager = Self {
            state: Arc::new(RwLock::new(SigningKeyRing {
                active_key_id: String::new(),
                keys: BTreeMap::new(),
            })),
            retention: access_token_ttl + Duration::seconds(30),
        };
        manager.replace(active_key_id, keys, now)?;
        {
            let mut state = manager
                .state
                .write()
                .map_err(|_| AgentOAuthConfigError::Invalid)?;
            for key in state.keys.values_mut() {
                // A process may have restarted while tokens signed by any key
                // in the manifest remain valid. Start conservatively rather
                // than treating in-memory issuance history as authoritative.
                key.last_issued_at = Some(now);
            }
        }
        Ok(manager)
    }

    /// Atomically replaces the key ring after validating every key and
    /// enforcing the old-token verification retention window.
    pub fn replace(
        &self,
        active_key_id: String,
        keys: Vec<AgentSigningKeyInput>,
        now: DateTime<Utc>,
    ) -> Result<(), AgentOAuthConfigError> {
        if keys.is_empty() || keys.len() > 10 || !valid_key_id(&active_key_id) {
            return Err(AgentOAuthConfigError::Invalid);
        }
        let mut replacement = BTreeMap::new();
        for input in keys {
            if !valid_key_id(&input.key_id) || replacement.contains_key(&input.key_id) {
                return Err(AgentOAuthConfigError::Invalid);
            }
            let public = parse_public_key(&input.public_key_pem)?;
            let encoding_key = if let Some(private_key_pem) = input.private_key_pem {
                let private = RsaPrivateKey::from_pkcs8_pem(
                    std::str::from_utf8(&private_key_pem)
                        .map_err(|_| AgentOAuthConfigError::Key)?,
                )
                .map_err(|_| AgentOAuthConfigError::Key)?;
                if private.n() != public.n() || private.e() != public.e() {
                    return Err(AgentOAuthConfigError::Key);
                }
                Some(
                    EncodingKey::from_rsa_pem(&private_key_pem)
                        .map_err(|_| AgentOAuthConfigError::Key)?,
                )
            } else {
                None
            };
            replacement.insert(
                input.key_id.clone(),
                ManagedSigningKey {
                    encoding_key,
                    decoding_key: DecodingKey::from_rsa_pem(&input.public_key_pem)
                        .map_err(|_| AgentOAuthConfigError::Key)?,
                    jwk: rsa_jwk(input.key_id, &public),
                    last_issued_at: None,
                },
            );
        }
        if replacement
            .get(&active_key_id)
            .and_then(|key| key.encoding_key.as_ref())
            .is_none()
        {
            return Err(AgentOAuthConfigError::Invalid);
        }
        let mut current = self
            .state
            .write()
            .map_err(|_| AgentOAuthConfigError::Invalid)?;
        for (key_id, key) in &current.keys {
            if replacement
                .get(key_id)
                .is_some_and(|replacement_key| replacement_key.jwk != key.jwk)
            {
                return Err(AgentOAuthConfigError::KeyIdReuse);
            }
            if let Some(last_issued_at) = key.last_issued_at {
                if last_issued_at + self.retention > now && !replacement.contains_key(key_id) {
                    return Err(AgentOAuthConfigError::UnsafeKeyRemoval);
                }
                if let Some(replacement_key) = replacement.get_mut(key_id) {
                    replacement_key.last_issued_at = Some(last_issued_at);
                }
            }
        }
        *current = SigningKeyRing {
            active_key_id,
            keys: replacement,
        };
        Ok(())
    }

    #[must_use]
    pub fn jwks(&self) -> AgentJwks {
        let state = self
            .state
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        AgentJwks {
            keys: state.keys.values().map(|key| key.jwk.clone()).collect(),
        }
    }

    fn sign<T: Serialize>(&self, claims: &T, now: DateTime<Utc>) -> Result<String, TokenError> {
        let mut state = self
            .state
            .write()
            .map_err(|_| TokenError::TemporarilyUnavailable)?;
        let active_key_id = state.active_key_id.clone();
        let key = state
            .keys
            .get_mut(&active_key_id)
            .ok_or(TokenError::TemporarilyUnavailable)?;
        let encoding_key = key
            .encoding_key
            .as_ref()
            .ok_or(TokenError::TemporarilyUnavailable)?;
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(active_key_id);
        header.typ = Some("at+jwt".into());
        let token = encode(&header, claims, encoding_key)
            .map_err(|_| TokenError::TemporarilyUnavailable)?;
        key.last_issued_at = Some(now);
        Ok(token)
    }

    fn decoding_key(&self, key_id: &str) -> Option<DecodingKey> {
        self.state
            .read()
            .ok()?
            .keys
            .get(key_id)
            .map(|key| key.decoding_key.clone())
    }
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn rsa_jwk(key_id: String, public: &RsaPublicKey) -> AgentJwk {
    AgentJwk {
        kty: "RSA".into(),
        usage: "sig".into(),
        alg: "RS256".into(),
        kid: key_id,
        n: URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
        e: URL_SAFE_NO_PAD.encode(public.e().to_bytes_be()),
    }
}

/// Validates an RSA SubjectPublicKeyInfo PEM and returns its canonical SHA-256
/// DER fingerprint for duplicate-key detection.
pub fn rsa_public_key_fingerprint(public_key_pem: &[u8]) -> Result<String, AgentOAuthConfigError> {
    let key = parse_public_key(public_key_pem)?;
    let der = key
        .to_public_key_der()
        .map_err(|_| AgentOAuthConfigError::Key)?;
    let mut fingerprint = String::with_capacity(64);
    for byte in Sha256::digest(der.as_ref()) {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(fingerprint)
}

fn parse_public_key(public_key_pem: &[u8]) -> Result<RsaPublicKey, AgentOAuthConfigError> {
    let pem = std::str::from_utf8(public_key_pem).map_err(|_| AgentOAuthConfigError::Key)?;
    let key = RsaPublicKey::from_public_key_pem(pem).map_err(|_| AgentOAuthConfigError::Key)?;
    if key.n().bits() < 2_048 || key.e().bits() > 32 {
        return Err(AgentOAuthConfigError::Key);
    }
    Ok(key)
}

fn valid_endpoint_url(value: &str, allow_insecure_http: bool) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return false;
    }
    if url.scheme() == "https" {
        return true;
    }
    allow_insecure_http
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AgentOAuthConfigError {
    #[error("Agent OAuth configuration is invalid")]
    Invalid,
    #[error("Agent OAuth RSA key is invalid")]
    Key,
    #[error("a signing key cannot be removed while issued tokens may still be valid")]
    UnsafeKeyRemoval,
    #[error("a signing key ID cannot be reused for different key material")]
    KeyIdReuse,
}

#[derive(Debug, Clone)]
pub struct TokenRequest {
    pub grant_type: String,
    pub client_id: String,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    InvalidRequest,
    InvalidClient,
    InvalidScope,
    TemporarilyUnavailable,
}

struct AuthenticatedAgentClient {
    credential_id: CredentialId,
    organization_id: copilotmail_domain::OrganizationId,
    allowed_scopes: BTreeSet<Permission>,
    expires_at: DateTime<Utc>,
    certificate_fingerprint: Option<String>,
}

/// Exchanges one replay-protected client assertion for a short-lived token.
pub struct AgentTokenService<S> {
    store: S,
    config: AgentOAuthConfig,
}

impl<S> AgentTokenService<S>
where
    S: AgentCredentialStore + AuthorizationStore,
{
    #[must_use]
    pub const fn new(store: S, config: AgentOAuthConfig) -> Self {
        Self { store, config }
    }

    pub async fn exchange(
        &self,
        request: TokenRequest,
        client_certificate_fingerprint: Option<&str>,
        source: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<TokenResponse, TokenError> {
        if request.grant_type != CLIENT_CREDENTIALS_GRANT {
            return Err(TokenError::InvalidRequest);
        }
        let client_id = request
            .client_id
            .parse::<PrincipalId>()
            .map_err(|_| TokenError::InvalidClient)?;
        let client = match (
            request.client_assertion_type.as_deref(),
            request.client_assertion.as_deref(),
            client_certificate_fingerprint,
        ) {
            (Some(assertion_type), Some(assertion), None)
                if assertion_type == CLIENT_ASSERTION_TYPE
                    && assertion.len() <= MAX_TOKEN_BYTES =>
            {
                self.authenticate_private_key_jwt(
                    client_id,
                    &request.client_id,
                    assertion,
                    source,
                    now,
                )
                .await?
            }
            (None, None, Some(fingerprint)) => {
                let credential = self
                    .store
                    .authenticate_agent_mtls(client_id, fingerprint, source, now)
                    .await
                    .map_err(store_token_error)?
                    .ok_or(TokenError::InvalidClient)?;
                AuthenticatedAgentClient {
                    credential_id: credential.credential_id,
                    organization_id: credential.organization_id,
                    allowed_scopes: credential.allowed_scopes,
                    expires_at: credential.expires_at,
                    certificate_fingerprint: Some(credential.certificate_fingerprint),
                }
            }
            _ => return Err(TokenError::InvalidRequest),
        };

        let authorization = self
            .store
            .resolve_authorization(client_id, now)
            .await
            .map_err(store_token_error)?
            .ok_or(TokenError::InvalidClient)?;
        if authorization.organization_id != client.organization_id {
            return Err(TokenError::InvalidClient);
        }
        let permitted = client
            .allowed_scopes
            .intersection(&authorization.permissions)
            .copied()
            .collect::<BTreeSet<_>>();
        let scopes = match request.scope.as_deref() {
            Some(requested) => parse_scopes(requested).ok_or(TokenError::InvalidScope)?,
            None => permitted.clone(),
        };
        if scopes.is_empty() || !scopes.is_subset(&permitted) {
            return Err(TokenError::InvalidScope);
        }

        let expires_at = (now + self.config.access_token_ttl).min(client.expires_at);
        let expires_in = (expires_at - now).num_seconds();
        let scope = scope_string(&scopes);
        let confirmation = client
            .certificate_fingerprint
            .as_deref()
            .map(certificate_confirmation)
            .transpose()
            .map_err(|_| TokenError::InvalidClient)?;
        let claims = AccessTokenClaims {
            iss: self.config.issuer.clone(),
            sub: client_id.to_string(),
            aud: self.config.audience.clone(),
            exp: expires_at.timestamp(),
            iat: now.timestamp(),
            jti: Uuid::now_v7().to_string(),
            client_id: client_id.to_string(),
            scope: scope.clone(),
            credential_id: client.credential_id.to_string(),
            organization_id: client.organization_id.to_string(),
            confirmation,
        };
        let access_token = self.config.signing_keys.sign(&claims, now)?;
        Ok(TokenResponse {
            access_token,
            token_type: "Bearer".into(),
            expires_in,
            scope,
        })
    }

    async fn authenticate_private_key_jwt(
        &self,
        client_id: PrincipalId,
        client_id_text: &str,
        assertion_text: &str,
        source: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<AuthenticatedAgentClient, TokenError> {
        let header = decode_header(assertion_text).map_err(|_| TokenError::InvalidClient)?;
        if header.alg != Algorithm::RS256
            || header.jku.is_some()
            || header.jwk.is_some()
            || header.crit.is_some()
        {
            return Err(TokenError::InvalidClient);
        }
        let credential_id = header
            .kid
            .as_deref()
            .ok_or(TokenError::InvalidClient)?
            .parse::<CredentialId>()
            .map_err(|_| TokenError::InvalidClient)?;
        let credential = self
            .store
            .get_agent_credential(client_id, credential_id, now)
            .await
            .map_err(store_token_error)?
            .ok_or(TokenError::InvalidClient)?;
        let key = DecodingKey::from_rsa_pem(credential.public_key_pem.as_bytes())
            .map_err(|_| TokenError::InvalidClient)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iss", "sub", "aud"]);
        validation.set_issuer(&[client_id_text]);
        validation.set_audience(&[self.config.token_endpoint.as_str()]);
        validation.validate_nbf = true;
        validation.leeway = 30;
        let assertion = decode::<ClientAssertion>(assertion_text, &key, &validation)
            .map_err(|_| TokenError::InvalidClient)?
            .claims;
        if assertion.iss != client_id_text
            || assertion.sub != client_id_text
            || assertion.jti.is_empty()
            || assertion.jti.len() > 200
            || !assertion.jti.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(TokenError::InvalidClient);
        }
        let assertion_exp = timestamp(assertion.exp).ok_or(TokenError::InvalidClient)?;
        let assertion_iat = timestamp(assertion.iat).ok_or(TokenError::InvalidClient)?;
        if assertion_exp > now + Duration::seconds(MAX_ASSERTION_LIFETIME_SECONDS)
            || assertion_iat < now - Duration::seconds(60)
            || assertion_iat > now + Duration::seconds(30)
            || assertion_exp > credential.expires_at
        {
            return Err(TokenError::InvalidClient);
        }

        let jti_hash = Sha256::digest(assertion.jti.as_bytes());
        if !self
            .store
            .consume_agent_assertion(
                credential_id,
                jti_hash.as_slice(),
                assertion_exp,
                source,
                now,
            )
            .await
            .map_err(store_token_error)?
        {
            return Err(TokenError::InvalidClient);
        }
        Ok(AuthenticatedAgentClient {
            credential_id,
            organization_id: credential.organization_id,
            allowed_scopes: credential.allowed_scopes,
            expires_at: credential.expires_at,
            certificate_fingerprint: None,
        })
    }

    #[must_use]
    pub fn authenticator(&self) -> AgentAccessTokenAuthenticator<S>
    where
        S: Clone,
    {
        AgentAccessTokenAuthenticator {
            store: self.store.clone(),
            issuer: self.config.issuer.clone(),
            audience: self.config.audience.clone(),
            signing_keys: self.config.signing_keys.clone(),
        }
    }

    #[must_use]
    pub fn signing_key_manager(&self) -> AgentSigningKeyManager {
        self.config.signing_keys.clone()
    }

    #[must_use]
    pub fn jwks(&self) -> AgentJwks {
        self.config.signing_keys.jwks()
    }
}

/// Validates server-issued Agent tokens and checks live revocation state.
pub struct AgentAccessTokenAuthenticator<S> {
    store: S,
    issuer: String,
    audience: String,
    signing_keys: AgentSigningKeyManager,
}

#[async_trait]
impl<S> AccessTokenAuthenticator for AgentAccessTokenAuthenticator<S>
where
    S: AgentCredentialStore + Send + Sync,
{
    async fn authenticate(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedIdentity, AuthenticationError> {
        self.verify(bearer_token, &AuthenticationContext::default())
            .await
    }

    async fn authenticate_with_context(
        &self,
        bearer_token: &str,
        context: &AuthenticationContext,
    ) -> Result<AuthenticatedIdentity, AuthenticationError> {
        self.verify(bearer_token, context).await
    }
}

impl<S> AgentAccessTokenAuthenticator<S>
where
    S: AgentCredentialStore + Send + Sync,
{
    async fn verify(
        &self,
        bearer_token: &str,
        context: &AuthenticationContext,
    ) -> Result<AuthenticatedIdentity, AuthenticationError> {
        if bearer_token.len() > MAX_TOKEN_BYTES {
            return Err(AuthenticationError::InvalidCredential);
        }
        let header =
            decode_header(bearer_token).map_err(|_| AuthenticationError::InvalidCredential)?;
        if header.alg != Algorithm::RS256
            || header.typ.as_deref() != Some("at+jwt")
            || header.jku.is_some()
            || header.jwk.is_some()
            || header.crit.is_some()
        {
            return Err(AuthenticationError::InvalidCredential);
        }
        let decoding_key = header
            .kid
            .as_deref()
            .and_then(|key_id| self.signing_keys.decoding_key(key_id))
            .ok_or(AuthenticationError::InvalidCredential)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iss", "sub", "aud"]);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.validate_nbf = true;
        validation.leeway = 30;
        validation.reject_tokens_expiring_in_less_than = 1;
        let claims = decode::<AccessTokenClaims>(bearer_token, &decoding_key, &validation)
            .map_err(|_| AuthenticationError::InvalidCredential)?
            .claims;
        if claims.sub != claims.client_id {
            return Err(AuthenticationError::InvalidCredential);
        }
        if let Some(confirmation) = &claims.confirmation {
            let presented = context
                .client_certificate_fingerprint
                .as_deref()
                .and_then(|fingerprint| certificate_confirmation(fingerprint).ok());
            if presented.as_ref() != Some(confirmation) {
                return Err(AuthenticationError::InvalidCredential);
            }
        }
        let principal_id = claims
            .sub
            .parse::<PrincipalId>()
            .map_err(|_| AuthenticationError::InvalidCredential)?;
        let credential_id = claims
            .credential_id
            .parse::<CredentialId>()
            .map_err(|_| AuthenticationError::InvalidCredential)?;
        let scopes = parse_scopes(&claims.scope).ok_or(AuthenticationError::InvalidCredential)?;
        if !self
            .store
            .validate_agent_token_binding(principal_id, credential_id, &scopes, Utc::now())
            .await
            .map_err(|_| AuthenticationError::Unavailable)?
        {
            return Err(AuthenticationError::InvalidCredential);
        }
        Ok(AuthenticatedIdentity {
            principal_id,
            principal_kind: AuthenticatedPrincipalKind::Agent,
            credential_id: Some(credential_id.to_string()),
            scope_restriction: Some(scopes),
        })
    }
}

#[derive(Debug, Deserialize)]
struct ClientAssertion {
    iss: String,
    sub: String,
    exp: i64,
    iat: i64,
    jti: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessTokenClaims {
    iss: String,
    sub: String,
    aud: String,
    exp: i64,
    iat: i64,
    jti: String,
    client_id: String,
    scope: String,
    #[serde(rename = "cm_credential_id")]
    credential_id: String,
    #[serde(rename = "cm_organization_id")]
    organization_id: String,
    #[serde(rename = "cnf", skip_serializing_if = "Option::is_none")]
    confirmation: Option<CertificateConfirmation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CertificateConfirmation {
    #[serde(rename = "x5t#S256")]
    x5t_s256: String,
}

fn certificate_confirmation(
    fingerprint: &str,
) -> Result<CertificateConfirmation, AgentOAuthConfigError> {
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AgentOAuthConfigError::Invalid);
    }
    let bytes = (0..fingerprint.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&fingerprint[offset..offset + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AgentOAuthConfigError::Invalid)?;
    Ok(CertificateConfirmation {
        x5t_s256: URL_SAFE_NO_PAD.encode(bytes),
    })
}

fn parse_scopes(value: &str) -> Option<BTreeSet<Permission>> {
    let scopes = value
        .split_ascii_whitespace()
        .map(permission_from_scope)
        .collect::<Option<BTreeSet<_>>>()?;
    (!scopes.is_empty()).then_some(scopes)
}

fn permission_from_scope(value: &str) -> Option<Permission> {
    Permission::AGENT_TOKEN
        .into_iter()
        .find(|permission| permission.as_str() == value)
}

fn scope_string(scopes: &BTreeSet<Permission>) -> String {
    scopes
        .iter()
        .map(|scope| scope.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn timestamp(value: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(value, 0).single()
}

fn store_token_error(_: StoreError) -> TokenError {
    TokenError::TemporarilyUnavailable
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use copilotmail_application::{
        ExistingAgentCredential, ExistingAgentMtlsCredential, StoreError,
    };
    use copilotmail_domain::{AuthorizationContext, OrganizationId};
    use jsonwebtoken_v2::{Header, encode};
    use rand_core_06::OsRng;
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    #[derive(Clone)]
    struct MemoryStore {
        credential: ExistingAgentCredential,
        mtls_credential: Option<ExistingAgentMtlsCredential>,
        permissions: BTreeSet<Permission>,
        used: Arc<Mutex<BTreeSet<Vec<u8>>>>,
        revoked: Arc<AtomicBool>,
    }

    #[async_trait]
    impl AgentCredentialStore for MemoryStore {
        async fn get_agent_credential(
            &self,
            client_id: PrincipalId,
            credential_id: CredentialId,
            _: DateTime<Utc>,
        ) -> Result<Option<ExistingAgentCredential>, StoreError> {
            Ok((!self.revoked.load(Ordering::SeqCst)
                && client_id == self.credential.principal_id
                && credential_id == self.credential.credential_id)
                .then(|| self.credential.clone()))
        }

        async fn consume_agent_assertion(
            &self,
            _: CredentialId,
            jti_hash: &[u8],
            _: DateTime<Utc>,
            _: Option<&str>,
            _: DateTime<Utc>,
        ) -> Result<bool, StoreError> {
            Ok(self.used.lock().unwrap().insert(jti_hash.to_vec()))
        }

        async fn authenticate_agent_mtls(
            &self,
            client_id: PrincipalId,
            certificate_fingerprint: &str,
            _: Option<&str>,
            _: DateTime<Utc>,
        ) -> Result<Option<ExistingAgentMtlsCredential>, StoreError> {
            Ok(self
                .mtls_credential
                .as_ref()
                .filter(|credential| {
                    !self.revoked.load(Ordering::SeqCst)
                        && credential.principal_id == client_id
                        && credential.certificate_fingerprint == certificate_fingerprint
                })
                .cloned())
        }

        async fn validate_agent_token_binding(
            &self,
            principal_id: PrincipalId,
            credential_id: CredentialId,
            scopes: &BTreeSet<Permission>,
            _: DateTime<Utc>,
        ) -> Result<bool, StoreError> {
            let private_key_matches = principal_id == self.credential.principal_id
                && credential_id == self.credential.credential_id
                && scopes.is_subset(&self.credential.allowed_scopes);
            let mtls_matches = self.mtls_credential.as_ref().is_some_and(|credential| {
                principal_id == credential.principal_id
                    && credential_id == credential.credential_id
                    && scopes.is_subset(&credential.allowed_scopes)
            });
            Ok(!self.revoked.load(Ordering::SeqCst) && (private_key_matches || mtls_matches))
        }
    }

    #[async_trait]
    impl AuthorizationStore for MemoryStore {
        async fn resolve_authorization(
            &self,
            principal_id: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<Option<AuthorizationContext>, StoreError> {
            Ok(
                (principal_id == self.credential.principal_id).then(|| AuthorizationContext {
                    principal_id,
                    organization_id: self.credential.organization_id,
                    permissions: self.permissions.clone(),
                }),
            )
        }
    }

    #[derive(Serialize)]
    struct TestAssertion {
        iss: String,
        sub: String,
        aud: String,
        exp: i64,
        iat: i64,
        jti: String,
    }

    fn rsa_pair() -> (String, String) {
        let private = RsaPrivateKey::new(&mut OsRng, 2_048).unwrap();
        let public = RsaPublicKey::from(&private);
        (
            private.to_pkcs8_pem(LineEnding::LF).unwrap().to_string(),
            public.to_public_key_pem(LineEnding::LF).unwrap(),
        )
    }

    fn fixture() -> (
        MemoryStore,
        AgentOAuthConfig,
        String,
        PrincipalId,
        CredentialId,
    ) {
        let (client_private, client_public) = rsa_pair();
        let (server_private, server_public) = rsa_pair();
        let principal_id = PrincipalId::new();
        let credential_id = CredentialId::new();
        let organization_id = OrganizationId::new();
        let store = MemoryStore {
            credential: ExistingAgentCredential {
                credential_id,
                organization_id,
                principal_id,
                public_key_pem: client_public,
                allowed_scopes: Permission::AGENT_TOKEN.into_iter().collect(),
                expires_at: Utc::now() + Duration::days(1),
            },
            mtls_credential: None,
            permissions: Permission::AGENT_TOKEN.into_iter().collect(),
            used: Arc::new(Mutex::new(BTreeSet::new())),
            revoked: Arc::new(AtomicBool::new(false)),
        };
        let config = AgentOAuthConfig::new(
            "https://mail.example.test".into(),
            "https://mail.example.test/api/v2".into(),
            "https://mail.example.test/oauth2/token".into(),
            "server-key-1".into(),
            server_private.as_bytes(),
            server_public.as_bytes(),
            300,
            false,
        )
        .unwrap();
        (store, config, client_private, principal_id, credential_id)
    }

    fn assertion(
        private_key: &str,
        principal_id: PrincipalId,
        credential_id: CredentialId,
        jti: &str,
        now: DateTime<Utc>,
    ) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(credential_id.to_string());
        encode(
            &header,
            &TestAssertion {
                iss: principal_id.to_string(),
                sub: principal_id.to_string(),
                aud: "https://mail.example.test/oauth2/token".into(),
                exp: (now + Duration::seconds(120)).timestamp(),
                iat: now.timestamp(),
                jti: jti.into(),
            },
            &EncodingKey::from_rsa_pem(private_key.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn assertion_is_one_time_and_issued_token_honors_live_revocation() {
        let (store, config, private_key, principal_id, credential_id) = fixture();
        let service = AgentTokenService::new(store.clone(), config);
        let authenticator = service.authenticator();
        let now = Utc::now();
        let client_assertion = assertion(
            &private_key,
            principal_id,
            credential_id,
            "assertion-1",
            now,
        );
        let request = TokenRequest {
            grant_type: CLIENT_CREDENTIALS_GRANT.into(),
            client_id: principal_id.to_string(),
            client_assertion_type: Some(CLIENT_ASSERTION_TYPE.into()),
            client_assertion: Some(client_assertion.clone()),
            scope: Some("organization.read mailbox.use".into()),
        };
        let token = service
            .exchange(request.clone(), None, Some("192.0.2.10"), now)
            .await
            .unwrap();
        assert_eq!(token.token_type, "Bearer");
        assert_eq!(token.expires_in, 300);
        let identity = authenticator
            .authenticate(&token.access_token)
            .await
            .unwrap();
        assert_eq!(identity.principal_id, principal_id);
        assert_eq!(
            identity.scope_restriction.unwrap(),
            [Permission::OrganizationRead, Permission::MailboxUse]
                .into_iter()
                .collect()
        );
        assert_eq!(
            service
                .exchange(request, None, None, now)
                .await
                .unwrap_err(),
            TokenError::InvalidClient
        );
        store.revoked.store(true, Ordering::SeqCst);
        assert_eq!(
            authenticator
                .authenticate(&token.access_token)
                .await
                .unwrap_err(),
            AuthenticationError::InvalidCredential
        );
    }

    #[tokio::test]
    async fn client_cannot_request_scope_outside_live_rbac() {
        let (mut store, config, private_key, principal_id, credential_id) = fixture();
        store.permissions.remove(&Permission::MailboxUse);
        let now = Utc::now();
        let error = AgentTokenService::new(store, config)
            .exchange(
                TokenRequest {
                    grant_type: CLIENT_CREDENTIALS_GRANT.into(),
                    client_id: principal_id.to_string(),
                    client_assertion_type: Some(CLIENT_ASSERTION_TYPE.into()),
                    client_assertion: Some(assertion(
                        &private_key,
                        principal_id,
                        credential_id,
                        "assertion-scope",
                        now,
                    )),
                    scope: Some("mailbox.use".into()),
                },
                None,
                None,
                now,
            )
            .await
            .unwrap_err();
        assert_eq!(error, TokenError::InvalidScope);
    }

    #[tokio::test]
    async fn mtls_token_is_bound_to_verified_leaf_certificate_and_live_revocation() {
        let (mut store, config, _, principal_id, _) = fixture();
        let fingerprint = "ab".repeat(32);
        let mtls_credential_id = CredentialId::new();
        store.mtls_credential = Some(ExistingAgentMtlsCredential {
            credential_id: mtls_credential_id,
            organization_id: store.credential.organization_id,
            principal_id,
            certificate_fingerprint: fingerprint.clone(),
            allowed_scopes: [Permission::OrganizationRead].into_iter().collect(),
            expires_at: Utc::now() + Duration::days(1),
        });
        let service = AgentTokenService::new(store.clone(), config);
        let authenticator = service.authenticator();
        let now = Utc::now();
        let token = service
            .exchange(
                TokenRequest {
                    grant_type: CLIENT_CREDENTIALS_GRANT.into(),
                    client_id: principal_id.to_string(),
                    client_assertion_type: None,
                    client_assertion: None,
                    scope: Some("organization.read".into()),
                },
                Some(&fingerprint),
                Some("192.0.2.20"),
                now,
            )
            .await
            .unwrap();
        assert_eq!(
            authenticator
                .authenticate(&token.access_token)
                .await
                .unwrap_err(),
            AuthenticationError::InvalidCredential
        );
        assert_eq!(
            authenticator
                .authenticate_with_context(
                    &token.access_token,
                    &AuthenticationContext {
                        client_certificate_fingerprint: Some("cd".repeat(32)),
                    },
                )
                .await
                .unwrap_err(),
            AuthenticationError::InvalidCredential
        );
        let identity = authenticator
            .authenticate_with_context(
                &token.access_token,
                &AuthenticationContext {
                    client_certificate_fingerprint: Some(fingerprint),
                },
            )
            .await
            .unwrap();
        assert_eq!(identity.credential_id, Some(mtls_credential_id.to_string()));
        store.revoked.store(true, Ordering::SeqCst);
        assert_eq!(
            authenticator
                .authenticate_with_context(
                    &token.access_token,
                    &AuthenticationContext {
                        client_certificate_fingerprint: Some("ab".repeat(32)),
                    },
                )
                .await
                .unwrap_err(),
            AuthenticationError::InvalidCredential
        );
    }

    #[test]
    fn configuration_rejects_mismatched_server_keys() {
        let (private, _) = rsa_pair();
        let (_, other_public) = rsa_pair();
        assert!(matches!(
            AgentOAuthConfig::new(
                "https://mail.example.test".into(),
                "https://mail.example.test/api".into(),
                "https://mail.example.test/oauth2/token".into(),
                "kid".into(),
                private.as_bytes(),
                other_public.as_bytes(),
                300,
                false,
            ),
            Err(AgentOAuthConfigError::Key)
        ));
    }

    #[test]
    fn signing_key_rotation_overlaps_jwks_and_blocks_unsafe_removal() {
        let (old_private, old_public) = rsa_pair();
        let (new_private, new_public) = rsa_pair();
        let now = Utc::now();
        let manager = AgentSigningKeyManager::new(
            "old".into(),
            vec![AgentSigningKeyInput {
                key_id: "old".into(),
                private_key_pem: Some(old_private.as_bytes().to_vec()),
                public_key_pem: old_public.as_bytes().to_vec(),
            }],
            Duration::seconds(300),
            now,
        )
        .unwrap();
        let old_token = manager
            .sign(&serde_json::json!({"sub": "rotation-test"}), now)
            .unwrap();
        assert_eq!(
            decode_header(&old_token).unwrap().kid.as_deref(),
            Some("old")
        );

        manager
            .replace(
                "new".into(),
                vec![
                    AgentSigningKeyInput {
                        key_id: "old".into(),
                        private_key_pem: None,
                        public_key_pem: old_public.as_bytes().to_vec(),
                    },
                    AgentSigningKeyInput {
                        key_id: "new".into(),
                        private_key_pem: Some(new_private.as_bytes().to_vec()),
                        public_key_pem: new_public.as_bytes().to_vec(),
                    },
                ],
                now + Duration::seconds(1),
            )
            .unwrap();
        assert_eq!(
            manager
                .jwks()
                .keys
                .iter()
                .map(|key| key.kid.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "old"]
        );
        let new_token = manager
            .sign(
                &serde_json::json!({"sub": "rotation-test"}),
                now + Duration::seconds(2),
            )
            .unwrap();
        assert_eq!(
            decode_header(&new_token).unwrap().kid.as_deref(),
            Some("new")
        );
        assert!(manager.decoding_key("old").is_some());
        assert_eq!(
            manager
                .replace(
                    "new".into(),
                    vec![
                        AgentSigningKeyInput {
                            key_id: "new".into(),
                            private_key_pem: Some(new_private.as_bytes().to_vec()),
                            public_key_pem: new_public.as_bytes().to_vec(),
                        },
                        AgentSigningKeyInput {
                            key_id: "old".into(),
                            private_key_pem: Some(new_private.as_bytes().to_vec()),
                            public_key_pem: new_public.as_bytes().to_vec(),
                        },
                    ],
                    now + Duration::seconds(3),
                )
                .unwrap_err(),
            AgentOAuthConfigError::KeyIdReuse
        );

        let new_only = || {
            vec![AgentSigningKeyInput {
                key_id: "new".into(),
                private_key_pem: Some(new_private.as_bytes().to_vec()),
                public_key_pem: new_public.as_bytes().to_vec(),
            }]
        };
        assert_eq!(
            manager
                .replace("new".into(), new_only(), now + Duration::seconds(60))
                .unwrap_err(),
            AgentOAuthConfigError::UnsafeKeyRemoval
        );
        manager
            .replace("new".into(), new_only(), now + Duration::seconds(333))
            .unwrap();
        assert!(manager.decoding_key("old").is_none());
        assert_eq!(manager.jwks().keys.len(), 1);
    }
}
