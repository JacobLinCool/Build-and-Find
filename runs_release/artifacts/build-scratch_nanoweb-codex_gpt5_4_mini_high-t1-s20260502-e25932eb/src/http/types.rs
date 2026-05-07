use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::net::SocketAddr;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::body;
use crate::error::{NanowebError, Result};
use crate::session::Session;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
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

    pub fn is_safe(&self) -> bool {
        matches!(self, Self::Get | Self::Head | Self::Options)
    }
}

impl Display for Method {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Method {
    type Err = NanowebError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "DELETE" => Ok(Self::Delete),
            "PATCH" => Ok(Self::Patch),
            "HEAD" => Ok(Self::Head),
            "OPTIONS" => Ok(Self::Options),
            other => Err(NanowebError::Request(format!(
                "unsupported method: {other}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Version {
    Http11,
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http11 => f.write_str("HTTP/1.1"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum StatusCode {
    Ok = 200,
    Created = 201,
    NoContent = 204,
    MovedPermanently = 301,
    SeeOther = 303,
    NotModified = 304,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    RequestTimeout = 408,
    Conflict = 409,
    PayloadTooLarge = 413,
    UnsupportedMediaType = 415,
    TooManyRequests = 429,
    InternalServerError = 500,
    ServiceUnavailable = 503,
}

impl StatusCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn class(self) -> u16 {
        self.as_u16() / 100
    }

    pub fn reason_phrase(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Created => "Created",
            Self::NoContent => "No Content",
            Self::MovedPermanently => "Moved Permanently",
            Self::SeeOther => "See Other",
            Self::NotModified => "Not Modified",
            Self::BadRequest => "Bad Request",
            Self::Unauthorized => "Unauthorized",
            Self::Forbidden => "Forbidden",
            Self::NotFound => "Not Found",
            Self::MethodNotAllowed => "Method Not Allowed",
            Self::RequestTimeout => "Request Timeout",
            Self::Conflict => "Conflict",
            Self::PayloadTooLarge => "Payload Too Large",
            Self::UnsupportedMediaType => "Unsupported Media Type",
            Self::TooManyRequests => "Too Many Requests",
            Self::InternalServerError => "Internal Server Error",
            Self::ServiceUnavailable => "Service Unavailable",
        }
    }
}

impl Display for StatusCode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.as_u16(), self.reason_phrase())
    }
}

#[derive(Clone, Debug, Default)]
pub struct HeaderMap {
    inner: HashMap<String, String>,
}

impl HeaderMap {
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.inner
            .insert(normalize_header_name(key.as_ref()), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.inner
            .get(&normalize_header_name(key))
            .map(String::as_str)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(&normalize_header_name(key))
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.inner.remove(&normalize_header_name(key))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.inner.iter()
    }
}

fn normalize_header_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[derive(Clone, Debug)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: Version,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_addr: Option<SocketAddr>,
    pub request_id: String,
    pub session: Option<Session>,
}

impl Request {
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(key)
    }

    pub fn cookies(&self) -> HashMap<String, String> {
        let mut cookies = HashMap::new();
        if let Some(raw) = self.header("cookie") {
            for pair in raw.split(';') {
                let mut parts = pair.trim().splitn(2, '=');
                if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                    cookies.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        cookies
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T> {
        body::parse_body(self.header("content-type"), &self.body)
    }

    pub fn set_param(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.params.insert(key.into(), value.into());
    }
}

#[derive(Clone, Debug)]
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

    pub fn text(status: StatusCode, body: impl Into<String>) -> Self {
        let mut response = Self::new(status);
        response
            .headers
            .insert("content-type", "text/plain; charset=utf-8");
        response.body = body.into().into_bytes();
        response
    }

    pub fn html(status: StatusCode, body: impl Into<String>) -> Self {
        let mut response = Self::new(status);
        response
            .headers
            .insert("content-type", "text/html; charset=utf-8");
        response.body = body.into().into_bytes();
        response
    }

    pub fn json<T: serde::Serialize>(status: StatusCode, value: &T) -> Result<Self> {
        let mut response = Self::new(status);
        response.headers.insert("content-type", "application/json");
        response.body = serde_json::to_vec(value)?;
        Ok(response)
    }

    pub fn with_header(mut self, key: impl AsRef<str>, value: impl Into<String>) -> Self {
        self.headers.insert(key, value);
        self
    }

    pub fn empty(status: StatusCode) -> Self {
        Self::new(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "text/plain");
        assert_eq!(headers.get("content-type"), Some("text/plain"));
    }

    #[test]
    fn methods_roundtrip() {
        assert_eq!("GET".parse::<Method>().unwrap().to_string(), "GET");
    }
}
