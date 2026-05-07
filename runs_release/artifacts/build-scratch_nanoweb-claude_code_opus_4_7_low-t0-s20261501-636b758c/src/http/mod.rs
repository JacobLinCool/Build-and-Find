//! HTTP/1.1 types and parser.

pub mod parser;

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
}

impl FromStr for Method {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        Ok(match s {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            _ => return Err(()),
        })
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

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
            501 => "Not Implemented",
            502 => "Bad Gateway",
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
            _ => "other",
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Headers {
    inner: Vec<(String, String)>,
}

impl Headers {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let lname = name.to_ascii_lowercase();
        self.inner.retain(|(k, _)| k.to_ascii_lowercase() != lname);
        self.inner.push((name, value.into()));
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.inner.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        let lname = name.to_ascii_lowercase();
        self.inner
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == lname)
            .map(|(_, v)| v.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.inner.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn remove(&mut self, name: &str) {
        let lname = name.to_ascii_lowercase();
        self.inner.retain(|(k, _)| k.to_ascii_lowercase() != lname);
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: String,
    pub version: String,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_addr: Option<std::net::SocketAddr>,
    pub request_id: String,
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Default)]
pub struct Extensions {
    pub user: Option<String>,
    pub session_id: Option<String>,
}

impl Request {
    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, crate::NanowebError> {
        crate::body::parse_body::<T>(self)
    }

    pub fn query_pairs(&self) -> Vec<(String, String)> {
        serde_urlencoded::from_str::<Vec<(String, String)>>(&self.query).unwrap_or_default()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }
}

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

    pub fn text(body: impl Into<String>) -> Self {
        let mut r = Self::new(StatusCode::OK);
        let s = body.into();
        r.headers
            .insert("Content-Type", "text/plain; charset=utf-8");
        r.body = s.into_bytes();
        r
    }

    pub fn html(body: impl Into<String>) -> Self {
        let mut r = Self::new(StatusCode::OK);
        let s = body.into();
        r.headers.insert("Content-Type", "text/html; charset=utf-8");
        r.body = s.into_bytes();
        r
    }

    pub fn json<T: serde::Serialize>(value: &T) -> Result<Self, crate::NanowebError> {
        let mut r = Self::new(StatusCode::OK);
        let body = serde_json::to_vec(value)?;
        r.headers.insert("Content-Type", "application/json");
        r.body = body;
        Ok(r)
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn empty(status: StatusCode) -> Self {
        Self::new(status)
    }
}

pub fn write_response(resp: &Response, suppress_body: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 + resp.body.len());
    out.extend_from_slice(
        format!("HTTP/1.1 {} {}\r\n", resp.status.0, resp.status.reason()).as_bytes(),
    );
    let mut has_cl = false;
    for (k, v) in resp.headers.iter() {
        if k.eq_ignore_ascii_case("content-length") {
            has_cl = true;
        }
        out.extend_from_slice(format!("{}: {}\r\n", k, v).as_bytes());
    }
    if !has_cl {
        out.extend_from_slice(format!("Content-Length: {}\r\n", resp.body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if !suppress_body {
        out.extend_from_slice(&resp.body);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_parse() {
        assert_eq!("GET".parse::<Method>().unwrap(), Method::Get);
        assert!("BOGUS".parse::<Method>().is_err());
    }

    #[test]
    fn headers_case_insensitive() {
        let mut h = Headers::new();
        h.insert("Content-Type", "text/plain");
        assert_eq!(h.get("content-type"), Some("text/plain"));
    }

    #[test]
    fn status_class() {
        assert_eq!(StatusCode::OK.class(), "2xx");
        assert_eq!(StatusCode::NOT_FOUND.class(), "4xx");
    }

    #[test]
    fn write_resp_includes_content_length() {
        let mut r = Response::ok();
        r.body = b"hi".to_vec();
        let bytes = write_response(&r, false);
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("Content-Length: 2"));
        assert!(s.ends_with("hi"));
    }
}
