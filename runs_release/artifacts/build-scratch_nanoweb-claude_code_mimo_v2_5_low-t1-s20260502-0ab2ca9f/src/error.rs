use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Http(String),
    Config(String),
    Template(String),
    BodyParse(String),
    Session(String),
    Tls(String),
    Route(String),
    Custom(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "IO error: {}", e),
            NanowebError::Http(msg) => write!(f, "HTTP error: {}", msg),
            NanowebError::Config(msg) => write!(f, "Config error: {}", msg),
            NanowebError::Template(msg) => write!(f, "Template error: {}", msg),
            NanowebError::BodyParse(msg) => write!(f, "Body parse error: {}", msg),
            NanowebError::Session(msg) => write!(f, "Session error: {}", msg),
            NanowebError::Tls(msg) => write!(f, "TLS error: {}", msg),
            NanowebError::Route(msg) => write!(f, "Route error: {}", msg),
            NanowebError::Custom(msg) => write!(f, "{}", msg),
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
        NanowebError::BodyParse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, NanowebError>;
