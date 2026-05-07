use crate::cli::Cli;
use crate::error::{NanowebError, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub listen: String,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub metrics: String,
    pub static_dir: Option<PathBuf>,
    pub log_level: String,
    pub graceful_timeout_s: u64,
    pub session_secret: String,
    pub routes: Vec<RouteConfig>,
    pub middleware: MiddlewareConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub response: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MiddlewareConfig {
    pub logging: bool,
    pub gzip: bool,
    pub cors_origins: Vec<String>,
    pub csrf: bool,
    pub basic_auth_user: Option<String>,
    pub basic_auth_password: Option<String>,
    pub rate_limit_capacity: Option<u32>,
    pub rate_limit_refill_per_s: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    listen: Option<String>,
    tls_cert: Option<PathBuf>,
    tls_key: Option<PathBuf>,
    metrics: Option<String>,
    static_dir: Option<PathBuf>,
    log_level: Option<String>,
    graceful_timeout_s: Option<u64>,
    session_secret: Option<String>,
    routes: Option<Vec<RouteConfig>>,
    middleware: Option<MiddlewareConfig>,
}

impl Config {
    pub fn load(cli: &Cli) -> Result<Self> {
        let file: FileConfig = match std::fs::read_to_string(&cli.config) {
            Ok(s) => toml::from_str(&s).map_err(|e| NanowebError::Config(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileConfig::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            listen: if cli.listen != "127.0.0.1:8080" {
                cli.listen.clone()
            } else {
                file.listen.unwrap_or_else(|| cli.listen.clone())
            },
            tls_cert: cli.tls_cert.as_ref().map(PathBuf::from).or(file.tls_cert),
            tls_key: cli.tls_key.as_ref().map(PathBuf::from).or(file.tls_key),
            metrics: cli
                .metrics
                .clone()
                .or(file.metrics)
                .unwrap_or_else(|| "/metrics".into()),
            static_dir: cli
                .static_dir
                .as_ref()
                .map(PathBuf::from)
                .or(file.static_dir),
            log_level: cli
                .log_level
                .clone()
                .or(file.log_level)
                .unwrap_or_else(|| "info".into()),
            graceful_timeout_s: file.graceful_timeout_s.unwrap_or(30),
            session_secret: file.session_secret.unwrap_or_else(|| "change-me".into()),
            routes: file.routes.unwrap_or_else(|| {
                vec![RouteConfig {
                    method: "GET".into(),
                    path: "/".into(),
                    response: "nanoweb".into(),
                }]
            }),
            middleware: file.middleware.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_exist() {
        let cli = Cli {
            listen: "127.0.0.1:0".into(),
            config: "missing.toml".into(),
            tls_cert: None,
            tls_key: None,
            metrics: None,
            static_dir: None,
            log_level: None,
        };
        assert_eq!(Config::load(&cli).unwrap().metrics, "/metrics");
    }
}
