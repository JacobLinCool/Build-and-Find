use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Parse(String),
    Config(String),
    NotFound(String),
    MethodNotAllowed(Vec<String>),
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    InternalServer(String),
    TlsError(String),
    SerdeJson(serde_json::Error),
    Utf8(std::string::FromUtf8Error),
    Template(String),
    Session(String),
    RateLimit(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "IO error: {}", e),
            NanowebError::Parse(s) => write!(f, "Parse error: {}", s),
            NanowebError::Config(s) => write!(f, "Config error: {}", s),
            NanowebError::NotFound(s) => write!(f, "Not found: {}", s),
            NanowebError::MethodNotAllowed(methods) => {
                write!(f, "Method not allowed. Allowed: {}", methods.join(", "))
            }
            NanowebError::BadRequest(s) => write!(f, "Bad request: {}", s),
            NanowebError::Unauthorized(s) => write!(f, "Unauthorized: {}", s),
            NanowebError::Forbidden(s) => write!(f, "Forbidden: {}", s),
            NanowebError::InternalServer(s) => write!(f, "Internal server error: {}", s),
            NanowebError::TlsError(s) => write!(f, "TLS error: {}", s),
            NanowebError::SerdeJson(e) => write!(f, "JSON error: {}", e),
            NanowebError::Utf8(e) => write!(f, "UTF-8 error: {}", e),
            NanowebError::Template(s) => write!(f, "Template error: {}", s),
            NanowebError::Session(s) => write!(f, "Session error: {}", s),
            NanowebError::RateLimit(s) => write!(f, "Rate limit: {}", s),
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
        NanowebError::SerdeJson(e)
    }
}

impl From<std::string::FromUtf8Error> for NanowebError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        NanowebError::Utf8(e)
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(e.to_string())
    }
}
