//! Crate-wide error type.

use std::fmt;
use std::io;

/// The single crate-wide error enum. All top-level fallible entry points
/// return `Result<_, NanowebError>`.
#[derive(Debug)]
pub enum NanowebError {
    /// I/O error from the OS.
    Io(io::Error),
    /// Error returned by the HTTP/1.1 parser.
    Parse(String),
    /// Configuration error (missing file, bad value, bad override, etc.).
    Config(String),
    /// Routing failure (no route, malformed pattern).
    Routing(String),
    /// Middleware rejected a request.
    Middleware(String),
    /// Body parsing / decoding error.
    Body(String),
    /// Template compile or render error.
    Template(String),
    /// Session / cookie signing error.
    Session(String),
    /// TLS error (may be produced even with the `tls` feature off when
    /// users pass `--tls-cert` in the CLI).
    Tls(String),
    /// A requested feature was compiled out (e.g. `--tls-cert` without
    /// the `tls` Cargo feature).
    FeatureDisabled(&'static str),
    /// Anything else.
    Other(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "i/o error: {}", e),
            NanowebError::Parse(s) => write!(f, "parse error: {}", s),
            NanowebError::Config(s) => write!(f, "config error: {}", s),
            NanowebError::Routing(s) => write!(f, "routing error: {}", s),
            NanowebError::Middleware(s) => write!(f, "middleware error: {}", s),
            NanowebError::Body(s) => write!(f, "body error: {}", s),
            NanowebError::Template(s) => write!(f, "template error: {}", s),
            NanowebError::Session(s) => write!(f, "session error: {}", s),
            NanowebError::Tls(s) => write!(f, "tls error: {}", s),
            NanowebError::FeatureDisabled(name) => {
                write!(f, "feature '{}' is not compiled in this build", name)
            }
            NanowebError::Other(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for NanowebError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NanowebError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for NanowebError {
    fn from(e: io::Error) -> Self {
        NanowebError::Io(e)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(e: serde_json::Error) -> Self {
        NanowebError::Body(format!("json: {}", e))
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(format!("toml: {}", e))
    }
}

/// Convenience alias for `Result<T, NanowebError>`.
pub type Result<T> = std::result::Result<T, NanowebError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_contains_kind() {
        let e = NanowebError::Parse("bad line".to_string());
        assert!(format!("{}", e).contains("parse error"));
    }

    #[test]
    fn feature_disabled_message() {
        let e = NanowebError::FeatureDisabled("tls");
        assert!(format!("{}", e).contains("tls"));
    }

    #[test]
    fn from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "missing");
        let ne: NanowebError = io_err.into();
        matches!(ne, NanowebError::Io(_));
    }
}
