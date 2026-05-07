pub mod parser;

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
            Self::Other(value) => value,
        }
    }
}

impl std::str::FromStr for Method {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
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
            _ => return Err("invalid method".into()),
        })
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
    pub const PAYLOAD_TOO_LARGE: Self = Self(413);
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
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }

    pub fn class(self) -> String {
        format!("{}xx", self.0 / 100)
    }
}

pub type HeaderMap = BTreeMap<String, String>;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: Option<String>,
    pub version: String,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub peer_addr: Option<std::net::SocketAddr>,
    pub route_pattern: Option<String>,
    pub params: BTreeMap<String, String>,
    pub request_id: String,
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
            peer_addr: None,
            route_pattern: None,
            params: BTreeMap::new(),
            request_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn set_header(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.headers
            .insert(key.as_ref().to_ascii_lowercase(), value.into());
    }

    pub fn path_param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
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

    pub fn text(status: StatusCode, text: impl Into<String>) -> Self {
        let mut response = Self::new(status);
        response.set_header("Content-Type", "text/plain; charset=utf-8");
        response.body = text.into().into_bytes();
        response
    }

    pub fn html(status: StatusCode, html: impl Into<String>) -> Self {
        let mut response = Self::new(status);
        response.set_header("Content-Type", "text/html; charset=utf-8");
        response.body = html.into().into_bytes();
        response
    }

    pub fn json<T: Serialize>(status: StatusCode, value: &T) -> crate::Result<Self> {
        let mut response = Self::new(status);
        response.set_header("Content-Type", "application/json");
        response.body = serde_json::to_vec(value)?;
        Ok(response)
    }

    pub fn set_header(&mut self, key: impl AsRef<str>, value: impl Into<String>) {
        self.headers
            .insert(key.as_ref().to_ascii_lowercase(), value.into());
    }

    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .get(&key.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn suppress_body(mut self) -> Self {
        self.body.clear();
        self
    }

    pub fn to_http_bytes(&self, keep_alive: bool) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(
            format!("HTTP/1.1 {} {}\r\n", self.status.0, self.status.reason()).as_bytes(),
        );
        let has_len = self.headers.contains_key("content-length");
        for (key, value) in &self.headers {
            out.extend_from_slice(format!("{}: {}\r\n", canonical_header(key), value).as_bytes());
        }
        if !has_len {
            out.extend_from_slice(format!("Content-Length: {}\r\n", self.body.len()).as_bytes());
        }
        out.extend_from_slice(
            format!(
                "Connection: {}\r\n\r\n",
                if keep_alive { "keep-alive" } else { "close" }
            )
            .as_bytes(),
        );
        out.extend_from_slice(&self.body);
        out
    }
}

fn split_target(target: &str) -> (String, Option<String>) {
    if let Some((path, query)) = target.split_once('?') {
        (path.to_string(), Some(query.to_string()))
    } else {
        (target.to_string(), None)
    }
}

fn canonical_header(key: &str) -> String {
    key.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                }
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
        let bytes = Response::text(StatusCode::OK, "hi").to_http_bytes(false);
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("Content-Length: 2")
        );
    }

    #[test]
    fn request_splits_query() {
        let req = Request::new(Method::Get, "/a?b=1");
        assert_eq!(req.path, "/a");
        assert_eq!(req.query.as_deref(), Some("b=1"));
    }
}
