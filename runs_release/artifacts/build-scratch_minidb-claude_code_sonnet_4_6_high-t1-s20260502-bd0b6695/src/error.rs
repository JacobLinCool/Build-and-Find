use std::fmt;
use std::io;

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Parse { msg: String, line: u32, col: u32 },
    TypeMismatch(String),
    ConstraintViolation(String),
    NoSuchTable(String),
    NoSuchColumn(String),
    NoSuchIndex(String),
    DuplicateKey(String),
    TxnError(String),
    StorageCorruption(String),
    Internal(String),
    NotSupported(String),
    TableExists(String),
    IndexExists(String),
    InvalidArgument(String),
    DivisionByZero,
    NullViolation(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(e) => write!(f, "IO error: {}", e),
            MiniDbError::Parse { msg, line, col } => {
                write!(f, "Parse error at line {}, col {}: {}", line, col, msg)
            }
            MiniDbError::TypeMismatch(s) => write!(f, "Type mismatch: {}", s),
            MiniDbError::ConstraintViolation(s) => write!(f, "Constraint violation: {}", s),
            MiniDbError::NoSuchTable(s) => write!(f, "No such table: {}", s),
            MiniDbError::NoSuchColumn(s) => write!(f, "No such column: {}", s),
            MiniDbError::NoSuchIndex(s) => write!(f, "No such index: {}", s),
            MiniDbError::DuplicateKey(s) => write!(f, "Duplicate key: {}", s),
            MiniDbError::TxnError(s) => write!(f, "Transaction error: {}", s),
            MiniDbError::StorageCorruption(s) => write!(f, "Storage corruption: {}", s),
            MiniDbError::Internal(s) => write!(f, "Internal error: {}", s),
            MiniDbError::NotSupported(s) => write!(f, "Not supported: {}", s),
            MiniDbError::TableExists(s) => write!(f, "Table already exists: {}", s),
            MiniDbError::IndexExists(s) => write!(f, "Index already exists: {}", s),
            MiniDbError::InvalidArgument(s) => write!(f, "Invalid argument: {}", s),
            MiniDbError::DivisionByZero => write!(f, "Division by zero"),
            MiniDbError::NullViolation(s) => write!(f, "NOT NULL constraint violated: {}", s),
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

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        MiniDbError::Internal(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let e = MiniDbError::Parse {
            msg: "unexpected token".into(),
            line: 1,
            col: 5,
        };
        assert!(e.to_string().contains("line 1"));
        assert!(e.to_string().contains("col 5"));

        let e2 = MiniDbError::NoSuchTable("users".into());
        assert!(e2.to_string().contains("users"));
    }
}
