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
    #[error("type error: {0}")]
    Type(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("transaction error: {0}")]
    Txn(String),
    #[error("cli error: {0}")]
    Cli(String),
}

impl MiniDbError {
    pub fn sql(message: impl Into<String>) -> Self {
        Self::Sql(message.into())
    }
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage(message.into())
    }
    pub fn txn(message: impl Into<String>) -> Self {
        Self::Txn(message.into())
    }
    pub fn cli(message: impl Into<String>) -> Self {
        Self::Cli(message.into())
    }
}
