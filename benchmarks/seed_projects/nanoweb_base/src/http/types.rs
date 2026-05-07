//! Core HTTP data types.

use std::collections::HashMap;
use std::fmt;

/// The header name under which the per-request id is mirrored back on the
/// response.
pub const X_REQUEST_ID_HEADER: &str = "x-request-id";

/// HTTP methods recognised by the framework.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Options,
    Patch,
    Trace,
    Connect,
}

impl Method {
    pub fn as_str(&self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Head => "HEAD",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Options => "OPTIONS",
            Method::Patch => "PATCH",
            Method::Trace => "TRACE",
            Method::Connect => "CONNECT",
        }
    }

    pub fn parse(raw: &str) -> Option<Method> {
        Some(match raw {
            "GET" => Method::Get,
            "HEAD" => Method::Head,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "OPTIONS" => Method::Options,
            "PATCH" => Method::Patch,
            "TRACE" => Method::Trace,
            "CONNECT" => Method::Connect,
            _ => return None,
        })
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// HTTP protocol version supported by this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

impl Version {
    pub fn as_str(&self) -> &'static str {
        match self {
            Version::Http10 => "HTTP/1.0",
            Version::Http11 => "HTTP/1.1",
        }
    }

    pub fn parse(raw: &str) -> Option<Version> {
        Some(match raw {
            "HTTP/1.0" => Version::Http10,
            "HTTP/1.1" => Version::Http11,
            _ => return None,
        })
    }
}

/// Case-insensitive header map that keeps the last-written value for a
/// given name. Iteration order follows insertion order.
#[derive(Debug, Clone, Default)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up a header value by case-insensitive name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Insert or replace a header.
    pub fn set<K: Into<String>, V: Into<String>>(&mut self, name: K, value: V) {
        let name = name.into();
        let value = value.into();
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|(k, _)| k.eq_ignore_ascii_case(&name))
        {
            slot.1 = value;
        } else {
            self.entries.push((name, value));
        }
    }

    /// Append without clobbering existing entries (multi-value header).
    pub fn append<K: Into<String>, V: Into<String>>(&mut self, name: K, value: V) {
        self.entries.push((name.into(), value.into()));
    }

    /// Remove every entry whose name matches case-insensitively.
    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// Per-request correlation id, attached to logs and mirrored back as
/// the `X-Request-Id` response header.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

impl RequestId {
    /// Create a reasonably-unique id without depending on `rand`.
    pub fn generate() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        RequestId(format!("{:016x}{:08x}", ts, n as u32))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A parsed HTTP request.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub version: Version,
    pub target: String,
    pub path: String,
    pub query: String,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub remote_addr: Option<String>,
    pub id: RequestId,
    pub path_params: HashMap<String, String>,
    pub route_pattern: Option<String>,
}

impl Request {
    pub fn new(method: Method, target: impl Into<String>) -> Self {
        let target = target.into();
        let (path, query) = match target.find('?') {
            Some(i) => (target[..i].to_string(), target[i + 1..].to_string()),
            None => (target.clone(), String::new()),
        };
        Request {
            method,
            version: Version::Http11,
            target,
            path,
            query,
            headers: Headers::new(),
            body: Vec::new(),
            remote_addr: None,
            id: RequestId::generate(),
            path_params: HashMap::new(),
            route_pattern: None,
        }
    }

    pub fn content_length(&self) -> Option<usize> {
        self.headers
            .get("content-length")
            .and_then(|v| v.trim().parse::<usize>().ok())
    }

    pub fn is_chunked(&self) -> bool {
        self.headers
            .get("transfer-encoding")
            .map(|v| v.to_ascii_lowercase().contains("chunked"))
            .unwrap_or(false)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.path_params.get(name).map(|s| s.as_str())
    }
}

/// Numeric status code with a canonical reason phrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK: StatusCode = StatusCode(200);
    pub const NO_CONTENT: StatusCode = StatusCode(204);
    pub const NOT_MODIFIED: StatusCode = StatusCode(304);
    pub const BAD_REQUEST: StatusCode = StatusCode(400);
    pub const UNAUTHORIZED: StatusCode = StatusCode(401);
    pub const FORBIDDEN: StatusCode = StatusCode(403);
    pub const NOT_FOUND: StatusCode = StatusCode(404);
    pub const METHOD_NOT_ALLOWED: StatusCode = StatusCode(405);
    pub const PAYLOAD_TOO_LARGE: StatusCode = StatusCode(413);
    pub const TOO_MANY_REQUESTS: StatusCode = StatusCode(429);
    pub const INTERNAL_SERVER_ERROR: StatusCode = StatusCode(500);
    pub const SERVICE_UNAVAILABLE: StatusCode = StatusCode(503);

    pub fn reason(&self) -> &'static str {
        match self.0 {
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
            411 => "Length Required",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Status",
        }
    }
}

/// A fully-buffered HTTP response.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode) -> Self {
        Response {
            status,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(StatusCode::OK)
    }

    pub fn text(body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut r = Response::new(StatusCode::OK);
        r.headers
            .set("content-type", "text/plain; charset=utf-8");
        r.headers.set("content-length", body.len().to_string());
        r.body = body;
        r
    }

    pub fn json_bytes(body: Vec<u8>) -> Self {
        let mut r = Response::new(StatusCode::OK);
        r.headers.set("content-type", "application/json");
        r.headers.set("content-length", body.len().to_string());
        r.body = body;
        r
    }

    pub fn html(body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut r = Response::new(StatusCode::OK);
        r.headers.set("content-type", "text/html; charset=utf-8");
        r.headers.set("content-length", body.len().to_string());
        r.body = body;
        r
    }

    pub fn status(status: StatusCode) -> Self {
        let mut r = Response::new(status);
        r.headers.set("content-length", "0");
        r
    }

    /// Serialise the response to a byte vector following the HTTP/1.1
    /// message format.
    pub fn to_bytes(&self, version: Version) -> Vec<u8> {
        let mut out = Vec::with_capacity(128 + self.body.len());
        let start_line = format!(
            "{} {} {}\r\n",
            version.as_str(),
            self.status.0,
            self.status.reason()
        );
        out.extend_from_slice(start_line.as_bytes());
        for (k, v) in self.headers.iter() {
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
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
    fn method_roundtrip() {
        assert_eq!(Method::parse("GET"), Some(Method::Get));
        assert_eq!(Method::Post.as_str(), "POST");
        assert!(Method::parse("BOGUS").is_none());
    }

    #[test]
    fn headers_case_insensitive() {
        let mut h = Headers::new();
        h.set("Content-Length", "5");
        assert_eq!(h.get("content-length"), Some("5"));
        h.set("content-length", "7");
        assert_eq!(h.get("CONTENT-LENGTH"), Some("7"));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn response_bytes_have_status_line() {
        let r = Response::text("hi");
        let bytes = r.to_bytes(Version::Http11);
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("content-length: 2"));
        assert!(s.ends_with("hi"));
    }

    #[test]
    fn request_parses_query_from_target() {
        let r = Request::new(Method::Get, "/foo?x=1");
        assert_eq!(r.path, "/foo");
        assert_eq!(r.query, "x=1");
    }
}
