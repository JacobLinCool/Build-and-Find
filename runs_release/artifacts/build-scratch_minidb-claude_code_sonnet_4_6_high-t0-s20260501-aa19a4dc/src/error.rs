use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MiniDbError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error at {line}:{col}: {msg}")]
    Parse {
        msg: String,
        line: usize,
        col: usize,
    },

    #[error("Lexer error at {line}:{col}: {msg}")]
    Lex {
        msg: String,
        line: usize,
        col: usize,
    },

    #[error("Type error: {0}")]
    Type(String),

    #[error("Runtime error: {0}")]
    Runtime(String),

    #[error("Constraint violation: {0}")]
    Constraint(String),

    #[error("Schema error: {0}")]
    Schema(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("WAL error: {0}")]
    Wal(String),

    #[error("Transaction error: {0}")]
    Transaction(String),

    #[error("Index error: {0}")]
    Index(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Corruption detected: {0}")]
    Corruption(String),

    #[error("CRC mismatch in WAL record")]
    CrcMismatch,

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

impl MiniDbError {
    pub fn parse(msg: impl fmt::Display, line: usize, col: usize) -> Self {
        MiniDbError::Parse {
            msg: msg.to_string(),
            line,
            col,
        }
    }

    pub fn lex(msg: impl fmt::Display, line: usize, col: usize) -> Self {
        MiniDbError::Lex {
            msg: msg.to_string(),
            line,
            col,
        }
    }

    pub fn is_sql_error(&self) -> bool {
        matches!(
            self,
            MiniDbError::Parse { .. }
                | MiniDbError::Lex { .. }
                | MiniDbError::Type(_)
                | MiniDbError::Runtime(_)
                | MiniDbError::Constraint(_)
                | MiniDbError::Schema(_)
                | MiniDbError::NotFound(_)
                | MiniDbError::AlreadyExists(_)
        )
    }

    pub fn is_storage_error(&self) -> bool {
        matches!(
            self,
            MiniDbError::Io(_)
                | MiniDbError::Storage(_)
                | MiniDbError::Wal(_)
                | MiniDbError::Corruption(_)
                | MiniDbError::CrcMismatch
        )
    }

    pub fn location(&self) -> Option<(usize, usize)> {
        match self {
            MiniDbError::Parse { line, col, .. } => Some((*line, *col)),
            MiniDbError::Lex { line, col, .. } => Some((*line, *col)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_classification() {
        let e = MiniDbError::parse("unexpected token", 1, 5);
        assert!(e.is_sql_error());
        assert!(!e.is_storage_error());
        assert_eq!(e.location(), Some((1, 5)));

        let e = MiniDbError::Storage("disk full".into());
        assert!(e.is_storage_error());
        assert!(!e.is_sql_error());
    }

    #[test]
    fn test_error_display() {
        let e = MiniDbError::parse("unexpected EOF", 3, 10);
        assert!(e.to_string().contains("3:10"));
    }
}
