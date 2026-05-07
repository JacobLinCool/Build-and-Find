use thiserror::Error;

pub type Result<T> = std::result::Result<T, NanowebError>;

#[derive(Debug, Error)]
pub enum NanowebError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("http parse error: {0}")]
    Parse(String),
    #[error("tls error: {0}")]
    Tls(String),
    #[error("template error: {0}")]
    Template(String),
    #[error("serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("urlencoded error: {0}")]
    UrlEncoded(#[from] serde_urlencoded::de::Error),
    #[error("{0}")]
    Other(String),
}
