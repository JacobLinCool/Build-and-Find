use crate::cli::Cli;
use crate::error::{NanowebError, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_metrics")]
    pub metrics: String,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub static_dir: Option<String>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_graceful")]
    pub graceful_timeout_s: u64,
    #[serde(default = "default_secret")]
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
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub json: Option<serde_json::Value>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub context: Option<serde_json::Value>,
    #[serde(default)]
    pub static_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiddlewareConfig {
    #[serde(default)]
    pub logging: bool,
    #[serde(default)]
    pub gzip: bool,
    #[serde(default)]
    pub csrf: bool,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    pub basic_auth: Option<BasicAuthConfig>,
    pub rate_limit: Option<RateLimitConfig>,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            logging: true,
            gzip: false,
            csrf: false,
            cors_origins: vec![],
            basic_auth: None,
            rate_limit: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthConfig {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rate_capacity")]
    pub capacity: u32,
    #[serde(default = "default_rate_refill")]
    pub refill_per_s: f64,
}

pub fn load(cli: &Cli) -> Result<Config> {
    let config_path = cli.config.as_deref().unwrap_or("./nanoweb.toml");
    let mut cfg = if Path::new(config_path).exists() {
        let text = std::fs::read_to_string(config_path)?;
        toml::from_str(&text).map_err(|e| NanowebError::Config(e.to_string()))?
    } else {
        Config::default()
    };
    if let Some(v) = &cli.listen {
        cfg.listen = v.clone();
    }
    if let Some(v) = &cli.metrics {
        cfg.metrics = v.clone();
    }
    if let Some(v) = &cli.tls_cert {
        cfg.tls_cert = Some(v.clone());
    }
    if let Some(v) = &cli.tls_key {
        cfg.tls_key = Some(v.clone());
    }
    if let Some(v) = &cli.static_dir {
        cfg.static_dir = Some(v.clone());
    }
    if let Some(v) = &cli.log_level {
        cfg.log_level = v.clone();
    }
    Ok(cfg)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            metrics: default_metrics(),
            tls_cert: None,
            tls_key: None,
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful(),
            session_secret: default_secret(),
            routes: vec![RouteConfig {
                method: "GET".into(),
                path: "/".into(),
                text: Some("nanoweb".into()),
                json: None,
                template: None,
                context: None,
                static_prefix: None,
            }],
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
fn default_secret() -> String {
    "change-me".into()
}
fn default_rate_capacity() -> u32 {
    60
}
fn default_rate_refill() -> f64 {
    1.0
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_has_route() {
        assert_eq!(super::Config::default().routes.len(), 1);
    }
}
