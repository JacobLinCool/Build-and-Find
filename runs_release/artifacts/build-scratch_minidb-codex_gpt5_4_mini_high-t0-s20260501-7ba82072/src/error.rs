use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MiniDbError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("lex error at line {line}, column {column}: {message}")]
    Lex {
        line: usize,
        column: usize,
        message: String,
    },
    #[error("parse error at line {line}, column {column}: {message}")]
    Parse {
        line: usize,
        column: usize,
        message: String,
    },
    #[error("type error: {0}")]
    Type(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("cli usage error: {0}")]
    Cli(String),
    #[error("corrupt database: {0}")]
    Corrupt(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
}

impl MiniDbError {
    pub fn exit_code(&self) -> i32 {
        match self {
            MiniDbError::Cli(_) => 2,
            MiniDbError::Io(_) | MiniDbError::Storage(_) | MiniDbError::Corrupt(_) => 3,
            MiniDbError::Lex { .. }
            | MiniDbError::Parse { .. }
            | MiniDbError::Type(_)
            | MiniDbError::Runtime(_)
            | MiniDbError::Unsupported(_) => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

pub trait WithSpan<T> {
    fn with_span(self, line: usize, column: usize, message: impl Into<String>) -> Result<T>;
}

impl<T> WithSpan<T> for Option<T> {
    fn with_span(self, line: usize, column: usize, message: impl Into<String>) -> Result<T> {
        match self {
            Some(value) => Ok(value),
            None => Err(MiniDbError::Parse {
                line,
                column,
                message: message.into(),
            }),
        }
    }
}
