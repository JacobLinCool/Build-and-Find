use thiserror::Error;

#[derive(Debug, Error)]
pub enum NanowebError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("router error: {0}")]
    Router(String),
    #[error("body error: {0}")]
    Body(String),
    #[error("tls error: {0}")]
    Tls(String),
    #[error("template error: {0}")]
    Template(String),
    #[error("session error: {0}")]
    Session(String),
    #[error("internal: {0}")]
    Internal(String),
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
