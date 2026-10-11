//! Enterprise OIDC access-token verification and server-side relying-party
//! adapter.
//!
//! The adapter trusts only configured discovery metadata, restricts signatures
//! to RS256, validates required issuer/audience/subject/expiry claims, and maps
//! the verified issuer/subject pair to an active internal human principal. The
//! optional relying-party flow uses Authorization Code with S256 PKCE for a
//! public client and never exposes or retains refresh tokens.

#![forbid(unsafe_code)]

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::DateTime;
use copilotmail_application::{
    AccessTokenAuthenticator, AuthenticatedIdentity, AuthenticatedPrincipalKind,
    AuthenticationError, ExternalIdentityResolver,
};
use copilotmail_domain::AuthenticationBinding;
use futures_util::StreamExt;
use jsonwebtoken_v2::{Algorithm, DecodingKey, Validation, decode, decode_header};
use rand_core::{OsRng, RngCore};
use reqwest::{
    Client,
    header::{ACCEPT, CONTENT_TYPE},
    redirect::Policy,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex, RwLock};
use url::{Url, form_urlencoded};

const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_METADATA_BYTES: usize = 1024 * 1024;
const MAX_TOKEN_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_URL_BYTES: usize = 4 * 1024;
const MAX_AUTHORIZATION_CODE_BYTES: usize = 4 * 1024;
const MAX_SCOPE_BYTES: usize = 2 * 1024;
const AUTHORIZATION_TRANSACTION_LIFETIME_SECONDS: u64 = 10 * 60;
const CLOCK_SKEW_SECONDS: u64 = 60;
const UNKNOWN_KID_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Validated OIDC relying-party settings.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    issuer: String,
    audience: String,
    issuer_url: Url,
    discovery_url: Url,
    jwks_refresh_interval: Duration,
}

impl OidcConfig {
    /// Validates an issuer and audience before any network request.
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
        allow_insecure_http: bool,
    ) -> Result<Self, OidcInitError> {
        let issuer = issuer.into();
        let audience = audience.into();
        if audience.trim().is_empty() || audience.len() > 512 {
            return Err(OidcInitError::InvalidConfiguration);
        }
        let issuer_url = validate_provider_url(&issuer, allow_insecure_http)?;
        if issuer_url.query().is_some() || issuer_url.fragment().is_some() {
            return Err(OidcInitError::InvalidConfiguration);
        }
        let discovery_url = Url::parse(&format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        ))
        .map_err(|_| OidcInitError::InvalidConfiguration)?;
        Ok(Self {
            issuer,
            audience,
            issuer_url,
            discovery_url,
            jwks_refresh_interval: Duration::from_secs(5 * 60),
        })
    }

    /// Overrides the normal JWKS refresh interval for tests or constrained IdPs.
    pub fn with_jwks_refresh_interval(mut self, interval: Duration) -> Self {
        self.jwks_refresh_interval = interval.max(UNKNOWN_KID_REFRESH_INTERVAL);
        self
    }

    /// Returns the exact configured issuer.
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
}

/// OIDC discovery or initial key-loading failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OidcInitError {
    /// Issuer, audience, or provider URL violated security constraints.
    #[error("OIDC configuration is invalid")]
    InvalidConfiguration,
    /// Provider metadata or JWKS could not be retrieved.
    #[error("OIDC provider is unavailable")]
    ProviderUnavailable,
    /// Provider metadata contradicted the configured issuer or algorithms.
    #[error("OIDC provider metadata is invalid")]
    InvalidProviderMetadata,
}

/// Validated settings for an OAuth public client using Authorization Code and
/// S256 PKCE. Client secrets are intentionally not represented.
#[derive(Debug, Clone)]
pub struct OidcPublicClientConfig {
    client_id: String,
    redirect_uri: Url,
    scopes: Vec<String>,
}

impl OidcPublicClientConfig {
    /// Creates a public-client configuration. `openid` is required and
    /// `offline_access` is rejected because this RP never requests or returns
    /// refresh tokens.
    pub fn new<I, S>(
        client_id: impl Into<String>,
        redirect_uri: impl AsRef<str>,
        scopes: I,
        allow_insecure_http: bool,
    ) -> Result<Self, OidcInitError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let client_id = client_id.into();
        if client_id.trim() != client_id
            || client_id.is_empty()
            || client_id.len() > 512
            || client_id.chars().any(char::is_control)
        {
            return Err(OidcInitError::InvalidConfiguration);
        }
        let redirect_uri = validate_redirect_url(redirect_uri.as_ref(), allow_insecure_http)?;
        let scopes = scopes.into_iter().map(Into::into).collect::<Vec<_>>();
        if scopes.is_empty()
            || scopes.len() > 32
            || !scopes.iter().any(|scope| scope == "openid")
            || scopes.iter().any(|scope| scope == "offline_access")
            || scopes.iter().any(|scope| !valid_scope_token(scope))
            || scopes
                .iter()
                .enumerate()
                .any(|(index, scope)| scopes[..index].contains(scope))
            || scopes.iter().map(String::len).sum::<usize>() + scopes.len() - 1 > MAX_SCOPE_BYTES
        {
            return Err(OidcInitError::InvalidConfiguration);
        }
        Ok(Self {
            client_id,
            redirect_uri,
            scopes,
        })
    }

    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    #[must_use]
    pub fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    #[must_use]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
}

/// Short-lived server-side state for one authorization attempt.
///
/// Callers must store the PKCE verifier encrypted, store only a digest of
/// `state`, and atomically consume the persisted transaction before exchanging
/// a code. The type intentionally contains no OIDC nonce because this flow does
/// not consume or trust an ID token.
pub struct OidcAuthorizationTransaction {
    state: String,
    code_verifier: SecretString,
    issued_at_unix_seconds: u64,
    expires_at_unix_seconds: u64,
}

impl fmt::Debug for OidcAuthorizationTransaction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OidcAuthorizationTransaction")
            .field("state", &"[REDACTED]")
            .field("code_verifier", &"[REDACTED]")
            .field("issued_at_unix_seconds", &self.issued_at_unix_seconds)
            .field("expires_at_unix_seconds", &self.expires_at_unix_seconds)
            .finish()
    }
}

impl OidcAuthorizationTransaction {
    /// Reconstructs a transaction after the caller has verified the callback
    /// state digest and decrypted the persisted PKCE verifier.
    pub fn from_persisted(
        state: impl Into<String>,
        code_verifier: impl Into<String>,
        issued_at_unix_seconds: u64,
        expires_at_unix_seconds: u64,
    ) -> Result<Self, OidcFlowError> {
        let state = state.into();
        let code_verifier = code_verifier.into();
        if !valid_generated_binding(&state)
            || !valid_pkce_verifier(&code_verifier)
            || expires_at_unix_seconds <= issued_at_unix_seconds
            || expires_at_unix_seconds - issued_at_unix_seconds
                > AUTHORIZATION_TRANSACTION_LIFETIME_SECONDS
        {
            return Err(OidcFlowError::InvalidAuthorizationResponse);
        }
        Ok(Self {
            state,
            code_verifier: SecretString::from(code_verifier),
            issued_at_unix_seconds,
            expires_at_unix_seconds,
        })
    }

    /// Raw state is exposed only so the caller can redirect it to the provider
    /// and persist a one-way digest.
    #[must_use]
    pub fn state(&self) -> &str {
        &self.state
    }

    /// The verifier must only be sent to the discovered token endpoint or
    /// encrypted for short-lived server-side persistence.
    #[must_use]
    pub fn code_verifier(&self) -> &SecretString {
        &self.code_verifier
    }

    #[must_use]
    pub const fn issued_at_unix_seconds(&self) -> u64 {
        self.issued_at_unix_seconds
    }

    #[must_use]
    pub const fn expires_at_unix_seconds(&self) -> u64 {
        self.expires_at_unix_seconds
    }

    #[must_use]
    pub fn into_parts(self) -> (String, SecretString, u64, u64) {
        (
            self.state,
            self.code_verifier,
            self.issued_at_unix_seconds,
            self.expires_at_unix_seconds,
        )
    }
}

/// Browser redirect plus the server-side transaction that must be persisted
/// before the redirect is issued.
pub struct OidcAuthorizationRequest {
    authorization_url: Url,
    transaction: OidcAuthorizationTransaction,
}

impl OidcAuthorizationRequest {
    #[must_use]
    pub fn authorization_url(&self) -> &Url {
        &self.authorization_url
    }

    #[must_use]
    pub const fn transaction(&self) -> &OidcAuthorizationTransaction {
        &self.transaction
    }

    #[must_use]
    pub fn into_parts(self) -> (Url, OidcAuthorizationTransaction) {
        (self.authorization_url, self.transaction)
    }
}

/// Authorization response query accepted from the browser redirect. Secret
/// authorization codes are private and this type intentionally has no `Debug`
/// implementation.
#[derive(Deserialize)]
pub struct OidcAuthorizationResponse {
    code: Option<String>,
    state: Option<String>,
    #[serde(rename = "iss")]
    issuer: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

impl OidcAuthorizationResponse {
    /// Constructs a successful response for non-HTTP adapters and tests.
    #[must_use]
    pub fn success(
        code: impl Into<String>,
        state: impl Into<String>,
        issuer: Option<String>,
    ) -> Self {
        Self {
            code: Some(code.into()),
            state: Some(state.into()),
            issuer,
            error: None,
            error_description: None,
        }
    }

    /// Constructs a provider-declined response for non-HTTP adapters and tests.
    #[must_use]
    pub fn error(error: impl Into<String>, state: Option<String>, issuer: Option<String>) -> Self {
        Self {
            code: None,
            state,
            issuer,
            error: Some(error.into()),
            error_description: None,
        }
    }

    /// State is exposed so a server can locate a digest-bound pending
    /// transaction before passing the complete response back to this adapter.
    #[must_use]
    pub fn state(&self) -> Option<&str> {
        self.state.as_deref()
    }

    #[must_use]
    pub fn issuer(&self) -> Option<&str> {
        self.issuer.as_deref()
    }
}

/// Stable, non-sensitive failure categories for the interactive RP flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OidcFlowError {
    #[error("authorization request generation failed")]
    EntropyUnavailable,
    #[error("authorization response is invalid")]
    InvalidAuthorizationResponse,
    #[error("authorization was declined by the provider")]
    AuthorizationRejected,
    #[error("OIDC provider is unavailable")]
    ProviderUnavailable,
    #[error("token endpoint rejected the authorization code")]
    TokenEndpointRejected,
    #[error("token endpoint response is invalid")]
    InvalidTokenResponse,
    #[error("returned access token is invalid")]
    InvalidAccessToken,
}

/// Human identity plus the expiry from the cryptographically verified JWT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOidcIdentity {
    identity: AuthenticatedIdentity,
    credential_expires_at_unix_seconds: u64,
}

impl VerifiedOidcIdentity {
    #[must_use]
    pub const fn identity(&self) -> &AuthenticatedIdentity {
        &self.identity
    }

    #[must_use]
    pub const fn credential_expires_at_unix_seconds(&self) -> u64 {
        self.credential_expires_at_unix_seconds
    }

    #[must_use]
    pub fn into_identity(self) -> AuthenticatedIdentity {
        self.identity
    }
}

/// Access token returned from a completed RP flow. The token is secret, the
/// expiry comes only from strict JWT verification, and no refresh token exists
/// in this result.
pub struct OidcAccessToken {
    access_token: SecretString,
    verified: VerifiedOidcIdentity,
}

impl fmt::Debug for OidcAccessToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OidcAccessToken")
            .field("access_token", &"[REDACTED]")
            .field("verified", &self.verified)
            .finish()
    }
}

impl OidcAccessToken {
    #[must_use]
    pub const fn access_token(&self) -> &SecretString {
        &self.access_token
    }

    #[must_use]
    pub const fn identity(&self) -> &AuthenticatedIdentity {
        self.verified.identity()
    }

    #[must_use]
    pub const fn credential_expires_at_unix_seconds(&self) -> u64 {
        self.verified.credential_expires_at_unix_seconds()
    }

    #[must_use]
    pub fn into_parts(self) -> (SecretString, AuthenticatedIdentity, u64) {
        (
            self.access_token,
            self.verified.identity,
            self.verified.credential_expires_at_unix_seconds,
        )
    }
}

/// Strict OIDC verifier backed by a refreshable JWKS cache.
pub struct OidcAuthenticator<R> {
    config: OidcConfig,
    resolver: R,
    client: Client,
    authorization_endpoint: Url,
    token_endpoint: Url,
    supports_public_pkce: bool,
    authorization_response_issuer_required: bool,
    jwks_uri: Url,
    jwks: Arc<RwLock<CachedJwks>>,
    refresh_lock: Mutex<()>,
    last_unknown_key_refresh: Mutex<Option<Instant>>,
}

impl<R> OidcAuthenticator<R>
where
    R: ExternalIdentityResolver,
{
    /// Discovers the provider, validates metadata, and loads the initial JWKS.
    pub async fn discover(config: OidcConfig, resolver: R) -> Result<Self, OidcInitError> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .redirect(Policy::none())
            .user_agent("CopilotMail-OIDC/0.2")
            .build()
            .map_err(|_| OidcInitError::ProviderUnavailable)?;
        let metadata: ProviderMetadata = fetch_json(&client, config.discovery_url.clone()).await?;
        if metadata.issuer != config.issuer {
            return Err(OidcInitError::InvalidProviderMetadata);
        }
        if metadata
            .id_token_signing_alg_values_supported
            .as_ref()
            .is_some_and(|algorithms| !algorithms.iter().any(|algorithm| algorithm == "RS256"))
        {
            return Err(OidcInitError::InvalidProviderMetadata);
        }
        let allow_insecure_http = config.discovery_url.scheme() == "http";
        let authorization_endpoint = validate_discovered_endpoint(
            &metadata.authorization_endpoint,
            &config.issuer_url,
            allow_insecure_http,
            true,
        )?;
        if authorization_endpoint.query_pairs().any(|(name, _)| {
            matches!(
                name.as_ref(),
                "response_type"
                    | "client_id"
                    | "redirect_uri"
                    | "scope"
                    | "state"
                    | "code_challenge"
                    | "code_challenge_method"
            )
        }) {
            return Err(OidcInitError::InvalidProviderMetadata);
        }
        let token_endpoint = validate_discovered_endpoint(
            &metadata.token_endpoint,
            &config.issuer_url,
            allow_insecure_http,
            true,
        )?;
        let jwks_uri = validate_discovered_endpoint(
            &metadata.jwks_uri,
            &config.issuer_url,
            allow_insecure_http,
            false,
        )?;
        let supports_public_pkce = metadata
            .response_types_supported
            .iter()
            .any(|response_type| response_type.split_ascii_whitespace().eq(["code"]))
            && metadata
                .grant_types_supported
                .as_ref()
                .is_none_or(|grants| grants.iter().any(|grant| grant == "authorization_code"))
            && metadata
                .code_challenge_methods_supported
                .iter()
                .any(|method| method == "S256")
            && metadata
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|method| method == "none");
        let jwks: RsaJwkSet = fetch_json(&client, jwks_uri.clone()).await?;
        validate_jwks(&jwks)?;
        Ok(Self {
            config,
            resolver,
            client,
            authorization_endpoint,
            token_endpoint,
            supports_public_pkce,
            authorization_response_issuer_required: metadata
                .authorization_response_iss_parameter_supported,
            jwks_uri,
            jwks: Arc::new(RwLock::new(CachedJwks {
                value: jwks,
                fetched_at: Instant::now(),
            })),
            refresh_lock: Mutex::new(()),
            last_unknown_key_refresh: Mutex::new(None),
        })
    }

    /// Applies the same strict verification used by the bearer-token adapter
    /// while also returning the trusted JWT `exp` for server-side session
    /// bounding.
    pub async fn authenticate_with_expiry(
        &self,
        bearer_token: &str,
    ) -> Result<VerifiedOidcIdentity, AuthenticationError> {
        if bearer_token.len() > MAX_TOKEN_BYTES {
            return Err(AuthenticationError::InvalidCredential);
        }
        let header =
            decode_header(bearer_token).map_err(|_| AuthenticationError::InvalidCredential)?;
        if header.alg != Algorithm::RS256
            || header.jku.is_some()
            || header.jwk.is_some()
            || header.crit.is_some()
        {
            return Err(AuthenticationError::InvalidCredential);
        }
        let key_id = header
            .kid
            .as_deref()
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 128
                    && value.bytes().all(|byte| byte.is_ascii_graphic())
            })
            .ok_or(AuthenticationError::InvalidCredential)?;
        let key = self.decoding_key(key_id).await?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.set_issuer(&[self.config.issuer.as_str()]);
        validation.set_audience(&[self.config.audience.as_str()]);
        validation.validate_nbf = true;
        validation.leeway = 60;
        validation.reject_tokens_expiring_in_less_than = 5;
        let claims = decode::<Claims>(bearer_token, &key, &validation)
            .map_err(|_| AuthenticationError::InvalidCredential)?
            .claims;
        if claims.sub.is_empty() || claims.sub.len() > 512 {
            return Err(AuthenticationError::InvalidCredential);
        }
        let principal_id = self
            .resolver
            .resolve_human_identity(&claims.iss, &claims.sub)
            .await
            .map_err(|_| AuthenticationError::Unavailable)?
            .ok_or(AuthenticationError::InvalidCredential)?;
        let credential_expires_at = i64::try_from(claims.exp)
            .ok()
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
            .ok_or(AuthenticationError::InvalidCredential)?;
        Ok(VerifiedOidcIdentity {
            identity: AuthenticatedIdentity {
                principal_id,
                principal_kind: AuthenticatedPrincipalKind::Human,
                credential_id: claims.jti.filter(|value| {
                    value.len() <= 200 && value.bytes().all(|byte| byte.is_ascii_graphic())
                }),
                scope_restriction: None,
                authentication: AuthenticationBinding::HumanBearer {
                    expires_at: credential_expires_at,
                },
            },
            credential_expires_at_unix_seconds: claims.exp,
        })
    }

    async fn decoding_key(&self, key_id: &str) -> Result<DecodingKey, AuthenticationError> {
        self.refresh_if_stale(false).await?;
        if let Some(key) = self.key_from_cache(key_id).await {
            return key;
        }
        self.refresh_if_stale(true).await?;
        self.key_from_cache(key_id)
            .await
            .ok_or(AuthenticationError::InvalidCredential)?
    }

    async fn key_from_cache(
        &self,
        key_id: &str,
    ) -> Option<Result<DecodingKey, AuthenticationError>> {
        let cache = self.jwks.read().await;
        cache.value.signing_key(key_id).map(|key| {
            DecodingKey::from_rsa_components(&key.n, &key.e)
                .map_err(|_| AuthenticationError::InvalidCredential)
        })
    }

    async fn refresh_if_stale(&self, unknown_key: bool) -> Result<(), AuthenticationError> {
        if !unknown_key
            && self.jwks.read().await.fetched_at.elapsed() < self.config.jwks_refresh_interval
        {
            return Ok(());
        }
        let _guard = self.refresh_lock.lock().await;
        if unknown_key {
            let mut last_refresh = self.last_unknown_key_refresh.lock().await;
            if last_refresh.is_some_and(|instant| instant.elapsed() < UNKNOWN_KID_REFRESH_INTERVAL)
            {
                return Ok(());
            }
            *last_refresh = Some(Instant::now());
        } else if self.jwks.read().await.fetched_at.elapsed() < self.config.jwks_refresh_interval {
            return Ok(());
        }
        let jwks: RsaJwkSet = fetch_json(&self.client, self.jwks_uri.clone())
            .await
            .map_err(|_| AuthenticationError::Unavailable)?;
        validate_jwks(&jwks).map_err(|_| AuthenticationError::Unavailable)?;
        *self.jwks.write().await = CachedJwks {
            value: jwks,
            fetched_at: Instant::now(),
        };
        Ok(())
    }
}

/// Server-side Authorization Code relying party for a public S256 PKCE client.
pub struct OidcRelyingParty<R> {
    authenticator: OidcAuthenticator<R>,
    public_client: OidcPublicClientConfig,
}

impl<R> OidcRelyingParty<R>
where
    R: ExternalIdentityResolver,
{
    /// Discovers and validates both token-verification metadata and the
    /// interactive public-client capabilities.
    pub async fn discover(
        config: OidcConfig,
        public_client: OidcPublicClientConfig,
        resolver: R,
    ) -> Result<Self, OidcInitError> {
        let authenticator = OidcAuthenticator::discover(config, resolver).await?;
        if !authenticator.supports_public_pkce {
            return Err(OidcInitError::InvalidProviderMetadata);
        }
        Ok(Self {
            authenticator,
            public_client,
        })
    }

    #[must_use]
    pub const fn authenticator(&self) -> &OidcAuthenticator<R> {
        &self.authenticator
    }

    /// Generates one short-lived state value and PKCE verifier, then builds the
    /// provider authorization URL. Persist the transaction before redirecting.
    pub fn begin_authorization(&self) -> Result<OidcAuthorizationRequest, OidcFlowError> {
        self.begin_authorization_at(unix_now()?)
    }

    fn begin_authorization_at(
        &self,
        issued_at_unix_seconds: u64,
    ) -> Result<OidcAuthorizationRequest, OidcFlowError> {
        let state = random_binding()?;
        let code_verifier = random_binding()?;
        let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
        let mut authorization_url = self.authenticator.authorization_endpoint.clone();
        {
            let mut query = authorization_url.query_pairs_mut();
            query.append_pair("response_type", "code");
            query.append_pair("client_id", &self.public_client.client_id);
            query.append_pair("redirect_uri", self.public_client.redirect_uri.as_str());
            query.append_pair("scope", &self.public_client.scopes.join(" "));
            query.append_pair("state", &state);
            query.append_pair("code_challenge", &code_challenge);
            query.append_pair("code_challenge_method", "S256");
        }
        Ok(OidcAuthorizationRequest {
            authorization_url,
            transaction: OidcAuthorizationTransaction {
                state,
                code_verifier: SecretString::from(code_verifier),
                issued_at_unix_seconds,
                expires_at_unix_seconds: issued_at_unix_seconds
                    .saturating_add(AUTHORIZATION_TRANSACTION_LIFETIME_SECONDS),
            },
        })
    }

    /// Validates and consumes one browser response, exchanges the code at the
    /// discovered token endpoint, and strictly authenticates the returned
    /// access token before exposing it.
    pub async fn complete_authorization(
        &self,
        transaction: OidcAuthorizationTransaction,
        response: OidcAuthorizationResponse,
    ) -> Result<OidcAccessToken, OidcFlowError> {
        self.complete_authorization_at(transaction, response, unix_now()?)
            .await
    }

    async fn complete_authorization_at(
        &self,
        transaction: OidcAuthorizationTransaction,
        response: OidcAuthorizationResponse,
        now_unix_seconds: u64,
    ) -> Result<OidcAccessToken, OidcFlowError> {
        if now_unix_seconds >= transaction.expires_at_unix_seconds
            || now_unix_seconds.saturating_add(CLOCK_SKEW_SECONDS)
                < transaction.issued_at_unix_seconds
        {
            return Err(OidcFlowError::InvalidAuthorizationResponse);
        }
        let response_state = response
            .state
            .as_deref()
            .filter(|state| valid_generated_binding(state))
            .ok_or(OidcFlowError::InvalidAuthorizationResponse)?;
        if !constant_time_eq(response_state.as_bytes(), transaction.state.as_bytes()) {
            return Err(OidcFlowError::InvalidAuthorizationResponse);
        }
        match response.issuer.as_deref() {
            Some(issuer) if issuer == self.authenticator.config.issuer => {}
            Some(_) => return Err(OidcFlowError::InvalidAuthorizationResponse),
            None if self.authenticator.authorization_response_issuer_required => {
                return Err(OidcFlowError::InvalidAuthorizationResponse);
            }
            None => {}
        }
        if response.error.is_some() {
            if response.code.is_some()
                || response.error.as_deref().is_none_or(|error| {
                    error.is_empty()
                        || error.len() > 128
                        || !error.bytes().all(valid_oauth_error_byte)
                })
                || response
                    .error_description
                    .as_deref()
                    .is_some_and(|description| {
                        description.len() > 1024 || description.chars().any(char::is_control)
                    })
            {
                return Err(OidcFlowError::InvalidAuthorizationResponse);
            }
            return Err(OidcFlowError::AuthorizationRejected);
        }
        let code = response
            .code
            .filter(|code| {
                !code.is_empty()
                    && code.len() <= MAX_AUTHORIZATION_CODE_BYTES
                    && !code.chars().any(char::is_control)
            })
            .ok_or(OidcFlowError::InvalidAuthorizationResponse)?;

        let body = form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "authorization_code")
            .append_pair("code", &code)
            .append_pair("redirect_uri", self.public_client.redirect_uri.as_str())
            .append_pair("client_id", &self.public_client.client_id)
            .append_pair("code_verifier", transaction.code_verifier.expose_secret())
            .finish();
        let token_response = self
            .authenticator
            .client
            .post(self.authenticator.token_endpoint.clone())
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|_| OidcFlowError::ProviderUnavailable)?;
        if !token_response.status().is_success() {
            return Err(OidcFlowError::TokenEndpointRejected);
        }
        if !token_response_is_json(&token_response) {
            return Err(OidcFlowError::InvalidTokenResponse);
        }
        let response_body = bounded_response_body(token_response, MAX_TOKEN_RESPONSE_BYTES).await?;
        let token: TokenEndpointResponse = serde_json::from_slice(&response_body)
            .map_err(|_| OidcFlowError::InvalidTokenResponse)?;
        if !token.token_type.eq_ignore_ascii_case("Bearer")
            || token.access_token.is_empty()
            || token.access_token.len() > MAX_TOKEN_BYTES
            || token.access_token.chars().any(char::is_control)
            || token.refresh_token.is_some()
            || token
                .scope
                .as_deref()
                .is_some_and(|scope| !valid_returned_scopes(scope, &self.public_client.scopes))
        {
            return Err(OidcFlowError::InvalidTokenResponse);
        }
        let verified = self
            .authenticator
            .authenticate_with_expiry(&token.access_token)
            .await
            .map_err(|error| match error {
                AuthenticationError::InvalidCredential => OidcFlowError::InvalidAccessToken,
                AuthenticationError::Unavailable => OidcFlowError::ProviderUnavailable,
            })?;
        Ok(OidcAccessToken {
            access_token: SecretString::from(token.access_token),
            verified,
        })
    }
}

#[async_trait]
impl<R> AccessTokenAuthenticator for OidcRelyingParty<R>
where
    R: ExternalIdentityResolver,
{
    async fn authenticate(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedIdentity, AuthenticationError> {
        self.authenticator.authenticate(bearer_token).await
    }
}

#[async_trait]
impl<R> AccessTokenAuthenticator for OidcAuthenticator<R>
where
    R: ExternalIdentityResolver,
{
    async fn authenticate(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedIdentity, AuthenticationError> {
        self.authenticate_with_expiry(bearer_token)
            .await
            .map(VerifiedOidcIdentity::into_identity)
    }
}

#[derive(Debug, Deserialize)]
struct ProviderMetadata {
    issuer: String,
    jwks_uri: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    response_types_supported: Vec<String>,
    grant_types_supported: Option<Vec<String>>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Vec<String>,
    #[serde(default)]
    authorization_response_iss_parameter_supported: bool,
    id_token_signing_alg_values_supported: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TokenEndpointResponse {
    access_token: String,
    token_type: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(rename = "expires_in")]
    _expires_in: Option<u64>,
    #[serde(rename = "id_token")]
    _id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    iss: String,
    #[serde(rename = "aud")]
    _aud: Audience,
    exp: u64,
    #[serde(rename = "nbf")]
    _nbf: Option<u64>,
    jti: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct RsaJwkSet {
    keys: Vec<RsaJwk>,
}

impl RsaJwkSet {
    fn signing_key(&self, key_id: &str) -> Option<&RsaJwk> {
        self.keys.iter().find(|key| {
            key.kid == key_id
                && key.kty == "RSA"
                && key
                    .public_key_use
                    .as_deref()
                    .is_none_or(|value| value == "sig")
                && key.alg.as_deref().is_none_or(|value| value == "RS256")
                && !key.n.is_empty()
                && !key.e.is_empty()
        })
    }
}

#[derive(Debug, Deserialize)]
struct RsaJwk {
    kty: String,
    kid: String,
    #[serde(rename = "use")]
    public_key_use: Option<String>,
    alg: Option<String>,
    n: String,
    e: String,
}

struct CachedJwks {
    value: RsaJwkSet,
    fetched_at: Instant,
}

fn validate_jwks(jwks: &RsaJwkSet) -> Result<(), OidcInitError> {
    if jwks.keys.is_empty() || jwks.keys.len() > 100 {
        return Err(OidcInitError::InvalidProviderMetadata);
    }
    Ok(())
}

fn validate_provider_url(value: &str, allow_insecure_http: bool) -> Result<Url, OidcInitError> {
    if value.is_empty() || value.len() > MAX_PROVIDER_URL_BYTES {
        return Err(OidcInitError::InvalidConfiguration);
    }
    let url = Url::parse(value).map_err(|_| OidcInitError::InvalidConfiguration)?;
    let secure = url.scheme() == "https" || (allow_insecure_http && url.scheme() == "http");
    if !secure
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(OidcInitError::InvalidConfiguration);
    }
    Ok(url)
}

fn validate_discovered_endpoint(
    value: &str,
    issuer_url: &Url,
    allow_insecure_http: bool,
    require_issuer_origin: bool,
) -> Result<Url, OidcInitError> {
    let url = validate_provider_url(value, allow_insecure_http)
        .map_err(|_| OidcInitError::InvalidProviderMetadata)?;
    if require_issuer_origin && url.origin() != issuer_url.origin() {
        return Err(OidcInitError::InvalidProviderMetadata);
    }
    Ok(url)
}

fn validate_redirect_url(value: &str, allow_insecure_http: bool) -> Result<Url, OidcInitError> {
    let url = validate_provider_url(value, allow_insecure_http)?;
    if url.query().is_some() && url.query().is_some_and(str::is_empty) {
        return Err(OidcInitError::InvalidConfiguration);
    }
    Ok(url)
}

fn valid_scope_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e) && byte != b'\\' && byte != b'"'
        })
}

fn valid_returned_scopes(value: &str, requested: &[String]) -> bool {
    if value.is_empty() || value.len() > MAX_SCOPE_BYTES {
        return false;
    }
    let scopes = value.split_ascii_whitespace().collect::<Vec<_>>();
    !scopes.is_empty()
        && scopes.join(" ") == value
        && scopes.iter().all(|scope| {
            valid_scope_token(scope) && requested.iter().any(|requested| requested == scope)
        })
        && !scopes
            .iter()
            .enumerate()
            .any(|(index, scope)| scopes[..index].contains(scope))
}

fn valid_generated_binding(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_pkce_verifier(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn random_binding() -> Result<String, OidcFlowError> {
    let mut bytes = [0_u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| OidcFlowError::EntropyUnavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn unix_now() -> Result<u64, OidcFlowError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| OidcFlowError::InvalidAuthorizationResponse)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

const fn valid_oauth_error_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

fn token_response_is_json(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

async fn bounded_response_body(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, OidcFlowError> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(OidcFlowError::InvalidTokenResponse);
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| OidcFlowError::ProviderUnavailable)?;
        if body
            .len()
            .checked_add(chunk.len())
            .is_none_or(|length| length > max_bytes)
        {
            return Err(OidcFlowError::InvalidTokenResponse);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn fetch_json<T>(client: &Client, url: Url) -> Result<T, OidcInitError>
where
    T: for<'de> Deserialize<'de>,
{
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| OidcInitError::ProviderUnavailable)?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_METADATA_BYTES as u64)
    {
        return Err(OidcInitError::ProviderUnavailable);
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| OidcInitError::ProviderUnavailable)?;
    if body.len() > MAX_METADATA_BYTES {
        return Err(OidcInitError::ProviderUnavailable);
    }
    serde_json::from_slice(&body).map_err(|_| OidcInitError::InvalidProviderMetadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        body::Bytes,
        http::{HeaderMap, StatusCode, header},
        response::IntoResponse,
        routing::{get, post},
    };
    use copilotmail_application::StoreError;
    use copilotmail_domain::PrincipalId;
    use jsonwebtoken_v2::{EncodingKey, Header, encode};
    use serde::Serialize;

    const TEST_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCsZPjnzKe2zrcg
PD7KlmmzW3ML5P+drAp+aicjJ5Sl3DrMkMlg3cbZbV/Fuvt+FEW0OVFEFNEcwiXk
GJNmjrxSs/hpr54FQP8gxkMIR8inqE0kjgw2PK1GvdUzWs3sMZhnXhNKi21ZxgGy
5qm4M8kKCPRa63/eI1TNezgutJRAjRQJsUpdH165282PBhtBuO/wWOIJBGHvKUjO
7OH3eX+gVcsEMJh7kj3K79fNnrx4CR22GgPapoH/E62EJPwCNabh5MoTf9179d4H
DZrMMlo7+APCMOEEdBFr0AKiV79lZnRZElnp0N3l+/MzyifOoK83uHp30FGhJFIj
X75HxnrBAgMBAAECggEAHVSjlZqYCU4D0af2XfT34GRlf0dTpzGbanUomf5Ut+Bv
n5jdrk2lVJ5bflKJEeMkldxvo2fwSRaOKE/3GiqcR2znzaFrvwSXM64PvxOpg77q
exbw5HRv3bqUEMP5B5/LiIC0wwjUTsMVzGiaxSSIZpzt6B8rtvFnbvtxLF8cXxvp
hmlkO27Z7v5kVu9QTBOR5x6jRZBQI38KIk5OYKnuNxIfubH8VD9KkVaNDGgMrRTW
/e4B4x7N8v7GDRnIub/K8H5n8WjWOaJ5KHBra8s6Ou1yf3wsgV1Qu1FzcnipWuMr
SZf4rbUbH1YE5aPOirZFt6xyCdIDUMkjWN8SYOmcbQKBgQDwGOdITTRmtIpQec46
aoTqq/T3VTD7cFQVwqxLW/X+c+Irs/uV/+bj2lQXmzfqOAq2kv0kXN+L9aQFWMCi
+XTi/HJfxqlMntA9d8DhJ1FbuidwKqVdlFQS1Je/NK14pGCUj7lbEUJI9LmH6Xlj
am+eDhf3BnMXT0E3rEjuhFLopQKBgQC30Bji9ltPlycn4rLtmko6b6ZtGBv7Ix/N
VjR8EUIbuRR6h/y2nuuZ1/h04tz9cc8YCuKbYVCgzkJ3b5cRiHCTfeqSwiQxfgdM
FWteCbkzIDeYiRUf+mlzM+lgH5KquneyHWIyI2Q1es3oXLrSmkayN2SbtFuTzsTD
/Ur1HkuS7QKBgQDZ3t9RHq3kXQP+Duny3GIMyUmbfvTZ1Y6XyHWsVKCe6+1alxI/
hV2xeZdBbJqKsZwXHL8VVimMybSzHVyBBzjoGQh4nL9YRezVOmwOZC1RvVS4VhqF
FmpMRBoHaffYN+FkLh+W4cxyg5AwIs8CRb1PgZ6BjJq3ljWqHW548h6B+QKBgQCK
CA0IMrPzGrDetNBkuN5Q75LsjZRFjOTOz5NuL4+TPfLw6LmPcdu+lX4QWGRXNmWr
ylDNGdDwULN8+SFamzjeqUnDl2KSf+9+5P7RiT+38tONCjQA5YzWY/82nb8H0isc
aBBbUlRoDKYffbMVf6DjJ9G3U0MJxdXgz47Ab1WVKQKBgB+itbt1q/3tNbQlJKvW
rJGR09JbvaRzBOpVgV1HTD1tp191Zob0IAndHLE/1MxlzccwTjZS/rXelq0vgSwT
1DSCzRQjmCuGfrz3gJM0V4TOfgBvIoL8DLPxd86o4BtoydVn7JfMl96hjBNsS5hF
5yXbnZhKMkzxhRxZZ7v2SIP+
-----END PRIVATE KEY-----"#;
    const TEST_MODULUS: &str = "rGT458ynts63IDw-ypZps1tzC-T_nawKfmonIyeUpdw6zJDJYN3G2W1fxbr7fhRFtDlRRBTRHMIl5BiTZo68UrP4aa-eBUD_IMZDCEfIp6hNJI4MNjytRr3VM1rN7DGYZ14TSottWcYBsuapuDPJCgj0Wut_3iNUzXs4LrSUQI0UCbFKXR9eudvNjwYbQbjv8FjiCQRh7ylIzuzh93l_oFXLBDCYe5I9yu_XzZ68eAkdthoD2qaB_xOthCT8AjWm4eTKE3_de_XeBw2azDJaO_gDwjDhBHQRa9ACole_ZWZ0WRJZ6dDd5fvzM8onzqCvN7h6d9BRoSRSI1--R8Z6wQ";

    #[derive(Clone)]
    struct MemoryResolver {
        issuer: String,
        subject: String,
        principal_id: PrincipalId,
    }

    #[async_trait]
    impl ExternalIdentityResolver for MemoryResolver {
        async fn resolve_human_identity(
            &self,
            issuer: &str,
            subject: &str,
        ) -> Result<Option<PrincipalId>, StoreError> {
            Ok((issuer == self.issuer && subject == self.subject).then_some(self.principal_id))
        }
    }

    #[derive(Serialize)]
    struct TestClaims<'a> {
        sub: &'a str,
        iss: &'a str,
        aud: &'a str,
        exp: u64,
        nbf: u64,
        jti: &'a str,
    }

    async fn test_provider() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let discovery = serde_json::json!({
            "issuer": issuer,
            "jwks_uri": format!("{issuer}/jwks"),
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}/token"),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "authorization_response_iss_parameter_supported": true,
            "id_token_signing_alg_values_supported": ["RS256"]
        });
        let jwks = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": "test-key",
                "use": "sig",
                "alg": "RS256",
                "n": TEST_MODULUS,
                "e": "AQAB"
            }]
        });
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || {
                    let discovery = discovery.clone();
                    async move { Json(discovery) }
                }),
            )
            .route(
                "/jwks",
                get(move || {
                    let jwks = jwks.clone();
                    async move { Json(jwks) }
                }),
            );
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (issuer, task)
    }

    fn token(issuer: &str, audience: &str, subject: &str, expires_in: i64) -> String {
        let now = chrono::Utc::now().timestamp();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test-key".into());
        encode(
            &header,
            &TestClaims {
                sub: subject,
                iss: issuer,
                aud: audience,
                exp: u64::try_from(now + expires_in).unwrap(),
                nbf: u64::try_from(now - 1).unwrap(),
                jti: "token-1",
            },
            &EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    #[derive(Clone, Copy)]
    enum TokenBehavior {
        Valid,
        Error,
        Oversized,
        RefreshToken,
        Redirect,
    }

    #[derive(Clone, Copy)]
    enum MetadataVariant {
        Valid,
        IssuerMismatch,
        CrossOriginAuthorization,
        CrossOriginToken,
    }

    async fn relying_party_provider(
        behavior: TokenBehavior,
        metadata_variant: MetadataVariant,
    ) -> (
        String,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let metadata_issuer = match metadata_variant {
            MetadataVariant::IssuerMismatch => format!("{issuer}/unexpected"),
            _ => issuer.clone(),
        };
        let authorization_endpoint = match metadata_variant {
            MetadataVariant::CrossOriginAuthorization => {
                "https://authorization.attacker.invalid/authorize".to_owned()
            }
            _ => format!("{issuer}/authorize"),
        };
        let token_endpoint = match metadata_variant {
            MetadataVariant::CrossOriginToken => "https://token.attacker.invalid/token".to_owned(),
            _ => format!("{issuer}/token"),
        };
        let discovery = serde_json::json!({
            "issuer": metadata_issuer,
            "jwks_uri": format!("{issuer}/jwks"),
            "authorization_endpoint": authorization_endpoint,
            "token_endpoint": token_endpoint,
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "authorization_response_iss_parameter_supported": true,
            "id_token_signing_alg_values_supported": ["RS256"]
        });
        let jwks = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": "test-key",
                "use": "sig",
                "alg": "RS256",
                "n": TEST_MODULUS,
                "e": "AQAB"
            }]
        });
        let access_token = token(&issuer, "copilotmail", "employee-123", 300);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let observed_token = Arc::clone(&observed);
        let observed_redirect = Arc::clone(&observed);
        let redirect_url = format!("{issuer}/redirected-token");
        let redirected_access_token = access_token.clone();
        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(move || {
                    let discovery = discovery.clone();
                    async move { Json(discovery) }
                }),
            )
            .route(
                "/jwks",
                get(move || {
                    let jwks = jwks.clone();
                    async move { Json(jwks) }
                }),
            )
            .route(
                "/token",
                post(move |headers: HeaderMap, body: Bytes| {
                    let observed = Arc::clone(&observed_token);
                    let access_token = access_token.clone();
                    let redirect_url = redirect_url.clone();
                    async move {
                        observed.lock().await.push(serde_json::json!({
                            "path": "token",
                            "content_type": headers
                                .get(header::CONTENT_TYPE)
                                .and_then(|value| value.to_str().ok()),
                            "authorization": headers
                                .get(header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok()),
                            "body": String::from_utf8_lossy(&body),
                        }));
                        match behavior {
                            TokenBehavior::Valid => (
                                StatusCode::OK,
                                [(header::CONTENT_TYPE, "application/json")],
                                serde_json::json!({
                                    "access_token": access_token,
                                    "token_type": "Bearer",
                                    "expires_in": 86_400,
                                    "scope": "openid profile",
                                    "id_token": "not-consumed"
                                })
                                .to_string(),
                            )
                                .into_response(),
                            TokenBehavior::Error => (
                                StatusCode::BAD_REQUEST,
                                [(header::CONTENT_TYPE, "application/json")],
                                r#"{"error":"invalid_grant"}"#.to_owned(),
                            )
                                .into_response(),
                            TokenBehavior::Oversized => (
                                StatusCode::OK,
                                [(header::CONTENT_TYPE, "application/json")],
                                format!(
                                    "{{\"padding\":\"{}\"}}",
                                    "x".repeat(MAX_TOKEN_RESPONSE_BYTES + 1)
                                ),
                            )
                                .into_response(),
                            TokenBehavior::RefreshToken => (
                                StatusCode::OK,
                                [(header::CONTENT_TYPE, "application/json")],
                                serde_json::json!({
                                    "access_token": access_token,
                                    "token_type": "Bearer",
                                    "scope": "openid profile",
                                    "refresh_token": "must-not-cross"
                                })
                                .to_string(),
                            )
                                .into_response(),
                            TokenBehavior::Redirect => {
                                let mut response = StatusCode::FOUND.into_response();
                                response
                                    .headers_mut()
                                    .insert(header::LOCATION, redirect_url.parse().unwrap());
                                response
                            }
                        }
                    }
                }),
            )
            .route(
                "/redirected-token",
                post(move || {
                    let observed = Arc::clone(&observed_redirect);
                    let access_token = redirected_access_token.clone();
                    async move {
                        observed
                            .lock()
                            .await
                            .push(serde_json::json!({"path": "redirected-token"}));
                        (
                            StatusCode::OK,
                            [(header::CONTENT_TYPE, "application/json")],
                            serde_json::json!({
                                "access_token": access_token,
                                "token_type": "Bearer",
                                "scope": "openid profile"
                            })
                            .to_string(),
                        )
                    }
                }),
            );
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (issuer, observed, task)
    }

    async fn test_relying_party(
        behavior: TokenBehavior,
    ) -> (
        OidcRelyingParty<MemoryResolver>,
        String,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let (issuer, observed, provider) =
            relying_party_provider(behavior, MetadataVariant::Valid).await;
        let relying_party = OidcRelyingParty::discover(
            OidcConfig::new(&issuer, "copilotmail", true).unwrap(),
            OidcPublicClientConfig::new(
                "mail-web",
                "http://mail.example.test/oidc/callback",
                ["openid", "profile"],
                true,
            )
            .unwrap(),
            MemoryResolver {
                issuer: issuer.clone(),
                subject: "employee-123".into(),
                principal_id: PrincipalId::new(),
            },
        )
        .await
        .unwrap();
        (relying_party, issuer, observed, provider)
    }

    #[test]
    fn production_issuer_requires_https_and_no_userinfo() {
        assert!(OidcConfig::new("https://login.example.com", "copilotmail", false).is_ok());
        assert!(OidcConfig::new("http://login.example.com", "copilotmail", false).is_err());
        assert!(OidcConfig::new("https://user@login.example.com", "copilotmail", false).is_err());
        assert!(
            OidcConfig::new("https://login.example.com#fragment", "copilotmail", false).is_err()
        );
    }

    #[test]
    fn public_client_requires_openid_without_refresh_scope() {
        assert!(
            OidcPublicClientConfig::new(
                "mail-web",
                "https://mail.example.test/oidc/callback",
                ["openid", "profile", "email"],
                false,
            )
            .is_ok()
        );
        assert!(
            OidcPublicClientConfig::new(
                "mail-web",
                "https://mail.example.test/oidc/callback",
                ["profile"],
                false,
            )
            .is_err()
        );
        assert!(
            OidcPublicClientConfig::new(
                "mail-web",
                "https://mail.example.test/oidc/callback",
                ["openid", "offline_access"],
                false,
            )
            .is_err()
        );
        assert!(
            OidcPublicClientConfig::new(
                "mail-web",
                "https://mail.example.test/oidc/callback#fragment",
                ["openid"],
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn jwks_reject_non_signing_and_wrong_algorithm_keys() {
        let set: RsaJwkSet = serde_json::from_str(
            r#"{"keys":[
                {"kty":"RSA","kid":"enc","use":"enc","alg":"RS256","n":"n","e":"e"},
                {"kty":"RSA","kid":"wrong","use":"sig","alg":"RS512","n":"n","e":"e"},
                {"kty":"RSA","kid":"valid","use":"sig","alg":"RS256","n":"n","e":"e"}
            ]}"#,
        )
        .unwrap();
        assert!(set.signing_key("enc").is_none());
        assert!(set.signing_key("wrong").is_none());
        assert!(set.signing_key("valid").is_some());
    }

    #[tokio::test]
    async fn discovery_and_rs256_validation_map_exact_human_identity() {
        let (issuer, provider) = test_provider().await;
        let principal_id = PrincipalId::new();
        let authenticator = OidcAuthenticator::discover(
            OidcConfig::new(&issuer, "copilotmail", true).unwrap(),
            MemoryResolver {
                issuer: issuer.clone(),
                subject: "employee-123".into(),
                principal_id,
            },
        )
        .await
        .unwrap();

        let identity = authenticator
            .authenticate(&token(&issuer, "copilotmail", "employee-123", 300))
            .await
            .unwrap();
        assert_eq!(identity.principal_id, principal_id);
        assert_eq!(identity.credential_id.as_deref(), Some("token-1"));

        assert_eq!(
            authenticator
                .authenticate(&token(&issuer, "wrong-audience", "employee-123", 300))
                .await
                .unwrap_err(),
            AuthenticationError::InvalidCredential
        );
        assert_eq!(
            authenticator
                .authenticate(&token(&issuer, "copilotmail", "unknown-subject", 300))
                .await
                .unwrap_err(),
            AuthenticationError::InvalidCredential
        );
        assert_eq!(
            authenticator
                .authenticate(&token(&issuer, "copilotmail", "employee-123", -120))
                .await
                .unwrap_err(),
            AuthenticationError::InvalidCredential
        );
        provider.abort();
    }

    #[tokio::test]
    async fn discovery_rejects_issuer_mismatch_and_cross_origin_rp_endpoints() {
        for variant in [
            MetadataVariant::IssuerMismatch,
            MetadataVariant::CrossOriginAuthorization,
            MetadataVariant::CrossOriginToken,
        ] {
            let (issuer, _observed, provider) =
                relying_party_provider(TokenBehavior::Valid, variant).await;
            let result = OidcAuthenticator::discover(
                OidcConfig::new(&issuer, "copilotmail", true).unwrap(),
                MemoryResolver {
                    issuer: issuer.clone(),
                    subject: "employee-123".into(),
                    principal_id: PrincipalId::new(),
                },
            )
            .await;
            assert!(matches!(
                result,
                Err(OidcInitError::InvalidProviderMetadata)
            ));
            provider.abort();
        }
    }

    #[tokio::test]
    async fn authorization_url_and_persisted_transaction_bind_exact_s256_pkce() {
        let (relying_party, _issuer, _observed, provider) =
            test_relying_party(TokenBehavior::Valid).await;
        let request = relying_party.begin_authorization_at(1_000).unwrap();
        let parameters = request
            .authorization_url()
            .query_pairs()
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let verifier = request
            .transaction()
            .code_verifier()
            .expose_secret()
            .to_owned();
        assert_eq!(
            parameters.get("response_type").map(String::as_str),
            Some("code")
        );
        assert_eq!(
            parameters.get("client_id").map(String::as_str),
            Some("mail-web")
        );
        assert_eq!(
            parameters.get("redirect_uri").map(String::as_str),
            Some("http://mail.example.test/oidc/callback")
        );
        assert_eq!(
            parameters.get("scope").map(String::as_str),
            Some("openid profile")
        );
        assert_eq!(
            parameters.get("state").map(String::as_str),
            Some(request.transaction().state())
        );
        assert_eq!(
            parameters.get("code_challenge").map(String::as_str),
            Some(
                URL_SAFE_NO_PAD
                    .encode(Sha256::digest(verifier.as_bytes()))
                    .as_str()
            )
        );
        assert_eq!(
            parameters.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        let serialized_url = request.authorization_url().as_str();
        assert!(!serialized_url.contains(&verifier));
        assert!(!serialized_url.contains("client_secret"));
        assert!(!parameters.contains_key("nonce"));

        let restored = OidcAuthorizationTransaction::from_persisted(
            request.transaction().state(),
            verifier,
            request.transaction().issued_at_unix_seconds(),
            request.transaction().expires_at_unix_seconds(),
        )
        .unwrap();
        assert_eq!(restored.state(), request.transaction().state());
        assert_eq!(restored.expires_at_unix_seconds(), 1_600);
        provider.abort();
    }

    #[tokio::test]
    async fn code_exchange_is_public_client_only_and_returns_strict_verified_expiry() {
        let (relying_party, issuer, observed, provider) =
            test_relying_party(TokenBehavior::Valid).await;
        let request = relying_party.begin_authorization().unwrap();
        let expected_state = request.transaction().state().to_owned();
        let expected_verifier = request
            .transaction()
            .code_verifier()
            .expose_secret()
            .to_owned();
        let result = relying_party
            .complete_authorization(
                request.into_parts().1,
                OidcAuthorizationResponse::success("one-time-code", expected_state, Some(issuer)),
            )
            .await
            .unwrap();
        assert_eq!(
            result.identity().principal_kind,
            AuthenticatedPrincipalKind::Human
        );
        let now = unix_now().unwrap();
        assert!(result.credential_expires_at_unix_seconds() > now);
        assert!(
            result.credential_expires_at_unix_seconds() < now + 600,
            "the trusted expiry must come from the verified JWT exp, not expires_in"
        );
        assert_eq!(result.identity().credential_id.as_deref(), Some("token-1"));

        let calls = observed.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["path"], "token");
        assert_eq!(
            calls[0]["content_type"],
            "application/x-www-form-urlencoded"
        );
        assert!(calls[0]["authorization"].is_null());
        let form = form_urlencoded::parse(calls[0]["body"].as_str().unwrap().as_bytes())
            .map(|(name, value)| (name.into_owned(), value.into_owned()))
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("authorization_code")
        );
        assert_eq!(form.get("code").map(String::as_str), Some("one-time-code"));
        assert_eq!(form.get("client_id").map(String::as_str), Some("mail-web"));
        assert_eq!(
            form.get("redirect_uri").map(String::as_str),
            Some("http://mail.example.test/oidc/callback")
        );
        assert_eq!(
            form.get("code_verifier").map(String::as_str),
            Some(expected_verifier.as_str())
        );
        assert!(!form.contains_key("client_secret"));
        drop(calls);
        provider.abort();
    }

    #[tokio::test]
    async fn callback_state_and_issuer_fail_before_token_endpoint_access() {
        let (relying_party, issuer, observed, provider) =
            test_relying_party(TokenBehavior::Valid).await;
        let request = relying_party.begin_authorization().unwrap();
        let error = relying_party
            .complete_authorization(
                request.into_parts().1,
                OidcAuthorizationResponse::success(
                    "one-time-code",
                    URL_SAFE_NO_PAD.encode([7_u8; 32]),
                    Some(issuer),
                ),
            )
            .await
            .unwrap_err();
        assert_eq!(error, OidcFlowError::InvalidAuthorizationResponse);
        assert!(observed.lock().await.is_empty());
        provider.abort();

        let (relying_party, _issuer, observed, provider) =
            test_relying_party(TokenBehavior::Valid).await;
        let request = relying_party.begin_authorization().unwrap();
        let state = request.transaction().state().to_owned();
        let error = relying_party
            .complete_authorization(
                request.into_parts().1,
                OidcAuthorizationResponse::success(
                    "one-time-code",
                    state,
                    Some("https://issuer.attacker.invalid".into()),
                ),
            )
            .await
            .unwrap_err();
        assert_eq!(error, OidcFlowError::InvalidAuthorizationResponse);
        assert!(observed.lock().await.is_empty());
        provider.abort();
    }

    async fn complete_with_behavior(
        behavior: TokenBehavior,
    ) -> (
        OidcFlowError,
        Arc<Mutex<Vec<serde_json::Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let (relying_party, issuer, observed, provider) = test_relying_party(behavior).await;
        let request = relying_party.begin_authorization().unwrap();
        let state = request.transaction().state().to_owned();
        let error = relying_party
            .complete_authorization(
                request.into_parts().1,
                OidcAuthorizationResponse::success("one-time-code", state, Some(issuer)),
            )
            .await
            .unwrap_err();
        (error, observed, provider)
    }

    #[tokio::test]
    async fn token_endpoint_errors_refresh_tokens_and_oversized_bodies_fail_closed() {
        let (error, _observed, provider) = complete_with_behavior(TokenBehavior::Error).await;
        assert_eq!(error, OidcFlowError::TokenEndpointRejected);
        provider.abort();

        let (error, _observed, provider) =
            complete_with_behavior(TokenBehavior::RefreshToken).await;
        assert_eq!(error, OidcFlowError::InvalidTokenResponse);
        provider.abort();

        let (error, _observed, provider) = complete_with_behavior(TokenBehavior::Oversized).await;
        assert_eq!(error, OidcFlowError::InvalidTokenResponse);
        provider.abort();
    }

    #[tokio::test]
    async fn token_endpoint_redirects_are_not_followed() {
        let (error, observed, provider) = complete_with_behavior(TokenBehavior::Redirect).await;
        assert_eq!(error, OidcFlowError::TokenEndpointRejected);
        let calls = observed.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["path"], "token");
        drop(calls);
        provider.abort();
    }

    #[test]
    fn authorization_callback_ignores_unmodeled_identity_provider_parameters() {
        use axum::extract::Query;
        // Identity providers append parameters such as `session_state`. The
        // callback must keep ignoring them; closing the query would break login.
        let uri: axum::http::Uri =
            "https://mail.example.test/callback?code=abc&state=def&session_state=extra"
                .parse()
                .unwrap();
        assert!(Query::<super::OidcAuthorizationResponse>::try_from_uri(&uri).is_ok());
    }
}
