use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl Method {
    pub fn from_bytes(s: &[u8]) -> Option<Self> {
        match s {
            b"GET" => Some(Self::Get),
            b"POST" => Some(Self::Post),
            b"PUT" => Some(Self::Put),
            b"DELETE" => Some(Self::Delete),
            b"PATCH" => Some(Self::Patch),
            b"HEAD" => Some(Self::Head),
            b"OPTIONS" => Some(Self::Options),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub remote_addr: Option<std::net::SocketAddr>,
    pub request_id: String,
    raw_body_consumed: bool,
}

impl Request {
    pub fn new(method: Method, path: String, version: String) -> Self {
        Self {
            method,
            path,
            version,
            headers: HashMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            query: HashMap::new(),
            remote_addr: None,
            request_id: String::new(),
            raw_body_consumed: false,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_body(&mut self, body: Vec<u8>) {
        self.body = body;
        self.raw_body_consumed = false;
    }

    pub fn take_body(&mut self) -> Vec<u8> {
        self.raw_body_consumed = true;
        std::mem::take(&mut self.body)
    }

    pub fn body_ref(&self) -> &[u8] {
        &self.body
    }

    pub fn body_as<T: serde::de::DeserializeOwned>(
        &mut self,
    ) -> Result<T, crate::error::NanowebError> {
        let content_type = self.header("content-type").unwrap_or("");
        if content_type.contains("application/json") {
            let body = self.take_body();
            serde_json::from_slice(&body)
                .map_err(|e| crate::error::NanowebError::Body(format!("JSON decode: {e}")))
        } else if content_type.contains("application/x-www-form-urlencoded") {
            let body = self.take_body();
            serde_urlencoded::from_bytes(&body)
                .map_err(|e| crate::error::NanowebError::Body(format!("Form decode: {e}")))
        } else {
            Err(crate::error::NanowebError::Body(format!(
                "Unsupported content type: {content_type}"
            )))
        }
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(200)
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_json<T: serde::Serialize>(self, value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(bytes) => self
                .with_header("content-type", "application/json")
                .with_body(bytes),
            Err(_) => Self::new(500).with_body("Internal Server Error"),
        }
    }

    pub fn with_html(self, html: impl Into<Vec<u8>>) -> Self {
        self.with_header("content-type", "text/html; charset=utf-8")
            .with_body(html)
    }

    pub fn set_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.headers
            .retain(|(k, _)| k.to_ascii_lowercase() != name.to_ascii_lowercase());
        self.headers.push((name, value.into()));
    }

    pub fn status_text(&self) -> &'static str {
        match self.status {
            100 => "Continue",
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
            408 => "Request Timeout",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCodeClass {
    Informational,
    Success,
    Redirection,
    ClientError,
    ServerError,
}

impl StatusCodeClass {
    pub fn from_status(status: u16) -> Self {
        match status {
            100..=199 => Self::Informational,
            200..=299 => Self::Success,
            300..=399 => Self::Redirection,
            400..=499 => Self::ClientError,
            _ => Self::ServerError,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Informational => "1xx",
            Self::Success => "2xx",
            Self::Redirection => "3xx",
            Self::ClientError => "4xx",
            Self::ServerError => "5xx",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_from_bytes() {
        assert_eq!(Method::from_bytes(b"GET"), Some(Method::Get));
        assert_eq!(Method::from_bytes(b"POST"), Some(Method::Post));
        assert_eq!(Method::from_bytes(b"INVALID"), None);
    }

    #[test]
    fn test_status_code_class() {
        assert_eq!(StatusCodeClass::from_status(200), StatusCodeClass::Success);
        assert_eq!(
            StatusCodeClass::from_status(404),
            StatusCodeClass::ClientError
        );
        assert_eq!(
            StatusCodeClass::from_status(500),
            StatusCodeClass::ServerError
        );
    }

    #[test]
    fn test_response_builder() {
        let resp = Response::ok()
            .with_header("x-test", "value")
            .with_body(b"hello");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
        assert_eq!(
            resp.headers.iter().find(|(k, _)| k == "x-test").unwrap().1,
            "value"
        );
    }
}
