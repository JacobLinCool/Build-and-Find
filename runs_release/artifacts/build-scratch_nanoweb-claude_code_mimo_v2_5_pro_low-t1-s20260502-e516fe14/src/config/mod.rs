use crate::error::NanowebError;
use serde::Deserialize;
use std::path::Path;

/// Framework configuration loaded from TOML and CLI.
#[derive(Debug, Clone)]
pub struct Config {
    pub listen: String,
    pub config_path: String,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
    pub metrics: String,
    pub static_dir: Option<String>,
    pub log_level: String,
    pub graceful_timeout_s: u64,
    pub session_secret: String,
    pub routes: Vec<RouteConfig>,
    pub middleware: Vec<MiddlewareConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub method: String,
    pub path: String,
    pub handler: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MiddlewareConfig {
    pub name: String,
    #[serde(default = "default_toml_value")]
    pub options: toml::Value,
}

fn default_toml_value() -> toml::Value {
    toml::Value::Table(toml::map::Map::new())
}

/// Raw TOML file structure.
#[derive(Debug, Deserialize)]
struct TomlConfig {
    #[serde(default = "default_listen")]
    listen: Option<String>,
    #[serde(default)]
    tls_cert: Option<String>,
    #[serde(default)]
    tls_key: Option<String>,
    #[serde(default = "default_metrics")]
    metrics: Option<String>,
    #[serde(default)]
    static_dir: Option<String>,
    #[serde(default = "default_log_level")]
    log_level: Option<String>,
    #[serde(default = "default_graceful_timeout")]
    graceful_timeout_s: Option<u64>,
    #[serde(default = "default_session_secret")]
    session_secret: Option<String>,
    #[serde(default)]
    routes: Vec<RouteConfig>,
    #[serde(default)]
    middleware: Vec<MiddlewareConfig>,
}

fn default_listen() -> Option<String> {
    Some("127.0.0.1:8080".into())
}
fn default_metrics() -> Option<String> {
    Some("/metrics".into())
}
fn default_log_level() -> Option<String> {
    Some("info".into())
}
fn default_graceful_timeout() -> Option<u64> {
    Some(30)
}
fn default_session_secret() -> Option<String> {
    Some("change-me-in-production".into())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8080".into(),
            config_path: "./nanoweb.toml".into(),
            tls_cert: None,
            tls_key: None,
            metrics: "/metrics".into(),
            static_dir: None,
            log_level: "info".into(),
            graceful_timeout_s: 30,
            session_secret: "change-me-in-production".into(),
            routes: Vec::new(),
            middleware: Vec::new(),
        }
    }
}

impl Config {
    /// Load config from a TOML file.
    pub fn from_toml(path: &str) -> Result<Self, NanowebError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| NanowebError::Config(format!("Failed to read config file: {}", e)))?;

        let toml_config: TomlConfig = toml::from_str(&content)
            .map_err(|e| NanowebError::Config(format!("Failed to parse config: {}", e)))?;

        Ok(Self {
            listen: toml_config
                .listen
                .unwrap_or_else(|| "127.0.0.1:8080".into()),
            config_path: path.to_string(),
            tls_cert: toml_config.tls_cert,
            tls_key: toml_config.tls_key,
            metrics: toml_config.metrics.unwrap_or_else(|| "/metrics".into()),
            static_dir: toml_config.static_dir,
            log_level: toml_config.log_level.unwrap_or_else(|| "info".into()),
            graceful_timeout_s: toml_config.graceful_timeout_s.unwrap_or(30),
            session_secret: toml_config
                .session_secret
                .unwrap_or_else(|| "change-me-in-production".into()),
            routes: toml_config.routes,
            middleware: toml_config.middleware,
        })
    }

    /// Merge CLI flags into config. CLI flags override TOML values.
    pub fn merge_cli(
        &mut self,
        listen: Option<&str>,
        config_path: Option<&str>,
        tls_cert: Option<&str>,
        tls_key: Option<&str>,
        metrics: Option<&str>,
        static_dir: Option<&str>,
        log_level: Option<&str>,
    ) {
        if let Some(l) = listen {
            self.listen = l.to_string();
        }
        if let Some(c) = config_path {
            self.config_path = c.to_string();
        }
        if let Some(cert) = tls_cert {
            self.tls_cert = Some(cert.to_string());
        }
        if let Some(key) = tls_key {
            self.tls_key = Some(key.to_string());
        }
        if let Some(m) = metrics {
            self.metrics = m.to_string();
        }
        if let Some(s) = static_dir {
            self.static_dir = Some(s.to_string());
        }
        if let Some(l) = log_level {
            self.log_level = l.to_string();
        }
    }

    /// Load config from TOML file if it exists, otherwise use defaults, then merge CLI.
    pub fn load(
        config_path: &str,
        listen: Option<&str>,
        tls_cert: Option<&str>,
        tls_key: Option<&str>,
        metrics: Option<&str>,
        static_dir: Option<&str>,
        log_level: Option<&str>,
    ) -> Result<Self, NanowebError> {
        let mut config = if Path::new(config_path).exists() {
            Self::from_toml(config_path)?
        } else {
            let mut c = Self::default();
            c.config_path = config_path.to_string();
            c
        };

        config.merge_cli(
            listen,
            Some(config_path),
            tls_cert,
            tls_key,
            metrics,
            static_dir,
            log_level,
        );
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.listen, "127.0.0.1:8080");
        assert_eq!(config.graceful_timeout_s, 30);
    }

    #[test]
    fn test_from_toml() {
        let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tmpfile,
            r#"
listen = "0.0.0.0:3000"
log_level = "debug"
graceful_timeout_s = 60

[[routes]]
method = "GET"
path = "/"
handler = "index"

[[middleware]]
name = "logging"
"#
        )
        .unwrap();

        let config = Config::from_toml(tmpfile.path().to_str().unwrap()).unwrap();
        assert_eq!(config.listen, "0.0.0.0:3000");
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.graceful_timeout_s, 60);
        assert_eq!(config.routes.len(), 1);
        assert_eq!(config.middleware.len(), 1);
    }

    #[test]
    fn test_cli_override() {
        let mut config = Config::default();
        config.merge_cli(
            Some("0.0.0.0:9090"),
            None,
            Some("/cert.pem"),
            Some("/key.pem"),
            None,
            Some("/static"),
            Some("debug"),
        );
        assert_eq!(config.listen, "0.0.0.0:9090");
        assert_eq!(config.tls_cert, Some("/cert.pem".into()));
        assert_eq!(config.tls_key, Some("/key.pem".into()));
        assert_eq!(config.static_dir, Some("/static".into()));
        assert_eq!(config.log_level, "debug");
    }
}
