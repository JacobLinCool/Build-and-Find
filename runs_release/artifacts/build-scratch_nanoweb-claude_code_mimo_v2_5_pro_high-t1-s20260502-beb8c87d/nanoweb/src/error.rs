use std::fmt;
use std::io;

#[derive(Debug)]
pub enum NanowebError {
    Io(io::Error),
    Http(String),
    Parse(String),
    Route(String),
    Config(String),
    Tls(String),
    Body(String),
    Template(String),
    Session(String),
    Other(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "IO error: {e}"),
            NanowebError::Http(msg) => write!(f, "HTTP error: {msg}"),
            NanowebError::Parse(msg) => write!(f, "Parse error: {msg}"),
            NanowebError::Route(msg) => write!(f, "Route error: {msg}"),
            NanowebError::Config(msg) => write!(f, "Config error: {msg}"),
            NanowebError::Tls(msg) => write!(f, "TLS error: {msg}"),
            NanowebError::Body(msg) => write!(f, "Body error: {msg}"),
            NanowebError::Template(msg) => write!(f, "Template error: {msg}"),
            NanowebError::Session(msg) => write!(f, "Session error: {msg}"),
            NanowebError::Other(msg) => write!(f, "Error: {msg}"),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<io::Error> for NanowebError {
    fn from(e: io::Error) -> Self {
        NanowebError::Io(e)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(e: serde_json::Error) -> Self {
        NanowebError::Body(e.to_string())
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(e.to_string())
    }
}
