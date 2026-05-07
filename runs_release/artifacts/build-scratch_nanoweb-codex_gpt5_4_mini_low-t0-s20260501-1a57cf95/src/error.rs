use thiserror::Error;

#[derive(Debug, Error)]
pub enum NanowebError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("tls error: {0}")]
    Tls(String),
    #[error("server error: {0}")]
    Server(String),
}

impl NanowebError {
    pub fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_contains_message() {
        let e = NanowebError::parse("x");
        assert!(e.to_string().contains('x'));
    }
}
