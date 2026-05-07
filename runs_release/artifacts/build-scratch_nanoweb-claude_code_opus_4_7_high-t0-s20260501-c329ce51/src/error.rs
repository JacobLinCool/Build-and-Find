use std::fmt;
use std::io;

#[derive(Debug)]
pub enum NanowebError {
    Io(io::Error),
    Config(String),
    InvalidRequest(String),
    InvalidConfig(String),
    Tls(String),
    Routing(String),
    Body(String),
    Template(String),
    Session(String),
    Other(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "io error: {}", e),
            NanowebError::Config(m) => write!(f, "config error: {}", m),
            NanowebError::InvalidRequest(m) => write!(f, "invalid request: {}", m),
            NanowebError::InvalidConfig(m) => write!(f, "invalid config: {}", m),
            NanowebError::Tls(m) => write!(f, "tls error: {}", m),
            NanowebError::Routing(m) => write!(f, "routing error: {}", m),
            NanowebError::Body(m) => write!(f, "body error: {}", m),
            NanowebError::Template(m) => write!(f, "template error: {}", m),
            NanowebError::Session(m) => write!(f, "session error: {}", m),
            NanowebError::Other(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<io::Error> for NanowebError {
    fn from(e: io::Error) -> Self {
        NanowebError::Io(e)
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(e.to_string())
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(e: serde_json::Error) -> Self {
        NanowebError::Body(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, NanowebError>;
