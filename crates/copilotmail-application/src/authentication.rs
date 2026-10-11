use async_trait::async_trait;
use copilotmail_domain::{AuthenticationBinding, Permission, PrincipalId};
use std::collections::BTreeSet;

/// Credential family established by the cryptographic adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticatedPrincipalKind {
    Human,
    Agent,
}

/// Identity established only after cryptographic token verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedIdentity {
    /// Internal principal mapped from the token subject or client identity.
    pub principal_id: PrincipalId,
    /// Human OIDC or machine OAuth/mTLS identity class.
    pub principal_kind: AuthenticatedPrincipalKind,
    /// Non-secret token or credential identifier used for audit correlation.
    pub credential_id: Option<String>,
    /// Optional machine-token ceiling applied in addition to live RBAC.
    pub scope_restriction: Option<BTreeSet<Permission>>,
    /// Verified credential generation/lifetime, propagated to write commits.
    pub authentication: AuthenticationBinding,
}

/// Cryptographically established transport proof available to an
/// authentication adapter. Values must originate from the native transport,
/// never from client-controlled HTTP headers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthenticationContext {
    /// Lowercase SHA-256 digest of the TLS-verified leaf certificate DER.
    pub client_certificate_fingerprint: Option<String>,
}

/// Safe authentication failure classification for transport adapters.
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum AuthenticationError {
    /// The credential is malformed, untrusted, revoked, or otherwise invalid.
    #[error("credential is invalid")]
    InvalidCredential,
    /// Authentication infrastructure is unavailable or not configured.
    #[error("authentication service is unavailable")]
    Unavailable,
}

/// Adapter boundary shared by OIDC, Agent OAuth2, mTLS, and workload identity.
#[async_trait]
pub trait AccessTokenAuthenticator: Send + Sync {
    /// Verifies a bearer token and maps it to an internal principal.
    async fn authenticate(
        &self,
        bearer_token: &str,
    ) -> Result<AuthenticatedIdentity, AuthenticationError>;

    /// Verifies a bearer token together with transport-level proof. Human OIDC
    /// adapters use the default token-only behavior; certificate-bound Agent
    /// tokens override this method.
    async fn authenticate_with_context(
        &self,
        bearer_token: &str,
        _context: &AuthenticationContext,
    ) -> Result<AuthenticatedIdentity, AuthenticationError> {
        self.authenticate(bearer_token).await
    }
}
