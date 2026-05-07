//! Configuration: TOML file + CLI overrides into a single Config struct.

use crate::cli::Args;
use crate::NanowebError;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FileConfig {
    pub listen: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub metrics: Option<String>,
    pub static_dir: Option<PathBuf>,
    pub log_level: Option<String>,
    pub graceful_timeout_s: Option<u64>,
    pub session_secret: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: String,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub metrics_path: String,
    pub static_dir: Option<PathBuf>,
    pub log_level: String,
    pub graceful_timeout_s: u64,
    pub session_secret: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            tls_cert: None,
            tls_key: None,
            metrics_path: "/metrics".into(),
            static_dir: None,
            log_level: "info".into(),
            graceful_timeout_s: 30,
            session_secret: "change-me-please-this-is-not-secret".into(),
        }
    }
}

pub fn load(args: &Args) -> Result<Config, NanowebError> {
    let mut cfg = Config::default();
    let cfg_path = &args.config;
    if std::path::Path::new(cfg_path).exists() {
        let text = std::fs::read_to_string(cfg_path).map_err(NanowebError::Io)?;
        let file: FileConfig = toml::from_str(&text)?;
        if let Some(v) = file.listen {
            cfg.listen = v;
        }
        if file.tls_cert.is_some() {
            cfg.tls_cert = file.tls_cert;
        }
        if file.tls_key.is_some() {
            cfg.tls_key = file.tls_key;
        }
        if let Some(v) = file.metrics {
            cfg.metrics_path = v;
        }
        if file.static_dir.is_some() {
            cfg.static_dir = file.static_dir;
        }
        if let Some(v) = file.log_level {
            cfg.log_level = v;
        }
        if let Some(v) = file.graceful_timeout_s {
            cfg.graceful_timeout_s = v;
        }
        if let Some(v) = file.session_secret {
            cfg.session_secret = v;
        }
    }
    if let Some(v) = &args.listen {
        cfg.listen = v.clone();
    }
    if let Some(v) = &args.tls_cert {
        cfg.tls_cert = Some(v.clone());
    }
    if let Some(v) = &args.tls_key {
        cfg.tls_key = Some(v.clone());
    }
    if let Some(v) = &args.metrics {
        cfg.metrics_path = v.clone();
    }
    if let Some(v) = &args.static_dir {
        cfg.static_dir = Some(v.clone());
    }
    if let Some(v) = &args.log_level {
        cfg.log_level = v.clone();
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.listen, "127.0.0.1:8080");
        assert_eq!(cfg.metrics_path, "/metrics");
    }

    #[test]
    fn cli_overrides_file() {
        let args = Args {
            listen: Some("0.0.0.0:9000".into()),
            config: "/no/such/file".into(),
            tls_cert: None,
            tls_key: None,
            metrics: None,
            static_dir: None,
            log_level: Some("debug".into()),
        };
        let cfg = load(&args).unwrap();
        assert_eq!(cfg.listen, "0.0.0.0:9000");
        assert_eq!(cfg.log_level, "debug");
    }
}
