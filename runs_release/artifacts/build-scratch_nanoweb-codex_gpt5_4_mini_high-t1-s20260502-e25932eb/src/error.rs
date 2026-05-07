use thiserror::Error;

pub type Result<T> = std::result::Result<T, NanowebError>;

#[derive(Debug, Error)]
pub enum NanowebError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("urlencoded error: {0}")]
    UrlEncoded(#[from] serde_urlencoded::de::Error),
    #[error("request error: {0}")]
    Request(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("body parse error: {0}")]
    Body(String),
    #[error("routing error: {0}")]
    Router(String),
    #[error("template error: {0}")]
    Template(String),
    #[error("session error: {0}")]
    Session(String),
    #[error("tls error: {0}")]
    Tls(String),
    #[error("static file error: {0}")]
    StaticFile(String),
    #[error("metrics error: {0}")]
    Metrics(String),
    #[error("server error: {0}")]
    Server(String),
}

impl NanowebError {
    pub fn request(msg: impl Into<String>) -> Self {
        Self::Request(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }
}
