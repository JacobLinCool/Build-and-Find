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
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }

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
    pub query: Option<String>,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub addr: std::net::SocketAddr,
    pub request_id: String,
}

impl Request {
    pub fn query_params(&self) -> HashMap<String, String> {
        self.query
            .as_deref()
            .map(|q| serde_urlencoded::from_str(q).unwrap_or_default())
            .unwrap_or_default()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
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

    pub fn text(status: u16, text: impl Into<String>) -> Self {
        let body = text.into();
        let mut resp = Self::new(status);
        resp.headers
            .push(("content-type".into(), "text/plain; charset=utf-8".into()));
        resp.headers
            .push(("content-length".into(), body.len().to_string()));
        resp.body = body.into_bytes();
        resp
    }

    pub fn html(status: u16, html: impl Into<String>) -> Self {
        let body = html.into();
        let mut resp = Self::new(status);
        resp.headers
            .push(("content-type".into(), "text/html; charset=utf-8".into()));
        resp.headers
            .push(("content-length".into(), body.len().to_string()));
        resp.body = body.into_bytes();
        resp
    }

    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        let body = serde_json::to_vec_pretty(value).unwrap_or_default();
        let mut resp = Self::new(status);
        resp.headers
            .push(("content-type".into(), "application/json".into()));
        resp.headers
            .push(("content-length".into(), body.len().to_string()));
        resp.body = body;
        resp
    }

    pub fn set_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.headers.push((name.into(), value.into()));
    }

    pub fn status_code(&self) -> u16 {
        self.status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVersion {
    Http10,
    Http11,
}
