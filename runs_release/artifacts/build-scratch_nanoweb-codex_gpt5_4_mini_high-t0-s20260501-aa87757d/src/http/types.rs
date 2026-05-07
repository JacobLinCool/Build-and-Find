use crate::{body, error::Result};
use serde::de::DeserializeOwned;
use std::{collections::HashMap, fmt, net::SocketAddr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
    HEAD,
    OPTIONS,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::GET => "GET",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::PATCH => "PATCH",
            Method::HEAD => "HEAD",
            Method::OPTIONS => "OPTIONS",
        }
    }

    pub fn is_safe(self) -> bool {
        matches!(self, Method::GET | Method::HEAD | Method::OPTIONS)
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Method {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "GET" => Ok(Method::GET),
            "POST" => Ok(Method::POST),
            "PUT" => Ok(Method::PUT),
            "DELETE" => Ok(Method::DELETE),
            "PATCH" => Ok(Method::PATCH),
            "HEAD" => Ok(Method::HEAD),
            "OPTIONS" => Ok(Method::OPTIONS),
            _ => Err(format!("unsupported method {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Version::Http10 => f.write_str("HTTP/1.0"),
            Version::Http11 => f.write_str("HTTP/1.1"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum StatusCode {
    Ok = 200,
    Created = 201,
    NoContent = 204,
    MovedPermanently = 301,
    NotModified = 304,
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    Conflict = 409,
    PayloadTooLarge = 413,
    TooManyRequests = 429,
    InternalServerError = 500,
    ServiceUnavailable = 503,
}

impl StatusCode {
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    pub fn reason(self) -> &'static str {
        match self {
            StatusCode::Ok => "OK",
            StatusCode::Created => "Created",
            StatusCode::NoContent => "No Content",
            StatusCode::MovedPermanently => "Moved Permanently",
            StatusCode::NotModified => "Not Modified",
            StatusCode::BadRequest => "Bad Request",
            StatusCode::Unauthorized => "Unauthorized",
            StatusCode::Forbidden => "Forbidden",
            StatusCode::NotFound => "Not Found",
            StatusCode::MethodNotAllowed => "Method Not Allowed",
            StatusCode::Conflict => "Conflict",
            StatusCode::PayloadTooLarge => "Payload Too Large",
            StatusCode::TooManyRequests => "Too Many Requests",
            StatusCode::InternalServerError => "Internal Server Error",
            StatusCode::ServiceUnavailable => "Service Unavailable",
        }
    }

    pub fn class(self) -> u16 {
        self.as_u16() / 100
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: Version,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub request_id: String,
    pub remote_addr: Option<SocketAddr>,
    pub route_pattern: Option<String>,
    pub connection_close: bool,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T> {
        body::decode_body(self)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub(crate) fn with_params(
        mut self,
        params: HashMap<String, String>,
        route_pattern: Option<String>,
    ) -> Self {
        self.params = params;
        self.route_pattern = route_pattern;
        self
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

    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::text(StatusCode::Ok, body)
    }

    pub fn text(status: StatusCode, body: impl Into<Vec<u8>>) -> Self {
        let mut response = Self::new(status);
        response.body = body.into();
        response.headers.insert(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );
        response
    }

    pub fn json<T: serde::Serialize>(value: &T) -> Result<Self> {
        let mut response = Self::new(StatusCode::Ok);
        response.body = serde_json::to_vec(value)?;
        response
            .headers
            .insert("content-type".to_string(), "application/json".to_string());
        Ok(response)
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
        self
    }

    pub fn set_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }

    pub fn empty(status: StatusCode) -> Self {
        Self::new(status)
    }

    pub fn into_http_bytes(mut self, request_id: Option<&str>, head_only: bool) -> Vec<u8> {
        if let Some(id) = request_id {
            self.headers
                .insert("x-request-id".to_string(), id.to_string());
        }
        if self.status == StatusCode::NoContent || self.status == StatusCode::NotModified {
            self.body.clear();
        }
        if !self.headers.contains_key("content-length") {
            self.headers
                .insert("content-length".to_string(), self.body.len().to_string());
        }
        let mut out = Vec::new();
        out.extend_from_slice(
            format!(
                "HTTP/1.1 {} {}\r\n",
                self.status.as_u16(),
                self.status.reason()
            )
            .as_bytes(),
        );
        for (name, value) in self.headers.iter() {
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        if !head_only {
            out.extend_from_slice(&self.body);
        }
        out
    }
}

pub fn response_status_from_error(status: StatusCode, message: impl Into<String>) -> Response {
    let mut response = Response::text(status, message.into());
    response.headers.insert(
        "content-type".to_string(),
        "text/plain; charset=utf-8".to_string(),
    );
    response
}
