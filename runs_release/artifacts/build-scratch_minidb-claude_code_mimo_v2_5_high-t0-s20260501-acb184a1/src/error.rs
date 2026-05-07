use std::fmt;

/// Crate-wide error type for MiniDB.
#[derive(Debug)]
pub enum MiniDbError {
    /// SQL parse error with optional location
    ParseError {
        message: String,
        line: usize,
        column: usize,
    },
    /// Type mismatch or type-check error
    TypeError(String),
    /// Runtime error during execution
    RuntimeError(String),
    /// SQL syntax/semantic error
    SqlError(String),
    /// Storage / IO error
    IoError(std::io::Error),
    /// Page-level storage error
    StorageError(String),
    /// WAL error
    WalError(String),
    /// Transaction error
    TransactionError(String),
    /// Catalog / schema error
    CatalogError(String),
    /// Index error
    IndexError(String),
    /// Constraint violation
    ConstraintViolation(String),
    /// Column not found
    ColumnNotFound(String),
    /// Table not found
    TableNotFound(String),
    /// Table already exists
    TableAlreadyExists(String),
    /// Index already exists
    IndexAlreadyExists(String),
    /// Index not found
    IndexNotFound(String),
    /// Division by zero
    DivisionByZero,
    /// Internal / unreachable error
    Internal(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::ParseError {
                message,
                line,
                column,
            } => {
                write!(f, "Parse error at {}:{}: {}", line, column, message)
            }
            MiniDbError::TypeError(msg) => write!(f, "Type error: {}", msg),
            MiniDbError::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            MiniDbError::SqlError(msg) => write!(f, "SQL error: {}", msg),
            MiniDbError::IoError(e) => write!(f, "IO error: {}", e),
            MiniDbError::StorageError(msg) => write!(f, "Storage error: {}", msg),
            MiniDbError::WalError(msg) => write!(f, "WAL error: {}", msg),
            MiniDbError::TransactionError(msg) => write!(f, "Transaction error: {}", msg),
            MiniDbError::CatalogError(msg) => write!(f, "Catalog error: {}", msg),
            MiniDbError::IndexError(msg) => write!(f, "Index error: {}", msg),
            MiniDbError::ConstraintViolation(msg) => write!(f, "Constraint violation: {}", msg),
            MiniDbError::ColumnNotFound(col) => write!(f, "Column not found: {}", col),
            MiniDbError::TableNotFound(t) => write!(f, "Table not found: {}", t),
            MiniDbError::TableAlreadyExists(t) => write!(f, "Table already exists: {}", t),
            MiniDbError::IndexAlreadyExists(i) => write!(f, "Index already exists: {}", i),
            MiniDbError::IndexNotFound(i) => write!(f, "Index not found: {}", i),
            MiniDbError::DivisionByZero => write!(f, "Division by zero"),
            MiniDbError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for MiniDbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MiniDbError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for MiniDbError {
    fn from(e: std::io::Error) -> Self {
        MiniDbError::IoError(e)
    }
}

impl From<csv::Error> for MiniDbError {
    fn from(e: csv::Error) -> Self {
        MiniDbError::RuntimeError(format!("CSV error: {}", e))
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        MiniDbError::RuntimeError(format!("JSON error: {}", e))
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;
