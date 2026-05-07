use std::collections::HashMap;
use std::fmt;

/// HTTP method.
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

    pub fn from_bytes(s: &[u8]) -> Option<Self> {
        match s {
            b"GET" => Some(Method::Get),
            b"POST" => Some(Method::Post),
            b"PUT" => Some(Method::Put),
            b"DELETE" => Some(Method::Delete),
            b"PATCH" => Some(Method::Patch),
            b"HEAD" => Some(Method::Head),
            b"OPTIONS" => Some(Method::Options),
            _ => None,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// HTTP status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK: StatusCode = StatusCode(200);
    pub const NOT_FOUND: StatusCode = StatusCode(404);
    pub const METHOD_NOT_ALLOWED: StatusCode = StatusCode(405);
    pub const BAD_REQUEST: StatusCode = StatusCode(400);
    pub const MOVED_PERMANENTLY: StatusCode = StatusCode(301);
    pub const NOT_MODIFIED: StatusCode = StatusCode(304);
    pub const INTERNAL_SERVER_ERROR: StatusCode = StatusCode(500);
    pub const UNAUTHORIZED: StatusCode = StatusCode(401);
    pub const FORBIDDEN: StatusCode = StatusCode(403);
    pub const PAYLOAD_TOO_LARGE: StatusCode = StatusCode(413);

    pub fn reason_phrase(&self) -> &'static str {
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
            413 => "Payload Too Large",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// HTTP headers – case-insensitive storage.
#[derive(Debug, Clone, Default)]
pub struct Headers {
    map: HashMap<String, String>,
}

impl Headers {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.map.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        let lower = name.to_lowercase();
        self.map
            .iter()
            .filter(|(k, _)| k.to_lowercase() == lower)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let key = name.into().to_lowercase();
        self.map.insert(key, value.into());
    }

    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(&name.to_lowercase())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Parsed HTTP request.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub query: String,
    pub version: String,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub request_id: String,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            method: Method::Get,
            path: String::new(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: Headers::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> crate::error::Result<T> {
        crate::body::parse_body(self)
    }

    pub fn body_str(&self) -> &str {
        // Safety: body parsers ensure valid UTF-8 for text content types
        std::str::from_utf8(&self.body).unwrap_or("")
    }
}

/// HTTP response.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(StatusCode::OK)
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND)
    }

    pub fn bad_request(msg: &str) -> Self {
        let mut resp = Self::new(StatusCode::BAD_REQUEST);
        resp.body = msg.as_bytes().to_vec();
        resp.headers
            .insert("content-type", "text/plain; charset=utf-8");
        resp
    }

    pub fn internal_error(msg: &str) -> Self {
        let mut resp = Self::new(StatusCode::INTERNAL_SERVER_ERROR);
        resp.body = msg.as_bytes().to_vec();
        resp.headers
            .insert("content-type", "text/plain; charset=utf-8");
        resp
    }

    pub fn html(body: impl Into<String>) -> Self {
        let mut resp = Self::ok();
        resp.body = body.into().into_bytes();
        resp.headers
            .insert("content-type", "text/html; charset=utf-8");
        resp
    }

    pub fn text(body: impl Into<String>) -> Self {
        let mut resp = Self::ok();
        resp.body = body.into().into_bytes();
        resp.headers
            .insert("content-type", "text/plain; charset=utf-8");
        resp
    }

    pub fn json(body: &impl serde::Serialize) -> Self {
        let mut resp = Self::ok();
        resp.body = serde_json::to_vec(body).unwrap_or_default();
        resp.headers
            .insert("content-type", "application/json; charset=utf-8");
        resp
    }

    pub fn redirect(url: &str) -> Self {
        let mut resp = Self::new(StatusCode::MOVED_PERMANENTLY);
        resp.headers.insert("location", url);
        resp
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }
}
