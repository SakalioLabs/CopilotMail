use anyhow::{Context, Result};
use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub smtp_addr: SocketAddr,
    pub database: PathBuf,
    pub domain: String,
    pub jwt_secret: String,
    pub admin_key: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            http_addr: value("COPILOTMAIL_HTTP_ADDR", "0.0.0.0:8080")
                .parse()
                .context("invalid HTTP address")?,
            smtp_addr: value("COPILOTMAIL_SMTP_ADDR", "0.0.0.0:2525")
                .parse()
                .context("invalid SMTP address")?,
            database: value("COPILOTMAIL_DATABASE", "./data/copilotmail.db").into(),
            domain: value("COPILOTMAIL_DOMAIN", "mail.local").to_ascii_lowercase(),
            jwt_secret: required("COPILOTMAIL_JWT_SECRET")?,
            admin_key: required("COPILOTMAIL_ADMIN_KEY")?,
        })
    }
}

fn value(name: &str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.into())
}
fn required(name: &str) -> Result<String> {
    let value = env::var(name).with_context(|| format!("{name} is required"))?;
    anyhow::ensure!(value.len() >= 16, "{name} must be at least 16 characters");
    Ok(value)
}
