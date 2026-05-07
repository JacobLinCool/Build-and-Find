use std::fmt;
use std::io;
use std::num::ParseIntError;

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Parse {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    Type {
        message: String,
    },
    Constraint {
        message: String,
    },
    Catalog {
        message: String,
    },
    Runtime {
        message: String,
    },
    Wal {
        message: String,
    },
    Cli {
        message: String,
    },
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Parse {
                message,
                line,
                column,
            } => {
                if let (Some(l), Some(c)) = (line, column) {
                    write!(f, "Parse error at line {} col {}: {}", l, c, message)
                } else {
                    write!(f, "Parse error: {}", message)
                }
            }
            Self::Type { message } => write!(f, "Type error: {}", message),
            Self::Constraint { message } => write!(f, "Constraint violation: {}", message),
            Self::Catalog { message } => write!(f, "Catalog error: {}", message),
            Self::Runtime { message } => write!(f, "Runtime error: {}", message),
            Self::Wal { message } => write!(f, "WAL error: {}", message),
            Self::Cli { message } => write!(f, "CLI error: {}", message),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for MiniDbError {
    fn from(e: serde_json::Error) -> Self {
        Self::Io(io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }
}

impl From<ParseIntError> for MiniDbError {
    fn from(e: ParseIntError) -> Self {
        Self::Parse {
            message: format!("invalid integer: {}", e),
            line: None,
            column: None,
        }
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_parse_error() {
        let e = MiniDbError::Parse {
            message: "unexpected token".into(),
            line: Some(1),
            column: Some(5),
        };
        assert!(e.to_string().contains("line 1"));
        assert!(e.to_string().contains("col 5"));
    }

    #[test]
    fn test_io_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let e: MiniDbError = io_err.into();
        assert!(e.to_string().contains("IO error"));
    }
}
