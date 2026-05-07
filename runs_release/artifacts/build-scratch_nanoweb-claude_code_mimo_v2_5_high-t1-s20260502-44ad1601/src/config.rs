use crate::error::{NanowebError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default)]
    pub tls_cert: Option<String>,

    #[serde(default)]
    pub tls_key: Option<String>,

    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,

    #[serde(default = "default_static_dir")]
    pub static_dir: Option<String>,

    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,

    #[serde(default)]
    pub session_secret: Option<String>,

    #[serde(default)]
    pub cors: CorsConfig,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    #[serde(default)]
    pub routes: Vec<RouteConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CorsConfig {
    #[serde(default = "default_cors_origins")]
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rate_limit")]
    pub requests_per_second: u32,
    #[serde(default = "default_rate_burst")]
    pub burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        RateLimitConfig {
            requests_per_second: default_rate_limit(),
            burst: default_rate_burst(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub handler: String,
    #[serde(default)]
    pub template: Option<String>,
}

fn default_listen() -> String {
    "127.0.0.1:8080".into()
}

fn default_log_level() -> String {
    "info".into()
}

fn default_metrics_path() -> String {
    "/metrics".into()
}

fn default_static_dir() -> Option<String> {
    None
}

fn default_graceful_timeout() -> u64 {
    30
}

fn default_cors_origins() -> Vec<String> {
    vec!["*".into()]
}

fn default_rate_limit() -> u32 {
    100
}

fn default_rate_burst() -> u32 {
    200
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| NanowebError::Config(format!("failed to read config file: {e}")))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| NanowebError::Config(format!("failed to parse config: {e}")))?;
        Ok(config)
    }

    pub fn with_cli_args(args: &CliArgs) -> Result<Self> {
        let mut config = if let Some(ref path) = args.config {
            Config::load(path)?
        } else {
            Config::default_config()
        };

        if let Some(ref listen) = args.listen {
            config.listen = listen.clone();
        }
        if let Some(ref level) = args.log_level {
            config.log_level = level.clone();
        }
        if let Some(ref cert) = args.tls_cert {
            config.tls_cert = Some(cert.clone());
        }
        if let Some(ref key) = args.tls_key {
            config.tls_key = Some(key.clone());
        }
        if let Some(ref metrics) = args.metrics {
            config.metrics_path = metrics.clone();
        }
        if let Some(ref dir) = args.static_dir {
            config.static_dir = Some(dir.clone());
        }

        Ok(config)
    }

    pub fn default_config() -> Self {
        Config {
            listen: default_listen(),
            log_level: default_log_level(),
            tls_cert: None,
            tls_key: None,
            metrics_path: default_metrics_path(),
            static_dir: None,
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: None,
            cors: CorsConfig::default(),
            rate_limit: RateLimitConfig::default(),
            routes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, clap::Args)]
pub struct CliArgs {
    #[arg(long)]
    pub listen: Option<String>,

    #[arg(long)]
    pub config: Option<String>,

    #[arg(long)]
    pub tls_cert: Option<String>,

    #[arg(long)]
    pub tls_key: Option<String>,

    #[arg(long)]
    pub metrics: Option<String>,

    #[arg(long)]
    pub static_dir: Option<String>,

    #[arg(long)]
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, clap::Parser)]
#[command(
    name = "nanoweb",
    version,
    about = "A self-contained HTTP/1.1 web framework"
)]
pub struct Cli {
    #[command(flatten)]
    pub args: CliArgs,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default_config();
        assert_eq!(config.listen, "127.0.0.1:8080");
        assert_eq!(config.graceful_timeout_s, 30);
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
listen = "0.0.0.0:9090"
log_level = "debug"
graceful_timeout_s = 60
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9090");
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.graceful_timeout_s, 60);
    }
}
