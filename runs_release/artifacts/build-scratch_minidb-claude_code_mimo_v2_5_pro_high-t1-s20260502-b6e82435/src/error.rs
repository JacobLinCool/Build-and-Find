use thiserror::Error;

#[derive(Error, Debug)]
pub enum MiniDbError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error at line {line}, col {col}: {message}")]
    Parse {
        line: usize,
        col: usize,
        message: String,
    },

    #[error("syntax error: {0}")]
    Syntax(String),

    #[error("type error: {0}")]
    Type(String),

    #[error("constraint violation: {0}")]
    Constraint(String),

    #[error("catalog error: {0}")]
    Catalog(String),

    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("transaction error: {0}")]
    Transaction(String),

    #[error("WAL error: {0}")]
    Wal(String),

    #[error("index error: {0}")]
    Index(String),

    #[error("{0}")]
    Other(String),
}

impl MiniDbError {
    pub fn exit_code(&self) -> i32 {
        match self {
            MiniDbError::Io(_) | MiniDbError::Storage(_) | MiniDbError::Wal(_) => 3,
            MiniDbError::Parse { .. }
            | MiniDbError::Syntax(_)
            | MiniDbError::Type(_)
            | MiniDbError::Constraint(_)
            | MiniDbError::Catalog(_)
            | MiniDbError::Runtime(_)
            | MiniDbError::Transaction(_)
            | MiniDbError::Index(_) => 1,
            MiniDbError::Other(_) => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;
