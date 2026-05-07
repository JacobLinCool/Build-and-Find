use thiserror::Error;

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[derive(Debug, Error)]
pub enum MiniDbError {
    #[error("usage error: {0}")]
    Usage(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error at line {line}, column {column}: {message}")]
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
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl MiniDbError {
    pub fn parse(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self::Parse {
            message: message.into(),
            line,
            column,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Io(_) | Self::Storage(_) | Self::Serde(_) => 3,
            Self::Parse { .. } | Self::Type(_) | Self::Sql(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_error_class() {
        assert_eq!(MiniDbError::Usage("bad".into()).exit_code(), 2);
        assert_eq!(MiniDbError::Sql("bad".into()).exit_code(), 1);
    }
}
