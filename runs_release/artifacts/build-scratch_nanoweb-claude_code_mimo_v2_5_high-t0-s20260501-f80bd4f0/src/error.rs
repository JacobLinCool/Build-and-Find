use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Http(String),
    Parse(String),
    NotFound,
    MethodNotAllowed,
    BodyTooLarge,
    Template(String),
    Session(String),
    Config(String),
    Tls(String),
    Internal(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "I/O error: {e}"),
            NanowebError::Http(msg) => write!(f, "HTTP error: {msg}"),
            NanowebError::Parse(msg) => write!(f, "Parse error: {msg}"),
            NanowebError::NotFound => write!(f, "Not Found"),
            NanowebError::MethodNotAllowed => write!(f, "Method Not Allowed"),
            NanowebError::BodyTooLarge => write!(f, "Body Too Large"),
            NanowebError::Template(msg) => write!(f, "Template error: {msg}"),
            NanowebError::Session(msg) => write!(f, "Session error: {msg}"),
            NanowebError::Config(msg) => write!(f, "Config error: {msg}"),
            NanowebError::Tls(msg) => write!(f, "TLS error: {msg}"),
            NanowebError::Internal(msg) => write!(f, "Internal error: {msg}"),
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
