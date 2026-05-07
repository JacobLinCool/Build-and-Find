use crate::error::NanowebError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,

    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,

    pub static_dir: Option<PathBuf>,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,

    pub session_secret: Option<String>,

    #[serde(default)]
    pub routes: Vec<RouteConfig>,

    #[serde(default)]
    pub middleware: MiddlewareConfig,

    #[serde(default = "default_healthz_path")]
    pub healthz_path: String,

    #[serde(default = "default_readyz_path")]
    pub readyz_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub handler: String,
    #[serde(default)]
    pub middleware: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MiddlewareConfig {
    #[serde(default)]
    pub logging: bool,

    #[serde(default)]
    pub compression: bool,

    #[serde(default)]
    pub cors: Option<CorsConfig>,

    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,

    #[serde(default)]
    pub csrf: bool,

    #[serde(default)]
    pub basic_auth: Option<BasicAuthConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    #[serde(default = "default_cors_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(default = "default_cors_methods")]
    pub allowed_methods: Vec<String>,
    #[serde(default)]
    pub allowed_headers: Vec<String>,
    #[serde(default)]
    pub allow_credentials: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub max_requests: u32,
    pub refill_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicAuthConfig {
    pub realm: String,
    pub username: String,
    pub password: String,
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
fn default_healthz_path() -> String {
    "/healthz".to_string()
}
fn default_readyz_path() -> String {
    "/readyz".to_string()
}
fn default_cors_origins() -> Vec<String> {
    vec!["*".to_string()]
}
fn default_cors_methods() -> Vec<String> {
    vec!["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"]
        .into_iter()
        .map(String::from)
        .collect()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            tls_cert: None,
            tls_key: None,
            metrics_path: default_metrics_path(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: None,
            routes: Vec::new(),
            middleware: MiddlewareConfig::default(),
            healthz_path: default_healthz_path(),
            readyz_path: default_readyz_path(),
        }
    }
}

impl Config {
    /// Load from a TOML file, then overlay CLI overrides.
    pub fn load(
        path: Option<&PathBuf>,
        overrides: &HashMap<String, String>,
    ) -> Result<Self, NanowebError> {
        let mut config = if let Some(p) = path {
            if p.exists() {
                let content = std::fs::read_to_string(p)
                    .map_err(|e| NanowebError::Config(format!("Cannot read config file: {}", e)))?;
                toml::from_str(&content)
                    .map_err(|e| NanowebError::Config(format!("Invalid TOML: {}", e)))?
            } else {
                Config::default()
            }
        } else {
            // Try default location
            let default_path = PathBuf::from("nanoweb.toml");
            if default_path.exists() {
                let content = std::fs::read_to_string(&default_path)
                    .map_err(|e| NanowebError::Config(format!("Cannot read config file: {}", e)))?;
                toml::from_str(&content)
                    .map_err(|e| NanowebError::Config(format!("Invalid TOML: {}", e)))?
            } else {
                Config::default()
            }
        };

        // Apply CLI overrides
        config.apply_overrides(overrides);
        Ok(config)
    }

    fn apply_overrides(&mut self, overrides: &HashMap<String, String>) {
        if let Some(v) = overrides.get("listen") {
            self.listen = v.clone();
        }
        if let Some(v) = overrides.get("tls_cert") {
            self.tls_cert = Some(PathBuf::from(v));
        }
        if let Some(v) = overrides.get("tls_key") {
            self.tls_key = Some(PathBuf::from(v));
        }
        if let Some(v) = overrides.get("metrics") {
            self.metrics_path = v.clone();
        }
        if let Some(v) = overrides.get("static_dir") {
            self.static_dir = Some(PathBuf::from(v));
        }
        if let Some(v) = overrides.get("log_level") {
            self.log_level = v.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let c = Config::default();
        assert_eq!(c.listen, "127.0.0.1:8080");
        assert_eq!(c.metrics_path, "/metrics");
        assert_eq!(c.graceful_timeout_s, 30);
    }

    #[test]
    fn test_config_from_toml() {
        let toml_str = r#"
listen = "0.0.0.0:9090"
log_level = "debug"
graceful_timeout_s = 60
"#;
        let c: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(c.listen, "0.0.0.0:9090");
        assert_eq!(c.log_level, "debug");
        assert_eq!(c.graceful_timeout_s, 60);
    }

    #[test]
    fn test_overrides() {
        let mut overrides = HashMap::new();
        overrides.insert("listen".to_string(), "0.0.0.0:3000".to_string());
        let config = Config::load(None, &overrides).unwrap();
        assert_eq!(config.listen, "0.0.0.0:3000");
    }
}
