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
    Catalog(String),
    Constraint(String),
    Storage(String),
    Wal(String),
    Transaction(String),
    NotSupported(String),
    Cli(String),
}

impl MiniDbError {
    pub fn parse<S: Into<String>>(msg: S, line: usize, column: usize) -> Self {
        MiniDbError::Parse {
            message: msg.into(),
            line,
            column,
        }
    }

    pub fn lex<S: Into<String>>(msg: S, line: usize, column: usize) -> Self {
        MiniDbError::Lex {
            message: msg.into(),
            line,
            column,
        }
    }

    pub fn ty<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Type(msg.into())
    }

    pub fn runtime<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Runtime(msg.into())
    }

    pub fn catalog<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Catalog(msg.into())
    }

    pub fn constraint<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Constraint(msg.into())
    }

    pub fn storage<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Storage(msg.into())
    }

    pub fn wal<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Wal(msg.into())
    }

    pub fn txn<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Transaction(msg.into())
    }

    pub fn unsupported<S: Into<String>>(msg: S) -> Self {
        MiniDbError::NotSupported(msg.into())
    }

    pub fn cli<S: Into<String>>(msg: S) -> Self {
        MiniDbError::Cli(msg.into())
    }

    /// Returns the (line, column) position when known.
    pub fn position(&self) -> Option<(usize, usize)> {
        match self {
            MiniDbError::Parse { line, column, .. } => Some((*line, *column)),
            MiniDbError::Lex { line, column, .. } => Some((*line, *column)),
            _ => None,
        }
    }
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(e) => write!(f, "io error: {}", e),
            MiniDbError::Parse {
                message,
                line,
                column,
            } => write!(f, "parse error at {}:{}: {}", line, column, message),
            MiniDbError::Lex {
                message,
                line,
                column,
            } => write!(f, "lex error at {}:{}: {}", line, column, message),
            MiniDbError::Type(m) => write!(f, "type error: {}", m),
            MiniDbError::Runtime(m) => write!(f, "runtime error: {}", m),
            MiniDbError::Catalog(m) => write!(f, "catalog error: {}", m),
            MiniDbError::Constraint(m) => write!(f, "constraint violation: {}", m),
            MiniDbError::Storage(m) => write!(f, "storage error: {}", m),
            MiniDbError::Wal(m) => write!(f, "wal error: {}", m),
            MiniDbError::Transaction(m) => write!(f, "transaction error: {}", m),
            MiniDbError::NotSupported(m) => write!(f, "not supported: {}", m),
            MiniDbError::Cli(m) => write!(f, "cli error: {}", m),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        MiniDbError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_position() {
        let e = MiniDbError::parse("unexpected token", 3, 7);
        let s = format!("{}", e);
        assert!(s.contains("3:7"));
        assert!(s.contains("unexpected token"));
    }

    #[test]
    fn position_propagates() {
        let e = MiniDbError::lex("bad char", 1, 2);
        assert_eq!(e.position(), Some((1, 2)));
        let e2 = MiniDbError::runtime("oops");
        assert_eq!(e2.position(), None);
    }

    #[test]
    fn from_io() {
        let io_err = io::Error::new(io::ErrorKind::Other, "x");
        let e: MiniDbError = io_err.into();
        assert!(matches!(e, MiniDbError::Io(_)));
    }
}
