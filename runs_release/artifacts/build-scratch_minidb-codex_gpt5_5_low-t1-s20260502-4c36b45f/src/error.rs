use thiserror::Error;

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[derive(Debug, Error)]
pub enum MiniDbError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error at {line}:{column}: {message}")]
    Parse {
        message: String,
        line: usize,
        column: usize,
    },
    #[error("type error: {0}")]
    Type(String),
    #[error("sql error: {0}")]
    Sql(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("transaction error: {0}")]
    Txn(String),
    #[error("usage error: {0}")]
    Usage(String),
}

impl MiniDbError {
    pub fn parse(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self::Parse {
            message: message.into(),
            line,
            column,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_error_formats_position() {
        let e = MiniDbError::parse("bad", 2, 3).to_string();
        assert!(e.contains("2:3"));
    }
}
