use std::fmt;

/// Crate-wide error type for nanoweb.
#[derive(Debug)]
pub enum NanowebError {
    /// HTTP parsing error.
    HttpParse(String),
    /// I/O error.
    Io(std::io::Error),
    /// JSON serialization/deserialization error.
    Json(serde_json::Error),
    /// TLS configuration error.
    Tls(String),
    /// Configuration error.
    Config(String),
    /// Template rendering error.
    Template(String),
    /// Session error.
    Session(String),
    /// Route not found.
    NotFound,
    /// Method not allowed.
    MethodNotAllowed,
    /// Internal server error.
    Internal(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::HttpParse(msg) => write!(f, "HTTP parse error: {}", msg),
            NanowebError::Io(err) => write!(f, "I/O error: {}", err),
            NanowebError::Json(err) => write!(f, "JSON error: {}", err),
            NanowebError::Tls(msg) => write!(f, "TLS error: {}", msg),
            NanowebError::Config(msg) => write!(f, "Config error: {}", msg),
            NanowebError::Template(msg) => write!(f, "Template error: {}", msg),
            NanowebError::Session(msg) => write!(f, "Session error: {}", msg),
            NanowebError::NotFound => write!(f, "Not Found"),
            NanowebError::MethodNotAllowed => write!(f, "Method Not Allowed"),
            NanowebError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<std::io::Error> for NanowebError {
    fn from(err: std::io::Error) -> Self {
        NanowebError::Io(err)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(err: serde_json::Error) -> Self {
        NanowebError::Json(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = NanowebError::NotFound;
        assert_eq!(err.to_string(), "Not Found");

        let err = NanowebError::Internal("test".into());
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: NanowebError = io_err.into();
        assert!(matches!(err, NanowebError::Io(_)));
    }
}
