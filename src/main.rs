mod config;
mod events;
mod outbound;
mod smtp;
mod store;

use anyhow::Result;
use config::Config;
use store::Store;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        let addr = std::env::var("COPILOTMAIL_HTTP_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".into())
            .replace("0.0.0.0", "127.0.0.1");
        tokio::net::TcpStream::connect(addr).await?;
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::from_env()?;
    let store = Store::open(&config.database)?;
    let events = events::Events::new();
    let smtp_task = tokio::spawn(smtp::serve(
        config.smtp_addr,
        store.clone(),
        config.domain.clone(),
        events.clone(),
    ));
    tracing::info!(http=%config.http_addr,smtp=%config.smtp_addr,"CopilotMail starting");
    let api_task = tokio::spawn(api::serve(config.clone(), store, events));
    tokio::select! { result=smtp_task => result??, result=api_task => result?? }
    Ok(())
}

mod api;
