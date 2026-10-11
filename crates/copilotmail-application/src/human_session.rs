use crate::{
    ControlPlaneStore, HumanLoginAdmissionResult, HumanLoginTransactionRecord,
    HumanLoginTransactionStore, HumanLoginTransactionWrite, HumanSessionClient,
    HumanSessionQueryStore, HumanSessionRecord, HumanSessionRevocationCause, HumanSessionWrite,
    PendingEvent, StoreError,
};
use chrono::{DateTime, Duration, Utc};
use copilotmail_domain::{
    AuditActor, AuditContext, AuditEvent, AuditOutcome, AuditResource, HumanLoginTransactionId,
    HumanSessionId, HumanSessionRevocationId, OrganizationId, PrincipalId,
};
use serde_json::json;
use sha2::{Digest, Sha256};

const LOGIN_LIFETIME: Duration = Duration::minutes(10);
const SESSION_MAX_LIFETIME: Duration = Duration::hours(8);
const MIN_BROWSER_SECRET_BYTES: usize = 32;
const MIN_SESSION_TOKEN_BYTES: usize = 43;
const MAX_BROWSER_SECRET_BYTES: usize = 512;
const MAX_OIDC_TOKEN_BYTES: usize = 16 * 1024;
const MIN_ENCRYPTED_VERIFIER_BYTES: usize = 32;
const MAX_ENCRYPTED_VERIFIER_BYTES: usize = 4 * 1024;
const MAX_RETURN_TO_BYTES: usize = 1024;
const MAX_OIDC_JTI_BYTES: usize = 200;
const MAX_LOGIN_ADMISSION_LIMIT: u32 = 100_000;
const MAX_LOGIN_CLEANUP_LIMIT: u32 = 1_000;

/// Starts an unauthenticated OIDC login transaction.
///
/// Raw state and browser nonce values exist only in this command. They are
/// immediately reduced to SHA-256 digests before crossing a persistence port.
/// The PKCE verifier must already be encrypted by the trusted OIDC boundary.
pub struct BeginHumanLoginCommand {
    pub client_key: HumanSessionClient,
    pub return_to: String,
    pub state: String,
    pub browser_nonce: String,
    pub encrypted_pkce_verifier: Vec<u8>,
    /// Cluster-wide maximum admitted attempts for this application client in
    /// the database-owned one-minute window.
    pub admission_limit: u32,
    /// Maximum expired, session-unbound login transactions pruned before this
    /// login is inserted.
    pub cleanup_limit: u32,
    pub now: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Consumes a login transaction after the IdP redirects the browser back.
///
/// Both raw values are required so a stolen callback state alone is
/// insufficient to retrieve the encrypted PKCE verifier.
pub struct ConsumeHumanLoginCommand {
    pub state: String,
    pub browser_nonce: String,
    pub now: DateTime<Utc>,
}

/// Creates a local Human session after OIDC token validation succeeds.
///
/// Both raw credentials exist only in this command. Persistence receives only
/// their independent SHA-256 digests.
pub struct CreateHumanSessionCommand {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub login_transaction_id: HumanLoginTransactionId,
    pub client_key: HumanSessionClient,
    pub session_token: String,
    pub oidc_access_token: String,
    pub oidc_jti: Option<String>,
    pub upstream_expires_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Resolves one opaque cookie value into a currently active Human session.
pub struct ResolveHumanSessionCommand {
    pub session_token: String,
    pub now: DateTime<Utc>,
}

/// Revokes one opaque cookie value without accepting a client-selected session
/// or Principal identifier.
pub struct RevokeHumanSessionCommand {
    pub session_token: String,
    pub cause: HumanSessionRevocationCause,
    pub trace_id: String,
    pub now: DateTime<Utc>,
}

/// Alias emphasizing that the encrypted verifier is returned only after a
/// successful one-time consumption.
pub type ConsumedHumanLogin = HumanLoginTransactionRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevokedHumanSession {
    pub session: HumanSessionRecord,
    pub cause: HumanSessionRevocationCause,
    pub revoked_at: DateTime<Utc>,
    pub newly_revoked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HumanSessionError {
    #[error("Human login or session input is invalid")]
    InvalidRequest,
    #[error("Human login transaction or session is unavailable")]
    NotFound,
    #[error("Human login admission is rate limited; retry after {retry_after_seconds} seconds")]
    RateLimited { retry_after_seconds: u32 },
    #[error("Human login or session state conflicts with existing evidence")]
    Conflict,
    #[error("Human session persistence is unavailable")]
    Unavailable,
}

/// Server-owned Human OIDC login and browser-session orchestration.
///
/// The pre-authentication login transaction intentionally has no tenant actor.
/// Once OIDC establishes a Human Principal, session creation and revocation
/// use the regular control-plane transaction so state, Audit, and Outbox
/// evidence commit atomically.
pub struct HumanSessionService<S> {
    store: S,
}

impl<S> HumanSessionService<S>
where
    S: ControlPlaneStore + HumanLoginTransactionStore + HumanSessionQueryStore,
{
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    pub async fn begin_login(
        &self,
        command: BeginHumanLoginCommand,
    ) -> Result<HumanLoginTransactionRecord, HumanSessionError> {
        if !valid_browser_secret(&command.state, MIN_BROWSER_SECRET_BYTES)
            || !valid_browser_secret(&command.browser_nonce, MIN_BROWSER_SECRET_BYTES)
            || !valid_return_to(&command.return_to)
            || !(MIN_ENCRYPTED_VERIFIER_BYTES..=MAX_ENCRYPTED_VERIFIER_BYTES)
                .contains(&command.encrypted_pkce_verifier.len())
            || !(1..=MAX_LOGIN_ADMISSION_LIMIT).contains(&command.admission_limit)
            || !(1..=MAX_LOGIN_CLEANUP_LIMIT).contains(&command.cleanup_limit)
            || command.expires_at <= command.now
            || command.expires_at > command.now + LOGIN_LIFETIME
        {
            return Err(HumanSessionError::InvalidRequest);
        }

        let record = HumanLoginTransactionRecord {
            login_transaction_id: HumanLoginTransactionId::new(),
            client_key: command.client_key,
            return_to: command.return_to,
            encrypted_pkce_verifier: command.encrypted_pkce_verifier,
            created_at: command.now,
            expires_at: command.expires_at,
        };
        let write = HumanLoginTransactionWrite {
            login_transaction_id: record.login_transaction_id,
            client_key: record.client_key,
            return_to: record.return_to.clone(),
            state_sha256: sha256(command.state.as_bytes()),
            browser_nonce_sha256: sha256(command.browser_nonce.as_bytes()),
            encrypted_pkce_verifier: record.encrypted_pkce_verifier.clone(),
            created_at: record.created_at,
            expires_at: record.expires_at,
        };
        match self
            .store
            .admit_human_login_transaction(&write, command.admission_limit, command.cleanup_limit)
            .await
            .map_err(map_store_error)?
        {
            HumanLoginAdmissionResult::Admitted => Ok(record),
            HumanLoginAdmissionResult::RateLimited {
                retry_after_seconds,
            } => Err(HumanSessionError::RateLimited {
                retry_after_seconds,
            }),
        }
    }

    pub async fn consume_login(
        &self,
        command: ConsumeHumanLoginCommand,
    ) -> Result<ConsumedHumanLogin, HumanSessionError> {
        if !valid_browser_secret(&command.state, MIN_BROWSER_SECRET_BYTES)
            || !valid_browser_secret(&command.browser_nonce, MIN_BROWSER_SECRET_BYTES)
        {
            return Err(HumanSessionError::NotFound);
        }
        self.store
            .consume_human_login_transaction(
                &sha256(command.state.as_bytes()),
                &sha256(command.browser_nonce.as_bytes()),
                command.now,
            )
            .await
            .map_err(map_store_error)?
            .ok_or(HumanSessionError::NotFound)
    }

    pub async fn create_session(
        &self,
        command: CreateHumanSessionCommand,
    ) -> Result<HumanSessionRecord, HumanSessionError> {
        if !valid_browser_secret(&command.session_token, MIN_SESSION_TOKEN_BYTES)
            || !valid_oidc_access_token(&command.oidc_access_token)
            || command.oidc_jti.as_deref().is_some_and(|jti| {
                jti.is_empty()
                    || jti.len() > MAX_OIDC_JTI_BYTES
                    || jti != jti.trim()
                    || jti.chars().any(char::is_control)
            })
            || command.upstream_expires_at <= command.now
            || command.expires_at <= command.now
            || command.expires_at > command.upstream_expires_at
            || command.expires_at > command.now + SESSION_MAX_LIFETIME
        {
            return Err(HumanSessionError::InvalidRequest);
        }

        let write = HumanSessionWrite {
            session_id: HumanSessionId::new(),
            organization_id: command.organization_id,
            principal_id: command.principal_id,
            login_transaction_id: command.login_transaction_id,
            client_key: command.client_key,
            token_sha256: sha256(command.session_token.as_bytes()),
            oidc_token_sha256: sha256(command.oidc_access_token.as_bytes()),
            oidc_jti: command.oidc_jti,
            upstream_expires_at: command.upstream_expires_at,
            created_at: command.now,
            expires_at: command.expires_at,
        };
        let record = session_record(&write);

        let mut transaction = self.store.begin().await.map_err(map_store_error)?;
        if !transaction
            .active_human_principal_exists(command.organization_id, command.principal_id)
            .await
            .map_err(map_store_error)?
        {
            return Err(HumanSessionError::NotFound);
        }
        transaction
            .insert_human_session(&write)
            .await
            .map_err(map_store_error)?;

        let mut audit_context =
            AuditContext::for_request(command.trace_id.clone()).map_err(map_domain_error)?;
        audit_context.subject_principal_id = Some(command.principal_id);
        audit_context.session_id = Some(record.session_id.to_string());
        audit_context.token_jti = record.oidc_jti.clone();
        let audit = AuditEvent::new(
            command.organization_id,
            AuditActor {
                principal_id: command.principal_id,
                on_behalf_of: None,
                credential_id: None,
            },
            "identity.human_session.created",
            AuditResource {
                kind: "identity.human_session".into(),
                id: record.session_id.to_string(),
            },
            AuditOutcome::Success,
            command.trace_id,
            json!({
                "client_key": record.client_key.as_str(),
                "expires_at": record.expires_at,
                "upstream_expires_at": record.upstream_expires_at,
            }),
            command.now,
        )
        .and_then(|event| event.with_context(audit_context))
        .map_err(map_domain_error)?;
        let event = PendingEvent::new(
            "identity.human_session.created.v1",
            "identity.human_session",
            record.session_id.to_string(),
            json!({
                "organization_id": record.organization_id,
                "principal_id": record.principal_id,
                "session_id": record.session_id,
                "client_key": record.client_key.as_str(),
                "expires_at": record.expires_at,
            }),
            command.now,
        );
        transaction
            .append_audit_event(&audit)
            .await
            .map_err(map_store_error)?;
        transaction
            .enqueue_event(&event)
            .await
            .map_err(map_store_error)?;
        transaction.commit().await.map_err(map_store_error)?;
        Ok(record)
    }

    pub async fn resolve_session(
        &self,
        command: ResolveHumanSessionCommand,
    ) -> Result<HumanSessionRecord, HumanSessionError> {
        if !valid_browser_secret(&command.session_token, MIN_SESSION_TOKEN_BYTES) {
            return Err(HumanSessionError::NotFound);
        }
        self.store
            .resolve_human_session_by_token_sha256(
                &sha256(command.session_token.as_bytes()),
                command.now,
            )
            .await
            .map_err(map_store_error)?
            .ok_or(HumanSessionError::NotFound)
    }

    pub async fn revoke_session(
        &self,
        command: RevokeHumanSessionCommand,
    ) -> Result<RevokedHumanSession, HumanSessionError> {
        if !valid_browser_secret(&command.session_token, MIN_SESSION_TOKEN_BYTES) {
            return Err(HumanSessionError::NotFound);
        }
        let token_sha256 = sha256(command.session_token.as_bytes());
        let mut transaction = self.store.begin().await.map_err(map_store_error)?;
        let result = transaction
            .revoke_human_session_by_token_sha256(
                &token_sha256,
                HumanSessionRevocationId::new(),
                command.cause,
                command.now,
            )
            .await
            .map_err(map_store_error)?
            .ok_or(HumanSessionError::NotFound)?;

        if result.newly_revoked {
            let mut audit_context =
                AuditContext::for_request(command.trace_id.clone()).map_err(map_domain_error)?;
            audit_context.subject_principal_id = Some(result.session.principal_id);
            audit_context.session_id = Some(result.session.session_id.to_string());
            audit_context.token_jti = result.session.oidc_jti.clone();
            let audit = AuditEvent::new(
                result.session.organization_id,
                AuditActor {
                    principal_id: result.session.principal_id,
                    on_behalf_of: None,
                    credential_id: None,
                },
                "identity.human_session.revoked",
                AuditResource {
                    kind: "identity.human_session".into(),
                    id: result.session.session_id.to_string(),
                },
                AuditOutcome::Success,
                command.trace_id,
                json!({
                    "cause": result.cause.as_str(),
                    "client_key": result.session.client_key.as_str(),
                    "revoked_at": result.revoked_at,
                }),
                command.now,
            )
            .and_then(|event| event.with_context(audit_context))
            .map_err(map_domain_error)?;
            let event = PendingEvent::new(
                "identity.human_session.revoked.v1",
                "identity.human_session",
                result.session.session_id.to_string(),
                json!({
                    "organization_id": result.session.organization_id,
                    "principal_id": result.session.principal_id,
                    "session_id": result.session.session_id,
                    "client_key": result.session.client_key.as_str(),
                    "cause": result.cause.as_str(),
                }),
                command.now,
            );
            transaction
                .append_audit_event(&audit)
                .await
                .map_err(map_store_error)?;
            transaction
                .enqueue_event(&event)
                .await
                .map_err(map_store_error)?;
        }
        transaction.commit().await.map_err(map_store_error)?;
        Ok(RevokedHumanSession {
            session: result.session,
            cause: result.cause,
            revoked_at: result.revoked_at,
            newly_revoked: result.newly_revoked,
        })
    }
}

fn session_record(write: &HumanSessionWrite) -> HumanSessionRecord {
    HumanSessionRecord {
        session_id: write.session_id,
        organization_id: write.organization_id,
        principal_id: write.principal_id,
        login_transaction_id: write.login_transaction_id,
        client_key: write.client_key,
        oidc_jti: write.oidc_jti.clone(),
        upstream_expires_at: write.upstream_expires_at,
        created_at: write.created_at,
        expires_at: write.expires_at,
    }
}

fn sha256(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

fn valid_browser_secret(value: &str, minimum: usize) -> bool {
    (minimum..=MAX_BROWSER_SECRET_BYTES).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_oidc_access_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_OIDC_TOKEN_BYTES
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

fn valid_return_to(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_RETURN_TO_BYTES
        && value == value.trim()
        && value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
}

const fn map_store_error(error: StoreError) -> HumanSessionError {
    match error {
        StoreError::Conflict => HumanSessionError::Conflict,
        StoreError::MissingReference | StoreError::AgentMailAdmission(_) => {
            HumanSessionError::NotFound
        }
        StoreError::AuthenticationInvalid | StoreError::Unavailable => {
            HumanSessionError::Unavailable
        }
    }
}

const fn map_domain_error(_: copilotmail_domain::DomainError) -> HumanSessionError {
    HumanSessionError::InvalidRequest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ControlPlaneTransaction, ExistingOrganizationBootstrap, HumanSessionRevocationResult,
    };
    use async_trait::async_trait;
    use copilotmail_domain::{Organization, OrganizationSlug, Principal, Role, RoleAssignment};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryState {
        login: Option<HumanLoginTransactionWrite>,
        login_consumed: bool,
        session: Option<HumanSessionWrite>,
        revocation: Option<HumanSessionRevocationResult>,
        audits: Vec<AuditEvent>,
        events: Vec<PendingEvent>,
        writes: Vec<&'static str>,
        active_human: bool,
        admitted_attempts: u32,
    }

    #[derive(Clone, Default)]
    struct MemoryStore {
        state: Arc<Mutex<MemoryState>>,
    }

    struct MemoryTransaction {
        state: Arc<Mutex<MemoryState>>,
    }

    impl MemoryStore {
        fn with_active_human() -> Self {
            let store = Self::default();
            store.state.lock().unwrap().active_human = true;
            store
        }
    }

    #[async_trait]
    impl HumanLoginTransactionStore for MemoryStore {
        async fn admit_human_login_transaction(
            &self,
            write: &HumanLoginTransactionWrite,
            admission_limit: u32,
            _: u32,
        ) -> Result<HumanLoginAdmissionResult, StoreError> {
            let mut state = self.state.lock().unwrap();
            if state.admitted_attempts >= admission_limit {
                return Ok(HumanLoginAdmissionResult::RateLimited {
                    retry_after_seconds: 60,
                });
            }
            state.admitted_attempts += 1;
            if state.login.is_some() {
                return Err(StoreError::Conflict);
            }
            state.login = Some(write.clone());
            Ok(HumanLoginAdmissionResult::Admitted)
        }

        async fn consume_human_login_transaction(
            &self,
            state_sha256: &[u8; 32],
            browser_nonce_sha256: &[u8; 32],
            consumed_at: DateTime<Utc>,
        ) -> Result<Option<HumanLoginTransactionRecord>, StoreError> {
            let mut state = self.state.lock().unwrap();
            if state.login_consumed {
                return Ok(None);
            }
            let Some(login) = state.login.as_ref() else {
                return Ok(None);
            };
            if login.state_sha256 != *state_sha256
                || login.browser_nonce_sha256 != *browser_nonce_sha256
                || login.expires_at <= consumed_at
            {
                return Ok(None);
            }
            let record = HumanLoginTransactionRecord {
                login_transaction_id: login.login_transaction_id,
                client_key: login.client_key,
                return_to: login.return_to.clone(),
                encrypted_pkce_verifier: login.encrypted_pkce_verifier.clone(),
                created_at: login.created_at,
                expires_at: login.expires_at,
            };
            state.login_consumed = true;
            Ok(Some(record))
        }
    }

    #[async_trait]
    impl HumanSessionQueryStore for MemoryStore {
        async fn resolve_human_session_by_token_sha256(
            &self,
            token_sha256: &[u8; 32],
            now: DateTime<Utc>,
        ) -> Result<Option<HumanSessionRecord>, StoreError> {
            let state = self.state.lock().unwrap();
            let Some(session) = state.session.as_ref() else {
                return Ok(None);
            };
            if session.token_sha256 != *token_sha256
                || session.expires_at <= now
                || session.upstream_expires_at <= now
                || state.revocation.is_some()
                || !state.active_human
            {
                return Ok(None);
            }
            Ok(Some(session_record(session)))
        }
    }

    #[async_trait]
    impl ControlPlaneStore for MemoryStore {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Ok(Box::new(MemoryTransaction {
                state: self.state.clone(),
            }))
        }
    }

    #[async_trait]
    impl ControlPlaneTransaction for MemoryTransaction {
        async fn find_organization_bootstrap(
            &mut self,
            _: &OrganizationSlug,
        ) -> Result<Option<ExistingOrganizationBootstrap>, StoreError> {
            Ok(None)
        }

        async fn insert_organization(&mut self, _: &Organization) -> Result<(), StoreError> {
            Ok(())
        }

        async fn insert_principal(&mut self, _: &Principal) -> Result<(), StoreError> {
            Ok(())
        }

        async fn insert_role(&mut self, _: &Role) -> Result<(), StoreError> {
            Ok(())
        }

        async fn insert_role_assignment(&mut self, _: &RoleAssignment) -> Result<(), StoreError> {
            Ok(())
        }

        async fn record_organization_bootstrap(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
            _: DateTime<Utc>,
        ) -> Result<(), StoreError> {
            Ok(())
        }

        async fn active_human_principal_exists(
            &mut self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<bool, StoreError> {
            Ok(self.state.lock().unwrap().active_human)
        }

        async fn insert_human_session(
            &mut self,
            write: &HumanSessionWrite,
        ) -> Result<(), StoreError> {
            let mut state = self.state.lock().unwrap();
            let Some(login) = state.login.as_ref() else {
                return Err(StoreError::MissingReference);
            };
            if !state.login_consumed
                || login.login_transaction_id != write.login_transaction_id
                || login.client_key != write.client_key
            {
                return Err(StoreError::MissingReference);
            }
            state.session = Some(write.clone());
            state.writes.push("session");
            Ok(())
        }

        async fn revoke_human_session_by_token_sha256(
            &mut self,
            token_sha256: &[u8; 32],
            revocation_id: HumanSessionRevocationId,
            cause: HumanSessionRevocationCause,
            revoked_at: DateTime<Utc>,
        ) -> Result<Option<HumanSessionRevocationResult>, StoreError> {
            let mut state = self.state.lock().unwrap();
            let Some(session) = state.session.as_ref() else {
                return Ok(None);
            };
            if session.token_sha256 != *token_sha256 {
                return Ok(None);
            }
            let session = session_record(session);
            state.writes.push("revocation");
            if let Some(existing) = state.revocation.as_ref() {
                return Ok(Some(HumanSessionRevocationResult {
                    session,
                    revocation_id: existing.revocation_id,
                    cause: existing.cause,
                    revoked_at: existing.revoked_at,
                    newly_revoked: false,
                }));
            }
            let result = HumanSessionRevocationResult {
                session,
                revocation_id,
                cause,
                revoked_at,
                newly_revoked: true,
            };
            state.revocation = Some(result.clone());
            Ok(Some(result))
        }

        async fn append_audit_event(&mut self, event: &AuditEvent) -> Result<(), StoreError> {
            let mut state = self.state.lock().unwrap();
            state.audits.push(event.clone());
            state.writes.push("audit");
            Ok(())
        }

        async fn enqueue_event(&mut self, event: &PendingEvent) -> Result<(), StoreError> {
            let mut state = self.state.lock().unwrap();
            state.events.push(event.clone());
            state.writes.push("outbox");
            Ok(())
        }

        async fn commit(self: Box<Self>) -> Result<(), StoreError> {
            self.state.lock().unwrap().writes.push("commit");
            Ok(())
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-27T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn consumed_login(
        service: &HumanSessionService<MemoryStore>,
        state: &str,
        nonce: &str,
        now: DateTime<Utc>,
    ) -> HumanLoginTransactionRecord {
        service
            .begin_login(BeginHumanLoginCommand {
                client_key: HumanSessionClient::Admin,
                return_to: "/audit?tab=events".into(),
                state: state.into(),
                browser_nonce: nonce.into(),
                encrypted_pkce_verifier: vec![0x5a; 64],
                admission_limit: 100,
                cleanup_limit: 10,
                now,
                expires_at: now + Duration::minutes(5),
            })
            .await
            .unwrap();
        service
            .consume_login(ConsumeHumanLoginCommand {
                state: state.into(),
                browser_nonce: nonce.into(),
                now: now + Duration::minutes(1),
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn login_persists_only_digests_and_consumes_exactly_once() {
        let store = MemoryStore::default();
        let service = HumanSessionService::new(store.clone());
        let now = now();
        let raw_state = "s".repeat(48);
        let raw_nonce = "n".repeat(48);

        let consumed = consumed_login(&service, &raw_state, &raw_nonce, now).await;
        let persisted = store.state.lock().unwrap().login.clone().unwrap();
        assert_eq!(persisted.state_sha256, sha256(raw_state.as_bytes()));
        assert_eq!(persisted.browser_nonce_sha256, sha256(raw_nonce.as_bytes()));
        assert_eq!(consumed.encrypted_pkce_verifier, vec![0x5a; 64]);
        assert_eq!(consumed.return_to, "/audit?tab=events");

        let replay = service
            .consume_login(ConsumeHumanLoginCommand {
                state: raw_state,
                browser_nonce: raw_nonce,
                now: now + Duration::minutes(2),
            })
            .await;
        assert_eq!(replay, Err(HumanSessionError::NotFound));
    }

    #[tokio::test]
    async fn rate_limited_login_does_not_replace_or_append_transaction_evidence() {
        let store = MemoryStore::default();
        let service = HumanSessionService::new(store.clone());
        let now = now();
        let first = service
            .begin_login(BeginHumanLoginCommand {
                client_key: HumanSessionClient::Admin,
                return_to: "/".into(),
                state: "s".repeat(48),
                browser_nonce: "n".repeat(48),
                encrypted_pkce_verifier: vec![0x5a; 64],
                admission_limit: 1,
                cleanup_limit: 10,
                now,
                expires_at: now + Duration::minutes(5),
            })
            .await
            .unwrap();
        let rejected = service
            .begin_login(BeginHumanLoginCommand {
                client_key: HumanSessionClient::Admin,
                return_to: "/".into(),
                state: "x".repeat(48),
                browser_nonce: "y".repeat(48),
                encrypted_pkce_verifier: vec![0x6b; 64],
                admission_limit: 1,
                cleanup_limit: 10,
                now,
                expires_at: now + Duration::minutes(5),
            })
            .await;

        assert_eq!(
            rejected,
            Err(HumanSessionError::RateLimited {
                retry_after_seconds: 60
            })
        );
        let state = store.state.lock().unwrap();
        assert_eq!(state.admitted_attempts, 1);
        assert_eq!(
            state.login.as_ref().unwrap().login_transaction_id,
            first.login_transaction_id
        );
    }

    #[tokio::test]
    async fn invalid_admission_bounds_are_rejected_before_store_access() {
        let store = MemoryStore::default();
        let service = HumanSessionService::new(store.clone());
        let now = now();
        let result = service
            .begin_login(BeginHumanLoginCommand {
                client_key: HumanSessionClient::Mail,
                return_to: "/".into(),
                state: "s".repeat(48),
                browser_nonce: "n".repeat(48),
                encrypted_pkce_verifier: vec![0x5a; 64],
                admission_limit: 0,
                cleanup_limit: MAX_LOGIN_CLEANUP_LIMIT + 1,
                now,
                expires_at: now + Duration::minutes(5),
            })
            .await;

        assert_eq!(result, Err(HumanSessionError::InvalidRequest));
        let state = store.state.lock().unwrap();
        assert_eq!(state.admitted_attempts, 0);
        assert!(state.login.is_none());
    }

    #[tokio::test]
    async fn session_creation_and_revocation_write_sanitized_evidence_once() {
        let store = MemoryStore::with_active_human();
        let service = HumanSessionService::new(store.clone());
        let now = now();
        let login = consumed_login(&service, &"s".repeat(48), &"n".repeat(48), now).await;
        let organization_id = OrganizationId::new();
        let principal_id = PrincipalId::new();
        let raw_session_token = "t".repeat(64);
        let raw_oidc_token = "header.payload.signature".to_owned();

        let created = service
            .create_session(CreateHumanSessionCommand {
                organization_id,
                principal_id,
                login_transaction_id: login.login_transaction_id,
                client_key: login.client_key,
                session_token: raw_session_token.clone(),
                oidc_access_token: raw_oidc_token.clone(),
                oidc_jti: None,
                upstream_expires_at: now + Duration::hours(2),
                expires_at: now + Duration::hours(1),
                trace_id: "trace-session-create".into(),
                now,
            })
            .await
            .unwrap();

        let persisted = store.state.lock().unwrap().session.clone().unwrap();
        assert_eq!(persisted.token_sha256, sha256(raw_session_token.as_bytes()));
        assert_eq!(
            persisted.oidc_token_sha256,
            sha256(raw_oidc_token.as_bytes())
        );
        assert_eq!(created.oidc_jti, None);
        assert_eq!(
            service
                .resolve_session(ResolveHumanSessionCommand {
                    session_token: raw_session_token.clone(),
                    now: now + Duration::minutes(1),
                })
                .await
                .unwrap(),
            created
        );

        let revoked = service
            .revoke_session(RevokeHumanSessionCommand {
                session_token: raw_session_token.clone(),
                cause: HumanSessionRevocationCause::Logout,
                trace_id: "trace-session-revoke".into(),
                now: now + Duration::minutes(2),
            })
            .await
            .unwrap();
        assert!(revoked.newly_revoked);
        assert_eq!(
            service
                .resolve_session(ResolveHumanSessionCommand {
                    session_token: raw_session_token.clone(),
                    now: now + Duration::minutes(3),
                })
                .await,
            Err(HumanSessionError::NotFound)
        );

        let replay = service
            .revoke_session(RevokeHumanSessionCommand {
                session_token: raw_session_token.clone(),
                cause: HumanSessionRevocationCause::SecurityResponse,
                trace_id: "trace-session-revoke-replay".into(),
                now: now + Duration::minutes(4),
            })
            .await
            .unwrap();
        assert!(!replay.newly_revoked);
        assert_eq!(replay.cause, HumanSessionRevocationCause::Logout);

        let state = store.state.lock().unwrap();
        assert_eq!(
            state
                .audits
                .iter()
                .map(|audit| audit.action.as_str())
                .collect::<Vec<_>>(),
            [
                "identity.human_session.created",
                "identity.human_session.revoked"
            ]
        );
        assert_eq!(
            state
                .events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            [
                "identity.human_session.created.v1",
                "identity.human_session.revoked.v1"
            ]
        );
        let serialized_evidence =
            serde_json::to_string(&(state.audits.clone(), state.events.clone())).unwrap();
        assert!(!serialized_evidence.contains(&raw_session_token));
        assert!(!serialized_evidence.contains(&raw_oidc_token));
        assert_eq!(
            state.writes,
            [
                "session",
                "audit",
                "outbox",
                "commit",
                "revocation",
                "audit",
                "outbox",
                "commit",
                "revocation",
                "commit",
            ]
        );
    }

    #[tokio::test]
    async fn invalid_redirect_and_overlong_session_are_rejected_before_persistence() {
        let store = MemoryStore::with_active_human();
        let service = HumanSessionService::new(store.clone());
        let now = now();
        let begin = service
            .begin_login(BeginHumanLoginCommand {
                client_key: HumanSessionClient::Mail,
                return_to: "https://attacker.example/callback".into(),
                state: "s".repeat(48),
                browser_nonce: "n".repeat(48),
                encrypted_pkce_verifier: vec![0x5a; 64],
                admission_limit: 100,
                cleanup_limit: 10,
                now,
                expires_at: now + Duration::minutes(5),
            })
            .await;
        assert_eq!(begin, Err(HumanSessionError::InvalidRequest));
        assert!(store.state.lock().unwrap().login.is_none());

        let session = service
            .create_session(CreateHumanSessionCommand {
                organization_id: OrganizationId::new(),
                principal_id: PrincipalId::new(),
                login_transaction_id: HumanLoginTransactionId::new(),
                client_key: HumanSessionClient::Mail,
                session_token: "t".repeat(64),
                oidc_access_token: "header.payload.signature".into(),
                oidc_jti: Some("jti-1".into()),
                upstream_expires_at: now + Duration::hours(12),
                expires_at: now + Duration::hours(9),
                trace_id: "trace-invalid-session".into(),
                now,
            })
            .await;
        assert_eq!(session, Err(HumanSessionError::InvalidRequest));
        assert!(store.state.lock().unwrap().session.is_none());
    }
}
