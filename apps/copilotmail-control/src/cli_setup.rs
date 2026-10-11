use crate::*;

/// Connects the V2 control-plane store using validated process settings.
pub async fn connect_store(settings: &ControlPlaneSettings) -> Result<PgControlPlaneStore> {
    PgControlPlaneStore::connect(settings.database_url(), settings.database_max_connections)
        .await
        .context("connect to CopilotMail PostgreSQL control plane")
}

/// Runs versioned PostgreSQL migrations and explicitly binds the configured
/// data-plane ID to its canonical origin. Serving processes never perform this
/// first-write operation, so a configuration typo cannot claim an identity.
pub async fn run_migrations(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
) -> Result<()> {
    store
        .migrate()
        .await
        .context("apply CopilotMail control-plane migrations")?;
    if let Some((data_plane_id, origin)) = configured_mail_data_plane(settings)? {
        anyhow::ensure!(
            store
                .bind_mail_data_plane_origin(
                    data_plane_id,
                    MAIL_DATA_PLANE_ADAPTER_KIND,
                    origin,
                    Utc::now(),
                )
                .await
                .context("bind configured mail data-plane canonical origin")?,
            "configured mail data-plane ID is not registered for this adapter and canonical origin"
        );
    }
    Ok(())
}

/// Verifies database connectivity and migration readiness.
pub async fn run_check(settings: &ControlPlaneSettings, store: &PgControlPlaneStore) -> Result<()> {
    store
        .ping()
        .await
        .context("control-plane PostgreSQL ping failed")?;
    anyhow::ensure!(
        store.schema_ready().await?,
        "control-plane schema is not migrated"
    );
    ensure_mail_data_plane_registration(
        store,
        settings.mail_data_plane_id.map(MailDataPlaneId::from_uuid),
        settings.mail_data_plane_origin.as_deref(),
    )
    .await?;
    tracing::info!("control-plane dependencies are ready");
    Ok(())
}

/// Performs an authenticated, protocol-level JMAP data-plane probe.
pub async fn run_check_mail_engine(
    settings: &ControlPlaneSettings,
    store: &PgControlPlaneStore,
) -> Result<()> {
    run_check(settings, store).await?;
    let engine_settings = settings
        .mail_engine
        .as_ref()
        .context("mail engine is not configured")?;
    let engine = build_mail_engine(engine_settings, configured_mail_data_plane_id(settings)?)?;
    let readiness = engine
        .probe()
        .await
        .context("mail engine JMAP probe failed")?;
    tracing::info!(
        username = %readiness.username,
        api_origin = %readiness.api_url.origin().ascii_serialization(),
        session_state = %readiness.session_state,
        mail_capability_advertised = readiness.mail_capability_advertised,
        "mail data plane is ready"
    );
    Ok(())
}

/// Resolves and evaluates one domain without persisting or activating it.
pub async fn run_check_domain_dns(
    domain: String,
    ownership_token_file: String,
    dkim_selector: String,
) -> Result<()> {
    let domain = domain
        .parse::<copilotmail_domain::MailDomainName>()
        .context("validate managed mail domain")?;
    let token = read_bounded_secret_file(FsPath::new(&ownership_token_file), 1024).await?;
    let token = std::str::from_utf8(&token)
        .context("ownership token file is not UTF-8")?
        .trim();
    let lookup = HickoryDnsLookup::from_system().context("initialize system DNS resolver")?;
    let config = DnsVerifierConfig::new(dkim_selector, std::time::Duration::from_secs(10))
        .context("validate DNS verifier configuration")?;
    let report = DnsVerifier::new(lookup, config)
        .verify(&domain, token, Utc::now())
        .await
        .context("verify managed mail domain DNS")?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

pub(crate) fn configured_mail_data_plane_id(
    settings: &ControlPlaneSettings,
) -> Result<MailDataPlaneId> {
    settings
        .mail_data_plane_id
        .map(MailDataPlaneId::from_uuid)
        .context("mail data-plane ID is not configured")
}

pub(crate) fn configured_mail_data_plane(
    settings: &ControlPlaneSettings,
) -> Result<Option<(MailDataPlaneId, &str)>> {
    match (
        settings.mail_data_plane_id,
        settings.mail_data_plane_origin.as_deref(),
    ) {
        (None, None) => Ok(None),
        (Some(data_plane_id), Some(origin)) => {
            Ok(Some((MailDataPlaneId::from_uuid(data_plane_id), origin)))
        }
        _ => anyhow::bail!("mail data-plane ID and canonical origin must be configured together"),
    }
}

pub(crate) const MAIL_DATA_PLANE_ADAPTER_KIND: &str = "stalwart_jmap";

pub(crate) const LEGACY_MAIL_DATA_PLANE_SOURCE: &str = "stalwart";

pub(crate) const LEGACY_MAIL_DATA_PLANE_ID: u128 = 0x018f_305d_3d0f_7b2a_9d68_7f4a_3f6e_2101;

pub(crate) fn is_legacy_mail_data_plane(data_plane_id: MailDataPlaneId) -> bool {
    data_plane_id.as_uuid().as_u128() == LEGACY_MAIL_DATA_PLANE_ID
}

pub(crate) fn mail_event_fingerprint_scheme(
    data_plane_id: MailDataPlaneId,
) -> Result<MailEventFingerprintScheme, MailEventIngestionError> {
    if is_legacy_mail_data_plane(data_plane_id) {
        MailEventFingerprintScheme::try_legacy_source(LEGACY_MAIL_DATA_PLANE_SOURCE)
    } else {
        Ok(MailEventFingerprintScheme::data_plane_v2())
    }
}

pub(crate) fn mail_data_plane_fingerprint_scheme(
    data_plane_id: MailDataPlaneId,
) -> MailDataPlaneFingerprintScheme {
    if is_legacy_mail_data_plane(data_plane_id) {
        MailDataPlaneFingerprintScheme::LegacyV1
    } else {
        MailDataPlaneFingerprintScheme::DataPlaneV2
    }
}

pub(crate) async fn ensure_mail_data_plane_registration(
    store: &PgControlPlaneStore,
    data_plane_id: Option<MailDataPlaneId>,
    origin: Option<&str>,
) -> Result<()> {
    let (data_plane_id, origin) = match (data_plane_id, origin) {
        (None, None) => return Ok(()),
        (Some(data_plane_id), Some(origin)) => (data_plane_id, origin),
        _ => anyhow::bail!("mail data-plane ID and canonical origin must be configured together"),
    };
    anyhow::ensure!(
        store
            .mail_data_plane_registered(data_plane_id, MAIL_DATA_PLANE_ADAPTER_KIND, origin)
            .await
            .context("verify configured mail data-plane registration")?,
        "configured mail data-plane ID is not registered for the Stalwart JMAP adapter and canonical origin"
    );
    Ok(())
}

pub(crate) fn build_mail_engine(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailEngine>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

pub(crate) fn build_mail_content_reader(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailContentReader>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

pub(crate) fn build_mail_draft_store(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailDraftStore>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

pub(crate) fn build_mail_draft_content_reader(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailDraftContentReader>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

pub(crate) fn build_mail_submission_adapter(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailSubmissionAdapter>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

pub(crate) fn build_mail_domain_projector(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<Arc<dyn MailDomainProjector>> {
    Ok(Arc::new(build_stalwart_client(settings, data_plane_id)?))
}

pub(crate) fn build_stalwart_client(
    settings: &MailEngineSettings,
    data_plane_id: MailDataPlaneId,
) -> Result<StalwartJmapClient> {
    let config = StalwartJmapConfig::new(
        &settings.base_url,
        settings.api_token_file.clone(),
        settings.timeout,
        settings.allow_insecure_http,
    )
    .context("validate mail engine configuration")?;
    StalwartJmapClient::new(data_plane_id, config).context("initialize mail engine adapter")
}
