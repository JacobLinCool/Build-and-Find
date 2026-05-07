use thiserror::Error;

#[derive(Debug, Error)]
pub enum MiniDbError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error at line {line}, col {col}: {msg}")]
    Parse {
        msg: String,
        line: usize,
        col: usize,
    },

    #[error("Lex error at line {line}, col {col}: {msg}")]
    Lex {
        msg: String,
        line: usize,
        col: usize,
    },

    #[error("Type error: {0}")]
    Type(String),

    #[error("Constraint violation: {0}")]
    Constraint(String),

    #[error("Runtime error: {0}")]
    Runtime(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Transaction error: {0}")]
    Transaction(String),

    #[error("Catalog error: {0}")]
    Catalog(String),

    #[error("Table '{0}' not found")]
    TableNotFound(String),

    #[error("Index '{0}' not found")]
    IndexNotFound(String),

    #[error("Column '{0}' not found")]
    ColumnNotFound(String),

    #[error("Table '{0}' already exists")]
    TableExists(String),

    #[error("Index '{0}' already exists")]
    IndexExists(String),

    #[error("WAL error: {0}")]
    Wal(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

impl MiniDbError {
    pub fn exit_code(&self) -> i32 {
        match self {
            MiniDbError::Io(_) | MiniDbError::Storage(_) | MiniDbError::Wal(_) => 3,
            _ => 1,
        }
    }
}
