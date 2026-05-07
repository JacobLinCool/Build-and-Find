use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MiniDbError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("parse error at {line}:{column}: {message}")]
    Parse {
        line: usize,
        column: usize,
        message: String,
    },
    #[error("sql error: {0}")]
    Sql(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("type error: {0}")]
    Type(String),
    #[error("cli error: {0}")]
    Cli(String),
    #[error("serialization error: {0}")]
    Serde(String),
}

impl From<serde_json::Error> for MiniDbError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;
