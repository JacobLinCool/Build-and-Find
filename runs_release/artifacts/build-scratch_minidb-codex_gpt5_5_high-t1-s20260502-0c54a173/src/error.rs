use thiserror::Error;

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[derive(Debug, Error)]
pub enum MiniDbError {
    #[error("CLI error: {0}")]
    Cli(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error at {line}:{column}: {message}")]
    Parse {
        message: String,
        line: usize,
        column: usize,
    },
    #[error("SQL error{location}: {message}")]
    Sql {
        message: String,
        location: SourceLocation,
    },
    #[error("type error: {0}")]
    Type(String),
    #[error("storage error: {0}")]
    Storage(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceLocation {
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(f, " at {line}:{column}"),
            _ => Ok(()),
        }
    }
}

impl MiniDbError {
    pub fn sql(message: impl Into<String>) -> Self {
        Self::Sql {
            message: message.into(),
            location: SourceLocation::default(),
        }
    }

    pub fn parse(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self::Parse {
            message: message.into(),
            line,
            column,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            MiniDbError::Cli(_) => 2,
            MiniDbError::Io(_) | MiniDbError::Storage(_) => 3,
            MiniDbError::Parse { .. } | MiniDbError::Sql { .. } | MiniDbError::Type(_) => 1,
        }
    }
}
