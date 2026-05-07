pub mod parser;

use crate::error::Result;
use serde::de::DeserializeOwned;
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
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Other(v) => v,
        }
    }
}

impl From<&str> for Method {
    fn from(value: &str) -> Self {
        match value {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "DELETE" => Self::Delete,
            "PATCH" => Self::Patch,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            other => Self::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub const OK: Self = Self(200);
    pub const CREATED: Self = Self(201);
    pub const NO_CONTENT: Self = Self(204);
    pub const NOT_MODIFIED: Self = Self(304);
    pub const BAD_REQUEST: Self = Self(400);
    pub const UNAUTHORIZED: Self = Self(401);
    pub const FORBIDDEN: Self = Self(403);
    pub const NOT_FOUND: Self = Self(404);
    pub const METHOD_NOT_ALLOWED: Self = Self(405);
    pub const TOO_MANY_REQUESTS: Self = Self(429);
    pub const INTERNAL_SERVER_ERROR: Self = Self(500);

    pub fn reason(self) -> &'static str {
        match self.0 {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
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
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub client_ip: Option<String>,
    pub request_id: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or(&self.target)
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T> {
        crate::body::parse_typed(self)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: body.into(),
        }
    }

    pub fn text(text: impl Into<String>) -> Self {
        let mut r = Self::new(StatusCode::OK, text.into().into_bytes());
        r.headers
            .insert("content-type".into(), "text/plain; charset=utf-8".into());
        r
    }

    pub fn html(text: impl Into<String>) -> Self {
        let mut r = Self::new(StatusCode::OK, text.into().into_bytes());
        r.headers
            .insert("content-type".into(), "text/html; charset=utf-8".into());
        r
    }

    pub fn json<T: serde::Serialize>(value: &T) -> Result<Self> {
        let mut r = Self::new(StatusCode::OK, serde_json::to_vec(value)?);
        r.headers
            .insert("content-type".into(), "application/json".into());
        Ok(r)
    }

    pub fn serialize(&self, suppress_body: bool) -> Vec<u8> {
        let mut out =
            format!("HTTP/1.1 {} {}\r\n", self.status.0, self.status.reason()).into_bytes();
        let mut headers = self.headers.clone();
        headers
            .entry("content-length".into())
            .or_insert_with(|| if suppress_body { 0 } else { self.body.len() }.to_string());
        headers
            .entry("connection".into())
            .or_insert_with(|| "close".into());
        for (k, v) in headers {
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        if !suppress_body {
            out.extend_from_slice(&self.body);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_serializes() {
        let res = Response::text("hi").serialize(false);
        assert!(String::from_utf8(res).unwrap().contains("HTTP/1.1 200 OK"));
    }
}
