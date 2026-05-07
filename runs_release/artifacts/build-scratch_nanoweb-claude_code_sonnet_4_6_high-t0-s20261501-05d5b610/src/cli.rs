use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::error::NanowebError;

#[derive(Parser, Debug)]
#[command(name = "nanoweb", version, about = "HTTP/1.1 web framework and server")]
pub struct Cli {
    /// Address to listen on (e.g. 127.0.0.1:8080)
    #[arg(long, value_name = "ADDR:PORT")]
    pub listen: Option<String>,

    /// Path to the TOML configuration file
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Path to TLS certificate PEM file
    #[arg(long, value_name = "PATH")]
    pub tls_cert: Option<PathBuf>,

    /// Path to TLS private key PEM file
    #[arg(long, value_name = "PATH")]
    pub tls_key: Option<PathBuf>,

    /// Path to mount the metrics endpoint
    #[arg(long, value_name = "PATH")]
    pub metrics: Option<String>,

    /// Directory to serve static files from
    #[arg(long, value_name = "PATH")]
    pub static_dir: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, value_name = "LEVEL")]
    pub log_level: Option<String>,
}

impl Cli {
    pub fn into_config(self) -> Result<Config, NanowebError> {
        let mut overrides = HashMap::new();

        if let Some(v) = self.listen {
            overrides.insert("listen".to_string(), v);
        }
        if let Some(v) = &self.tls_cert {
            overrides.insert("tls_cert".to_string(), v.to_string_lossy().to_string());
        }
        if let Some(v) = &self.tls_key {
            overrides.insert("tls_key".to_string(), v.to_string_lossy().to_string());
        }
        if let Some(v) = self.metrics {
            overrides.insert("metrics".to_string(), v);
        }
        if let Some(v) = &self.static_dir {
            overrides.insert("static_dir".to_string(), v.to_string_lossy().to_string());
        }
        if let Some(v) = self.log_level {
            overrides.insert("log_level".to_string(), v);
        }

        Config::load(self.config.as_ref(), &overrides)
    }
}

/// Initialize the tracing subscriber with the given log level.
pub fn init_logging(log_level: &str) {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let _ = fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_defaults() {
        let cli = Cli::parse_from(["nanoweb"]);
        assert!(cli.listen.is_none());
        assert!(cli.config.is_none());
    }

    #[test]
    fn test_cli_override_listen() {
        let cli = Cli::parse_from(["nanoweb", "--listen", "0.0.0.0:9090"]);
        assert_eq!(cli.listen.as_deref(), Some("0.0.0.0:9090"));
    }

    #[test]
    fn test_cli_into_config() {
        let cli = Cli::parse_from(["nanoweb", "--listen", "0.0.0.0:3000"]);
        let config = cli.into_config().unwrap();
        assert_eq!(config.listen, "0.0.0.0:3000");
    }
}
