use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Http(String),
    Parse(String),
    Router(String),
    Config(String),
    Tls(String),
    Template(String),
    Session(String),
    Body(String),
    Timeout,
    Shutdown,
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Http(msg) => write!(f, "HTTP error: {msg}"),
            Self::Parse(msg) => write!(f, "Parse error: {msg}"),
            Self::Router(msg) => write!(f, "Router error: {msg}"),
            Self::Config(msg) => write!(f, "Config error: {msg}"),
            Self::Tls(msg) => write!(f, "TLS error: {msg}"),
            Self::Template(msg) => write!(f, "Template error: {msg}"),
            Self::Session(msg) => write!(f, "Session error: {msg}"),
            Self::Body(msg) => write!(f, "Body error: {msg}"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Shutdown => write!(f, "Server shutting down"),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<std::io::Error> for NanowebError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(e: serde_json::Error) -> Self {
        Self::Body(format!("JSON error: {e}"))
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        Self::Config(format!("TOML parse error: {e}"))
    }
}
