use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    HttpParse(String),
    Http(String),
    Config(String),
    Template(String),
    Session(String),
    Tls(String),
    RouteNotFound(String),
    Body(String),
    Middleware(String),
    Internal(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "IO error: {e}"),
            NanowebError::HttpParse(s) => write!(f, "HTTP parse error: {s}"),
            NanowebError::Http(s) => write!(f, "HTTP error: {s}"),
            NanowebError::Config(s) => write!(f, "Config error: {s}"),
            NanowebError::Template(s) => write!(f, "Template error: {s}"),
            NanowebError::Session(s) => write!(f, "Session error: {s}"),
            NanowebError::Tls(s) => write!(f, "TLS error: {s}"),
            NanowebError::RouteNotFound(s) => write!(f, "Route not found: {s}"),
            NanowebError::Body(s) => write!(f, "Body error: {s}"),
            NanowebError::Middleware(s) => write!(f, "Middleware error: {s}"),
            NanowebError::Internal(s) => write!(f, "Internal error: {s}"),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<std::io::Error> for NanowebError {
    fn from(e: std::io::Error) -> Self {
        NanowebError::Io(e)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(e: serde_json::Error) -> Self {
        NanowebError::Body(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, NanowebError>;
