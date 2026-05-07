use std::{fmt, io};

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Cli(String),
    Parse {
        message: String,
        line: usize,
        column: usize,
    },
    Sql(String),
    Type(String),
    Storage(String),
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

impl MiniDbError {
    pub fn parse(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self::Parse {
            message: message.into(),
            line,
            column,
        }
    }
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(e) => write!(f, "storage/io error: {e}"),
            MiniDbError::Cli(s) => write!(f, "usage error: {s}"),
            MiniDbError::Parse {
                message,
                line,
                column,
            } => {
                write!(f, "SQL error at {line}:{column}: {message}")
            }
            MiniDbError::Sql(s) => write!(f, "SQL error: {s}"),
            MiniDbError::Type(s) => write!(f, "type error: {s}"),
            MiniDbError::Storage(s) => write!(f, "storage error: {s}"),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(value: io::Error) -> Self {
        MiniDbError::Io(value)
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(value: serde_json::Error) -> Self {
        MiniDbError::Storage(value.to_string())
    }
}
