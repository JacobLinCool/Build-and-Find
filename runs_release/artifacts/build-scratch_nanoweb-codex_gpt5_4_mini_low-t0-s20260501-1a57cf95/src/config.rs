use crate::error::NanowebError;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_metrics")]
    pub metrics: String,
    #[serde(default)]
    pub static_dir: Option<String>,
    #[serde(default)]
    pub tls_cert: Option<String>,
    #[serde(default)]
    pub tls_key: Option<String>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,
    #[serde(default)]
    pub session_secret: String,
    #[serde(default)]
    pub cors_allowed_origins: Vec<String>,
    #[serde(default)]
    pub basic_auth_user: Option<String>,
    #[serde(default)]
    pub basic_auth_pass: Option<String>,
}

fn default_listen() -> String {
    "127.0.0.1:8080".into()
}
fn default_metrics() -> String {
    "/metrics".into()
}
fn default_log_level() -> String {
    "info".into()
}
fn default_graceful_timeout() -> u64 {
    30
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            metrics: default_metrics(),
            static_dir: None,
            tls_cert: None,
            tls_key: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: "nanoweb-secret".into(),
            cors_allowed_origins: vec![],
            basic_auth_user: None,
            basic_auth_pass: None,
        }
    }
}

pub fn load(path: impl AsRef<Path>) -> Result<Config, NanowebError> {
    let content = std::fs::read_to_string(path).map_err(NanowebError::Io)?;
    let cfg: Config = toml::from_str(&content).map_err(|e| NanowebError::Config(e.to_string()))?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_works() {
        assert!(Config::default().listen.contains("8080"));
    }
}
