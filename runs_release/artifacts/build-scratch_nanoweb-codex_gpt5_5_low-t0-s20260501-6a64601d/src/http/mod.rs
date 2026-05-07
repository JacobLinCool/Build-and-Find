pub mod parser;

use crate::body;
use crate::error::{NanowebError, Result};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};

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
            v => Self::Other(v.to_string()),
        }
    }
}

impl Display for Method {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StatusCode {
    Ok = 200,
    Created = 201,
    NoContent = 204,
    NotModified = 304,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    PayloadTooLarge = 413,
    InternalServerError = 500,
}

impl StatusCode {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Created => "Created",
            Self::NoContent => "No Content",
            Self::NotModified => "Not Modified",
            Self::BadRequest => "Bad Request",
            Self::Unauthorized => "Unauthorized",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::PayloadTooLarge => "Payload Too Large",
            Self::InternalServerError => "Internal Server Error",
        }
    }

    pub fn class(self) -> &'static str {
        match self as u16 {
            100..=199 => "1xx",
            200..=299 => "2xx",
            300..=399 => "3xx",
            400..=499 => "4xx",
            _ => "5xx",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub peer_addr: Option<std::net::SocketAddr>,
    pub request_id: String,
    pub params: HashMap<String, String>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.header("cookie")?.split(';').find_map(|part| {
            let (k, v) = part.trim().split_once('=')?;
            (k == name).then(|| v.to_string())
        })
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T> {
        body::parse(self).map_err(NanowebError::Body)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    pub fn text(status: StatusCode, text: impl Into<String>) -> Self {
        let mut res = Self::new(status);
        res.header("Content-Type", "text/plain; charset=utf-8");
        res.body = text.into().into_bytes();
        res
    }

    pub fn html(status: StatusCode, html: impl Into<String>) -> Self {
        let mut res = Self::new(status);
        res.header("Content-Type", "text/html; charset=utf-8");
        res.body = html.into().into_bytes();
        res
    }

    pub fn json<T: serde::Serialize>(value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => {
                let mut res = Self::new(StatusCode::Ok);
                res.header("Content-Type", "application/json");
                res.body = body;
                res
            }
            Err(_) => Self::text(StatusCode::InternalServerError, "json serialization failed"),
        }
    }

    pub fn header(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.headers.insert(key.into(), value.into());
    }

    pub fn to_bytes(&self, suppress_body: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let body_len = if suppress_body { 0 } else { self.body.len() };
        out.extend_from_slice(
            format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n",
                self.status as u16,
                self.status.reason(),
                body_len
            )
            .as_bytes(),
        );
        for (k, v) in &self.headers {
            if k.eq_ignore_ascii_case("content-length") {
                continue;
            }
            out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
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
        let res = Response::text(StatusCode::Ok, "hello");
        let bytes = String::from_utf8(res.to_bytes(false)).unwrap();
        assert!(bytes.starts_with("HTTP/1.1 200 OK"));
        assert!(bytes.ends_with("hello"));
    }
}
