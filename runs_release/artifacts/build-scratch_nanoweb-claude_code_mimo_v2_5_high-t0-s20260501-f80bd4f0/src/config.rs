use crate::error::{NanowebError, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,

    #[serde(default = "default_health_path")]
    pub health_path: String,

    #[serde(default = "default_readyz_path")]
    pub readyz_path: String,

    #[serde(default)]
    pub tls_cert: Option<String>,

    #[serde(default)]
    pub tls_key: Option<String>,

    #[serde(default)]
    pub static_dir: Option<String>,

    #[serde(default = "default_log_level")]
    pub log_level: String,

    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,

    #[serde(default = "default_max_body_size")]
    pub max_body_size: usize,

    #[serde(default)]
    pub secret: String,

    #[serde(default)]
    pub routes: Vec<RouteConfig>,

    #[serde(default)]
    pub middleware: Vec<String>,

    #[serde(default)]
    pub cors: CorsConfig,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub handler: String,
    #[serde(default)]
    pub middleware: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CorsConfig {
    #[serde(default = "default_cors_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(default = "default_cors_methods")]
    pub allowed_methods: Vec<String>,
    #[serde(default = "default_cors_headers")]
    pub allowed_headers: Vec<String>,
    #[serde(default = "default_cors_max_age")]
    pub max_age: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_cors_origins(),
            allowed_methods: default_cors_methods(),
            allowed_headers: default_cors_headers(),
            max_age: default_cors_max_age(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rate_limit_requests")]
    pub requests_per_second: u32,
    #[serde(default = "default_rate_limit_burst")]
    pub burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: default_rate_limit_requests(),
            burst: default_rate_limit_burst(),
        }
    }
}

fn default_listen() -> String {
    "127.0.0.1:8080".into()
}
fn default_metrics_path() -> String {
    "/metrics".into()
}
fn default_health_path() -> String {
    "/healthz".into()
}
fn default_readyz_path() -> String {
    "/readyz".into()
}
fn default_log_level() -> String {
    "info".into()
}
fn default_graceful_timeout() -> u64 {
    30
}
fn default_max_body_size() -> usize {
    64 * 1024 * 1024
}
fn default_cors_origins() -> Vec<String> {
    vec!["*".into()]
}
fn default_cors_methods() -> Vec<String> {
    vec!["GET".into(), "POST".into(), "PUT".into(), "DELETE".into()]
}
fn default_cors_headers() -> Vec<String> {
    vec!["content-type".into(), "authorization".into()]
}
fn default_cors_max_age() -> u64 {
    3600
}
fn default_rate_limit_requests() -> u32 {
    100
}
fn default_rate_limit_burst() -> u32 {
    50
}

#[derive(Debug, Default, Clone)]
pub struct CliArgs {
    pub listen: Option<String>,
    pub config: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub metrics: Option<String>,
    pub static_dir: Option<String>,
    pub log_level: Option<String>,
    pub version: bool,
}

impl Config {
    pub fn load(args: &CliArgs) -> Result<Self> {
        let config_path = args.config.clone().unwrap_or_else(|| "nanoweb.toml".into());

        let mut config: Config = if std::path::Path::new(&config_path).exists() {
            let content = std::fs::read_to_string(&config_path)?;
            toml::from_str(&content).map_err(|e| NanowebError::Config(e.to_string()))?
        } else {
            Config::default()
        };

        // CLI flags override TOML
        if let Some(ref listen) = args.listen {
            config.listen = listen.clone();
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
        if let Some(ref level) = args.log_level {
            config.log_level = level.clone();
        }

        Ok(config)
    }

    pub fn metrics_endpoint(&self) -> &str {
        &self.metrics_path
    }

    pub fn tls_configured(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            metrics_path: default_metrics_path(),
            health_path: default_health_path(),
            readyz_path: default_readyz_path(),
            tls_cert: None,
            tls_key: None,
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            max_body_size: default_max_body_size(),
            secret: String::new(),
            routes: Vec::new(),
            middleware: Vec::new(),
            cors: CorsConfig::default(),
            rate_limit: RateLimitConfig::default(),
        }
    }
}

/// Parse CLI arguments from std::env::args().
pub fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut cli = CliArgs::default();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--listen" => {
                i += 1;
                cli.listen = args.get(i).cloned();
            }
            "--config" => {
                i += 1;
                cli.config = args.get(i).cloned();
            }
            "--tls-cert" => {
                i += 1;
                cli.tls_cert = args.get(i).cloned();
            }
            "--tls-key" => {
                i += 1;
                cli.tls_key = args.get(i).cloned();
            }
            "--metrics" => {
                i += 1;
                cli.metrics = args.get(i).cloned();
            }
            "--static-dir" => {
                i += 1;
                cli.static_dir = args.get(i).cloned();
            }
            "--log-level" => {
                i += 1;
                cli.log_level = args.get(i).cloned();
            }
            "--version" => {
                cli.version = true;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }
    cli
}

fn print_help() {
    println!(
        "\
nanoweb {} - HTTP/1.1 web framework and server

USAGE:
    nanoweb [OPTIONS]

OPTIONS:
    --listen <ADDR:PORT>    Address to listen on [default: 127.0.0.1:8080]
    --config <PATH>         TOML config file [default: ./nanoweb.toml]
    --tls-cert <PATH>       TLS certificate PEM file
    --tls-key <PATH>        TLS private key PEM file
    --metrics <PATH>        Metrics endpoint path [default: /metrics]
    --static-dir <PATH>     Static files directory
    --log-level <LEVEL>     Log level: trace,debug,info,warn,error [default: info]
    --version               Print version
    --help                  Print this help",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = Config::default();
        assert_eq!(config.listen, "127.0.0.1:8080");
        assert_eq!(config.metrics_path, "/metrics");
        assert_eq!(config.graceful_timeout_s, 30);
    }

    #[test]
    fn parse_toml() {
        let toml = r#"
listen = "0.0.0.0:3000"
log_level = "debug"
secret = "test-secret"

[[routes]]
method = "GET"
path = "/hello"
handler = "hello"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:3000");
        assert_eq!(config.secret, "test-secret");
        assert_eq!(config.routes.len(), 1);
    }
}
