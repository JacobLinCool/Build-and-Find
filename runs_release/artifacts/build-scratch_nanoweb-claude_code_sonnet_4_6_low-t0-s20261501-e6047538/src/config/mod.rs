use crate::error::NanowebError;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionConfig {
    pub secret: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            secret: "change-me-in-production".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        CorsConfig {
            allowed_origins: vec!["*".to_string()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RateLimitConfig {
    pub requests_per_second: f64,
    pub burst: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        RateLimitConfig {
            requests_per_second: 100.0,
            burst: 200.0,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BasicAuthConfig {
    pub username: String,
    pub password: String,
}

impl Default for BasicAuthConfig {
    fn default() -> Self {
        BasicAuthConfig {
            username: "admin".to_string(),
            password: "secret".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,

    #[serde(default)]
    pub static_dir: Option<String>,

    #[serde(default)]
    pub tls_cert: Option<String>,

    #[serde(default)]
    pub tls_key: Option<String>,

    #[serde(default)]
    pub session: SessionConfig,

    #[serde(default)]
    pub cors: CorsConfig,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    #[serde(default)]
    pub basic_auth: Option<BasicAuthConfig>,
}

fn default_listen() -> String {
    "127.0.0.1:8080".to_string()
}
fn default_graceful_timeout() -> u64 {
    30
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_metrics_path() -> String {
    "/metrics".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: default_listen(),
            graceful_timeout_s: default_graceful_timeout(),
            log_level: default_log_level(),
            metrics_path: default_metrics_path(),
            static_dir: None,
            tls_cert: None,
            tls_key: None,
            session: SessionConfig::default(),
            cors: CorsConfig::default(),
            rate_limit: RateLimitConfig::default(),
            basic_auth: None,
        }
    }
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, NanowebError> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(NanowebError::from)
    }

    pub fn from_str(s: &str) -> Result<Self, NanowebError> {
        toml::from_str(s).map_err(NanowebError::from)
    }

    /// Merge CLI overrides: if a CLI value is Some, use it; otherwise keep self
    pub fn apply_overrides(mut self, overrides: ConfigOverrides) -> Self {
        if let Some(v) = overrides.listen {
            self.listen = v;
        }
        if let Some(v) = overrides.log_level {
            self.log_level = v;
        }
        if let Some(v) = overrides.tls_cert {
            self.tls_cert = Some(v);
        }
        if let Some(v) = overrides.tls_key {
            self.tls_key = Some(v);
        }
        if let Some(v) = overrides.metrics_path {
            self.metrics_path = v;
        }
        if let Some(v) = overrides.static_dir {
            self.static_dir = Some(v);
        }
        self
    }
}

/// CLI overrides for config values
#[derive(Debug, Default)]
pub struct ConfigOverrides {
    pub listen: Option<String>,
    pub log_level: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub metrics_path: Option<String>,
    pub static_dir: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.listen, "127.0.0.1:8080");
        assert_eq!(cfg.graceful_timeout_s, 30);
        assert_eq!(cfg.log_level, "info");
    }

    #[test]
    fn test_parse_toml() {
        let toml = r#"
listen = "0.0.0.0:9000"
log_level = "debug"

[session]
secret = "my-secret"
"#;
        let cfg = Config::from_str(toml).unwrap();
        assert_eq!(cfg.listen, "0.0.0.0:9000");
        assert_eq!(cfg.log_level, "debug");
        assert_eq!(cfg.session.secret, "my-secret");
    }

    #[test]
    fn test_apply_overrides() {
        let cfg = Config::default();
        let overrides = ConfigOverrides {
            listen: Some("0.0.0.0:3000".to_string()),
            ..Default::default()
        };
        let cfg = cfg.apply_overrides(overrides);
        assert_eq!(cfg.listen, "0.0.0.0:3000");
    }
}
