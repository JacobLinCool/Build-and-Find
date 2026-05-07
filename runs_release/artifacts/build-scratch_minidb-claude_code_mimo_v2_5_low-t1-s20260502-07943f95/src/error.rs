use std::fmt;

#[derive(Debug)]
pub enum MiniDbError {
    Parse(String),
    Syntax(String, Option<Position>),
    TypeMismatch(String),
    TypeError(String),
    Runtime(String),
    Storage(String),
    Io(std::io::Error),
    Transaction(String),
    Constraint(String),
    NotFound(String),
    Serialization(String),
}

#[derive(Debug, Clone)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Parse(msg) => write!(f, "Parse error: {}", msg),
            MiniDbError::Syntax(msg, Some(pos)) => {
                write!(f, "Syntax error at {}:{}: {}", pos.line, pos.column, msg)
            }
            MiniDbError::Syntax(msg, None) => write!(f, "Syntax error: {}", msg),
            MiniDbError::TypeMismatch(msg) => write!(f, "Type mismatch: {}", msg),
            MiniDbError::TypeError(msg) => write!(f, "Type error: {}", msg),
            MiniDbError::Runtime(msg) => write!(f, "Runtime error: {}", msg),
            MiniDbError::Storage(msg) => write!(f, "Storage error: {}", msg),
            MiniDbError::Io(e) => write!(f, "IO error: {}", e),
            MiniDbError::Transaction(msg) => write!(f, "Transaction error: {}", msg),
            MiniDbError::Constraint(msg) => write!(f, "Constraint violation: {}", msg),
            MiniDbError::NotFound(msg) => write!(f, "Not found: {}", msg),
            MiniDbError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<std::io::Error> for MiniDbError {
    fn from(e: std::io::Error) -> Self {
        MiniDbError::Io(e)
    }
}

impl From<csv::Error> for MiniDbError {
    fn from(e: csv::Error) -> Self {
        MiniDbError::Runtime(e.to_string())
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        MiniDbError::Runtime(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;
