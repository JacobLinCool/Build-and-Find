use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::cli::Cli;
use crate::error::Result;
use crate::http::Method;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub tls_listen: Option<String>,
    #[serde(default)]
    pub tls_cert: Option<String>,
    #[serde(default)]
    pub tls_key: Option<String>,
    #[serde(default = "default_metrics_path")]
    pub metrics: String,
    #[serde(default)]
    pub static_dir: Option<String>,
    #[serde(default = "default_static_mount")]
    pub static_mount: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_graceful_timeout")]
    pub graceful_timeout_s: u64,
    #[serde(default = "default_session_secret")]
    pub session_secret: String,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
    #[serde(default)]
    pub middleware: Vec<MiddlewareConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub method: Method,
    pub path: String,
    #[serde(flatten)]
    pub handler: RouteHandlerConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteHandlerConfig {
    Text {
        body: String,
        #[serde(default = "default_status_ok")]
        status: u16,
        #[serde(default = "default_text_content_type")]
        content_type: String,
    },
    Json {
        body: serde_json::Value,
        #[serde(default = "default_status_ok")]
        status: u16,
    },
    Template {
        template: String,
        #[serde(default)]
        context: serde_json::Value,
        #[serde(default = "default_status_ok")]
        status: u16,
    },
    EchoBody {
        #[serde(default = "default_status_ok")]
        status: u16,
    },
    Empty {
        #[serde(default = "default_status_ok")]
        status: u16,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MiddlewareConfig {
    Logging,
    Gzip,
    RateLimit {
        permits_per_second: f64,
        burst: f64,
    },
    Cors {
        #[serde(default)]
        allowed_origins: Vec<String>,
    },
    Csrf {
        #[serde(default = "default_csrf_cookie_name")]
        cookie_name: String,
        #[serde(default = "default_csrf_header_name")]
        header_name: String,
    },
    BasicAuth {
        username: String,
        password: String,
    },
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            tls_listen: None,
            tls_cert: None,
            tls_key: None,
            metrics: default_metrics_path(),
            static_dir: None,
            static_mount: default_static_mount(),
            log_level: default_log_level(),
            graceful_timeout_s: default_graceful_timeout(),
            session_secret: default_session_secret(),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(cli: Cli) -> Result<Self> {
        let mut config = if Path::new(&cli.config).exists() {
            let contents = fs::read_to_string(&cli.config)?;
            toml::from_str::<Config>(&contents)?
        } else {
            Config::default()
        };

        config.listen = cli.listen;
        config.metrics = cli.metrics;
        config.log_level = cli.log_level;
        if let Some(static_dir) = cli.static_dir {
            config.static_dir = Some(static_dir);
        }
        if let Some(tls_cert) = cli.tls_cert {
            config.tls_cert = Some(tls_cert);
        }
        if let Some(tls_key) = cli.tls_key {
            config.tls_key = Some(tls_key);
        }
        Ok(config)
    }
}

fn default_listen() -> String {
    "127.0.0.1:8080".into()
}

fn default_metrics_path() -> String {
    "/metrics".into()
}

fn default_static_mount() -> String {
    "/static".into()
}

fn default_log_level() -> String {
    "info".into()
}

fn default_graceful_timeout() -> u64 {
    30
}

fn default_session_secret() -> String {
    "nanoweb-secret".into()
}

fn default_status_ok() -> u16 {
    200
}

fn default_text_content_type() -> String {
    "text/plain; charset=utf-8".into()
}

fn default_csrf_cookie_name() -> String {
    "csrf_token".into()
}

fn default_csrf_header_name() -> String {
    "x-csrf-token".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toml_config() {
        let config: Config = toml::from_str(
            r#"
listen = "127.0.0.1:9000"
metrics = "/metrics"

[[routes]]
method = "GET"
path = "/"
kind = "text"
body = "hello"
"#,
        )
        .unwrap();
        assert_eq!(config.listen, "127.0.0.1:9000");
        assert_eq!(config.routes.len(), 1);
    }
}
