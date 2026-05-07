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

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            _ => return None,
        })
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

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
}

#[derive(Debug, Clone, Copy)]
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
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "OK",
        }
    }

    pub fn class(&self) -> &'static str {
        match self.0 / 100 {
            1 => "1xx",
            2 => "2xx",
            3 => "3xx",
            4 => "4xx",
            5 => "5xx",
            _ => "xxx",
        }
    }
}

/// Case-insensitive header map preserving insertion order for serialization.
#[derive(Debug, Clone, Default)]
pub struct Headers {
    inner: Vec<(String, String)>,
}

impl Headers {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let lower = name.to_ascii_lowercase();
        self.inner.retain(|(n, _)| n.to_ascii_lowercase() != lower);
        self.inner.push((name, value.into()));
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.inner.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        self.inner
            .iter()
            .find(|(n, _)| n.to_ascii_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.inner.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn remove(&mut self, name: &str) {
        let lower = name.to_ascii_lowercase();
        self.inner.retain(|(n, _)| n.to_ascii_lowercase() != lower);
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: String,
    pub version: Version,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_addr: Option<std::net::SocketAddr>,
    pub request_id: String,
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Default)]
pub struct Extensions {
    map: HashMap<String, String>,
}

impl Extensions {
    pub fn set(&mut self, k: impl Into<String>, v: impl Into<String>) {
        self.map.insert(k.into(), v.into());
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        self.map.get(k).map(|s| s.as_str())
    }
}

impl Request {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn query_pairs(&self) -> Vec<(String, String)> {
        crate::body::form::parse_urlencoded(self.query.as_bytes())
    }

    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> crate::error::Result<T> {
        crate::body::parse_body(self)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub version: Version,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            version: Version::Http11,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(StatusCode::OK)
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

    pub fn serialize(&self, suppress_body: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(256 + self.body.len());
        let line = format!(
            "{} {} {}\r\n",
            self.version.as_str(),
            self.status.0,
            self.status.reason()
        );
        out.extend_from_slice(line.as_bytes());
        let has_cl = self.headers.contains("Content-Length");
        let has_te = self.headers.contains("Transfer-Encoding");
        for (n, v) in self.headers.iter() {
            out.extend_from_slice(n.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        if !has_cl && !has_te {
            let cl = format!("Content-Length: {}\r\n", self.body.len());
            out.extend_from_slice(cl.as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        if !suppress_body {
            out.extend_from_slice(&self.body);
        }
        out
    }
}
