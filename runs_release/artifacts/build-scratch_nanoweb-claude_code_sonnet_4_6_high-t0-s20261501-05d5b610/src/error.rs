use std::fmt;

#[derive(Debug)]
pub enum NanowebError {
    Io(std::io::Error),
    Parse(String),
    BodyTooLarge,
    InvalidContentType(String),
    Json(serde_json::Error),
    Template(String),
    Session(String),
    Config(String),
    Tls(String),
    NotFound,
    MethodNotAllowed(Vec<String>),
    BadRequest(String),
    Internal(String),
    Forbidden(String),
    Unauthorized(String),
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Parse(s) => write!(f, "Parse error: {}", s),
            Self::BodyTooLarge => write!(f, "Request body too large"),
            Self::InvalidContentType(s) => write!(f, "Invalid content type: {}", s),
            Self::Json(e) => write!(f, "JSON error: {}", e),
            Self::Template(s) => write!(f, "Template error: {}", s),
            Self::Session(s) => write!(f, "Session error: {}", s),
            Self::Config(s) => write!(f, "Config error: {}", s),
            Self::Tls(s) => write!(f, "TLS error: {}", s),
            Self::NotFound => write!(f, "Not found"),
            Self::MethodNotAllowed(methods) => {
                write!(f, "Method not allowed. Allowed: {}", methods.join(", "))
            }
            Self::BadRequest(s) => write!(f, "Bad request: {}", s),
            Self::Internal(s) => write!(f, "Internal error: {}", s),
            Self::Forbidden(s) => write!(f, "Forbidden: {}", s),
            Self::Unauthorized(s) => write!(f, "Unauthorized: {}", s),
        }
    }
}

impl std::error::Error for NanowebError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for NanowebError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for NanowebError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl NanowebError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::NotFound => 404,
            Self::MethodNotAllowed(_) => 405,
            Self::BadRequest(_) | Self::Parse(_) | Self::InvalidContentType(_) => 400,
            Self::Forbidden(_) => 403,
            Self::Unauthorized(_) => 401,
            Self::BodyTooLarge => 413,
            _ => 500,
        }
    }

    pub fn to_response_body(&self) -> String {
        format!("{{\"error\":\"{}\"}}", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_codes() {
        assert_eq!(NanowebError::NotFound.status_code(), 404);
        assert_eq!(NanowebError::BadRequest("x".into()).status_code(), 400);
        assert_eq!(NanowebError::Internal("x".into()).status_code(), 500);
        assert_eq!(NanowebError::Unauthorized("x".into()).status_code(), 401);
        assert_eq!(NanowebError::Forbidden("x".into()).status_code(), 403);
    }

    #[test]
    fn test_display() {
        let e = NanowebError::NotFound;
        assert_eq!(format!("{}", e), "Not found");
    }
}
