use std::{fmt, io};

#[derive(Debug)]
pub enum NanowebError {
    Io(io::Error),
    Toml(toml::de::Error),
    Json(serde_json::Error),
    UrlEncoded(serde_urlencoded::de::Error),
    Parse(String),
    Config(String),
    Tls(String),
    Body(String),
    Auth(String),
    Other(String),
}

pub type Result<T> = std::result::Result<T, NanowebError>;

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(err) => write!(f, "io error: {err}"),
            NanowebError::Toml(err) => write!(f, "toml error: {err}"),
            NanowebError::Json(err) => write!(f, "json error: {err}"),
            NanowebError::UrlEncoded(err) => write!(f, "form error: {err}"),
            NanowebError::Parse(err) => write!(f, "parse error: {err}"),
            NanowebError::Config(err) => write!(f, "config error: {err}"),
            NanowebError::Tls(err) => write!(f, "tls error: {err}"),
            NanowebError::Body(err) => write!(f, "body error: {err}"),
            NanowebError::Auth(err) => write!(f, "auth error: {err}"),
            NanowebError::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<io::Error> for NanowebError {
    fn from(value: io::Error) -> Self {
        NanowebError::Io(value)
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(value: toml::de::Error) -> Self {
        NanowebError::Toml(value)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(value: serde_json::Error) -> Self {
        NanowebError::Json(value)
    }
}

impl From<serde_urlencoded::de::Error> for NanowebError {
    fn from(value: serde_urlencoded::de::Error) -> Self {
        NanowebError::UrlEncoded(value)
    }
}

impl From<&str> for NanowebError {
    fn from(value: &str) -> Self {
        NanowebError::Other(value.to_string())
    }
}

impl From<String> for NanowebError {
    fn from(value: String) -> Self {
        NanowebError::Other(value)
    }
}
