use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Parse(String),
    NotFound,
    MethodNotAllowed(Vec<String>),
    BadRequest(String),
    Unauthorized,
    Forbidden,
    TooManyRequests,
    Internal(String),
    ConnectionClosed,
    Config(String),
    #[cfg(feature = "tls")]
    Tls(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NanowebError::Io(e) => write!(f, "IO error: {}", e),
            NanowebError::Parse(s) => write!(f, "Parse error: {}", s),
            NanowebError::NotFound => write!(f, "Not found"),
            NanowebError::MethodNotAllowed(methods) => {
                write!(f, "Method not allowed. Allowed: {}", methods.join(", "))
            }
            NanowebError::BadRequest(s) => write!(f, "Bad request: {}", s),
            NanowebError::Unauthorized => write!(f, "Unauthorized"),
            NanowebError::Forbidden => write!(f, "Forbidden"),
            NanowebError::TooManyRequests => write!(f, "Too many requests"),
            NanowebError::Internal(s) => write!(f, "Internal error: {}", s),
            NanowebError::ConnectionClosed => write!(f, "Connection closed"),
            NanowebError::Config(s) => write!(f, "Config error: {}", s),
            #[cfg(feature = "tls")]
            NanowebError::Tls(s) => write!(f, "TLS error: {}", s),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<std::io::Error> for NanowebError {
    fn from(e: std::io::Error) -> Self {
        NanowebError::Io(e)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(e: serde_json::Error) -> Self {
        NanowebError::Parse(e.to_string())
    }
}

impl From<toml::de::Error> for NanowebError {
    fn from(e: toml::de::Error) -> Self {
        NanowebError::Config(e.to_string())
    }
}

impl NanowebError {
    pub fn status_code(&self) -> u16 {
        match self {
            NanowebError::NotFound => 404,
            NanowebError::MethodNotAllowed(_) => 405,
            NanowebError::BadRequest(_) | NanowebError::Parse(_) => 400,
            NanowebError::Unauthorized => 401,
            NanowebError::Forbidden => 403,
            NanowebError::TooManyRequests => 429,
            _ => 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_status_codes() {
        assert_eq!(NanowebError::NotFound.status_code(), 404);
        assert_eq!(NanowebError::Unauthorized.status_code(), 401);
        assert_eq!(NanowebError::Forbidden.status_code(), 403);
        assert_eq!(NanowebError::TooManyRequests.status_code(), 429);
        assert_eq!(NanowebError::Internal("x".into()).status_code(), 500);
    }

    #[test]
    fn test_error_display() {
        let e = NanowebError::Parse("bad data".into());
        assert!(e.to_string().contains("bad data"));
    }
}
