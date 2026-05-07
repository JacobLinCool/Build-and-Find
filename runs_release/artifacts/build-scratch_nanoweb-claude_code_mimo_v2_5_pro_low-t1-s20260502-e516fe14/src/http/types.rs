use std::collections::HashMap;
use std::fmt;

/// HTTP methods supported by nanoweb.
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
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "GET" => Some(Method::Get),
            "POST" => Some(Method::Post),
            "PUT" => Some(Method::Put),
            "DELETE" => Some(Method::Delete),
            "PATCH" => Some(Method::Patch),
            "HEAD" => Some(Method::Head),
            "OPTIONS" => Some(Method::Options),
            _ => None,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// HTTP status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK: StatusCode = StatusCode(200);
    pub const CREATED: StatusCode = StatusCode(201);
    pub const NO_CONTENT: StatusCode = StatusCode(204);
    pub const MOVED_PERMANENTLY: StatusCode = StatusCode(301);
    pub const NOT_MODIFIED: StatusCode = StatusCode(304);
    pub const BAD_REQUEST: StatusCode = StatusCode(400);
    pub const UNAUTHORIZED: StatusCode = StatusCode(401);
    pub const FORBIDDEN: StatusCode = StatusCode(403);
    pub const NOT_FOUND: StatusCode = StatusCode(404);
    pub const METHOD_NOT_ALLOWED: StatusCode = StatusCode(405);
    pub const TOO_MANY_REQUESTS: StatusCode = StatusCode(429);
    pub const INTERNAL_SERVER_ERROR: StatusCode = StatusCode(500);

    pub fn reason(&self) -> &'static str {
        match self.0 {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            301 => "Moved Permanently",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }

    /// Returns the status class for metrics: 1xx, 2xx, 3xx, 4xx, 5xx.
    pub fn class(&self) -> &'static str {
        match self.0 / 100 {
            1 => "1xx",
            2 => "2xx",
            3 => "3xx",
            4 => "4xx",
            5 => "5xx",
            _ => "other",
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A case-insensitive header map.
#[derive(Debug, Clone, Default)]
pub struct HeaderMap {
    headers: Vec<(String, String)>,
}

impl HeaderMap {
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
        }
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        // Remove existing
        self.headers
            .retain(|(k, _)| k.to_lowercase() != name.to_lowercase());
        self.headers.push((name, value.into()));
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.headers.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.headers.iter()
    }

    pub fn len(&self) -> usize {
        self.headers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}

/// An HTTP request.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub version: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub query: HashMap<String, String>,
    pub params: HashMap<String, String>,
    pub request_id: String,
    /// Session data set by session middleware.
    pub session_data: HashMap<String, String>,
}

impl Request {
    /// Get a typed body using serde deserialization.
    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        let content_type = self.headers.get("content-type").unwrap_or("");
        if content_type.contains("application/json")
            || self.body.starts_with_with_check(b"{")
            || self.body.starts_with_with_check(b"[")
        {
            serde_json::from_slice(&self.body)
        } else {
            // Try JSON by default
            serde_json::from_slice(&self.body)
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }
}

use serde::de::DeserializeOwned;

trait StartsExt {
    fn starts_with_with_check(&self, prefix: &[u8]) -> bool;
}

impl StartsExt for Vec<u8> {
    fn starts_with_with_check(&self, prefix: &[u8]) -> bool {
        self.starts_with(prefix)
    }
}

/// An HTTP response.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            headers: HeaderMap::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(StatusCode::OK)
    }

    pub fn text(status: StatusCode, text: impl Into<String>) -> Self {
        let body = text.into();
        let mut resp = Self::new(status);
        resp.headers
            .insert("content-type", "text/plain; charset=utf-8");
        resp.body = body.into_bytes();
        resp
    }

    pub fn html(status: StatusCode, html: impl Into<String>) -> Self {
        let body = html.into();
        let mut resp = Self::new(status);
        resp.headers
            .insert("content-type", "text/html; charset=utf-8");
        resp.body = body.into_bytes();
        resp
    }

    pub fn json(status: StatusCode, value: &serde_json::Value) -> Self {
        let mut resp = Self::new(status);
        resp.headers.insert("content-type", "application/json");
        resp.body = serde_json::to_vec(value).unwrap_or_default();
        resp
    }

    pub fn json_string(status: StatusCode, json_str: &str) -> Self {
        let mut resp = Self::new(status);
        resp.headers.insert("content-type", "application/json");
        resp.body = json_str.as_bytes().to_vec();
        resp
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn set_body(&mut self, body: Vec<u8>) {
        self.body = body;
    }

    pub fn body_string(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// Serialize this response into bytes for sending over the wire.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        // Status line
        let status_line = format!("HTTP/1.1 {} {}\r\n", self.status.0, self.status.reason());
        out.extend_from_slice(status_line.as_bytes());
        // Headers
        for (name, value) in self.headers.iter() {
            let header_line = format!("{}: {}\r\n", name, value);
            out.extend_from_slice(header_line.as_bytes());
        }
        // Content-Length if not already set and not chunked
        if !self.headers.contains("transfer-encoding") {
            let cl = format!("content-length: {}\r\n", self.body.len());
            out.extend_from_slice(cl.as_bytes());
        }
        // End of headers
        out.extend_from_slice(b"\r\n");
        // Body
        out.extend_from_slice(&self.body);
        out
    }
}

/// URL-decode a percent-encoded string.
pub fn url_decode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                output.push(byte as char);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            output.push(' ');
        } else {
            output.push(bytes[i] as char);
        }
        i += 1;
    }
    output
}

/// Parse a query string into key-value pairs.
pub fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = url_decode(parts.next().unwrap_or(""));
        let value = url_decode(parts.next().unwrap_or(""));
        params.insert(key, value);
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_from_str() {
        assert_eq!(Method::from_str("GET"), Some(Method::Get));
        assert_eq!(Method::from_str("POST"), Some(Method::Post));
        assert_eq!(Method::from_str("INVALID"), None);
    }

    #[test]
    fn test_status_code_class() {
        assert_eq!(StatusCode(200).class(), "2xx");
        assert_eq!(StatusCode(404).class(), "4xx");
        assert_eq!(StatusCode(500).class(), "5xx");
    }

    #[test]
    fn test_header_map_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "text/plain");
        assert_eq!(headers.get("content-type"), Some("text/plain"));
        assert_eq!(headers.get("CONTENT-TYPE"), Some("text/plain"));
    }

    #[test]
    fn test_response_to_bytes() {
        let resp = Response::text(StatusCode::OK, "hello");
        let bytes = resp.to_bytes();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("content-type: text/plain"));
        assert!(s.ends_with("hello"));
    }

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("a+b"), "a b");
        assert_eq!(url_decode("no%2Bencoding"), "no+encoding");
    }

    #[test]
    fn test_parse_query_string() {
        let params = parse_query_string("foo=bar&baz=qux%20ux");
        assert_eq!(params.get("foo").unwrap(), "bar");
        assert_eq!(params.get("baz").unwrap(), "qux ux");
    }
}
