use crate::error::NanowebError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,

    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default)]
    pub tls: Option<TlsConfig>,

    #[serde(default)]
    pub session: SessionConfig,

    #[serde(default)]
    pub static_files: Vec<StaticFilesConfig>,

    #[serde(default)]
    pub middleware: Vec<MiddlewareConfig>,

    #[serde(default)]
    pub static_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert: String,
    pub key: String,
    #[serde(default = "default_tls_listen")]
    pub listen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(default = "default_session_secret")]
    pub secret: String,
    #[serde(default = "default_session_cookie_name")]
    pub cookie_name: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            secret: default_session_secret(),
            cookie_name: default_session_cookie_name(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticFilesConfig {
    pub route: String,
    pub dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MiddlewareConfig {
    Logging,
    Gzip {
        #[serde(default = "default_gzip_min_size")]
        min_size: usize,
    },
    RateLimit {
        #[serde(default = "default_rate_limit_rps")]
        requests_per_second: f64,
    },
    Cors {
        #[serde(default = "default_allowed_origins")]
        allowed_origins: Vec<String>,
        #[serde(default)]
        allow_credentials: bool,
    },
    Csrf,
    BasicAuth {
        realm: String,
        username: String,
        password: String,
    },
}

fn default_listen() -> String {
    "127.0.0.1:8080".to_string()
}

fn default_graceful_timeout() -> u64 {
    30
}

fn default_metrics_path() -> String {
    "/metrics".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_tls_listen() -> String {
    "127.0.0.1:8443".to_string()
}

fn default_session_secret() -> String {
    "change-me-in-production".to_string()
}

fn default_session_cookie_name() -> String {
    "nanoweb_session".to_string()
}

fn default_gzip_min_size() -> usize {
    1024
}

fn default_rate_limit_rps() -> f64 {
    100.0
}

fn default_allowed_origins() -> Vec<String> {
    vec!["*".to_string()]
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: default_listen(),
            graceful_timeout_s: default_graceful_timeout(),
            metrics_path: default_metrics_path(),
            log_level: default_log_level(),
            tls: None,
            session: SessionConfig::default(),
            static_files: Vec::new(),
            middleware: Vec::new(),
            static_dir: None,
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, NanowebError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            NanowebError::Config(format!("Cannot read config file '{}': {}", path, e))
        })?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| NanowebError::Config(format!("Invalid config: {}", e)))?;
        Ok(config)
    }

    pub fn from_file_or_default(path: &str) -> Self {
        match Self::from_file(path) {
            Ok(c) => c,
            Err(_) => Self::default(),
        }
    }

    pub fn apply_cli_overrides(
        &mut self,
        listen: Option<String>,
        tls_cert: Option<String>,
        tls_key: Option<String>,
        metrics: Option<String>,
        static_dir: Option<String>,
        log_level: Option<String>,
    ) {
        if let Some(l) = listen {
            self.listen = l;
        }
        if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
            self.tls = Some(TlsConfig {
                cert,
                key,
                listen: default_tls_listen(),
            });
        }
        if let Some(m) = metrics {
            self.metrics_path = m;
        }
        if let Some(d) = static_dir {
            self.static_dir = Some(d);
        }
        if let Some(l) = log_level {
            self.log_level = l;
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
        assert_eq!(config.graceful_timeout_s, 30);
        assert_eq!(config.metrics_path, "/metrics");
    }

    #[test]
    fn test_config_from_toml_string() {
        let toml = r#"
listen = "0.0.0.0:9090"
graceful_timeout_s = 60
metrics_path = "/prometheus"
log_level = "debug"

[session]
secret = "my-secret"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9090");
        assert_eq!(config.graceful_timeout_s, 60);
        assert_eq!(config.session.secret, "my-secret");
    }

    #[test]
    fn test_cli_override() {
        let mut config = Config::default();
        config.apply_cli_overrides(
            Some("0.0.0.0:3000".into()),
            None,
            None,
            Some("/prom".into()),
            None,
            Some("debug".into()),
        );
        assert_eq!(config.listen, "0.0.0.0:3000");
        assert_eq!(config.metrics_path, "/prom");
        assert_eq!(config.log_level, "debug");
    }
}
