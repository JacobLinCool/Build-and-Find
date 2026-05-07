use std::fmt;
use std::io;

#[derive(Debug)]
pub enum NanowebError {
    Io(io::Error),
    Http(HttpError),
    Config(String),
    Tls(String),
    Router(String),
    Body(String),
    Session(String),
    Template(String),
    Metrics(String),
}

#[derive(Debug, Clone)]
pub struct HttpError {
    pub status: u16,
    pub message: String,
}

impl HttpError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: msg.into(),
        }
    }
}

impl fmt::Display for NanowebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IO error: {e}"),
            Self::Http(e) => write!(f, "HTTP {} {}", e.status, e.message),
            Self::Config(s) => write!(f, "Config error: {s}"),
            Self::Tls(s) => write!(f, "TLS error: {s}"),
            Self::Router(s) => write!(f, "Router error: {s}"),
            Self::Body(s) => write!(f, "Body error: {s}"),
            Self::Session(s) => write!(f, "Session error: {s}"),
            Self::Template(s) => write!(f, "Template error: {s}"),
            Self::Metrics(s) => write!(f, "Metrics error: {s}"),
        }
    }
}

impl std::error::Error for NanowebError {}

impl From<io::Error> for NanowebError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<HttpError> for NanowebError {
    fn from(e: HttpError) -> Self {
        Self::Http(e)
    }
}

impl HttpError {
    pub fn status_text(code: u16) -> &'static str {
        match code {
            100 => "Continue",
            101 => "Switching Protocols",
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            301 => "Moved Permanently",
            302 => "Found",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }
}
