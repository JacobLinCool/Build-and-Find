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
    pub routes: Vec<RouteConfig>,

    #[serde(default)]
    pub middleware: MiddlewareConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub path: String,
    pub method: String,
    #[serde(default)]
    pub handler: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MiddlewareConfig {
    #[serde(default)]
    pub logging: bool,

    #[serde(default)]
    pub gzip: bool,

    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,

    #[serde(default)]
    pub cors: Option<CorsConfig>,

    #[serde(default)]
    pub csrf: bool,

    #[serde(default)]
    pub basic_auth: Option<BasicAuthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rate_capacity")]
    pub capacity: u32,
    #[serde(default = "default_rate_refill")]
    pub refill_rate: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    #[serde(default)]
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthConfig {
    pub username: String,
    pub password: String,
}

fn default_listen() -> String {
    "127.0.0.1:8080".into()
}

fn default_metrics() -> String {
    "/metrics".into()
}

fn default_log_level() -> String {
    "info".into()
}

fn default_graceful_timeout() -> u64 {
    30
}

fn default_session_secret() -> String {
    "change-me-in-production".into()
}

fn default_rate_capacity() -> u32 {
    100
}

fn default_rate_refill() -> f64 {
    10.0
}

impl Config {
    pub fn load(path: &str) -> Result<Self, crate::error::NanowebError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::NanowebError::Config(format!("failed to read config: {e}"))
        })?;
        toml::from_str(&content)
            .map_err(|e| crate::error::NanowebError::Config(format!("failed to parse config: {e}")))
    }

    pub fn default_config() -> Self {
        Self {
            listen: default_listen(),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: default_session_secret(),
            routes: Vec::new(),
            middleware: MiddlewareConfig::default(),
        }
    }

    pub fn merge_cli(&mut self, cli: &crate::cli::Cli) {
        if cli.listen != default_listen() {
            self.listen = cli.listen.clone();
        }
        if cli.config != "./nanoweb.toml" {
            // config path itself is not merged
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

impl Default for Config {
    fn default() -> Self {
        Self::default_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default_config();
        assert_eq!(config.listen, "127.0.0.1:8080");
        assert_eq!(config.metrics, "/metrics");
        assert_eq!(config.graceful_timeout_s, 30);
    }

    #[test]
    fn test_parse_toml() {
        let toml = r#"
listen = "0.0.0.0:3000"
log_level = "debug"

[[routes]]
path = "/hello"
method = "GET"
handler = "hello"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:3000");
        assert_eq!(config.routes.len(), 1);
    }
}
