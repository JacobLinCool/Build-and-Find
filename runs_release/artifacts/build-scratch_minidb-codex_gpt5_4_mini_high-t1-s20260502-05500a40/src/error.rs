use std::fmt::{self, Display, Formatter};
use std::io;

#[derive(Debug)]
pub enum MiniDbError {
    Sql {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    Parse {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    Type {
        message: String,
    },
    Runtime {
        message: String,
    },
    Storage {
        message: String,
    },
    Io(io::Error),
    Usage {
        message: String,
    },
}

impl MiniDbError {
    pub fn sql(message: impl Into<String>, line: Option<usize>, column: Option<usize>) -> Self {
        Self::Sql {
            message: message.into(),
            line,
            column,
        }
    }

    pub fn parse(message: impl Into<String>, line: Option<usize>, column: Option<usize>) -> Self {
        Self::Parse {
            message: message.into(),
            line,
            column,
        }
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        Self::Type {
            message: message.into(),
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime {
            message: message.into(),
        }
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage {
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Sql { .. } | Self::Parse { .. } | Self::Type { .. } | Self::Runtime { .. } => 1,
            Self::Usage { .. } => 2,
            Self::Storage { .. } | Self::Io(_) => 3,
        }
    }
}

impl Display for MiniDbError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql {
                message,
                line,
                column,
            }
            | Self::Parse {
                message,
                line,
                column,
            } => {
                if let (Some(line), Some(column)) = (line, column) {
                    write!(f, "{message} at line {line}, column {column}")
                } else {
                    write!(f, "{message}")
                }
            }
            Self::Type { message }
            | Self::Runtime { message }
            | Self::Storage { message }
            | Self::Usage { message } => {
                write!(f, "{message}")
            }
            Self::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for MiniDbError {}

impl From<io::Error> for MiniDbError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub type Result<T> = std::result::Result<T, MiniDbError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_stable() {
        assert_eq!(MiniDbError::usage("x").exit_code(), 2);
        assert_eq!(MiniDbError::storage("x").exit_code(), 3);
        assert_eq!(MiniDbError::runtime("x").exit_code(), 1);
    }
}
