use thiserror::Error;

#[derive(Debug, Error)]
pub enum NanowebError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("configuration error: {0}")]
    Config(String),
    #[error("http parse error: {0}")]
    Parse(String),
    #[error("routing error: {0}")]
    Router(String),
    #[error("tls error: {0}")]
    Tls(String),
    #[error("template error: {0}")]
    Template(String),
    #[error("body parse error: {0}")]
    Body(String),
    #[error("server error: {0}")]
    Server(String),
}

pub type Result<T> = std::result::Result<T, NanowebError>;
