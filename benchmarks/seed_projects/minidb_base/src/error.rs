//! Crate-wide error type and `Result` alias for minidb.
//!
//! Every fallible operation in the database surfaces errors through
//! [`MiniDbError`]. Keeping a single enum makes error handling uniform at
//! module boundaries and allows the CLI to map specific classes of failure
//! to the documented exit codes.

use std::fmt;
use std::io;
use std::num::{ParseFloatError, ParseIntError};

/// Primary error type for the whole crate.
#[derive(Debug)]
pub enum MiniDbError {
    /// Underlying OS / I/O failure.
    Io(io::Error),
    /// SQL lexing or parsing failed.
    Parse(String),
    /// A value did not satisfy the required column or expression type.
    Type(String),
    /// A runtime execution error (division by zero, overflow, etc).
    Runtime(String),
    /// Catalog lookup / definition error (missing table, duplicate index…).
    Catalog(String),
    /// Transaction lifecycle violation (double commit, no active txn…).
    Transaction(String),
    /// Storage subsystem failure (corrupt page, CRC mismatch…).
    Storage(String),
    /// Invalid argument supplied via CLI or REPL meta-command.
    InvalidArgument(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(e) => write!(f, "io error: {e}"),
            MiniDbError::Parse(m) => write!(f, "parse error: {m}"),
            MiniDbError::Type(m) => write!(f, "type error: {m}"),
            MiniDbError::Runtime(m) => write!(f, "runtime error: {m}"),
            MiniDbError::Catalog(m) => write!(f, "catalog error: {m}"),
            MiniDbError::Transaction(m) => write!(f, "transaction error: {m}"),
            MiniDbError::Storage(m) => write!(f, "storage error: {m}"),
            MiniDbError::InvalidArgument(m) => write!(f, "invalid argument: {m}"),
        }
    }
}

impl std::error::Error for MiniDbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MiniDbError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        MiniDbError::Io(e)
    }
}

impl From<ParseIntError> for MiniDbError {
    fn from(e: ParseIntError) -> Self {
        MiniDbError::Parse(format!("invalid integer literal: {e}"))
    }
}

impl From<ParseFloatError> for MiniDbError {
    fn from(e: ParseFloatError) -> Self {
        MiniDbError::Parse(format!("invalid real literal: {e}"))
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        MiniDbError::Runtime(format!("json error: {e}"))
    }
}

/// Convenience `Result` alias used throughout the crate.
pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_converts() {
        let e = io::Error::new(io::ErrorKind::Other, "boom");
        let me: MiniDbError = e.into();
        match me {
            MiniDbError::Io(_) => {}
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn display_contains_prefix() {
        let e = MiniDbError::Parse("oops".into());
        assert!(format!("{e}").contains("parse error"));
    }
}
