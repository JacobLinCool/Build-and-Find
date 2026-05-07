use crate::error::NanowebError;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Other(String),
}

impl Method {
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            other => Method::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Other(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum HttpVersion {
    Http10,
    Http11,
}

impl std::fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpVersion::Http10 => write!(f, "HTTP/1.0"),
            HttpVersion::Http11 => write!(f, "HTTP/1.1"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: HttpVersion,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_addr: String,
    pub request_id: String,
}

impl Request {
    pub fn get_header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_all_headers(&self, name: &str) -> Vec<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .filter(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, NanowebError> {
        let ct = self.get_header("content-type").unwrap_or("").to_lowercase();
        if ct.contains("application/json") {
            serde_json::from_slice(&self.body).map_err(NanowebError::from)
        } else if ct.contains("application/x-www-form-urlencoded") {
            serde_urlencoded::from_bytes(&self.body).map_err(|e| NanowebError::Parse(e.to_string()))
        } else {
            serde_json::from_slice(&self.body).map_err(NanowebError::from)
        }
    }

    pub fn query_params(&self) -> HashMap<String, String> {
        match &self.query {
            None => HashMap::new(),
            Some(q) => serde_urlencoded::from_str(q).unwrap_or_default(),
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
        Response {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Response::new(200)
    }

    pub fn with_body(status: u16, body: impl Into<Vec<u8>>) -> Self {
        let body = body.into();
        let len = body.len();
        let mut r = Response::new(status);
        r.body = body;
        r.set_header("Content-Length", &len.to_string());
        r
    }

    pub fn json<T: serde::Serialize>(value: &T) -> Result<Self, NanowebError> {
        let body = serde_json::to_vec(value).map_err(NanowebError::from)?;
        let len = body.len();
        let mut r = Response::new(200);
        r.body = body;
        r.set_header("Content-Type", "application/json");
        r.set_header("Content-Length", &len.to_string());
        Ok(r)
    }

    pub fn text(body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let len = body.len();
        let mut r = Response::new(200);
        r.body = body;
        r.set_header("Content-Type", "text/plain; charset=utf-8");
        r.set_header("Content-Length", &len.to_string());
        r
    }

    pub fn html(body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let len = body.len();
        let mut r = Response::new(200);
        r.body = body;
        r.set_header("Content-Type", "text/html; charset=utf-8");
        r.set_header("Content-Length", &len.to_string());
        r
    }

    pub fn set_header(&mut self, name: &str, value: &str) -> &mut Self {
        let name_lower = name.to_lowercase();
        if let Some(pos) = self
            .headers
            .iter()
            .position(|(k, _)| k.to_lowercase() == name_lower)
        {
            self.headers[pos].1 = value.to_string();
        } else {
            self.headers.push((name.to_string(), value.to_string()));
        }
        self
    }

    pub fn add_header(&mut self, name: &str, value: &str) -> &mut Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn get_header(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn status_text(&self) -> &str {
        match self.status {
            100 => "Continue",
            101 => "Switching Protocols",
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            206 => "Partial Content",
            301 => "Moved Permanently",
            302 => "Found",
            303 => "See Other",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            409 => "Conflict",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            422 => "Unprocessable Entity",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let status_line = format!("HTTP/1.1 {} {}\r\n", self.status, self.status_text());
        out.extend_from_slice(status_line.as_bytes());
        for (k, v) in &self.headers {
            out.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_from_str() {
        assert_eq!(Method::from_str("GET"), Method::Get);
        assert_eq!(Method::from_str("post"), Method::Post);
        assert_eq!(Method::from_str("PATCH"), Method::Patch);
    }

    #[test]
    fn test_response_set_header() {
        let mut r = Response::ok();
        r.set_header("Content-Type", "text/html");
        r.set_header("Content-Type", "application/json");
        assert_eq!(r.get_header("content-type"), Some("application/json"));
        assert_eq!(r.headers.len(), 1);
    }

    #[test]
    fn test_response_serialize() {
        let r = Response::text("hello");
        let bytes = r.serialize();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.ends_with("hello"));
    }
}
