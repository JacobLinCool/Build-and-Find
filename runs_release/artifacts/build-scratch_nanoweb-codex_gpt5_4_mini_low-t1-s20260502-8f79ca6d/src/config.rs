use crate::error::NanowebError;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    pub listen: Option<String>,
    pub metrics: Option<String>,
    pub static_dir: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub log_level: Option<String>,
    pub graceful_timeout_s: Option<u64>,
    pub session_secret: Option<String>,
}

impl Config {
    pub fn load(path: PathBuf) -> Result<Self, NanowebError> {
        if path.exists() {
            Ok(toml::from_str(&fs::read_to_string(path)?)?)
        } else {
            Ok(Self::default())
        }
    }
    pub fn finalize(mut self, cli: crate::cli::Cli) -> Self {
        if let Some(v) = cli.listen {
            self.listen = Some(v)
        }
        if let Some(v) = cli.metrics {
            self.metrics = Some(v)
        }
        if let Some(v) = cli.static_dir {
            self.static_dir = Some(v)
        }
        if let Some(v) = cli.tls_cert {
            self.tls_cert = Some(v)
        }
        if let Some(v) = cli.tls_key {
            self.tls_key = Some(v)
        }
        if let Some(v) = cli.log_level {
            self.log_level = Some(v)
        }
        self
    }
}
