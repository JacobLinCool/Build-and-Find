use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Http(String),
    ParseError(String),
    NotFound,
    MethodNotAllowed,
    BadRequest(String),
    InternalServerError(String),
    Unauthorized,
    Forbidden,
    TooManyRequests,
    Config(String),
    Template(String),
    Session(String),
    #[cfg(feature = "tls")]
    Tls(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "IO error: {}", e),
            NanowebError::Http(s) => write!(f, "HTTP error: {}", s),
            NanowebError::ParseError(s) => write!(f, "Parse error: {}", s),
            NanowebError::NotFound => write!(f, "Not found"),
            NanowebError::MethodNotAllowed => write!(f, "Method not allowed"),
            NanowebError::BadRequest(s) => write!(f, "Bad request: {}", s),
            NanowebError::InternalServerError(s) => write!(f, "Internal server error: {}", s),
            NanowebError::Unauthorized => write!(f, "Unauthorized"),
            NanowebError::Forbidden => write!(f, "Forbidden"),
            NanowebError::TooManyRequests => write!(f, "Too many requests"),
            NanowebError::Config(s) => write!(f, "Config error: {}", s),
            NanowebError::Template(s) => write!(f, "Template error: {}", s),
            NanowebError::Session(s) => write!(f, "Session error: {}", s),
            #[cfg(feature = "tls")]
            NanowebError::Tls(s) => write!(f, "TLS error: {}", s),
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
        NanowebError::ParseError(e.to_string())
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(e.to_string())
    }
}
