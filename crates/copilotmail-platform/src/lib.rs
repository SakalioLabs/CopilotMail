//! Configuration and observability shared by control-plane processes.

#![forbid(unsafe_code)]

use opentelemetry::{KeyValue, global, trace::TracerProvider as _};
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::{Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider};
use secrecy::{ExposeSecret, SecretString};
use std::{collections::BTreeSet, env, net::SocketAddr, path::PathBuf, time::Duration};
use tracing_subscriber::{
    EnvFilter, Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};
use url::Url;
use uuid::Uuid;

/// Validated process settings loaded from environment variables.
#[derive(Debug)]
pub struct ControlPlaneSettings {
    /// Address used by the V2 HTTP control plane.
    pub http_addr: SocketAddr,
    /// Secret-bearing PostgreSQL connection URL.
    pub database_url: SecretString,
    /// Maximum PostgreSQL pool size for this process.
    pub database_max_connections: u32,
    /// Number of outbox records claimed per worker iteration.
    pub worker_batch_size: i64,
    /// Delay between idle worker polls.
    pub worker_poll_interval: Duration,
    /// Logical deployment environment.
    pub environment: String,
    /// Optional enterprise human OIDC provider.
    pub oidc: Option<OidcSettings>,
    /// Whether readiness requires a configured human identity provider.
    pub require_human_auth: bool,
    /// Optional server-side Human browser-session relying-party configuration.
    pub human_session: Option<HumanSessionSettings>,
    /// Whether Human API access must use an HttpOnly browser session.
    pub require_human_session: bool,
    /// Optional first-party Agent OAuth authorization server.
    pub agent_oauth: Option<AgentOAuthSettings>,
    /// Whether readiness requires Agent OAuth to be configured.
    pub require_agent_auth: bool,
    /// Maximum active Agent SSE leases across one tenant and all API replicas.
    pub agent_event_stream_tenant_limit: u32,
    /// Maximum active Agent SSE leases for one Agent across all API replicas.
    pub agent_event_stream_principal_limit: u32,
    /// Maximum Agent Cursor/SSE requests per minute for one tenant.
    pub agent_ingress_tenant_requests_per_minute: u32,
    /// Maximum Agent Cursor/SSE requests per minute for one Agent.
    pub agent_ingress_principal_requests_per_minute: u32,
    /// Dedicated listener for non-sensitive Agent SSE Prometheus metrics.
    pub agent_event_stream_metrics_addr: SocketAddr,
    /// Stable namespace for all opaque references issued by one mail data-plane instance.
    pub mail_data_plane_id: Option<Uuid>,
    /// Canonical origin shared by every configured credential for that instance.
    pub mail_data_plane_origin: Option<String>,
    /// Optional documented JMAP mail data-plane adapter.
    pub mail_engine: Option<MailEngineSettings>,
    /// Whether readiness requires a live mail data plane.
    pub require_mail_engine: bool,
    /// Optional least-privilege JMAP content credential, isolated from management projection.
    pub mail_content: Option<MailEngineSettings>,
    /// Whether readiness requires Human mailbox content access.
    pub require_mail_content: bool,
    /// Optional least-privilege JMAP draft credential, isolated from content reads.
    pub mail_compose: Option<MailEngineSettings>,
    /// Whether readiness requires Human draft creation access.
    pub require_mail_compose: bool,
    /// Optional least-privilege JMAP submission credential, isolated from
    /// draft creation and content reads.
    pub mail_submission: Option<MailEngineSettings>,
    /// Whether readiness and the durable worker require outbound submission.
    pub require_mail_submission: bool,
    /// Optional authenticated telemetry ingress from the mail data plane.
    pub mail_event_ingress: Option<MailEventIngressSettings>,
    /// Whether readiness requires authenticated mail event ingestion.
    pub require_mail_event_ingress: bool,
    /// Optional signed Agent webhook delivery adapter.
    pub webhook_delivery: Option<WebhookDeliverySettings>,
    /// Whether production startup requires webhook delivery configuration.
    pub require_webhook_delivery: bool,
    /// Optional native HTTPS listener with verified client certificates.
    pub native_tls: Option<NativeTlsSettings>,
    /// Optional immutable audit-export archive.
    pub audit_archive: Option<AuditArchiveSettings>,
    /// Optional dedicated tenant audit checkpoint/archive worker.
    pub audit_maintenance: Option<AuditMaintenanceSettings>,
}

/// Non-secret enterprise OIDC relying-party configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcSettings {
    /// Exact issuer identifier expected in discovery metadata and tokens.
    pub issuer: String,
    /// Required access-token audience.
    pub audience: String,
    /// Development-only allowance for a local HTTP test provider.
    pub allow_insecure_http: bool,
}

/// One browser application registered on the same enterprise OIDC provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanBrowserClientSettings {
    /// Stable route key (`admin` or `mail`).
    pub key: String,
    /// OIDC client identifier used only by the server-side code flow.
    pub client_id: String,
    /// Exact externally visible origin that owns the Host-only cookies.
    pub public_origin: String,
    /// Space-separated scopes sent to the authorization endpoint.
    pub scope: String,
}

/// Server-side Authorization Code + PKCE and opaque browser-session settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanSessionSettings {
    pub clients: Vec<HumanBrowserClientSettings>,
    /// A read-only 32-byte key used to protect transient PKCE verifiers.
    pub encryption_key_file: PathBuf,
    /// Session lifetime is also capped by the verified upstream token expiry.
    pub maximum_lifetime: Duration,
    /// Cluster-wide per-client login-start admission ceiling.
    pub login_requests_per_minute: u32,
    /// Development-only escape hatch for loopback HTTP cookies.
    pub allow_insecure_cookie: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOAuthSettings {
    pub issuer: String,
    pub audience: String,
    pub token_endpoint: String,
    pub signing_key_id: Option<String>,
    pub private_key_file: Option<PathBuf>,
    pub public_key_file: Option<PathBuf>,
    pub signing_key_set_file: Option<PathBuf>,
    pub signing_key_reload_seconds: u64,
    pub access_token_ttl_seconds: i64,
    pub allow_insecure_http: bool,
}

/// Native TLS files. The client CA is used to verify optional Agent
/// certificates during the handshake; HTTP headers are never a substitute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTlsSettings {
    pub server_certificate_file: PathBuf,
    pub server_private_key_file: PathBuf,
    pub client_ca_file: PathBuf,
}

/// Non-secret connection settings for the replaceable mail data plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailEngineSettings {
    pub base_url: String,
    pub api_token_file: PathBuf,
    pub timeout: Duration,
    pub allow_insecure_http: bool,
}

/// File-backed dual authentication for inbound mail-engine telemetry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailEventIngressSettings {
    pub bearer_token_file: PathBuf,
    pub signature_key_file: PathBuf,
}

/// File-backed signing keys and bounded outbound HTTP policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookDeliverySettings {
    pub signing_key_set_file: PathBuf,
    pub timeout: Duration,
    pub max_attempts: i32,
    pub allow_insecure_http: bool,
    pub allow_private_network: bool,
}

/// Validated S3-compatible Object Lock configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditArchiveSettings {
    pub endpoint_url: Option<String>,
    pub region: String,
    pub bucket: String,
    pub force_path_style: bool,
    pub allow_insecure_http: bool,
    pub retention_mode: AuditArchiveRetentionMode,
    pub retention_days: u32,
}

/// One maintenance process is deliberately bound to one tenant and one
/// dedicated export Principal. Multi-tenant deployments run one isolated
/// instance per configured tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditMaintenanceSettings {
    pub organization_id: String,
    pub actor_principal_id: String,
    pub key_id: String,
    pub private_key_file: PathBuf,
    pub public_key_file: PathBuf,
    pub object_prefix: String,
    pub work_directory: PathBuf,
    pub interval: Duration,
    pub metrics_addr: SocketAddr,
    pub failure_threshold: u32,
    pub max_pending_age: Duration,
    pub max_cycle_duration: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditArchiveRetentionMode {
    Governance,
    Compliance,
}

/// Validated, optional OTLP/HTTP trace-export configuration.
///
/// The endpoint is deliberately configured through a CopilotMail-owned
/// variable so deployment policy cannot be bypassed by signal-specific OTEL
/// environment variables. Standard OTEL header variables remain available for
/// collector authentication and are consumed by the exporter without logging.
#[derive(Debug, Clone, PartialEq)]
struct OtlpTraceSettings {
    endpoint: String,
    service_name: String,
    environment: String,
    timeout: Duration,
}

impl OtlpTraceSettings {
    fn from_values(
        endpoint: Option<String>,
        service_name: String,
        environment: String,
        allow_insecure_http: bool,
        timeout_seconds: u64,
    ) -> Result<Option<Self>, SettingsError> {
        let Some(endpoint) = endpoint else {
            return Ok(None);
        };
        if !matches!(
            environment.as_str(),
            "development" | "testing" | "production"
        ) {
            return Err(SettingsError::Invalid("COPILOTMAIL_ENVIRONMENT"));
        }
        let mut parsed = Url::parse(&endpoint)
            .map_err(|_| SettingsError::Invalid("COPILOTMAIL_OTEL_EXPORTER_OTLP_ENDPOINT"))?;
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !matches!(parsed.scheme(), "https" | "http")
            || parsed.scheme() == "http" && (!allow_insecure_http || environment == "production")
        {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_OTEL_EXPORTER_OTLP_ENDPOINT",
            ));
        }
        if allow_insecure_http && environment == "production" {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_OTEL_ALLOW_INSECURE_HTTP",
            ));
        }
        if service_name.is_empty()
            || service_name.len() > 100
            || !service_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(SettingsError::Invalid("COPILOTMAIL_OTEL_SERVICE_NAME"));
        }
        if !(1..=30).contains(&timeout_seconds) {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_OTEL_EXPORT_TIMEOUT_SECONDS",
            ));
        }
        let base_path = parsed.path().trim_end_matches('/');
        parsed.set_path(&format!("{base_path}/v1/traces"));
        Ok(Some(Self {
            endpoint: parsed.into(),
            service_name,
            environment,
            timeout: Duration::from_secs(timeout_seconds),
        }))
    }

    fn from_env() -> Result<Option<Self>, SettingsError> {
        Self::from_values(
            optional_value("COPILOTMAIL_OTEL_EXPORTER_OTLP_ENDPOINT"),
            value("COPILOTMAIL_OTEL_SERVICE_NAME", "copilotmail-control"),
            value("COPILOTMAIL_ENVIRONMENT", "development"),
            parse("COPILOTMAIL_OTEL_ALLOW_INSECURE_HTTP", "false")?,
            parse("COPILOTMAIL_OTEL_EXPORT_TIMEOUT_SECONDS", "10")?,
        )
    }
}

impl ControlPlaneSettings {
    /// Loads and validates settings from the process environment.
    pub fn from_env() -> Result<Self, SettingsError> {
        let http_addr = value("COPILOTMAIL_V2_HTTP_ADDR", "0.0.0.0:8081")
            .parse()
            .map_err(|_| SettingsError::Invalid("COPILOTMAIL_V2_HTTP_ADDR"))?;
        let database_url = env::var("COPILOTMAIL_DATABASE_URL")
            .map_err(|_| SettingsError::Missing("COPILOTMAIL_DATABASE_URL"))?;
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err(SettingsError::Invalid("COPILOTMAIL_DATABASE_URL"));
        }
        let database_max_connections = parse("COPILOTMAIL_DATABASE_MAX_CONNECTIONS", "10")?;
        if database_max_connections == 0 {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_DATABASE_MAX_CONNECTIONS",
            ));
        }
        let worker_batch_size: i64 = parse("COPILOTMAIL_WORKER_BATCH_SIZE", "100")?;
        if !(1..=1_000).contains(&worker_batch_size) {
            return Err(SettingsError::Invalid("COPILOTMAIL_WORKER_BATCH_SIZE"));
        }
        let worker_poll_ms: u64 = parse("COPILOTMAIL_WORKER_POLL_MS", "1000")?;
        if !(100..=60_000).contains(&worker_poll_ms) {
            return Err(SettingsError::Invalid("COPILOTMAIL_WORKER_POLL_MS"));
        }

        let environment = value("COPILOTMAIL_ENVIRONMENT", "development");
        if !matches!(
            environment.as_str(),
            "development" | "testing" | "production"
        ) {
            return Err(SettingsError::Invalid("COPILOTMAIL_ENVIRONMENT"));
        }
        let oidc_issuer = optional_value("COPILOTMAIL_OIDC_ISSUER");
        let oidc_audience = optional_value("COPILOTMAIL_OIDC_AUDIENCE");
        let allow_insecure_http = parse::<bool>("COPILOTMAIL_OIDC_ALLOW_INSECURE_HTTP", "false")?;
        if allow_insecure_http && environment == "production" {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_OIDC_ALLOW_INSECURE_HTTP",
            ));
        }
        let oidc = match (oidc_issuer, oidc_audience) {
            (Some(issuer), Some(audience)) => Some(OidcSettings {
                issuer,
                audience,
                allow_insecure_http,
            }),
            (None, None) => None,
            _ => return Err(SettingsError::Missing("COPILOTMAIL_OIDC_ISSUER/AUDIENCE")),
        };
        let require_human_auth = parse::<bool>(
            "COPILOTMAIL_REQUIRE_HUMAN_AUTH",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        let require_human_session = parse::<bool>(
            "COPILOTMAIL_REQUIRE_HUMAN_SESSION",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        let human_session_allow_insecure =
            parse::<bool>("COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE", "false")?;
        if human_session_allow_insecure && environment == "production" {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_HUMAN_SESSION_ALLOW_INSECURE_COOKIE",
            ));
        }
        let human_session_key_file =
            optional_value("COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE");
        let human_session_ttl_seconds =
            parse::<u64>("COPILOTMAIL_HUMAN_SESSION_TTL_SECONDS", "3600")?;
        let human_login_requests_per_minute =
            parse::<u32>("COPILOTMAIL_HUMAN_LOGIN_REQUESTS_PER_MINUTE", "600")?;
        let browser_client_values = [
            (
                "admin",
                optional_value("COPILOTMAIL_ADMIN_OIDC_CLIENT_ID"),
                optional_value("COPILOTMAIL_ADMIN_PUBLIC_ORIGIN"),
                value("COPILOTMAIL_ADMIN_OIDC_SCOPE", "openid profile email"),
            ),
            (
                "mail",
                optional_value("COPILOTMAIL_MAIL_APP_OIDC_CLIENT_ID"),
                optional_value("COPILOTMAIL_MAIL_APP_PUBLIC_ORIGIN"),
                value("COPILOTMAIL_MAIL_APP_OIDC_SCOPE", "openid profile email"),
            ),
        ];
        let mut browser_clients = Vec::new();
        for (key, client_id, public_origin, scope) in browser_client_values {
            match (client_id, public_origin) {
                (Some(client_id), Some(public_origin)) => {
                    browser_clients.push(validate_human_browser_client(
                        key,
                        client_id,
                        public_origin,
                        scope,
                        human_session_allow_insecure,
                    )?);
                }
                (None, None) => {}
                _ => {
                    return Err(SettingsError::Missing(
                        "COPILOTMAIL_*_OIDC_CLIENT_ID/PUBLIC_ORIGIN",
                    ));
                }
            }
        }
        if !human_browser_cookie_hosts_are_distinct(&browser_clients) {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_*_PUBLIC_ORIGIN_COOKIE_HOST",
            ));
        }
        let human_session = match (human_session_key_file, browser_clients.is_empty()) {
            (Some(encryption_key_file), false) => {
                let encryption_key_file = PathBuf::from(encryption_key_file);
                if !(300..=28_800).contains(&human_session_ttl_seconds)
                    || !(1..=100_000).contains(&human_login_requests_per_minute)
                    || environment == "production" && !encryption_key_file.is_absolute()
                {
                    return Err(SettingsError::Invalid("COPILOTMAIL_HUMAN_SESSION_*"));
                }
                Some(HumanSessionSettings {
                    clients: browser_clients,
                    encryption_key_file,
                    maximum_lifetime: Duration::from_secs(human_session_ttl_seconds),
                    login_requests_per_minute: human_login_requests_per_minute,
                    allow_insecure_cookie: human_session_allow_insecure,
                })
            }
            (None, true) => None,
            _ => {
                return Err(SettingsError::Missing(
                    "COPILOTMAIL_HUMAN_SESSION_ENCRYPTION_KEY_FILE/CLIENTS",
                ));
            }
        };
        let agent_core_values = [
            optional_value("COPILOTMAIL_AGENT_OAUTH_ISSUER"),
            optional_value("COPILOTMAIL_AGENT_OAUTH_AUDIENCE"),
            optional_value("COPILOTMAIL_AGENT_OAUTH_TOKEN_ENDPOINT"),
        ];
        let agent_single_key_values = [
            optional_value("COPILOTMAIL_AGENT_OAUTH_SIGNING_KEY_ID"),
            optional_value("COPILOTMAIL_AGENT_OAUTH_PRIVATE_KEY_FILE"),
            optional_value("COPILOTMAIL_AGENT_OAUTH_PUBLIC_KEY_FILE"),
        ];
        let agent_signing_key_set_file =
            optional_value("COPILOTMAIL_AGENT_OAUTH_SIGNING_KEY_SET_FILE");
        let agent_allow_insecure =
            parse::<bool>("COPILOTMAIL_AGENT_OAUTH_ALLOW_INSECURE_HTTP", "false")?;
        if agent_allow_insecure && environment == "production" {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_AGENT_OAUTH_ALLOW_INSECURE_HTTP",
            ));
        }
        let no_agent_configuration = agent_core_values.iter().all(Option::is_none)
            && agent_single_key_values.iter().all(Option::is_none)
            && agent_signing_key_set_file.is_none();
        let agent_oauth = if no_agent_configuration {
            None
        } else if agent_core_values.iter().all(Option::is_some) {
            let single_key_configured = agent_single_key_values.iter().all(Option::is_some);
            let single_key_absent = agent_single_key_values.iter().all(Option::is_none);
            let valid_signing_mode = (single_key_configured
                && agent_signing_key_set_file.is_none())
                || (single_key_absent && agent_signing_key_set_file.is_some());
            if !valid_signing_mode {
                return Err(SettingsError::Missing(
                    "exactly one Agent OAuth signing mode: SIGNING_KEY_SET_FILE or SIGNING_KEY_ID/PRIVATE_KEY_FILE/PUBLIC_KEY_FILE",
                ));
            }
            let mut core = agent_core_values.into_iter().map(Option::unwrap);
            let mut single = agent_single_key_values.into_iter();
            Some(AgentOAuthSettings {
                issuer: core.next().unwrap(),
                audience: core.next().unwrap(),
                token_endpoint: core.next().unwrap(),
                signing_key_id: single.next().flatten(),
                private_key_file: single.next().flatten().map(PathBuf::from),
                public_key_file: single.next().flatten().map(PathBuf::from),
                signing_key_set_file: agent_signing_key_set_file.map(PathBuf::from),
                signing_key_reload_seconds: parse(
                    "COPILOTMAIL_AGENT_OAUTH_SIGNING_KEY_RELOAD_SECONDS",
                    "30",
                )?,
                access_token_ttl_seconds: parse(
                    "COPILOTMAIL_AGENT_OAUTH_ACCESS_TOKEN_TTL_SECONDS",
                    "300",
                )?,
                allow_insecure_http: agent_allow_insecure,
            })
        } else {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_AGENT_OAUTH_ISSUER/AUDIENCE/TOKEN_ENDPOINT",
            ));
        };
        if agent_oauth
            .as_ref()
            .is_some_and(|settings| !(5..=3_600).contains(&settings.signing_key_reload_seconds))
        {
            return Err(SettingsError::Invalid(
                "COPILOTMAIL_AGENT_OAUTH_SIGNING_KEY_RELOAD_SECONDS",
            ));
        }
        let require_agent_auth = parse::<bool>(
            "COPILOTMAIL_REQUIRE_AGENT_AUTH",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        let agent_event_stream_tenant_limit =
            parse("COPILOTMAIL_AGENT_EVENT_STREAM_TENANT_LIMIT", "100")?;
        let agent_event_stream_principal_limit =
            parse("COPILOTMAIL_AGENT_EVENT_STREAM_PRINCIPAL_LIMIT", "5")?;
        validate_agent_event_stream_limits(
            agent_event_stream_tenant_limit,
            agent_event_stream_principal_limit,
        )?;
        let agent_ingress_tenant_requests_per_minute = parse(
            "COPILOTMAIL_AGENT_INGRESS_TENANT_REQUESTS_PER_MINUTE",
            "6000",
        )?;
        let agent_ingress_principal_requests_per_minute = parse(
            "COPILOTMAIL_AGENT_INGRESS_PRINCIPAL_REQUESTS_PER_MINUTE",
            "600",
        )?;
        validate_agent_ingress_rate_limits(
            agent_ingress_tenant_requests_per_minute,
            agent_ingress_principal_requests_per_minute,
        )?;
        let agent_event_stream_metrics_addr = value(
            "COPILOTMAIL_AGENT_EVENT_STREAM_METRICS_ADDR",
            "127.0.0.1:9092",
        )
        .parse()
        .map_err(|_| SettingsError::Invalid("COPILOTMAIL_AGENT_EVENT_STREAM_METRICS_ADDR"))?;
        let mail_engine_base_url = optional_value("COPILOTMAIL_MAIL_ENGINE_BASE_URL");
        let mail_engine_api_token_file = optional_value("COPILOTMAIL_MAIL_ENGINE_API_TOKEN_FILE");
        let mail_engine_configured = [
            "COPILOTMAIL_MAIL_ENGINE_BASE_URL",
            "COPILOTMAIL_MAIL_ENGINE_API_TOKEN_FILE",
            "COPILOTMAIL_MAIL_ENGINE_TIMEOUT_SECONDS",
            "COPILOTMAIL_MAIL_ENGINE_ALLOW_INSECURE_HTTP",
        ]
        .iter()
        .any(|name| env::var(name).is_ok());
        let mail_engine = if mail_engine_configured {
            let base_url = mail_engine_base_url
                .ok_or(SettingsError::Missing("COPILOTMAIL_MAIL_ENGINE_BASE_URL"))?;
            let api_token_file = mail_engine_api_token_file.ok_or(SettingsError::Missing(
                "COPILOTMAIL_MAIL_ENGINE_API_TOKEN_FILE",
            ))?;
            let allow_insecure_http =
                parse::<bool>("COPILOTMAIL_MAIL_ENGINE_ALLOW_INSECURE_HTTP", "false")?;
            if allow_insecure_http && environment == "production" {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_ENGINE_ALLOW_INSECURE_HTTP",
                ));
            }
            let timeout_seconds = parse::<u64>("COPILOTMAIL_MAIL_ENGINE_TIMEOUT_SECONDS", "10")?;
            if !(1..=60).contains(&timeout_seconds) {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_ENGINE_TIMEOUT_SECONDS",
                ));
            }
            Some(MailEngineSettings {
                base_url,
                api_token_file: PathBuf::from(api_token_file),
                timeout: Duration::from_secs(timeout_seconds),
                allow_insecure_http,
            })
        } else {
            None
        };
        let require_mail_engine = parse::<bool>(
            "COPILOTMAIL_REQUIRE_MAIL_ENGINE",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        let mail_content_values = [
            optional_value("COPILOTMAIL_MAIL_CONTENT_BASE_URL"),
            optional_value("COPILOTMAIL_MAIL_CONTENT_TOKEN_FILE"),
        ];
        let mail_content_configured = [
            "COPILOTMAIL_MAIL_CONTENT_BASE_URL",
            "COPILOTMAIL_MAIL_CONTENT_TOKEN_FILE",
            "COPILOTMAIL_MAIL_CONTENT_TIMEOUT_SECONDS",
            "COPILOTMAIL_MAIL_CONTENT_ALLOW_INSECURE_HTTP",
        ]
        .iter()
        .any(|name| env::var(name).is_ok());
        let mail_content = if mail_content_configured {
            let base_url = mail_content_values[0]
                .clone()
                .ok_or(SettingsError::Missing("COPILOTMAIL_MAIL_CONTENT_BASE_URL"))?;
            let token_file = mail_content_values[1]
                .clone()
                .ok_or(SettingsError::Missing(
                    "COPILOTMAIL_MAIL_CONTENT_TOKEN_FILE",
                ))?;
            let allow_insecure_http =
                parse::<bool>("COPILOTMAIL_MAIL_CONTENT_ALLOW_INSECURE_HTTP", "false")?;
            let timeout_seconds = parse::<u64>("COPILOTMAIL_MAIL_CONTENT_TIMEOUT_SECONDS", "10")?;
            if allow_insecure_http && environment == "production" {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_CONTENT_ALLOW_INSECURE_HTTP",
                ));
            }
            if !(1..=60).contains(&timeout_seconds) {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_CONTENT_TIMEOUT_SECONDS",
                ));
            }
            Some(MailEngineSettings {
                base_url,
                api_token_file: PathBuf::from(token_file),
                timeout: Duration::from_secs(timeout_seconds),
                allow_insecure_http,
            })
        } else {
            None
        };
        let require_mail_content = parse::<bool>(
            "COPILOTMAIL_REQUIRE_MAIL_CONTENT",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        if require_mail_content && mail_content.is_none() {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_MAIL_CONTENT_BASE_URL/TOKEN_FILE",
            ));
        }
        let mail_compose_values = [
            optional_value("COPILOTMAIL_MAIL_COMPOSE_BASE_URL"),
            optional_value("COPILOTMAIL_MAIL_COMPOSE_TOKEN_FILE"),
        ];
        let mail_compose_configured = [
            "COPILOTMAIL_MAIL_COMPOSE_BASE_URL",
            "COPILOTMAIL_MAIL_COMPOSE_TOKEN_FILE",
            "COPILOTMAIL_MAIL_COMPOSE_TIMEOUT_SECONDS",
            "COPILOTMAIL_MAIL_COMPOSE_ALLOW_INSECURE_HTTP",
        ]
        .iter()
        .any(|name| env::var(name).is_ok());
        let mail_compose = if mail_compose_configured {
            let base_url = mail_compose_values[0]
                .clone()
                .ok_or(SettingsError::Missing("COPILOTMAIL_MAIL_COMPOSE_BASE_URL"))?;
            let token_file = mail_compose_values[1]
                .clone()
                .ok_or(SettingsError::Missing(
                    "COPILOTMAIL_MAIL_COMPOSE_TOKEN_FILE",
                ))?;
            let allow_insecure_http =
                parse::<bool>("COPILOTMAIL_MAIL_COMPOSE_ALLOW_INSECURE_HTTP", "false")?;
            let timeout_seconds = parse::<u64>("COPILOTMAIL_MAIL_COMPOSE_TIMEOUT_SECONDS", "10")?;
            if allow_insecure_http && environment == "production" {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_COMPOSE_ALLOW_INSECURE_HTTP",
                ));
            }
            if !(1..=60).contains(&timeout_seconds) {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_COMPOSE_TIMEOUT_SECONDS",
                ));
            }
            Some(MailEngineSettings {
                base_url,
                api_token_file: PathBuf::from(token_file),
                timeout: Duration::from_secs(timeout_seconds),
                allow_insecure_http,
            })
        } else {
            None
        };
        let require_mail_compose = parse::<bool>(
            "COPILOTMAIL_REQUIRE_MAIL_COMPOSE",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        if require_mail_compose && mail_compose.is_none() {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_MAIL_COMPOSE_BASE_URL/TOKEN_FILE",
            ));
        }
        let mail_submission_values = [
            optional_value("COPILOTMAIL_MAIL_SUBMISSION_BASE_URL"),
            optional_value("COPILOTMAIL_MAIL_SUBMISSION_TOKEN_FILE"),
        ];
        let mail_submission_configured = [
            "COPILOTMAIL_MAIL_SUBMISSION_BASE_URL",
            "COPILOTMAIL_MAIL_SUBMISSION_TOKEN_FILE",
            "COPILOTMAIL_MAIL_SUBMISSION_TIMEOUT_SECONDS",
            "COPILOTMAIL_MAIL_SUBMISSION_ALLOW_INSECURE_HTTP",
        ]
        .iter()
        .any(|name| env::var(name).is_ok());
        let mail_submission = if mail_submission_configured {
            let base_url = mail_submission_values[0]
                .clone()
                .ok_or(SettingsError::Missing(
                    "COPILOTMAIL_MAIL_SUBMISSION_BASE_URL",
                ))?;
            let token_file = mail_submission_values[1]
                .clone()
                .ok_or(SettingsError::Missing(
                    "COPILOTMAIL_MAIL_SUBMISSION_TOKEN_FILE",
                ))?;
            let allow_insecure_http =
                parse::<bool>("COPILOTMAIL_MAIL_SUBMISSION_ALLOW_INSECURE_HTTP", "false")?;
            let timeout_seconds =
                parse::<u64>("COPILOTMAIL_MAIL_SUBMISSION_TIMEOUT_SECONDS", "10")?;
            if allow_insecure_http && environment == "production" {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_SUBMISSION_ALLOW_INSECURE_HTTP",
                ));
            }
            if !(1..=60).contains(&timeout_seconds) {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_MAIL_SUBMISSION_TIMEOUT_SECONDS",
                ));
            }
            Some(MailEngineSettings {
                base_url,
                api_token_file: PathBuf::from(token_file),
                timeout: Duration::from_secs(timeout_seconds),
                allow_insecure_http,
            })
        } else {
            None
        };
        let require_mail_submission =
            parse::<bool>("COPILOTMAIL_REQUIRE_MAIL_SUBMISSION", "false")?;
        if require_mail_submission && mail_submission.is_none() {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_MAIL_SUBMISSION_BASE_URL/TOKEN_FILE",
            ));
        }
        let mail_event_ingress_values = [
            optional_value("COPILOTMAIL_MAIL_EVENT_INGRESS_BEARER_TOKEN_FILE"),
            optional_value("COPILOTMAIL_MAIL_EVENT_INGRESS_SIGNATURE_KEY_FILE"),
        ];
        let mail_event_ingress = if mail_event_ingress_values.iter().all(Option::is_none) {
            None
        } else if mail_event_ingress_values.iter().all(Option::is_some) {
            let mut values = mail_event_ingress_values.into_iter().map(Option::unwrap);
            Some(MailEventIngressSettings {
                bearer_token_file: PathBuf::from(values.next().unwrap()),
                signature_key_file: PathBuf::from(values.next().unwrap()),
            })
        } else {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_MAIL_EVENT_INGRESS_BEARER_TOKEN_FILE/SIGNATURE_KEY_FILE",
            ));
        };
        let require_mail_event_ingress = parse::<bool>(
            "COPILOTMAIL_REQUIRE_MAIL_EVENT_INGRESS",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        if require_mail_event_ingress && mail_event_ingress.is_none() {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_MAIL_EVENT_INGRESS_BEARER_TOKEN_FILE/SIGNATURE_KEY_FILE",
            ));
        }
        let mail_data_plane_id = optional_value("COPILOTMAIL_MAIL_DATA_PLANE_ID")
            .map(|value| {
                Uuid::parse_str(&value)
                    .map_err(|_| SettingsError::Invalid("COPILOTMAIL_MAIL_DATA_PLANE_ID"))
            })
            .transpose()?;
        let configured_mail_data_plane_origin =
            optional_value("COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN");
        let mail_data_plane_origin = validate_mail_data_plane_configuration(
            mail_data_plane_id,
            configured_mail_data_plane_origin.as_deref(),
            [
                mail_engine.as_ref(),
                mail_content.as_ref(),
                mail_compose.as_ref(),
                mail_submission.as_ref(),
            ],
            mail_event_ingress.is_some(),
        )?;
        let webhook_key_set_file = optional_value("COPILOTMAIL_WEBHOOK_SIGNING_KEY_SET_FILE");
        let webhook_configured = [
            "COPILOTMAIL_WEBHOOK_SIGNING_KEY_SET_FILE",
            "COPILOTMAIL_WEBHOOK_TIMEOUT_SECONDS",
            "COPILOTMAIL_WEBHOOK_MAX_ATTEMPTS",
            "COPILOTMAIL_WEBHOOK_ALLOW_INSECURE_HTTP",
            "COPILOTMAIL_WEBHOOK_ALLOW_PRIVATE_NETWORK",
        ]
        .iter()
        .any(|name| env::var(name).is_ok());
        let webhook_delivery = if webhook_configured {
            let signing_key_set_file = webhook_key_set_file.ok_or(SettingsError::Missing(
                "COPILOTMAIL_WEBHOOK_SIGNING_KEY_SET_FILE",
            ))?;
            let timeout_seconds = parse::<u64>("COPILOTMAIL_WEBHOOK_TIMEOUT_SECONDS", "10")?;
            let max_attempts = parse::<i32>("COPILOTMAIL_WEBHOOK_MAX_ATTEMPTS", "12")?;
            let allow_insecure_http =
                parse::<bool>("COPILOTMAIL_WEBHOOK_ALLOW_INSECURE_HTTP", "false")?;
            let allow_private_network =
                parse::<bool>("COPILOTMAIL_WEBHOOK_ALLOW_PRIVATE_NETWORK", "false")?;
            if !(1..=30).contains(&timeout_seconds)
                || !(1..=100).contains(&max_attempts)
                || allow_private_network && !allow_insecure_http
                || environment == "production" && (allow_insecure_http || allow_private_network)
            {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_WEBHOOK_DELIVERY_CONFIGURATION",
                ));
            }
            Some(WebhookDeliverySettings {
                signing_key_set_file: PathBuf::from(signing_key_set_file),
                timeout: Duration::from_secs(timeout_seconds),
                max_attempts,
                allow_insecure_http,
                allow_private_network,
            })
        } else {
            None
        };
        let require_webhook_delivery = parse::<bool>(
            "COPILOTMAIL_REQUIRE_WEBHOOK_DELIVERY",
            if environment == "production" {
                "true"
            } else {
                "false"
            },
        )?;
        if require_webhook_delivery && webhook_delivery.is_none() {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_WEBHOOK_SIGNING_KEY_SET_FILE",
            ));
        }
        let native_tls_values = [
            optional_value("COPILOTMAIL_NATIVE_TLS_CERT_FILE"),
            optional_value("COPILOTMAIL_NATIVE_TLS_KEY_FILE"),
            optional_value("COPILOTMAIL_NATIVE_TLS_CLIENT_CA_FILE"),
        ];
        let native_tls = if native_tls_values.iter().all(Option::is_none) {
            None
        } else if native_tls_values.iter().all(Option::is_some) {
            let mut values = native_tls_values.into_iter().map(Option::unwrap);
            Some(NativeTlsSettings {
                server_certificate_file: PathBuf::from(values.next().unwrap()),
                server_private_key_file: PathBuf::from(values.next().unwrap()),
                client_ca_file: PathBuf::from(values.next().unwrap()),
            })
        } else {
            return Err(SettingsError::Missing(
                "COPILOTMAIL_NATIVE_TLS_CERT_FILE/KEY_FILE/CLIENT_CA_FILE",
            ));
        };
        let archive_endpoint = optional_value("COPILOTMAIL_AUDIT_ARCHIVE_ENDPOINT");
        let archive_region = optional_value("COPILOTMAIL_AUDIT_ARCHIVE_REGION");
        let archive_bucket = optional_value("COPILOTMAIL_AUDIT_ARCHIVE_BUCKET");
        let archive_configured = [
            "COPILOTMAIL_AUDIT_ARCHIVE_ENDPOINT",
            "COPILOTMAIL_AUDIT_ARCHIVE_REGION",
            "COPILOTMAIL_AUDIT_ARCHIVE_BUCKET",
            "COPILOTMAIL_AUDIT_ARCHIVE_FORCE_PATH_STYLE",
            "COPILOTMAIL_AUDIT_ARCHIVE_ALLOW_INSECURE_HTTP",
            "COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_MODE",
            "COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_DAYS",
        ]
        .iter()
        .any(|name| env::var(name).is_ok());
        let audit_archive = if archive_configured {
            let region =
                archive_region.ok_or(SettingsError::Missing("COPILOTMAIL_AUDIT_ARCHIVE_REGION"))?;
            let bucket =
                archive_bucket.ok_or(SettingsError::Missing("COPILOTMAIL_AUDIT_ARCHIVE_BUCKET"))?;
            let allow_insecure_http =
                parse::<bool>("COPILOTMAIL_AUDIT_ARCHIVE_ALLOW_INSECURE_HTTP", "false")?;
            if allow_insecure_http && environment == "production" {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_AUDIT_ARCHIVE_ALLOW_INSECURE_HTTP",
                ));
            }
            let retention_mode = match value(
                "COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_MODE",
                if environment == "production" {
                    "compliance"
                } else {
                    "governance"
                },
            )
            .as_str()
            {
                "governance" => AuditArchiveRetentionMode::Governance,
                "compliance" => AuditArchiveRetentionMode::Compliance,
                _ => {
                    return Err(SettingsError::Invalid(
                        "COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_MODE",
                    ));
                }
            };
            if environment == "production"
                && retention_mode != AuditArchiveRetentionMode::Compliance
            {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_MODE",
                ));
            }
            let retention_days = parse::<u32>("COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_DAYS", "2555")?;
            if !(1..=36_500).contains(&retention_days) {
                return Err(SettingsError::Invalid(
                    "COPILOTMAIL_AUDIT_ARCHIVE_RETENTION_DAYS",
                ));
            }
            Some(AuditArchiveSettings {
                endpoint_url: archive_endpoint,
                region,
                bucket,
                force_path_style: parse("COPILOTMAIL_AUDIT_ARCHIVE_FORCE_PATH_STYLE", "false")?,
                allow_insecure_http,
                retention_mode,
                retention_days,
            })
        } else {
            None
        };
        let maintenance_names = [
            "COPILOTMAIL_AUDIT_MAINTENANCE_ORGANIZATION_ID",
            "COPILOTMAIL_AUDIT_MAINTENANCE_ACTOR_PRINCIPAL_ID",
            "COPILOTMAIL_AUDIT_MAINTENANCE_KEY_ID",
            "COPILOTMAIL_AUDIT_MAINTENANCE_PRIVATE_KEY_FILE",
            "COPILOTMAIL_AUDIT_MAINTENANCE_PUBLIC_KEY_FILE",
            "COPILOTMAIL_AUDIT_MAINTENANCE_OBJECT_PREFIX",
            "COPILOTMAIL_AUDIT_MAINTENANCE_WORK_DIRECTORY",
            "COPILOTMAIL_AUDIT_MAINTENANCE_INTERVAL_SECONDS",
            "COPILOTMAIL_AUDIT_MAINTENANCE_METRICS_ADDR",
            "COPILOTMAIL_AUDIT_MAINTENANCE_FAILURE_THRESHOLD",
            "COPILOTMAIL_AUDIT_MAINTENANCE_MAX_PENDING_AGE_SECONDS",
            "COPILOTMAIL_AUDIT_MAINTENANCE_MAX_CYCLE_SECONDS",
        ];
        let maintenance_configured = maintenance_names.iter().any(|name| env::var(name).is_ok());
        let audit_maintenance = if maintenance_configured {
            if audit_archive.is_none() {
                return Err(SettingsError::Missing(
                    "COPILOTMAIL_AUDIT_ARCHIVE_REGION/BUCKET",
                ));
            }
            let organization_id = optional_value(maintenance_names[0])
                .ok_or(SettingsError::Missing(maintenance_names[0]))?;
            let actor_principal_id = optional_value(maintenance_names[1])
                .ok_or(SettingsError::Missing(maintenance_names[1]))?;
            let key_id = optional_value(maintenance_names[2])
                .ok_or(SettingsError::Missing(maintenance_names[2]))?;
            let private_key_file = optional_value(maintenance_names[3])
                .map(PathBuf::from)
                .ok_or(SettingsError::Missing(maintenance_names[3]))?;
            let public_key_file = optional_value(maintenance_names[4])
                .map(PathBuf::from)
                .ok_or(SettingsError::Missing(maintenance_names[4]))?;
            let object_prefix = optional_value(maintenance_names[5])
                .ok_or(SettingsError::Missing(maintenance_names[5]))?;
            let work_directory = PathBuf::from(value(
                maintenance_names[6],
                "/var/lib/copilotmail/audit-maintenance",
            ));
            let interval_seconds = parse::<u64>(maintenance_names[7], "86400")?;
            let metrics_addr = value(maintenance_names[8], "0.0.0.0:9090")
                .parse()
                .map_err(|_| SettingsError::Invalid(maintenance_names[8]))?;
            let failure_threshold = parse::<u32>(maintenance_names[9], "3")?;
            let max_pending_age_seconds = parse::<u64>(maintenance_names[10], "172800")?;
            let max_cycle_seconds = parse::<u64>(maintenance_names[11], "21600")?;
            if !(300..=604_800).contains(&interval_seconds)
                || !(1..=100).contains(&failure_threshold)
                || max_pending_age_seconds < interval_seconds
                || max_pending_age_seconds > 2_592_000
                || !(60..=604_800).contains(&max_cycle_seconds)
                || key_id.len() > 128
                || key_id.chars().any(char::is_control)
                || !valid_audit_object_prefix(&object_prefix)
                || work_directory.as_os_str().is_empty()
                || environment == "production" && !work_directory.is_absolute()
            {
                return Err(SettingsError::Invalid("COPILOTMAIL_AUDIT_MAINTENANCE_*"));
            }
            Some(AuditMaintenanceSettings {
                organization_id,
                actor_principal_id,
                key_id,
                private_key_file,
                public_key_file,
                object_prefix,
                work_directory,
                interval: Duration::from_secs(interval_seconds),
                metrics_addr,
                failure_threshold,
                max_pending_age: Duration::from_secs(max_pending_age_seconds),
                max_cycle_duration: Duration::from_secs(max_cycle_seconds),
            })
        } else {
            None
        };

        Ok(Self {
            http_addr,
            database_url: SecretString::from(database_url),
            database_max_connections,
            worker_batch_size,
            worker_poll_interval: Duration::from_millis(worker_poll_ms),
            environment,
            oidc,
            require_human_auth,
            human_session,
            require_human_session,
            agent_oauth,
            require_agent_auth,
            agent_event_stream_tenant_limit,
            agent_event_stream_principal_limit,
            agent_ingress_tenant_requests_per_minute,
            agent_ingress_principal_requests_per_minute,
            agent_event_stream_metrics_addr,
            mail_data_plane_id,
            mail_data_plane_origin,
            mail_engine,
            require_mail_engine,
            mail_content,
            require_mail_content,
            mail_compose,
            require_mail_compose,
            mail_submission,
            require_mail_submission,
            mail_event_ingress,
            require_mail_event_ingress,
            webhook_delivery,
            require_webhook_delivery,
            native_tls,
            audit_archive,
            audit_maintenance,
        })
    }

    /// Exposes the database URL only at the database adapter boundary.
    #[must_use]
    pub fn database_url(&self) -> &str {
        self.database_url.expose_secret()
    }
}

fn validate_human_browser_client(
    key: &str,
    client_id: String,
    public_origin: String,
    scope: String,
    allow_insecure_http: bool,
) -> Result<HumanBrowserClientSettings, SettingsError> {
    let origin = Url::parse(&public_origin)
        .map_err(|_| SettingsError::Invalid("COPILOTMAIL_*_PUBLIC_ORIGIN"))?;
    let loopback_host = match origin.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    let secure_scheme = origin.scheme() == "https"
        || allow_insecure_http && origin.scheme() == "http" && loopback_host;
    let path_is_origin = origin.path().is_empty() || origin.path() == "/";
    if !matches!(key, "admin" | "mail")
        || client_id.is_empty()
        || client_id.len() > 512
        || client_id.chars().any(char::is_control)
        || !secure_scheme
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || !path_is_origin
        || origin.query().is_some()
        || origin.fragment().is_some()
        || scope.is_empty()
        || scope.len() > 512
        || scope.split_ascii_whitespace().count() > 32
        || scope
            .split_ascii_whitespace()
            .any(|item| item.is_empty() || item.len() > 128)
    {
        return Err(SettingsError::Invalid("COPILOTMAIL_HUMAN_SESSION_*"));
    }
    Ok(HumanBrowserClientSettings {
        key: key.into(),
        client_id,
        public_origin: public_origin.trim_end_matches('/').into(),
        scope,
    })
}

fn human_browser_cookie_hosts_are_distinct(clients: &[HumanBrowserClientSettings]) -> bool {
    let mut hosts = BTreeSet::new();
    clients.iter().all(|client| {
        Url::parse(&client.public_origin)
            .ok()
            .and_then(|origin| origin.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| hosts.insert(host))
    })
}

fn validate_agent_event_stream_limits(
    tenant_limit: u32,
    principal_limit: u32,
) -> Result<(), SettingsError> {
    if !(1..=10_000).contains(&tenant_limit)
        || !(1..=1_000).contains(&principal_limit)
        || principal_limit > tenant_limit
    {
        return Err(SettingsError::Invalid(
            "COPILOTMAIL_AGENT_EVENT_STREAM_*_LIMIT",
        ));
    }
    Ok(())
}

fn validate_agent_ingress_rate_limits(
    tenant_requests_per_minute: u32,
    principal_requests_per_minute: u32,
) -> Result<(), SettingsError> {
    if !(1..=1_000_000).contains(&tenant_requests_per_minute)
        || !(1..=100_000).contains(&principal_requests_per_minute)
        || principal_requests_per_minute > tenant_requests_per_minute
    {
        return Err(SettingsError::Invalid(
            "COPILOTMAIL_AGENT_INGRESS_*_REQUESTS_PER_MINUTE",
        ));
    }
    Ok(())
}

fn validate_mail_data_plane_configuration(
    data_plane_id: Option<Uuid>,
    configured_origin: Option<&str>,
    adapters: [Option<&MailEngineSettings>; 4],
    ingress_configured: bool,
) -> Result<Option<String>, SettingsError> {
    let configured = data_plane_id.is_some()
        || configured_origin.is_some()
        || ingress_configured
        || adapters.iter().any(Option::is_some);
    if configured && data_plane_id.is_none() {
        return Err(SettingsError::Missing("COPILOTMAIL_MAIL_DATA_PLANE_ID"));
    }
    if configured && configured_origin.is_none() {
        return Err(SettingsError::Missing("COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN"));
    }
    if data_plane_id.is_some_and(|value| value.is_nil()) {
        return Err(SettingsError::Invalid("COPILOTMAIL_MAIL_DATA_PLANE_ID"));
    }

    let mut origin = configured_origin
        .map(|value| {
            let parsed = Url::parse(value)
                .map_err(|_| SettingsError::Invalid("COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN"))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
                || !matches!(parsed.path(), "" | "/")
            {
                return Err(SettingsError::Invalid("COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN"));
            }
            Ok(parsed.origin().ascii_serialization())
        })
        .transpose()?;
    for settings in adapters.into_iter().flatten() {
        let parsed = Url::parse(&settings.base_url)
            .map_err(|_| SettingsError::Invalid("COPILOTMAIL_MAIL_*_BASE_URL"))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !matches!(parsed.path(), "" | "/")
        {
            return Err(SettingsError::Invalid("COPILOTMAIL_MAIL_*_BASE_URL"));
        }
        let candidate = parsed.origin().ascii_serialization();
        if origin.as_ref().is_some_and(|current| current != &candidate) {
            return Err(SettingsError::Invalid("COPILOTMAIL_MAIL_*_BASE_URL"));
        }
        origin = Some(candidate);
    }
    Ok(origin)
}

/// Configuration loading or validation error.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsError {
    /// A required environment variable was absent.
    #[error("{0} is required")]
    Missing(&'static str),
    /// An environment variable failed validation.
    #[error("{0} is invalid")]
    Invalid(&'static str),
}

/// Keeps the batch tracer provider alive and flushes it during clean shutdown.
#[must_use]
pub struct ObservabilityGuard {
    tracer_provider: Option<SdkTracerProvider>,
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.tracer_provider.take() {
            let _ = provider.shutdown_with_timeout(Duration::from_secs(5));
        }
    }
}

/// Installs JSON logs and, when configured, an OTLP/HTTP trace exporter.
///
/// Exporter construction is fail closed: invalid or insecure production
/// endpoints prevent startup. Export delivery is asynchronous and never holds
/// a business database transaction.
pub fn init_observability() -> Result<ObservabilityGuard, Box<dyn std::error::Error + Send + Sync>>
{
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("copilotmail=info,tower_http=info"));
    let json_logs = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(true);
    let Some(settings) = OtlpTraceSettings::from_env()? else {
        tracing_subscriber::registry()
            .with(filter)
            .with(json_logs)
            .try_init()?;
        return Ok(ObservabilityGuard {
            tracer_provider: None,
        });
    };

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(settings.endpoint)
        .with_timeout(settings.timeout)
        .with_protocol(Protocol::HttpBinary)
        .build()?;
    let provider = SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name(settings.service_name)
                .with_attributes([
                    KeyValue::new("deployment.environment.name", settings.environment),
                    KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                ])
                .build(),
        )
        .with_batch_exporter(exporter)
        .build();
    global::set_text_map_propagator(TraceContextPropagator::new());
    let tracer = provider.tracer("copilotmail-control");
    // Only CopilotMail span metadata crosses the process boundary. `tracing`
    // events remain in the local structured log so arbitrary error strings and
    // exporter/client internals cannot recurse into remote span events.
    let otlp_spans = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            metadata.is_span() && metadata.target().starts_with("copilotmail")
        }));
    tracing_subscriber::registry()
        .with(filter)
        .with(json_logs)
        .with(otlp_spans)
        .try_init()?;
    Ok(ObservabilityGuard {
        tracer_provider: Some(provider),
    })
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}

fn optional_value(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn valid_audit_object_prefix(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix.len() <= 700
        && !prefix.starts_with('/')
        && !prefix.ends_with('/')
        && !prefix.contains('\\')
        && !prefix.chars().any(char::is_control)
        && prefix
            .split('/')
            .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."))
}

fn parse<T>(name: &'static str, default: &str) -> Result<T, SettingsError>
where
    T: std::str::FromStr,
{
    value(name, default)
        .parse()
        .map_err(|_| SettingsError::Invalid(name))
}

#[cfg(test)]
mod tests {
    use super::{
        MailEngineSettings, OtlpTraceSettings, SettingsError,
        human_browser_cookie_hosts_are_distinct, valid_audit_object_prefix,
        validate_agent_event_stream_limits, validate_agent_ingress_rate_limits,
        validate_human_browser_client, validate_mail_data_plane_configuration,
    };
    use std::{path::PathBuf, time::Duration};
    use uuid::Uuid;

    #[test]
    fn agent_event_stream_limits_are_bounded_and_hierarchical() {
        assert!(validate_agent_event_stream_limits(100, 5).is_ok());
        for (tenant, principal) in [(0, 1), (1, 0), (5, 6), (10_001, 1), (1_000, 1_001)] {
            assert_eq!(
                validate_agent_event_stream_limits(tenant, principal),
                Err(SettingsError::Invalid(
                    "COPILOTMAIL_AGENT_EVENT_STREAM_*_LIMIT"
                ))
            );
        }
    }

    #[test]
    fn agent_ingress_limits_are_bounded_and_hierarchical() {
        assert!(validate_agent_ingress_rate_limits(6_000, 600).is_ok());
        for (tenant, principal) in [
            (0, 1),
            (1, 0),
            (500, 501),
            (1_000_001, 1),
            (100_000, 100_001),
        ] {
            assert_eq!(
                validate_agent_ingress_rate_limits(tenant, principal),
                Err(SettingsError::Invalid(
                    "COPILOTMAIL_AGENT_INGRESS_*_REQUESTS_PER_MINUTE"
                ))
            );
        }
    }

    #[test]
    fn mail_data_plane_identity_is_required_and_all_credentials_share_one_origin() {
        let settings = |base_url: &str| MailEngineSettings {
            base_url: base_url.into(),
            api_token_file: PathBuf::from("/run/secrets/mail-token"),
            timeout: Duration::from_secs(10),
            allow_insecure_http: false,
        };
        let id = Uuid::parse_str("018f305d-3d0f-7b2a-9d68-7f4a3f6e2101").unwrap();
        let management = settings("https://mail.example.test");
        let content = settings("https://mail.example.test/");
        assert_eq!(
            validate_mail_data_plane_configuration(
                Some(id),
                Some("https://mail.example.test"),
                [Some(&management), Some(&content), None, None],
                true,
            )
            .unwrap(),
            Some("https://mail.example.test".into())
        );
        assert_eq!(
            validate_mail_data_plane_configuration(
                None,
                Some("https://mail.example.test"),
                [Some(&management), None, None, None],
                false,
            ),
            Err(SettingsError::Missing("COPILOTMAIL_MAIL_DATA_PLANE_ID"))
        );
        assert_eq!(
            validate_mail_data_plane_configuration(
                Some(Uuid::nil()),
                Some("https://mail.example.test"),
                [None, None, None, None],
                true,
            ),
            Err(SettingsError::Invalid("COPILOTMAIL_MAIL_DATA_PLANE_ID"))
        );
        assert_eq!(
            validate_mail_data_plane_configuration(
                Some(id),
                Some("ftp://mail.example.test"),
                [None, None, None, None],
                true,
            ),
            Err(SettingsError::Invalid("COPILOTMAIL_MAIL_DATA_PLANE_ORIGIN"))
        );
        let foreign = settings("https://other-mail.example.test");
        assert_eq!(
            validate_mail_data_plane_configuration(
                Some(id),
                Some("https://mail.example.test"),
                [Some(&management), Some(&foreign), None, None],
                false,
            ),
            Err(SettingsError::Invalid("COPILOTMAIL_MAIL_*_BASE_URL"))
        );
    }

    #[test]
    fn human_browser_clients_require_exact_secure_origins_and_bounded_scopes() {
        let client = validate_human_browser_client(
            "mail",
            "copilotmail-mail".into(),
            "https://mail.company.example".into(),
            "openid profile email".into(),
            false,
        )
        .unwrap();
        assert_eq!(client.public_origin, "https://mail.company.example");

        for origin in [
            "http://mail.company.example",
            "https://user:secret@mail.company.example",
            "https://mail.company.example/path",
            "https://mail.company.example?token=secret",
        ] {
            assert!(
                validate_human_browser_client(
                    "mail",
                    "copilotmail-mail".into(),
                    origin.into(),
                    "openid profile email".into(),
                    false,
                )
                .is_err(),
                "{origin} must fail closed"
            );
        }
        assert!(
            validate_human_browser_client(
                "mail",
                "copilotmail-mail".into(),
                "http://127.0.0.1:8084".into(),
                "openid profile email".into(),
                true,
            )
            .is_ok()
        );
        assert!(
            validate_human_browser_client(
                "mail",
                "copilotmail-mail".into(),
                "http://localhost:8084".into(),
                "openid profile email".into(),
                true,
            )
            .is_ok()
        );
        assert!(
            validate_human_browser_client(
                "mail",
                "copilotmail-mail".into(),
                "http://192.0.2.10:8084".into(),
                "openid profile email".into(),
                true,
            )
            .is_err()
        );

        let admin = validate_human_browser_client(
            "admin",
            "copilotmail-admin".into(),
            "https://admin.company.example".into(),
            "openid profile email".into(),
            false,
        )
        .unwrap();
        let mail = validate_human_browser_client(
            "mail",
            "copilotmail-mail".into(),
            "https://mail.company.example".into(),
            "openid profile email".into(),
            false,
        )
        .unwrap();
        assert!(human_browser_cookie_hosts_are_distinct(&[
            admin.clone(),
            mail
        ]));
        let same_host = validate_human_browser_client(
            "mail",
            "copilotmail-mail".into(),
            "https://admin.company.example:8443".into(),
            "openid profile email".into(),
            false,
        )
        .unwrap();
        assert!(!human_browser_cookie_hosts_are_distinct(&[
            admin, same_host
        ]));
    }

    #[test]
    fn audit_object_prefix_rejects_ambiguous_or_traversing_paths() {
        assert!(valid_audit_object_prefix("audit/2026"));

        for invalid in [
            "",
            "/audit",
            "audit/",
            "audit//2026",
            "audit/./2026",
            "audit/../2026",
            r"audit\2026",
            "audit/\n2026",
        ] {
            assert!(
                !valid_audit_object_prefix(invalid),
                "{invalid:?} must be rejected"
            );
        }
    }

    #[test]
    fn otlp_exporter_requires_a_bounded_secure_production_endpoint() {
        let secure = OtlpTraceSettings::from_values(
            Some("https://otel.example.test:4318".into()),
            "copilotmail-control".into(),
            "production".into(),
            false,
            10,
        )
        .unwrap()
        .unwrap();
        assert_eq!(secure.timeout, std::time::Duration::from_secs(10));
        assert_eq!(secure.endpoint, "https://otel.example.test:4318/v1/traces");

        for (endpoint, allow_insecure, environment) in [
            ("http://otel.example.test:4318", false, "development"),
            ("http://otel.example.test:4318", true, "production"),
            ("https://user:secret@otel.example.test", false, "production"),
            (
                "https://otel.example.test?token=secret",
                false,
                "production",
            ),
            ("file:///tmp/collector", false, "development"),
        ] {
            assert_eq!(
                OtlpTraceSettings::from_values(
                    Some(endpoint.into()),
                    "copilotmail-control".into(),
                    environment.into(),
                    allow_insecure,
                    10,
                ),
                Err(SettingsError::Invalid(
                    "COPILOTMAIL_OTEL_EXPORTER_OTLP_ENDPOINT"
                )),
                "{endpoint} must fail closed"
            );
        }
    }
}
