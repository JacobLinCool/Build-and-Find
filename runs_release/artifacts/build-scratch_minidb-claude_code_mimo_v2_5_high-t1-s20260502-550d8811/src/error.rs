use std::fmt;

#[derive(Debug, Clone)]
pub enum MiniDbError {
    Io(String),
    Parse(String),
    ParseAt {
        msg: String,
        line: usize,
        col: usize,
    },
    Type(String),
    Runtime(String),
    Storage(String),
    Sql(String),
    Internal(String),
    Transaction(String),
    Index(String),
    Constraint(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(msg) => write!(f, "IO error: {}", msg),
            MiniDbError::Parse(msg) => write!(f, "Parse error: {}", msg),
            MiniDbError::ParseAt { msg, line, col } => {
                write!(f, "Parse error at {}:{}: {}", line, col, msg)
            }
            MiniDbError::Type(msg) => write!(f, "Type error: {}", msg),
            MiniDbError::Runtime(msg) => write!(f, "Runtime error: {}", msg),
            MiniDbError::Storage(msg) => write!(f, "Storage error: {}", msg),
            MiniDbError::Sql(msg) => write!(f, "SQL error: {}", msg),
            MiniDbError::Internal(msg) => write!(f, "Internal error: {}", msg),
            MiniDbError::Transaction(msg) => write!(f, "Transaction error: {}", msg),
            MiniDbError::Index(msg) => write!(f, "Index error: {}", msg),
            MiniDbError::Constraint(msg) => write!(f, "Constraint violation: {}", msg),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<std::io::Error> for MiniDbError {
    fn from(e: std::io::Error) -> Self {
        MiniDbError::Io(e.to_string())
    }
}

impl From<csv::Error> for MiniDbError {
    fn from(e: csv::Error) -> Self {
        MiniDbError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        MiniDbError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

pub fn exit_code_for(err: &MiniDbError) -> i32 {
    match err {
        MiniDbError::Sql(_)
        | MiniDbError::Constraint(_)
        | MiniDbError::Type(_)
        | MiniDbError::Runtime(_)
        | MiniDbError::Index(_) => 1,
        MiniDbError::Parse(_) | MiniDbError::ParseAt { .. } => 1,
        MiniDbError::Io(_) | MiniDbError::Storage(_) => 3,
        MiniDbError::Transaction(_) => 1,
        MiniDbError::Internal(_) => 3,
    }
}
