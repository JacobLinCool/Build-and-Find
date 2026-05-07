use crate::{
    cli::Cli,
    error::{NanowebError, Result},
    middleware::config::{CorsConfig, CsrfConfig, RateLimitConfig},
};
use serde::{Deserialize, Serialize};
use std::{fs, net::SocketAddr, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    #[serde(default)]
    pub tls_listen: Option<SocketAddr>,
    #[serde(default)]
    pub tls_cert: Option<PathBuf>,
    #[serde(default)]
    pub tls_key: Option<PathBuf>,
    #[serde(default = "default_metrics")]
    pub metrics: String,
    #[serde(default = "default_static_mount")]
    pub static_mount: String,
    #[serde(default)]
    pub static_dir: Option<PathBuf>,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,
    #[serde(default)]
    pub session_secret: Option<String>,
    #[serde(default)]
    pub session_cookie_name: Option<String>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub middleware: Vec<MiddlewareConfig>,
    #[serde(default)]
    pub cors: Option<CorsConfig>,
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,
    #[serde(default)]
    pub csrf: Option<CsrfConfig>,
    #[serde(default)]
    pub basic_auth: Option<BasicAuthConfig>,
}

fn default_listen() -> SocketAddr {
    "127.0.0.1:8080".parse().expect("valid default listen")
}

fn default_metrics() -> String {
    "/metrics".to_string()
}

fn default_static_mount() -> String {
    "/static".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_graceful_timeout() -> u64 {
    30
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            tls_listen: None,
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics(),
            static_mount: default_static_mount(),
            static_dir: None,
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: None,
            session_cookie_name: None,
            routes: Vec::new(),
            middleware: Vec::new(),
            cors: None,
            rate_limit: None,
            csrf: None,
            basic_auth: None,
        }
    }
}

impl Config {
    pub fn load(path: PathBuf, cli: &Cli) -> Result<Self> {
        let mut config = if path.exists() {
            let source = fs::read_to_string(&path)?;
            toml::from_str::<Config>(&source)?
        } else {
            Config::default()
        };

        config.apply_cli(cli)?;
        config.validate()?;
        Ok(config)
    }

    fn apply_cli(&mut self, cli: &Cli) -> Result<()> {
        self.listen = cli
            .listen
            .parse()
            .map_err(|err| NanowebError::Config(format!("invalid --listen value: {err}")))?;
        if let Some(cert) = &cli.tls_cert {
            self.tls_cert = Some(cert.clone());
        }
        if let Some(key) = &cli.tls_key {
            self.tls_key = Some(key.clone());
        }
        if let Some(metrics) = &cli.metrics {
            self.metrics = metrics.clone();
        }
        if let Some(dir) = &cli.static_dir {
            self.static_dir = Some(dir.clone());
        }
        if let Some(level) = &cli.log_level {
            self.log_level = level.clone();
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        if (self.tls_cert.is_some() || self.tls_key.is_some())
            && self.tls_cert.is_none() ^ self.tls_key.is_none()
        {
            return Err(NanowebError::Config(
                "both tls_cert and tls_key must be set together".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicAuthConfig {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub method: crate::http::types::Method,
    pub path: String,
    #[serde(flatten)]
    pub handler: HandlerConfig,
    #[serde(default)]
    pub middleware: Vec<MiddlewareConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandlerConfig {
    Text {
        body: String,
        #[serde(default)]
        content_type: Option<String>,
    },
    Json {
        value: serde_json::Value,
    },
    Template {
        template: String,
        context: serde_json::Value,
        #[serde(default)]
        content_type: Option<String>,
    },
    Echo,
    File {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MiddlewareConfig {
    Logging,
    Gzip,
    RateLimit {
        tokens_per_second: f64,
        burst: u32,
    },
    Cors {
        allowed_origins: Vec<String>,
    },
    Csrf {
        cookie_name: String,
        header_name: String,
    },
    BasicAuth {
        username: String,
        password: String,
    },
}

impl MiddlewareConfig {
    pub fn to_kind_string(&self) -> &'static str {
        match self {
            MiddlewareConfig::Logging => "logging",
            MiddlewareConfig::Gzip => "gzip",
            MiddlewareConfig::RateLimit { .. } => "rate_limit",
            MiddlewareConfig::Cors { .. } => "cors",
            MiddlewareConfig::Csrf { .. } => "csrf",
            MiddlewareConfig::BasicAuth { .. } => "basic_auth",
        }
    }
}
