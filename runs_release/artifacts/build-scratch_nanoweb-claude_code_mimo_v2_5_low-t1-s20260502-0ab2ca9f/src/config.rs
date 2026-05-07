use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,

    #[serde(default)]
    pub tls_cert: Option<String>,

    #[serde(default)]
    pub tls_key: Option<String>,

    #[serde(default = "default_metrics")]
    pub metrics: String,

    #[serde(default)]
    pub static_dir: Option<String>,

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

    #[serde(default)]
    pub cors: CorsConfig,

    #[serde(default)]
    pub basic_auth: Option<BasicAuthConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RouteConfig {
    pub path: String,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MiddlewareConfig {
    #[serde(default = "default_true")]
    pub logging: bool,
    #[serde(default)]
    pub gzip: bool,
    #[serde(default)]
    pub rate_limit: Option<f64>,
    #[serde(default = "default_true")]
    pub cors: bool,
    #[serde(default)]
    pub csrf: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CorsConfig {
    #[serde(default = "default_cors_origins")]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub allow_credentials: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BasicAuthConfig {
    pub username: String,
    pub password: String,
}

fn default_listen() -> String {
    "127.0.0.1:8080".to_string()
}

fn default_metrics() -> String {
    "/metrics".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_graceful_timeout() -> u64 {
    30
}

fn default_session_secret() -> String {
    "change-me-in-production".to_string()
}

fn default_true() -> bool {
    true
}

fn default_cors_origins() -> Vec<String> {
    vec!["*".to_string()]
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: default_listen(),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: default_session_secret(),
            routes: vec![],
            middleware: MiddlewareConfig::default(),
            cors: CorsConfig::default(),
            basic_auth: None,
        }
    }
}

impl Config {
    pub fn from_file(path: &str) -> crate::error::Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            crate::error::NanowebError::Config(format!("Cannot read config file: {}", e))
        })?;
        let config: Config = toml::from_str(&content).map_err(|e| {
            crate::error::NanowebError::Config(format!("Cannot parse config: {}", e))
        })?;
        Ok(config)
    }

    pub fn with_cli_overrides(mut self, cli: &CliArgs) -> Self {
        if let Some(ref listen) = cli.listen {
            self.listen = listen.clone();
        }
        if let Some(ref cert) = cli.tls_cert {
            self.tls_cert = Some(cert.clone());
        }
        if let Some(ref key) = cli.tls_key {
            self.tls_key = Some(key.clone());
        }
        if let Some(ref metrics) = cli.metrics {
            self.metrics = metrics.clone();
        }
        if let Some(ref dir) = cli.static_dir {
            self.static_dir = Some(dir.clone());
        }
        if let Some(ref level) = cli.log_level {
            self.log_level = level.clone();
        }
        self
    }
}

#[derive(Debug, Clone, Default)]
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

impl CliArgs {
    pub fn parse() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut cli = CliArgs::default();
        let mut i = 0;
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
}

fn print_help() {
    println!(
        "nanoweb {} - An HTTP/1.1 web framework and server",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("Usage: nanoweb [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --listen <ADDR:PORT>   Address to listen on (default: 127.0.0.1:8080)");
    println!("  --config <PATH>        Path to TOML config file (default: ./nanoweb.toml)");
    println!("  --tls-cert <PATH>      Path to TLS certificate (PEM)");
    println!("  --tls-key <PATH>       Path to TLS private key (PEM)");
    println!("  --metrics <PATH>       Path for metrics endpoint (default: /metrics)");
    println!("  --static-dir <DIR>     Directory for static file serving");
    println!("  --log-level <LEVEL>    Log level: trace, debug, info, warn, error (default: info)");
    println!("  --version              Print version information");
    println!("  --help                 Print this help message");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config {
            listen: default_listen(),
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: default_session_secret(),
            routes: vec![],
            middleware: MiddlewareConfig::default(),
            cors: CorsConfig::default(),
            basic_auth: None,
        };
        assert_eq!(config.listen, "127.0.0.1:8080");
        assert_eq!(config.graceful_timeout_s, 30);
    }

    #[test]
    fn test_toml_parse() {
        let toml = r#"
listen = "0.0.0.0:9090"
log_level = "debug"

[[routes]]
path = "/users"
methods = ["GET", "POST"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.listen, "0.0.0.0:9090");
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.routes[0].path, "/users");
    }
}
