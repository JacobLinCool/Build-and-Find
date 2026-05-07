use crate::cli::CliArgs;
use crate::error::{NanowebError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct RawConfig {
    pub listen: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_listen: Option<String>,
    pub metrics: Option<String>,
    pub static_dir: Option<PathBuf>,
    pub log_level: Option<String>,
    pub graceful_timeout_s: Option<u64>,
    pub session_secret: Option<String>,
    pub allowed_origins: Option<Vec<String>>,
    pub rate_limit_capacity: Option<f64>,
    pub rate_limit_refill_per_sec: Option<f64>,
    pub basic_auth_realm: Option<String>,
    pub basic_auth_users: Option<Vec<RawUser>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawUser {
    pub user: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: String,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_listen: Option<String>,
    pub metrics_path: String,
    pub static_dir: Option<PathBuf>,
    pub log_level: String,
    pub graceful_timeout_s: u64,
    pub session_secret: String,
    pub allowed_origins: Vec<String>,
    pub rate_limit_capacity: f64,
    pub rate_limit_refill_per_sec: f64,
    pub basic_auth_realm: Option<String>,
    pub basic_auth_users: Vec<(String, String)>,
}

impl Config {
    pub fn load(cli: &CliArgs) -> Result<Self> {
        let raw = if let Some(path) = cli.config.as_ref() {
            load_toml(path)?
        } else {
            // try default file, but missing default is ok
            let default = PathBuf::from("./nanoweb.toml");
            if default.exists() {
                load_toml(&default)?
            } else {
                RawConfig::default()
            }
        };
        // CLI overrides TOML.
        let listen = cli
            .listen
            .clone()
            .or(raw.listen)
            .unwrap_or_else(|| "127.0.0.1:8080".to_string());
        let tls_cert = cli.tls_cert.clone().or(raw.tls_cert);
        let tls_key = cli.tls_key.clone().or(raw.tls_key);
        let tls_listen = raw.tls_listen;
        let metrics_path = cli
            .metrics
            .clone()
            .or(raw.metrics)
            .unwrap_or_else(|| "/metrics".to_string());
        let static_dir = cli.static_dir.clone().or(raw.static_dir);
        let log_level = cli
            .log_level
            .clone()
            .or(raw.log_level)
            .unwrap_or_else(|| "info".to_string());
        let graceful_timeout_s = raw.graceful_timeout_s.unwrap_or(30);
        let session_secret = raw
            .session_secret
            .unwrap_or_else(|| "nanoweb-default-insecure-change-me".to_string());
        let allowed_origins = raw.allowed_origins.unwrap_or_else(|| vec!["*".to_string()]);
        let rate_limit_capacity = raw.rate_limit_capacity.unwrap_or(100.0);
        let rate_limit_refill_per_sec = raw.rate_limit_refill_per_sec.unwrap_or(50.0);
        let basic_auth_realm = raw.basic_auth_realm;
        let basic_auth_users = raw
            .basic_auth_users
            .unwrap_or_default()
            .into_iter()
            .map(|u| (u.user, u.password))
            .collect();

        // Validate.
        if (tls_cert.is_some()) ^ (tls_key.is_some()) {
            return Err(NanowebError::InvalidConfig(
                "tls_cert and tls_key must be set together".into(),
            ));
        }

        Ok(Config {
            listen,
            tls_cert,
            tls_key,
            tls_listen,
            metrics_path,
            static_dir,
            log_level,
            graceful_timeout_s,
            session_secret,
            allowed_origins,
            rate_limit_capacity,
            rate_limit_refill_per_sec,
            basic_auth_realm,
            basic_auth_users,
        })
    }

    pub fn tls_enabled(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }

    pub fn tls_listen_addr(&self) -> Option<String> {
        if !self.tls_enabled() {
            return None;
        }
        Some(
            self.tls_listen
                .clone()
                .unwrap_or_else(|| "0.0.0.0:8443".to_string()),
        )
    }
}

fn load_toml(path: &Path) -> Result<RawConfig> {
    let s = std::fs::read_to_string(path)
        .map_err(|e| NanowebError::Config(format!("reading {:?}: {}", path, e)))?;
    let raw: RawConfig = toml::from_str(&s)?;
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_toml() {
        let cli = CliArgs {
            listen: Some("1.2.3.4:9999".into()),
            config: None,
            tls_cert: None,
            tls_key: None,
            metrics: None,
            static_dir: None,
            log_level: None,
        };
        let cfg = Config::load(&cli).unwrap();
        assert_eq!(cfg.listen, "1.2.3.4:9999");
    }
}
