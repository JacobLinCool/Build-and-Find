//! Crate-wide error type.

use std::fmt;
use std::io;

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Parse {
        message: String,
        line: usize,
        column: usize,
    },
    Lex {
        message: String,
        line: usize,
        column: usize,
    },
    Type(String),
    Runtime(String),
    Storage(String),
    Catalog(String),
    Txn(String),
    Cli(String),
    Internal(String),
}

impl MiniDbError {
    pub fn parse<S: Into<String>>(message: S, line: usize, column: usize) -> Self {
        Self::Parse {
            message: message.into(),
            line,
            column,
        }
    }

    pub fn lex<S: Into<String>>(message: S, line: usize, column: usize) -> Self {
        Self::Lex {
            message: message.into(),
            line,
            column,
        }
    }

    pub fn ty<S: Into<String>>(message: S) -> Self {
        Self::Type(message.into())
    }

    pub fn runtime<S: Into<String>>(message: S) -> Self {
        Self::Runtime(message.into())
    }

    pub fn storage<S: Into<String>>(message: S) -> Self {
        Self::Storage(message.into())
    }

    pub fn catalog<S: Into<String>>(message: S) -> Self {
        Self::Catalog(message.into())
    }

    pub fn txn<S: Into<String>>(message: S) -> Self {
        Self::Txn(message.into())
    }

    pub fn cli<S: Into<String>>(message: S) -> Self {
        Self::Cli(message.into())
    }

    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::Internal(message.into())
    }

    /// CLI exit code mapping per the spec.
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Cli(_) => 2,
            Self::Io(_) | Self::Storage(_) => 3,
            _ => 1,
        }
    }

    pub fn position(&self) -> Option<(usize, usize)> {
        match self {
            Self::Parse { line, column, .. } | Self::Lex { line, column, .. } => {
                Some((*line, *column))
            }
            _ => None,
        }
    }
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {}", e),
            Self::Parse {
                message,
                line,
                column,
            } => write!(f, "parse error at {}:{}: {}", line, column, message),
            Self::Lex {
                message,
                line,
                column,
            } => write!(f, "lex error at {}:{}: {}", line, column, message),
            Self::Type(m) => write!(f, "type error: {}", m),
            Self::Runtime(m) => write!(f, "runtime error: {}", m),
            Self::Storage(m) => write!(f, "storage error: {}", m),
            Self::Catalog(m) => write!(f, "catalog error: {}", m),
            Self::Txn(m) => write!(f, "transaction error: {}", m),
            Self::Cli(m) => write!(f, "cli error: {}", m),
            Self::Internal(m) => write!(f, "internal error: {}", m),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<bincode::Error> for MiniDbError {
    fn from(e: bincode::Error) -> Self {
        Self::Storage(format!("serialization: {}", e))
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        Self::Storage(format!("json: {}", e))
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_map_per_spec() {
        assert_eq!(MiniDbError::cli("x").exit_code(), 2);
        assert_eq!(
            MiniDbError::Io(io::Error::new(io::ErrorKind::Other, "y")).exit_code(),
            3
        );
        assert_eq!(MiniDbError::storage("z").exit_code(), 3);
        assert_eq!(MiniDbError::ty("t").exit_code(), 1);
    }

    #[test]
    fn parse_error_carries_position() {
        let e = MiniDbError::parse("oops", 3, 7);
        assert_eq!(e.position(), Some((3, 7)));
        let s = format!("{}", e);
        assert!(s.contains("3:7"));
    }
}
