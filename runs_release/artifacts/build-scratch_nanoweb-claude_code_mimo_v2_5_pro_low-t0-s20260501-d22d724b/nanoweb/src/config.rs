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

    #[serde(default = "default_metrics")]
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
    pub tls_listen: Option<String>,

    #[serde(default)]
    pub healthz_path: Option<String>,

    #[serde(default)]
    pub readyz_path: Option<String>,

    #[serde(default)]
    pub metrics_path: Option<String>,
}

fn default_listen() -> String {
    "127.0.0.1:8080".to_string()
}

fn default_metrics() -> String {
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

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: default_session_secret(),
            tls_listen: None,
            healthz_path: None,
            readyz_path: None,
            metrics_path: None,
        }
    }
}

impl Config {
    pub fn load(path: &str) -> Result<Self, NanowebError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                NanowebError::Config(format!("Config file not found: {path}"))
            } else {
                NanowebError::Config(format!("Failed to read config: {e}"))
            }
        })?;
        toml::from_str(&content).map_err(|e| NanowebError::Config(format!("Invalid TOML: {e}")))
    }

    pub fn merge_cli(&mut self, cli: &CliArgs) {
        if cli.listen != default_listen() {
            self.listen = cli.listen.clone();
        }
        if cli.config.as_deref() != Some("./nanoweb.toml") {
            // Config path is handled separately
        }
        if cli.tls_cert.is_some() {
            self.tls_cert = cli.tls_cert.clone();
        }
        if cli.tls_key.is_some() {
            self.tls_key = cli.tls_key.clone();
        }
        if cli.metrics != default_metrics() {
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
pub struct CliArgs {
    pub listen: String,
    pub config: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub metrics: String,
    pub static_dir: Option<PathBuf>,
    pub log_level: String,
    pub version: bool,
    pub help: bool,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            config: Some("./nanoweb.toml".to_string()),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics(),
            static_dir: None,
            log_level: default_log_level(),
            version: false,
            help: false,
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
    }

    #[test]
    fn test_load_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nanoweb.toml");
        std::fs::write(
            &path,
            r#"
listen = "0.0.0.0:3000"
log_level = "debug"
graceful_timeout_s = 60
"#,
        )
        .unwrap();

        let config = Config::load(path.to_str().unwrap()).unwrap();
        assert_eq!(config.listen, "0.0.0.0:3000");
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.graceful_timeout_s, 60);
    }

    #[test]
    fn test_cli_override() {
        let mut config = Config::default();
        let cli = CliArgs {
            listen: "0.0.0.0:9090".to_string(),
            log_level: "debug".to_string(),
            ..Default::default()
        };
        config.merge_cli(&cli);
        assert_eq!(config.listen, "0.0.0.0:9090");
        assert_eq!(config.log_level, "debug");
    }
}
