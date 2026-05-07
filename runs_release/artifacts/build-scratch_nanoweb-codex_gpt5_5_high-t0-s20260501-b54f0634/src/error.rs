use std::{fmt, io};

pub type Result<T> = std::result::Result<T, NanowebError>;

#[derive(Debug)]
pub enum NanowebError {
    BadRequest(String),
    Config(String),
    Io(io::Error),
    Json(serde_json::Error),
    Toml(toml::de::Error),
    Tls(String),
    Other(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadRequest(s) => write!(f, "bad request: {s}"),
            Self::Config(s) => write!(f, "configuration error: {s}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Json(e) => write!(f, "json error: {e}"),
            Self::Toml(e) => write!(f, "toml error: {e}"),
            Self::Tls(s) => write!(f, "tls error: {s}"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<io::Error> for NanowebError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(value: toml::de::Error) -> Self {
        Self::Toml(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_bad_request() {
        assert!(
            NanowebError::BadRequest("x".into())
                .to_string()
                .contains("x")
        );
    }
}
