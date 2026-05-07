use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

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
    pub fn as_str(self) -> &'static str {
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
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Method {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            other => return Err(format!("unknown method: {}", other)),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

impl Version {
    pub fn as_str(self) -> &'static str {
        match self {
            Version::Http10 => "HTTP/1.0",
            Version::Http11 => "HTTP/1.1",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HeaderMap {
    inner: Vec<(String, String)>,
}

impl HeaderMap {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        let lc = name.to_ascii_lowercase();
        self.inner.retain(|(n, _)| n.to_ascii_lowercase() != lc);
        self.inner.push((name, value));
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.inner.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        let lc = name.to_ascii_lowercase();
        self.inner
            .iter()
            .find(|(n, _)| n.to_ascii_lowercase() == lc)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        let lc = name.to_ascii_lowercase();
        self.inner
            .iter()
            .filter(|(n, _)| n.to_ascii_lowercase() == lc)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn remove(&mut self, name: &str) {
        let lc = name.to_ascii_lowercase();
        self.inner.retain(|(n, _)| n.to_ascii_lowercase() != lc);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.inner.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK: StatusCode = StatusCode(200);
    pub const CREATED: StatusCode = StatusCode(201);
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

    pub fn as_u16(self) -> u16 {
        self.0
    }

    pub fn class(self) -> &'static str {
        match self.0 / 100 {
            1 => "1xx",
            2 => "2xx",
            3 => "3xx",
            4 => "4xx",
            5 => "5xx",
            _ => "unknown",
        }
    }

    pub fn reason(self) -> &'static str {
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
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            503 => "Service Unavailable",
            _ => "OK",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: String,
    pub version: Version,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_addr: Option<std::net::SocketAddr>,
    pub request_id: String,
    pub extensions: Extensions,
}

impl Request {
    pub fn new(method: Method, target: String, version: Version) -> Self {
        let (path, query) = match target.split_once('?') {
            Some((p, q)) => (p.to_string(), q.to_string()),
            None => (target.clone(), String::new()),
        };
        Self {
            method,
            target,
            path,
            query,
            version,
            headers: HeaderMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            remote_addr: None,
            request_id: String::new(),
            extensions: Extensions::default(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn query_pairs(&self) -> Vec<(String, String)> {
        if self.query.is_empty() {
            return Vec::new();
        }
        form_urlencoded::parse(self.query.as_bytes())
            .into_owned()
            .collect()
    }

    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> crate::error::Result<T> {
        crate::body::parse_body(self)
    }
}

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

    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn text(body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut r = Self::new(StatusCode::OK);
        r.headers
            .insert("Content-Type", "text/plain; charset=utf-8");
        r.body = body;
        r
    }

    pub fn html(body: impl Into<String>) -> Self {
        let body = body.into().into_bytes();
        let mut r = Self::new(StatusCode::OK);
        r.headers.insert("Content-Type", "text/html; charset=utf-8");
        r.body = body;
        r
    }

    pub fn json<T: serde::Serialize>(value: &T) -> crate::error::Result<Self> {
        let body = serde_json::to_vec(value)?;
        let mut r = Self::new(StatusCode::OK);
        r.headers.insert("Content-Type", "application/json");
        r.body = body;
        Ok(r)
    }

    pub fn empty(status: StatusCode) -> Self {
        Self::new(status)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Extensions {
    map: HashMap<String, String>,
}

impl Extensions {
    pub fn insert(&mut self, k: impl Into<String>, v: impl Into<String>) {
        self.map.insert(k.into(), v.into());
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        self.map.get(k).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_case_insensitive() {
        let mut h = HeaderMap::new();
        h.insert("Content-Type", "text/plain");
        assert_eq!(h.get("content-type"), Some("text/plain"));
        assert_eq!(h.get("CONTENT-TYPE"), Some("text/plain"));
    }

    #[test]
    fn method_parse() {
        assert_eq!("GET".parse::<Method>().unwrap(), Method::Get);
        assert!("foo".parse::<Method>().is_err());
    }

    #[test]
    fn request_target_split() {
        let r = Request::new(Method::Get, "/foo?bar=baz".into(), Version::Http11);
        assert_eq!(r.path, "/foo");
        assert_eq!(r.query, "bar=baz");
    }
}
