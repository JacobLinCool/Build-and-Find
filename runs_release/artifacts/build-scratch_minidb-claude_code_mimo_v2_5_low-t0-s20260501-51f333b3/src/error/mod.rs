use std::fmt;

#[derive(Debug, Clone)]
pub enum MiniDbError {
    ParseError {
        message: String,
        line: usize,
        column: usize,
    },
    TypeError {
        message: String,
    },
    StorageError(String),
    IoError(String),
    SqlError(String),
    TransactionError(String),
    CatalogError(String),
    IndexError(String),
    ExitRequested,
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
            MiniDbError::TypeError { message } => write!(f, "Type error: {}", message),
            MiniDbError::StorageError(msg) => write!(f, "Storage error: {}", msg),
            MiniDbError::IoError(msg) => write!(f, "IO error: {}", msg),
            MiniDbError::SqlError(msg) => write!(f, "SQL error: {}", msg),
            MiniDbError::TransactionError(msg) => write!(f, "Transaction error: {}", msg),
            MiniDbError::CatalogError(msg) => write!(f, "Catalog error: {}", msg),
            MiniDbError::IndexError(msg) => write!(f, "Index error: {}", msg),
            MiniDbError::ExitRequested => write!(f, "Exit requested"),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<std::io::Error> for MiniDbError {
    fn from(e: std::io::Error) -> Self {
        MiniDbError::IoError(e.to_string())
    }
}

impl From<csv::Error> for MiniDbError {
    fn from(e: csv::Error) -> Self {
        MiniDbError::IoError(e.to_string())
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        MiniDbError::IoError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = MiniDbError::ParseError {
            message: "unexpected token".to_string(),
            line: 1,
            column: 5,
        };
        assert_eq!(err.to_string(), "Parse error at 1:5: unexpected token");
    }

    #[test]
    fn test_error_variants() {
        let err = MiniDbError::SqlError("table not found".to_string());
        assert_eq!(err.to_string(), "SQL error: table not found");

        let err = MiniDbError::TypeError {
            message: "mismatch".to_string(),
        };
        assert_eq!(err.to_string(), "Type error: mismatch");
    }
}
