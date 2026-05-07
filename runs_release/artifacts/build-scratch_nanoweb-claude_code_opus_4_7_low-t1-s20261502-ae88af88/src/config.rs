use crate::cli::CliArgs;
use crate::error::{NanowebError, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub listen: String,
    pub tls_listen: Option<String>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub static_dir: Option<PathBuf>,
    pub metrics: String,
    pub log_level: String,
    pub graceful_timeout_s: u64,
    pub session_secret: String,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    /// One of: "text", "html", "json", "template"
    pub kind: String,
    pub body: Option<String>,
    pub template: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            tls_listen: None,
            tls_cert: None,
            tls_key: None,
            static_dir: None,
            metrics: "/metrics".into(),
            log_level: "info".into(),
            graceful_timeout_s: 30,
            session_secret: "change-me".into(),
            routes: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| NanowebError::Config(format!("read config: {}", e)))?;
        let cfg: Config = toml::from_str(&text)?;
        Ok(cfg)
    }

    /// Apply CLI overrides. CLI values take precedence over TOML.
    pub fn apply_cli(mut self, cli: &CliArgs) -> Self {
        if let Some(v) = &cli.listen {
            self.listen = v.clone();
        }
        if let Some(v) = &cli.tls_cert {
            self.tls_cert = Some(v.clone());
        }
        if let Some(v) = &cli.tls_key {
            self.tls_key = Some(v.clone());
        }
        if let Some(v) = &cli.metrics {
            self.metrics = v.clone();
        }
        if let Some(v) = &cli.static_dir {
            self.static_dir = Some(v.clone());
        }
        if let Some(v) = &cli.log_level {
            self.log_level = v.clone();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_overrides_toml() {
        let cfg = Config::default();
        let cli = CliArgs {
            listen: Some("0.0.0.0:9000".into()),
            ..Default::default()
        };
        let merged = cfg.apply_cli(&cli);
        assert_eq!(merged.listen, "0.0.0.0:9000");
    }
}
