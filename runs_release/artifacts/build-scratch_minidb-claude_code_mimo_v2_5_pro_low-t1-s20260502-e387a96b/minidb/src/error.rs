use std::fmt;
use std::io;

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Parse {
        message: String,
        line: usize,
        col: usize,
    },
    ParseGeneric(String),
    TypeMismatch(String),
    ConstraintViolation(String),
    TableNotFound(String),
    ColumnNotFound(String),
    IndexNotFound(String),
    TableExists(String),
    IndexExists(String),
    DuplicateColumn(String),
    InvalidExpression(String),
    RuntimeError(String),
    Storage(String),
    WalCorruption(String),
    Syntax(String),
    NotImplemented(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Parse { message, line, col } => {
                write!(f, "Parse error at {line}:{col}: {message}")
            }
            Self::ParseGeneric(s) => write!(f, "Parse error: {s}"),
            Self::TypeMismatch(s) => write!(f, "Type mismatch: {s}"),
            Self::ConstraintViolation(s) => write!(f, "Constraint violation: {s}"),
            Self::TableNotFound(s) => write!(f, "Table not found: {s}"),
            Self::ColumnNotFound(s) => write!(f, "Column not found: {s}"),
            Self::IndexNotFound(s) => write!(f, "Index not found: {s}"),
            Self::TableExists(s) => write!(f, "Table already exists: {s}"),
            Self::IndexExists(s) => write!(f, "Index already exists: {s}"),
            Self::DuplicateColumn(s) => write!(f, "Duplicate column: {s}"),
            Self::InvalidExpression(s) => write!(f, "Invalid expression: {s}"),
            Self::RuntimeError(s) => write!(f, "Runtime error: {s}"),
            Self::Storage(s) => write!(f, "Storage error: {s}"),
            Self::WalCorruption(s) => write!(f, "WAL corruption: {s}"),
            Self::Syntax(s) => write!(f, "Syntax error: {s}"),
            Self::NotImplemented(s) => write!(f, "Not implemented: {s}"),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        Self::RuntimeError(format!("JSON error: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;
