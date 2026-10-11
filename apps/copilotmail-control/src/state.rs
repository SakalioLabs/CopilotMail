use crate::*;

pub(crate) const SERVICE: &str = "copilotmail-control";

pub(crate) const API_VERSION: &str = "v2";

pub(crate) const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) const HUMAN_LOGIN_CLEANUP_LIMIT: u32 = 1_000;

pub(crate) const WORKER_ERROR_BACKOFF_INITIAL: Duration = Duration::from_secs(1);

pub(crate) const WORKER_ERROR_BACKOFF_MAX: Duration = Duration::from_secs(30);

pub(crate) fn worker_error_backoff(consecutive_failures: u32) -> Duration {
    if consecutive_failures == 0 {
        return Duration::ZERO;
    }
    let shift = consecutive_failures.saturating_sub(1).min(5);
    WORKER_ERROR_BACKOFF_INITIAL
        .saturating_mul(1_u32 << shift)
        .min(WORKER_ERROR_BACKOFF_MAX)
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: PgControlPlaneStore,
    pub(crate) authenticator: Arc<dyn AccessTokenAuthenticator>,
    pub(crate) authentication_ready: bool,
    pub(crate) human_auth_required: bool,
    pub(crate) human_session: Option<HumanSessionRuntime>,
    pub(crate) human_session_required: bool,
    pub(crate) agent_token_service: Option<Arc<AgentTokenService<PgControlPlaneStore>>>,
    pub(crate) agent_authentication_ready: bool,
    pub(crate) agent_auth_required: bool,
    pub(crate) agent_event_stream_tenant_limit: u32,
    pub(crate) agent_event_stream_principal_limit: u32,
    pub(crate) agent_ingress_tenant_requests_per_minute: u32,
    pub(crate) agent_ingress_principal_requests_per_minute: u32,
    pub(crate) agent_event_stream_metrics: AgentEventStreamMetrics,
    pub(crate) mail_data_plane_id: Option<MailDataPlaneId>,
    pub(crate) mail_engine: Option<Arc<dyn MailEngine>>,
    pub(crate) mail_engine_required: bool,
    pub(crate) mail_content: Option<Arc<dyn MailContentReader>>,
    pub(crate) mail_content_required: bool,
    pub(crate) mail_drafts: Option<Arc<dyn MailDraftStore>>,
    pub(crate) mail_draft_content: Option<Arc<dyn MailDraftContentReader>>,
    pub(crate) mail_drafts_required: bool,
    pub(crate) mail_event_ingress: Option<MailEventIngressSettings>,
    pub(crate) mail_event_ingress_required: bool,
    pub(crate) audit_archive: Option<AuditArchiveSettings>,
}

#[derive(Clone)]
pub(crate) struct HumanBrowserClientRuntime {
    pub(crate) client_key: HumanSessionClient,
    pub(crate) public_origin: String,
    pub(crate) relying_party: Arc<OidcRelyingParty<PgControlPlaneStore>>,
}

#[derive(Clone)]
pub(crate) struct HumanSessionRuntime {
    pub(crate) clients: Arc<BTreeMap<String, HumanBrowserClientRuntime>>,
    // Session resolution needs to verify that the stored cookie client is an
    // enabled browser client, but it does not need the OIDC relying-party
    // object itself. Keeping this small allowlist separate preserves that
    // boundary and lets HTTP tests exercise the BFF cookie path without a
    // synthetic OIDC provider.
    pub(crate) session_clients: Arc<Vec<HumanSessionClient>>,
    pub(crate) protector: PkceProtector,
    pub(crate) maximum_lifetime: std::time::Duration,
    pub(crate) login_requests_per_minute: u32,
    pub(crate) secure_cookie: bool,
}

impl HumanSessionRuntime {
    pub(crate) fn client(&self, key: &str) -> Option<&HumanBrowserClientRuntime> {
        self.clients.get(key)
    }

    pub(crate) fn session_client_is_configured(&self, client: HumanSessionClient) -> bool {
        self.session_clients.contains(&client)
    }

    pub(crate) const fn login_cookie_name(&self, client: HumanSessionClient) -> &'static str {
        human_login_cookie_name(client, self.secure_cookie)
    }

    pub(crate) const fn session_cookie_name(&self, client: HumanSessionClient) -> &'static str {
        human_session_cookie_name(client, self.secure_cookie)
    }
}

pub(crate) const fn human_login_cookie_name(
    client: HumanSessionClient,
    secure_cookie: bool,
) -> &'static str {
    match (client, secure_cookie) {
        (HumanSessionClient::Admin, true) => "__Host-copilotmail-admin-login",
        (HumanSessionClient::Mail, true) => "__Host-copilotmail-mail-login",
        (HumanSessionClient::Admin, false) => "copilotmail-admin-login",
        (HumanSessionClient::Mail, false) => "copilotmail-mail-login",
    }
}

pub(crate) const fn human_session_cookie_name(
    client: HumanSessionClient,
    secure_cookie: bool,
) -> &'static str {
    match (client, secure_cookie) {
        (HumanSessionClient::Admin, true) => "__Host-copilotmail-admin-session",
        (HumanSessionClient::Mail, true) => "__Host-copilotmail-mail-session",
        (HumanSessionClient::Admin, false) => "copilotmail-admin-session",
        (HumanSessionClient::Mail, false) => "copilotmail-mail-session",
    }
}

pub(crate) const fn other_human_session_client(client: HumanSessionClient) -> HumanSessionClient {
    match client {
        HumanSessionClient::Admin => HumanSessionClient::Mail,
        HumanSessionClient::Mail => HumanSessionClient::Admin,
    }
}

#[derive(Clone)]
pub(crate) struct PkceProtector {
    pub(crate) key: Arc<LessSafeKey>,
}

impl PkceProtector {
    pub(crate) fn new(key_bytes: &[u8]) -> Result<Self> {
        anyhow::ensure!(key_bytes.len() == 32, "Human session key must be 32 bytes");
        let key = UnboundKey::new(&AES_256_GCM, key_bytes)
            .map_err(|_| anyhow::anyhow!("Human session key is invalid"))?;
        Ok(Self {
            key: Arc::new(LessSafeKey::new(key)),
        })
    }

    pub(crate) fn protect(&self, verifier: &str, state_sha256: &[u8; 32]) -> Result<Vec<u8>> {
        anyhow::ensure!(
            (43..=128).contains(&verifier.len()) && verifier.is_ascii(),
            "PKCE verifier is invalid"
        );
        let nonce_bytes = rand::random::<[u8; 12]>();
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut ciphertext = verifier.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::from(state_sha256), &mut ciphertext)
            .map_err(|_| anyhow::anyhow!("protect PKCE verifier"))?;
        let mut protected = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
        protected.extend_from_slice(&nonce_bytes);
        protected.extend_from_slice(&ciphertext);
        Ok(protected)
    }

    pub(crate) fn unprotect(&self, protected: &[u8], state_sha256: &[u8; 32]) -> Result<String> {
        anyhow::ensure!(
            (12 + 43 + 16..=12 + 128 + 16).contains(&protected.len()),
            "protected PKCE verifier is invalid"
        );
        let (nonce_bytes, ciphertext) = protected.split_at(12);
        let nonce_bytes: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("protected PKCE nonce is invalid"))?;
        let mut plaintext = ciphertext.to_vec();
        let plaintext = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(state_sha256),
                &mut plaintext,
            )
            .map_err(|_| anyhow::anyhow!("protected PKCE verifier authentication failed"))?;
        let verifier = std::str::from_utf8(plaintext)
            .map_err(|_| anyhow::anyhow!("PKCE verifier is not UTF-8"))?;
        anyhow::ensure!(
            (43..=128).contains(&verifier.len()) && verifier.is_ascii(),
            "PKCE verifier is invalid"
        );
        Ok(verifier.to_owned())
    }
}

pub(crate) struct UnconfiguredAuthenticator;

pub(crate) struct RejectingAuthenticator;

pub(crate) struct CompositeAuthenticator {
    pub(crate) agent: Arc<dyn AccessTokenAuthenticator>,
    pub(crate) human: Arc<dyn AccessTokenAuthenticator>,
}

#[derive(Clone, Default)]
pub(crate) struct AgentEventStreamMetrics {
    pub(crate) active: Arc<AtomicU64>,
    pub(crate) opened_total: Arc<AtomicU64>,
    pub(crate) tenant_rejected_total: Arc<AtomicU64>,
    pub(crate) principal_rejected_total: Arc<AtomicU64>,
    pub(crate) stream_errors_total: Arc<AtomicU64>,
    pub(crate) cleanup_failures_total: Arc<AtomicU64>,
    pub(crate) ingress_allowed_total: Arc<AtomicU64>,
    pub(crate) ingress_tenant_rejected_total: Arc<AtomicU64>,
    pub(crate) ingress_principal_rejected_total: Arc<AtomicU64>,
    pub(crate) ingress_failures_total: Arc<AtomicU64>,
}

impl AgentEventStreamMetrics {
    pub(crate) fn opened(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
        self.opened_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn closed(&self) {
        let _ = self
            .active
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                Some(value.saturating_sub(1))
            });
    }

    pub(crate) fn render(&self) -> String {
        format!(
            concat!(
                "# HELP copilotmail_agent_event_stream_active Active SSE connections in this process.\n",
                "# TYPE copilotmail_agent_event_stream_active gauge\n",
                "copilotmail_agent_event_stream_active {}\n",
                "# HELP copilotmail_agent_event_stream_opened_total SSE connections opened by this process.\n",
                "# TYPE copilotmail_agent_event_stream_opened_total counter\n",
                "copilotmail_agent_event_stream_opened_total {}\n",
                "# HELP copilotmail_agent_event_stream_rejected_total Cluster quota rejections observed by this process.\n",
                "# TYPE copilotmail_agent_event_stream_rejected_total counter\n",
                "copilotmail_agent_event_stream_rejected_total{{scope=\"tenant\"}} {}\n",
                "copilotmail_agent_event_stream_rejected_total{{scope=\"principal\"}} {}\n",
                "# HELP copilotmail_agent_event_stream_errors_total Streams terminated after an internal polling or serialization error.\n",
                "# TYPE copilotmail_agent_event_stream_errors_total counter\n",
                "copilotmail_agent_event_stream_errors_total {}\n",
                "# HELP copilotmail_agent_event_stream_lease_cleanup_failures_total Lease cleanup failures recovered by expiry.\n",
                "# TYPE copilotmail_agent_event_stream_lease_cleanup_failures_total counter\n",
                "copilotmail_agent_event_stream_lease_cleanup_failures_total {}\n",
                "# HELP copilotmail_agent_ingress_requests_total Protected Agent API admissions observed by this process.\n",
                "# TYPE copilotmail_agent_ingress_requests_total counter\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"allowed\"}} {}\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"tenant_limited\"}} {}\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"principal_limited\"}} {}\n",
                "copilotmail_agent_ingress_requests_total{{outcome=\"failure\"}} {}\n"
            ),
            self.active.load(Ordering::Relaxed),
            self.opened_total.load(Ordering::Relaxed),
            self.tenant_rejected_total.load(Ordering::Relaxed),
            self.principal_rejected_total.load(Ordering::Relaxed),
            self.stream_errors_total.load(Ordering::Relaxed),
            self.cleanup_failures_total.load(Ordering::Relaxed),
            self.ingress_allowed_total.load(Ordering::Relaxed),
            self.ingress_tenant_rejected_total.load(Ordering::Relaxed),
            self.ingress_principal_rejected_total
                .load(Ordering::Relaxed),
            self.ingress_failures_total.load(Ordering::Relaxed),
        )
    }
}

pub(crate) fn agent_event_stream_metrics_router(metrics: AgentEventStreamMetrics) -> Router {
    Router::new()
        .route(
            "/metrics",
            get(
                |State(metrics): State<AgentEventStreamMetrics>| async move {
                    (
                        StatusCode::OK,
                        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                        metrics.render(),
                    )
                },
            ),
        )
        .with_state(metrics)
}

#[async_trait]
impl AccessTokenAuthenticator for UnconfiguredAuthenticator {
    async fn authenticate(
        &self,
        _: &str,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        Err(AuthenticationError::Unavailable)
    }
}

#[async_trait]
impl AccessTokenAuthenticator for RejectingAuthenticator {
    async fn authenticate(
        &self,
        _: &str,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        Err(AuthenticationError::InvalidCredential)
    }
}

#[async_trait]
impl AccessTokenAuthenticator for CompositeAuthenticator {
    async fn authenticate(
        &self,
        token: &str,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        match self.agent.authenticate(token).await {
            Ok(identity) => Ok(identity),
            Err(AuthenticationError::Unavailable) => Err(AuthenticationError::Unavailable),
            Err(AuthenticationError::InvalidCredential) => self.human.authenticate(token).await,
        }
    }

    async fn authenticate_with_context(
        &self,
        token: &str,
        context: &AuthenticationContext,
    ) -> Result<copilotmail_application::AuthenticatedIdentity, AuthenticationError> {
        match self.agent.authenticate_with_context(token, context).await {
            Ok(identity) => Ok(identity),
            Err(AuthenticationError::Unavailable) => Err(AuthenticationError::Unavailable),
            Err(AuthenticationError::InvalidCredential) => {
                self.human.authenticate_with_context(token, context).await
            }
        }
    }
}
