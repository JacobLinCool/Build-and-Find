use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Parse(String),
    Config(String),
    Tls(String),
    BadRequest(String),
    NotFound,
    MethodNotAllowed,
    Internal(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "io error: {}", e),
            NanowebError::Parse(s) => write!(f, "parse error: {}", s),
            NanowebError::Config(s) => write!(f, "config error: {}", s),
            NanowebError::Tls(s) => write!(f, "tls error: {}", s),
            NanowebError::BadRequest(s) => write!(f, "bad request: {}", s),
            NanowebError::NotFound => write!(f, "not found"),
            NanowebError::MethodNotAllowed => write!(f, "method not allowed"),
            NanowebError::Internal(s) => write!(f, "internal error: {}", s),
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
        NanowebError::Parse(e.to_string())
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, NanowebError>;
