use std::fmt;
use std::io;

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Lex {
        line: usize,
        col: usize,
        msg: String,
    },
    Parse {
        line: usize,
        col: usize,
        msg: String,
    },
    Type(String),
    Runtime(String),
    Catalog(String),
    Storage(String),
    Wal(String),
    Constraint(String),
    NoSuchTable(String),
    NoSuchColumn(String),
    NoSuchIndex(String),
    AlreadyExists(String),
    Cli(String),
    Other(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(e) => write!(f, "io error: {}", e),
            MiniDbError::Lex { line, col, msg } => {
                write!(f, "lex error at line {} col {}: {}", line, col, msg)
            }
            MiniDbError::Parse { line, col, msg } => {
                write!(f, "parse error at line {} col {}: {}", line, col, msg)
            }
            MiniDbError::Type(s) => write!(f, "type error: {}", s),
            MiniDbError::Runtime(s) => write!(f, "runtime error: {}", s),
            MiniDbError::Catalog(s) => write!(f, "catalog error: {}", s),
            MiniDbError::Storage(s) => write!(f, "storage error: {}", s),
            MiniDbError::Wal(s) => write!(f, "wal error: {}", s),
            MiniDbError::Constraint(s) => write!(f, "constraint violation: {}", s),
            MiniDbError::NoSuchTable(t) => write!(f, "no such table: {}", t),
            MiniDbError::NoSuchColumn(c) => write!(f, "no such column: {}", c),
            MiniDbError::NoSuchIndex(i) => write!(f, "no such index: {}", i),
            MiniDbError::AlreadyExists(s) => write!(f, "already exists: {}", s),
            MiniDbError::Cli(s) => write!(f, "cli error: {}", s),
            MiniDbError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        MiniDbError::Io(e)
    }
}

impl MiniDbError {
    /// Classify this error for CLI exit-code purposes.
    pub fn category(&self) -> ErrorCategory {
        match self {
            MiniDbError::Cli(_) => ErrorCategory::Usage,
            MiniDbError::Io(_) | MiniDbError::Storage(_) | MiniDbError::Wal(_) => {
                ErrorCategory::Storage
            }
            _ => ErrorCategory::Sql,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Sql,
    Usage,
    Storage,
}

pub type Result<T> = std::result::Result<T, MiniDbError>;
