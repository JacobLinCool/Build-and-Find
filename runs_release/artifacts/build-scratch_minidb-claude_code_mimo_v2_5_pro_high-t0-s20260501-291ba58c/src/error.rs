use std::fmt;
use std::io;
use std::num::{ParseFloatError, ParseIntError};

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Parse {
        message: String,
        line: usize,
        col: usize,
    },
    Syntax(String),
    Type(String),
    Constraint(String),
    Runtime(String),
    Catalog(String),
    Storage(String),
    Wal(String),
    BTree(String),
    Transaction(String),
    NotImplemented(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Parse { message, line, col } => {
                write!(f, "Parse error at {line}:{col}: {message}")
            }
            Self::Syntax(msg) => write!(f, "Syntax error: {msg}"),
            Self::Type(msg) => write!(f, "Type error: {msg}"),
            Self::Constraint(msg) => write!(f, "Constraint violation: {msg}"),
            Self::Runtime(msg) => write!(f, "Runtime error: {msg}"),
            Self::Catalog(msg) => write!(f, "Catalog error: {msg}"),
            Self::Storage(msg) => write!(f, "Storage error: {msg}"),
            Self::Wal(msg) => write!(f, "WAL error: {msg}"),
            Self::BTree(msg) => write!(f, "B-tree error: {msg}"),
            Self::Transaction(msg) => write!(f, "Transaction error: {msg}"),
            Self::NotImplemented(msg) => write!(f, "Not implemented: {msg}"),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ParseIntError> for MiniDbError {
    fn from(e: ParseIntError) -> Self {
        Self::Type(format!("integer parse error: {e}"))
    }
}

impl From<ParseFloatError> for MiniDbError {
    fn from(e: ParseFloatError) -> Self {
        Self::Type(format!("float parse error: {e}"))
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        Self::Runtime(format!("JSON error: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;
