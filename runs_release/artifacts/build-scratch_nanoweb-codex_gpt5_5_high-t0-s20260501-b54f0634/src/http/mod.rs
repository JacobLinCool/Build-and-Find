pub mod parser;

use crate::{body, error::Result};
use rand::{Rng, distributions::Alphanumeric};
use serde::de::DeserializeOwned;
use std::{collections::HashMap, fmt, net::SocketAddr, str::FromStr};

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
            Self::Other(s) => s.as_str(),
        }
    }

    pub fn routable() -> [Self; 7] {
        [
            Self::Get,
            Self::Post,
            Self::Put,
            Self::Delete,
            Self::Patch,
            Self::Head,
            Self::Options,
        ]
    }
}

impl FromStr for Method {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "DELETE" => Self::Delete,
            "PATCH" => Self::Patch,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            other if other.chars().all(|c| c.is_ascii_uppercase() || c == '-') => {
                Self::Other(other.to_string())
            }
            _ => return Err(()),
        })
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub type HeaderMap = HashMap<String, String>;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub request_id: String,
    pub remote_addr: Option<SocketAddr>,
}

impl Request {
    pub fn new(method: Method, target: impl Into<String>) -> Self {
        let target = target.into();
        let (path, query) = split_target(&target);
        Self {
            method,
            target,
            path,
            query,
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            request_id: new_request_id(),
            remote_addr: None,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn set_header(&mut self, name: &str, value: impl Into<String>) {
        self.headers.insert(name.to_ascii_lowercase(), value.into());
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T> {
        body::parse_typed(self)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            reason: reason_phrase(status).into(),
            headers: HeaderMap::new(),
            body: Vec::new(),
        }
    }

    pub fn text(status: u16, text: impl Into<String>) -> Self {
        let mut response = Self::new(status);
        response.set_header("Content-Type", "text/plain; charset=utf-8");
        response.body = text.into().into_bytes();
        response
    }

    pub fn html(status: u16, text: impl Into<String>) -> Self {
        let mut response = Self::new(status);
        response.set_header("Content-Type", "text/html; charset=utf-8");
        response.body = text.into().into_bytes();
        response
    }

    pub fn json<T: serde::Serialize>(status: u16, value: &T) -> Result<Self> {
        let mut response = Self::new(status);
        response.set_header("Content-Type", "application/json");
        response.body = serde_json::to_vec(value)?;
        Ok(response)
    }

    pub fn set_header(&mut self, name: &str, value: impl Into<String>) {
        self.headers.insert(name.to_ascii_lowercase(), value.into());
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn to_http_bytes(&self, suppress_body: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let body_len = if suppress_body { 0 } else { self.body.len() };
        out.extend_from_slice(format!("HTTP/1.1 {} {}\r\n", self.status, self.reason).as_bytes());
        let mut has_len = false;
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case("content-length") {
                has_len = true;
            }
            out.extend_from_slice(format!("{}: {}\r\n", canonical_header(k), v).as_bytes());
        }
        if !has_len {
            out.extend_from_slice(format!("Content-Length: {body_len}\r\n").as_bytes());
        }
        out.extend_from_slice(b"Connection: close\r\n\r\n");
        if !suppress_body {
            out.extend_from_slice(&self.body);
        }
        out
    }
}

pub fn split_target(target: &str) -> (String, Option<String>) {
    match target.split_once('?') {
        Some((path, query)) => (path.to_string(), Some(query.to_string())),
        None => (target.to_string(), None),
    }
}

pub fn new_request_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .collect()
}

pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
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

fn canonical_header(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_serializes() {
        let bytes = Response::text(200, "ok").to_http_bytes(false);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("Content-Length: 2")
        );
    }

    #[test]
    fn request_headers_are_case_insensitive() {
        let mut req = Request::new(Method::Get, "/");
        req.set_header("Content-Type", "application/json");
        assert_eq!(req.header("content-type"), Some("application/json"));
    }
}
