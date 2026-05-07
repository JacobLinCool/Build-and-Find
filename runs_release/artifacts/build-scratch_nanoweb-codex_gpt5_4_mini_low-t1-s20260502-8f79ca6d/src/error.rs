use thiserror::Error;

#[derive(Debug, Error)]
pub enum NanowebError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    UrlEncoded(#[from] url::ParseError),
    #[error(transparent)]
    Prometheus(#[from] prometheus::Error),
    #[error(transparent)]
    AddrParse(#[from] std::net::AddrParseError),
    #[error(transparent)]
    Rustls(
        #[cfg(feature = "tls")]
        #[from]
        rustls::Error,
    ),
}

impl From<&str> for NanowebError {
    fn from(value: &str) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<String> for NanowebError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}
