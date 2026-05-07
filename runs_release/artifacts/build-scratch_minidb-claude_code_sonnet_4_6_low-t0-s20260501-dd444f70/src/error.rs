use std::fmt;

#[derive(Debug)]
pub enum MiniDbError {
    Io(std::io::Error),
    Parse {
        line: usize,
        col: usize,
        msg: String,
    },
    Type(String),
    Runtime(String),
    Storage(String),
    Wal(String),
    Transaction(String),
    Constraint(String),
    TableNotFound(String),
    ColumnNotFound(String),
    IndexNotFound(String),
    DuplicateTable(String),
    DuplicateIndex(String),
    DuplicateKey(String),
    Ambiguous(String),
    NotImplemented(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(e) => write!(f, "IO error: {}", e),
            MiniDbError::Parse { line, col, msg } => {
                write!(f, "Parse error at {}:{}: {}", line, col, msg)
            }
            MiniDbError::Type(m) => write!(f, "Type error: {}", m),
            MiniDbError::Runtime(m) => write!(f, "Runtime error: {}", m),
            MiniDbError::Storage(m) => write!(f, "Storage error: {}", m),
            MiniDbError::Wal(m) => write!(f, "WAL error: {}", m),
            MiniDbError::Transaction(m) => write!(f, "Transaction error: {}", m),
            MiniDbError::Constraint(m) => write!(f, "Constraint violation: {}", m),
            MiniDbError::TableNotFound(n) => write!(f, "Table not found: {}", n),
            MiniDbError::ColumnNotFound(n) => write!(f, "Column not found: {}", n),
            MiniDbError::IndexNotFound(n) => write!(f, "Index not found: {}", n),
            MiniDbError::DuplicateTable(n) => write!(f, "Table already exists: {}", n),
            MiniDbError::DuplicateIndex(n) => write!(f, "Index already exists: {}", n),
            MiniDbError::DuplicateKey(m) => write!(f, "Duplicate key: {}", m),
            MiniDbError::Ambiguous(m) => write!(f, "Ambiguous reference: {}", m),
            MiniDbError::NotImplemented(m) => write!(f, "Not implemented: {}", m),
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

impl From<std::io::Error> for MiniDbError {
    fn from(e: std::io::Error) -> Self {
        MiniDbError::Io(e)
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        MiniDbError::Runtime(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        let e = MiniDbError::TableNotFound("foo".to_string());
        assert!(e.to_string().contains("foo"));
    }

    #[test]
    fn test_parse_error() {
        let e = MiniDbError::Parse {
            line: 1,
            col: 5,
            msg: "unexpected token".to_string(),
        };
        assert!(e.to_string().contains("1:5"));
    }
}
