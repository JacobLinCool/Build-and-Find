//! Configuration: TOML loading and merging with CLI flags.
//!
//! The crate's other modules consume only the [`Config`] struct produced
//! here; CLI overrides happen in this module rather than scattered around.

use std::path::PathBuf;

use serde::Deserialize;

use crate::cli::Args;
use crate::error::{NanowebError, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: String,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub metrics_path: String,
    pub static_dir: Option<PathBuf>,
    pub log_level: String,
    pub graceful_timeout_s: u64,
    pub session_secret: String,
    pub middleware: MiddlewareConfig,
    pub routes: Vec<RouteConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct MiddlewareConfig {
    pub logging: bool,
    pub gzip: bool,
    pub rate_limit: Option<RateLimitConfig>,
    pub cors: Option<CorsConfig>,
    pub csrf: bool,
    pub basic_auth: Option<BasicAuthConfig>,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub capacity: u32,
    pub refill_per_sec: f64,
}

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BasicAuthConfig {
    pub realm: String,
    pub credentials: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub response: RouteResponse,
}

#[derive(Debug, Clone)]
pub enum RouteResponse {
    Text(String),
    Json(serde_json::Value),
    Html(String),
    Template {
        template: String,
        context: serde_json::Value,
    },
    StaticDir,
}

#[derive(Debug, Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    listen: Option<String>,
    #[serde(default)]
    tls_cert: Option<PathBuf>,
    #[serde(default)]
    tls_key: Option<PathBuf>,
    #[serde(default)]
    metrics: Option<String>,
    #[serde(default)]
    static_dir: Option<PathBuf>,
    #[serde(default)]
    log_level: Option<String>,
    #[serde(default)]
    graceful_timeout_s: Option<u64>,
    #[serde(default)]
    session_secret: Option<String>,
    #[serde(default)]
    middleware: RawMiddleware,
    #[serde(default)]
    routes: Vec<RawRoute>,
}

#[derive(Debug, Deserialize, Default)]
struct RawMiddleware {
    #[serde(default)]
    logging: Option<bool>,
    #[serde(default)]
    gzip: Option<bool>,
    #[serde(default)]
    rate_limit: Option<RawRateLimit>,
    #[serde(default)]
    cors: Option<RawCors>,
    #[serde(default)]
    csrf: Option<bool>,
    #[serde(default)]
    basic_auth: Option<RawBasicAuth>,
}

#[derive(Debug, Deserialize)]
struct RawRateLimit {
    capacity: u32,
    refill_per_sec: f64,
}

#[derive(Debug, Deserialize)]
struct RawCors {
    allowed_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawBasicAuth {
    realm: String,
    users: Vec<RawBasicUser>,
}

#[derive(Debug, Deserialize)]
struct RawBasicUser {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RawRoute {
    method: String,
    path: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    json: Option<serde_json::Value>,
    #[serde(default)]
    html: Option<String>,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    context: Option<serde_json::Value>,
    #[serde(default)]
    serve_static: Option<bool>,
}

impl Config {
    /// Build the final [`Config`] by reading the TOML file (if it exists)
    /// and overlaying CLI flags.
    pub fn load(args: &Args) -> Result<Self> {
        let raw = if args.config.exists() {
            let text = std::fs::read_to_string(&args.config)
                .map_err(|e| NanowebError::Config(format!("read {:?}: {}", args.config, e)))?;
            toml::from_str(&text)?
        } else {
            RawConfig::default()
        };

        let listen = args
            .listen
            .clone()
            .or(raw.listen)
            .unwrap_or_else(|| "127.0.0.1:8080".into());
        let tls_cert = args.tls_cert.clone().or(raw.tls_cert);
        let tls_key = args.tls_key.clone().or(raw.tls_key);
        let metrics_path = args
            .metrics
            .clone()
            .or(raw.metrics)
            .unwrap_or_else(|| "/metrics".into());
        let static_dir = args.static_dir.clone().or(raw.static_dir);
        let log_level = args
            .log_level
            .clone()
            .or(raw.log_level)
            .unwrap_or_else(|| "info".into());
        let graceful_timeout_s = raw.graceful_timeout_s.unwrap_or(30);
        let session_secret = raw
            .session_secret
            .unwrap_or_else(|| "nanoweb-default-secret-change-me".into());

        let middleware = MiddlewareConfig {
            logging: raw.middleware.logging.unwrap_or(true),
            gzip: raw.middleware.gzip.unwrap_or(false),
            rate_limit: raw.middleware.rate_limit.map(|r| RateLimitConfig {
                capacity: r.capacity,
                refill_per_sec: r.refill_per_sec,
            }),
            cors: raw.middleware.cors.map(|c| CorsConfig {
                allowed_origins: c.allowed_origins,
            }),
            csrf: raw.middleware.csrf.unwrap_or(false),
            basic_auth: raw.middleware.basic_auth.map(|b| BasicAuthConfig {
                realm: b.realm,
                credentials: b
                    .users
                    .into_iter()
                    .map(|u| (u.username, u.password))
                    .collect(),
            }),
        };

        let mut routes = Vec::new();
        for r in raw.routes {
            let response = if let Some(t) = r.text {
                RouteResponse::Text(t)
            } else if let Some(j) = r.json {
                RouteResponse::Json(j)
            } else if let Some(h) = r.html {
                RouteResponse::Html(h)
            } else if let Some(t) = r.template {
                RouteResponse::Template {
                    template: t,
                    context: r.context.unwrap_or(serde_json::Value::Null),
                }
            } else if r.serve_static.unwrap_or(false) {
                RouteResponse::StaticDir
            } else {
                return Err(NanowebError::Config(format!(
                    "route {} {} has no response",
                    r.method, r.path
                )));
            };
            routes.push(RouteConfig {
                method: r.method.to_uppercase(),
                path: r.path,
                response,
            });
        }

        Ok(Self {
            listen,
            tls_cert,
            tls_key,
            metrics_path,
            static_dir,
            log_level,
            graceful_timeout_s,
            session_secret,
            middleware,
            routes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Args;
    use std::path::PathBuf;

    #[test]
    fn cli_overrides_toml() {
        let mut args = Args::default();
        args.listen = Some("0.0.0.0:9000".into());
        args.config = PathBuf::from("nonexistent.toml");
        let cfg = Config::load(&args).unwrap();
        assert_eq!(cfg.listen, "0.0.0.0:9000");
        assert_eq!(cfg.metrics_path, "/metrics");
    }

    #[test]
    fn defaults_apply() {
        let mut args = Args::default();
        args.config = PathBuf::from("nonexistent.toml");
        let cfg = Config::load(&args).unwrap();
        assert_eq!(cfg.listen, "127.0.0.1:8080");
        assert_eq!(cfg.graceful_timeout_s, 30);
    }
}
