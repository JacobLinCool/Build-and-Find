use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Parse(String),
    Config(String),
    Tls(String),
    Body(String),
    Template(String),
    BadRequest(String),
    NotFound,
    Forbidden(String),
    Unauthorized(String),
    Internal(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "io error: {}", e),
            NanowebError::Parse(m) => write!(f, "parse error: {}", m),
            NanowebError::Config(m) => write!(f, "config error: {}", m),
            NanowebError::Tls(m) => write!(f, "tls error: {}", m),
            NanowebError::Body(m) => write!(f, "body error: {}", m),
            NanowebError::Template(m) => write!(f, "template error: {}", m),
            NanowebError::BadRequest(m) => write!(f, "bad request: {}", m),
            NanowebError::NotFound => write!(f, "not found"),
            NanowebError::Forbidden(m) => write!(f, "forbidden: {}", m),
            NanowebError::Unauthorized(m) => write!(f, "unauthorized: {}", m),
            NanowebError::Internal(m) => write!(f, "internal error: {}", m),
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

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, NanowebError>;
