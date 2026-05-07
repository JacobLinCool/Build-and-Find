use crate::error::NanowebError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    #[serde(default)]
    pub tls_cert: Option<PathBuf>,

    #[serde(default)]
    pub tls_key: Option<PathBuf>,

    #[serde(default = "default_metrics_path")]
    pub metrics: String,

    #[serde(default)]
    pub static_dir: Option<PathBuf>,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,

    #[serde(default)]
    pub session_secret: String,

    #[serde(default)]
    pub cors: CorsConfig,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    #[serde(default)]
    pub routes: Vec<RouteConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: default_listen(),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics_path(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: String::new(),
            cors: CorsConfig::default(),
            rate_limit: RateLimitConfig::default(),
            routes: Vec::new(),
        }
    }
}

fn default_listen() -> String {
    "127.0.0.1:8080".to_string()
}

fn default_metrics_path() -> String {
    "/metrics".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_graceful_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub allow_credentials: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        CorsConfig {
            allowed_origins: vec![],
            allow_credentials: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rps")]
    pub requests_per_second: f64,
    #[serde(default = "default_burst")]
    pub burst: f64,
    #[serde(default)]
    pub enabled: bool,
}

fn default_rps() -> f64 {
    10.0
}

fn default_burst() -> f64 {
    20.0
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        RateLimitConfig {
            requests_per_second: default_rps(),
            burst: default_burst(),
            enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub response: String,
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub content_type: Option<String>,
}

fn default_status() -> u16 {
    200
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, NanowebError> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn apply_cli_overrides(&mut self, cli: &crate::cli::CliArgs) {
        if let Some(listen) = &cli.listen {
            self.listen = listen.clone();
        }
        if let Some(cert) = &cli.tls_cert {
            self.tls_cert = Some(cert.clone());
        }
        if let Some(key) = &cli.tls_key {
            self.tls_key = Some(key.clone());
        }
        if let Some(metrics) = &cli.metrics {
            self.metrics = metrics.clone();
        }
        if let Some(static_dir) = &cli.static_dir {
            self.static_dir = Some(static_dir.clone());
        }
        if let Some(log_level) = &cli.log_level {
            self.log_level = log_level.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.listen, "127.0.0.1:8080");
        assert_eq!(config.metrics, "/metrics");
        assert_eq!(config.log_level, "info");
        assert_eq!(config.graceful_timeout_s, 30);
    }

    #[test]
    fn test_config_from_toml() {
        let toml_str = r#"
listen = "0.0.0.0:9090"
log_level = "debug"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9090");
        assert_eq!(config.log_level, "debug");
    }
}
