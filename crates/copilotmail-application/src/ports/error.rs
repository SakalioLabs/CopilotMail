use super::*;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The authenticated request expired or was revoked before acceptance.
    #[error("request authentication is no longer valid")]
    AuthenticationInvalid,
    /// A unique or check constraint rejected the requested state.
    #[error("requested state conflicts with existing data")]
    Conflict,
    /// A referenced organization, principal, or resource does not exist.
    #[error("referenced resource does not exist")]
    MissingReference,
    /// An Agent message was rejected by one stable outbound safety rule.
    #[error("Agent mail admission was denied")]
    AgentMailAdmission(AgentMailAdmissionFailure),
    /// Infrastructure failed without exposing credentials or database details.
    #[error("control-plane persistence failed")]
    Unavailable,
}
