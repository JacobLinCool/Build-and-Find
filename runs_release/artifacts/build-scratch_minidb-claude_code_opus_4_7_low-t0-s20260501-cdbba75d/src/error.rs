use std::fmt;
use std::io;

#[derive(Debug)]
pub enum MiniDbError {
    Io(io::Error),
    Lex {
        line: usize,
        col: usize,
        msg: String,
    },
    Parse {
        line: usize,
        col: usize,
        msg: String,
    },
    Type(String),
    Runtime(String),
    Catalog(String),
    Storage(String),
    Constraint(String),
    Cli(String),
}

impl fmt::Display for MiniDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MiniDbError::Io(e) => write!(f, "io error: {}", e),
            MiniDbError::Lex { line, col, msg } => {
                write!(f, "lex error at line {} col {}: {}", line, col, msg)
            }
            MiniDbError::Parse { line, col, msg } => {
                write!(f, "parse error at line {} col {}: {}", line, col, msg)
            }
            MiniDbError::Type(m) => write!(f, "type error: {}", m),
            MiniDbError::Runtime(m) => write!(f, "runtime error: {}", m),
            MiniDbError::Catalog(m) => write!(f, "catalog error: {}", m),
            MiniDbError::Storage(m) => write!(f, "storage error: {}", m),
            MiniDbError::Constraint(m) => write!(f, "constraint error: {}", m),
            MiniDbError::Cli(m) => write!(f, "cli error: {}", m),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(e: io::Error) -> Self {
        MiniDbError::Io(e)
    }
}

impl From<bincode::Error> for MiniDbError {
    fn from(e: bincode::Error) -> Self {
        MiniDbError::Storage(format!("bincode: {}", e))
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

/// Classify error for CLI exit code.
pub fn exit_code_for(err: &MiniDbError) -> i32 {
    match err {
        MiniDbError::Cli(_) => 2,
        MiniDbError::Io(_) | MiniDbError::Storage(_) => 3,
        _ => 1,
    }
}
