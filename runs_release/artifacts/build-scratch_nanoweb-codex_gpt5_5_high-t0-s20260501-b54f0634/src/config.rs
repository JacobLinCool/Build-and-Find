use crate::{cli::Cli, error::Result};
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub config: PathBuf,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_listen: Option<String>,
    #[serde(default = "default_metrics")]
    pub metrics: String,
    pub static_dir: Option<PathBuf>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_graceful")]
    pub graceful_timeout_s: u64,
    #[serde(default = "default_session_secret")]
    pub session_secret: String,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub middleware: MiddlewareConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_content_type")]
    pub content_type: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MiddlewareConfig {
    #[serde(default)]
    pub logging: bool,
    #[serde(default)]
    pub gzip: bool,
    pub cors_allowed_origins: Option<Vec<String>>,
    pub basic_auth_username: Option<String>,
    pub basic_auth_password: Option<String>,
    pub rate_limit_capacity: Option<u32>,
    pub rate_limit_refill_per_sec: Option<f64>,
    #[serde(default)]
    pub csrf: bool,
}

impl Config {
    pub fn load(cli: Cli) -> Result<Self> {
        let config_path = cli
            .config
            .clone()
            .unwrap_or_else(|| PathBuf::from("./nanoweb.toml"));
        let mut config = if config_path.exists() {
            let text = fs::read_to_string(&config_path)?;
            toml::from_str::<Config>(&text)?
        } else {
            Config::default()
        };
        config.config = config_path;
        if let Some(v) = cli.listen {
            config.listen = v;
        }
        if let Some(v) = cli.tls_cert {
            config.tls_cert = Some(v);
        }
        if let Some(v) = cli.tls_key {
            config.tls_key = Some(v);
        }
        if let Some(v) = cli.metrics {
            config.metrics = v;
        }
        if let Some(v) = cli.static_dir {
            config.static_dir = Some(v);
        }
        if let Some(v) = cli.log_level {
            config.log_level = v;
        }
        Ok(config)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            config: PathBuf::from("./nanoweb.toml"),
            tls_cert: None,
            tls_key: None,
            tls_listen: None,
            metrics: default_metrics(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful(),
            session_secret: default_session_secret(),
            routes: Vec::new(),
            middleware: MiddlewareConfig::default(),
        }
    }
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

fn default_graceful() -> u64 {
    30
}

fn default_session_secret() -> String {
    "change-me".into()
}

fn default_status() -> u16 {
    200
}

fn default_content_type() -> String {
    "text/plain; charset=utf-8".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_defaults() {
        let cfg = Config::load(Cli {
            listen: Some("127.0.0.1:0".into()),
            ..Cli::default()
        })
        .unwrap();
        assert_eq!(cfg.listen, "127.0.0.1:0");
    }
}
