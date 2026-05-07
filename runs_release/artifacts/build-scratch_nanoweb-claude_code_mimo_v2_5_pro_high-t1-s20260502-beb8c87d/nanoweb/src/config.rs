use crate::error::NanowebError;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
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

    #[serde(default = "default_session_secret")]
    pub session_secret: String,

    #[serde(default)]
    pub allowed_origins: Vec<String>,

    #[serde(default = "default_rate_limit")]
    pub rate_limit: f64,

    #[serde(default)]
    pub basic_auth_user: Option<String>,

    #[serde(default)]
    pub basic_auth_pass: Option<String>,
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

fn default_session_secret() -> String {
    "change-me-in-production".to_string()
}

fn default_rate_limit() -> f64 {
    100.0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics_path(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: default_session_secret(),
            allowed_origins: vec!["*".to_string()],
            rate_limit: default_rate_limit(),
            basic_auth_user: None,
            basic_auth_pass: None,
        }
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Self, NanowebError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| NanowebError::Config(format!("Cannot read config file {path}: {e}")))?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn merge_cli(&mut self, cli: &CliConfig) {
        if cli.listen != default_listen() {
            self.listen = cli.listen.clone();
        }
        if cli.tls_cert.is_some() {
            self.tls_cert = cli.tls_cert.clone();
        }
        if cli.tls_key.is_some() {
            self.tls_key = cli.tls_key.clone();
        }
        if cli.metrics != default_metrics_path() {
            self.metrics = cli.metrics.clone();
        }
        if cli.static_dir.is_some() {
            self.static_dir = cli.static_dir.clone();
        }
        if cli.log_level != default_log_level() {
            self.log_level = cli.log_level.clone();
        }
    }
}

#[derive(Debug, Clone)]
pub struct CliConfig {
    pub listen: String,
    pub config: PathBuf,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub metrics: String,
    pub static_dir: Option<PathBuf>,
    pub log_level: String,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            config: PathBuf::from("./nanoweb.toml"),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics_path(),
            static_dir: None,
            log_level: default_log_level(),
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
        assert_eq!(config.metrics, "/metrics");
    }

    #[test]
    fn test_load_from_toml() {
        let dir = std::env::temp_dir().join("nanoweb_config_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.toml");
        std::fs::write(&path, "listen = \"0.0.0.0:3000\"\nlog_level = \"debug\"\n").unwrap();
        let config = Config::load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.listen, "0.0.0.0:3000");
        assert_eq!(config.log_level, "debug");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_cli_override() {
        let mut config = Config::default();
        let cli = CliConfig {
            listen: "0.0.0.0:9090".to_string(),
            ..Default::default()
        };
        config.merge_cli(&cli);
        assert_eq!(config.listen, "0.0.0.0:9090");
    }
}
