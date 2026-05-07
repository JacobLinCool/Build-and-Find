//! Configuration loader.
//!
//! The single owner of "CLI overrides TOML" semantics. The supported
//! source formats are:
//!
//! * TOML (default)
//! * INI (legacy — selectable via `--config-format ini`; kept for
//!   compatibility and scheduled to be removed in a future release)

pub mod ini_loader;
pub mod toml_loader;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cli::CliOverrides;
use crate::error::NanowebError;
use crate::middleware::logger::LogLevel;

/// Which on-disk format is used for the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    Toml,
    /// Legacy INI format — slated for removal.
    Ini,
}

impl ConfigFormat {
    pub fn parse(raw: &str) -> Option<ConfigFormat> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "toml" => ConfigFormat::Toml,
            "ini" => ConfigFormat::Ini,
            _ => return None,
        })
    }
}

impl Default for ConfigFormat {
    fn default() -> Self {
        ConfigFormat::Toml
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DigestAuthConfigFile {
    pub enabled: bool,
    pub realm: String,
    pub users: Vec<DigestUserEntry>,
}

impl Default for DigestAuthConfigFile {
    fn default() -> Self {
        Self {
            enabled: false,
            realm: "nanoweb".into(),
            users: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DigestUserEntry {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub listen: String,
    pub static_dir: Option<PathBuf>,
    pub metrics: bool,
    pub log_level: String,
    pub graceful_timeout_s: u64,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            static_dir: None,
            metrics: true,
            log_level: "info".into(),
            graceful_timeout_s: 30,
            tls_cert: None,
            tls_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitConfigFile {
    pub enabled: bool,
    pub capacity: f64,
    pub refill_per_sec: f64,
}

impl Default for RateLimitConfigFile {
    fn default() -> Self {
        Self {
            enabled: true,
            capacity: 60.0,
            refill_per_sec: 30.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfigFile {
    pub secret: String,
    pub ttl_seconds: u64,
}

impl Default for SessionConfigFile {
    fn default() -> Self {
        Self {
            secret: "change-me".into(),
            ttl_seconds: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NanowebConfig {
    pub server: ServerConfig,
    pub rate_limit: RateLimitConfigFile,
    pub session: SessionConfigFile,
    pub digest_auth: DigestAuthConfigFile,
}

impl Default for NanowebConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            rate_limit: RateLimitConfigFile::default(),
            session: SessionConfigFile::default(),
            digest_auth: DigestAuthConfigFile::default(),
        }
    }
}

impl NanowebConfig {
    pub fn listen_addr(&self) -> Result<SocketAddr, NanowebError> {
        self.server
            .listen
            .parse::<SocketAddr>()
            .map_err(|e| NanowebError::Config(format!("invalid listen address: {}", e)))
    }

    pub fn log_level(&self) -> LogLevel {
        LogLevel::parse(&self.server.log_level).unwrap_or(LogLevel::Info)
    }
}

/// Load a config file, auto-detecting format unless overridden by the
/// CLI.
pub fn load(path: &Path, format: ConfigFormat) -> Result<NanowebConfig, NanowebError> {
    let bytes = std::fs::read(path).map_err(|e| NanowebError::Config(format!("{}: {}", path.display(), e)))?;
    let text = String::from_utf8(bytes).map_err(|_| NanowebError::Config("config file not utf-8".into()))?;
    match format {
        ConfigFormat::Toml => toml_loader::parse(&text),
        ConfigFormat::Ini => ini_loader::parse(&text),
    }
}

/// Apply the CLI overrides onto `cfg`. Only flags actually set by the
/// user replace their configured equivalents. This is the single place
/// in the codebase where that precedence is implemented.
pub fn apply_cli_overrides(cfg: &mut NanowebConfig, cli: &CliOverrides) {
    if let Some(v) = &cli.listen {
        cfg.server.listen = v.clone();
    }
    if let Some(v) = &cli.log_level {
        cfg.server.log_level = v.clone();
    }
    if let Some(v) = &cli.static_dir {
        cfg.server.static_dir = Some(PathBuf::from(v));
    }
    if let Some(v) = &cli.tls_cert {
        cfg.server.tls_cert = Some(PathBuf::from(v));
    }
    if let Some(v) = &cli.tls_key {
        cfg.server.tls_key = Some(PathBuf::from(v));
    }
    if let Some(v) = cli.metrics {
        cfg.server.metrics = v;
    }
}

/// Resolve the effective config from filesystem + CLI. This is the
/// canonical top-level entry point.
pub fn resolve(cli: &CliOverrides) -> Result<NanowebConfig, NanowebError> {
    let format = cli.config_format.unwrap_or_default();
    let mut cfg = match &cli.config_path {
        Some(path) => load(Path::new(path), format)?,
        None => NanowebConfig::default(),
    };
    apply_cli_overrides(&mut cfg, cli);
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_listen_overrides_toml() {
        let toml = r#"
[server]
listen = "0.0.0.0:1000"
"#;
        let mut cfg = toml_loader::parse(toml).unwrap();
        let cli = CliOverrides {
            listen: Some("127.0.0.1:9999".into()),
            ..Default::default()
        };
        apply_cli_overrides(&mut cfg, &cli);
        assert_eq!(cfg.server.listen, "127.0.0.1:9999");
    }

    #[test]
    fn unset_cli_leaves_defaults() {
        let mut cfg = NanowebConfig::default();
        apply_cli_overrides(&mut cfg, &CliOverrides::default());
        assert_eq!(cfg.server.listen, "127.0.0.1:8080");
    }

    #[test]
    fn config_format_parses() {
        assert_eq!(ConfigFormat::parse("toml"), Some(ConfigFormat::Toml));
        assert_eq!(ConfigFormat::parse("INI"), Some(ConfigFormat::Ini));
        assert!(ConfigFormat::parse("yaml").is_none());
    }
}
