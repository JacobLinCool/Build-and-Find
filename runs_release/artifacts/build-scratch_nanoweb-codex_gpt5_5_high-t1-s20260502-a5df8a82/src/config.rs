use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::Cli;
use crate::error::Result;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub listen: String,
    pub config: Option<PathBuf>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub metrics: String,
    pub static_dir: Option<PathBuf>,
    pub log_level: String,
    pub version: Option<bool>,
    pub graceful_timeout_s: u64,
    pub session_secret: String,
    pub routes: Vec<RouteConfig>,
    pub middleware: MiddlewareConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub response: String,
    pub status: u16,
    pub content_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MiddlewareConfig {
    pub logging: bool,
    pub gzip: bool,
    pub rate_limit: Option<RateLimitConfig>,
    pub cors: Option<CorsConfig>,
    pub csrf: bool,
    pub basic_auth: Option<BasicAuthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub capacity: u32,
    pub refill_per_second: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthConfig {
    pub username: String,
    pub password: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            config: None,
            tls_cert: None,
            tls_key: None,
            metrics: "/metrics".into(),
            static_dir: None,
            log_level: "info".into(),
            version: None,
            graceful_timeout_s: 30,
            session_secret: "change-me".into(),
            routes: vec![RouteConfig::default()],
            middleware: MiddlewareConfig::default(),
        }
    }
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            method: "GET".into(),
            path: "/".into(),
            response: "nanoweb".into(),
            status: 200,
            content_type: "text/plain; charset=utf-8".into(),
        }
    }
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            logging: true,
            gzip: false,
            rate_limit: None,
            cors: None,
            csrf: false,
            basic_auth: None,
        }
    }
}

impl Config {
    pub fn load(cli: &Cli) -> Result<Self> {
        let path = cli
            .config
            .clone()
            .unwrap_or_else(|| PathBuf::from("./nanoweb.toml"));
        let mut config = if Path::new(&path).exists() {
            toml::from_str::<Config>(&std::fs::read_to_string(&path)?)?
        } else {
            Config::default()
        };
        config.config = Some(path);
        config.apply_cli(cli);
        Ok(config)
    }

    fn apply_cli(&mut self, cli: &Cli) {
        if let Some(value) = &cli.listen {
            self.listen = value.clone();
        }
        if let Some(value) = &cli.tls_cert {
            self.tls_cert = Some(value.clone());
        }
        if let Some(value) = &cli.tls_key {
            self.tls_key = Some(value.clone());
        }
        if let Some(value) = &cli.metrics {
            self.metrics = value.clone();
        }
        if let Some(value) = &cli.static_dir {
            self.static_dir = Some(value.clone());
        }
        if let Some(value) = &cli.log_level {
            self.log_level = value.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn cli_overrides_defaults() {
        let cli = Cli::try_parse_from(["nanoweb", "--listen", "127.0.0.1:0"]).unwrap();
        let config = Config::load(&cli).unwrap();
        assert_eq!(config.listen, "127.0.0.1:0");
    }
}
