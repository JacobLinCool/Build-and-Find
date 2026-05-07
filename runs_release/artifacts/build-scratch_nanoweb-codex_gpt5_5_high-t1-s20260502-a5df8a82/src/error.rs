use thiserror::Error;

pub type Result<T> = std::result::Result<T, NanowebError>;

#[derive(Debug, Error)]
pub enum NanowebError {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("HTTP parse error: {0}")]
    Parse(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("TLS error: {0}")]
    Tls(String),
    #[error("server error: {0}")]
    Server(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_error() {
        assert!(
            NanowebError::Config("missing".into())
                .to_string()
                .contains("missing")
        );
    }
}
