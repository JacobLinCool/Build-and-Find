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
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            other => Method::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
            Method::Other(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub const TOO_MANY_REQUESTS: StatusCode = StatusCode(429);
    pub const INTERNAL_SERVER_ERROR: StatusCode = StatusCode(500);

    pub fn reason_phrase(&self) -> &'static str {
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

impl std::fmt::Display for StatusCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Headers(pub HashMap<String, String>);

impl Headers {
    pub fn new() -> Self {
        Headers(HashMap::new())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(&key.to_lowercase()).map(|s| s.as_str())
    }

    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into().to_lowercase(), value.into());
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(&key.to_lowercase())
    }
}

impl Default for Headers {
    fn default() -> Self {
        Headers::new()
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_addr: std::net::SocketAddr,
    pub request_id: String,
}

impl Request {
    pub fn new(
        method: Method,
        path: String,
        query: HashMap<String, String>,
        headers: Headers,
        body: Vec<u8>,
        remote_addr: std::net::SocketAddr,
    ) -> Self {
        Request {
            method,
            path,
            query,
            headers,
            body,
            params: HashMap::new(),
            remote_addr,
            request_id: String::new(),
        }
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
        Response {
            status,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Response::new(StatusCode::OK)
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.set(key, value);
        self
    }

    pub fn with_json<T: serde::Serialize>(mut self, data: &T) -> Result<Self, crate::error::NanowebError> {
        let body = serde_json::to_vec(data)?;
        self.headers.set("content-type", "application/json");
        self.body = body;
        Ok(self)
    }

    pub fn text(body: impl Into<String>) -> Self {
        let body_str = body.into();
        let body_bytes = body_str.into_bytes();
        Response {
            status: StatusCode::OK,
            headers: {
                let mut h = Headers::new();
                h.set("content-type", "text/plain; charset=utf-8");
                h
            },
            body: body_bytes,
        }
    }

    pub fn json<T: serde::Serialize>(data: &T) -> Result<Self, crate::error::NanowebError> {
        let body = serde_json::to_vec(data)?;
        Ok(Response {
            status: StatusCode::OK,
            headers: {
                let mut h = Headers::new();
                h.set("content-type", "application/json");
                h
            },
            body,
        })
    }

    pub fn into_bytes(mut self) -> Vec<u8> {
        if !self.headers.contains("content-length") {
            self.headers.set("content-length", self.body.len().to_string());
        }

        let mut output = format!(
            "HTTP/1.1 {} {}\r\n",
            self.status.0,
            self.status.reason_phrase()
        )
        .into_bytes();

        for (k, v) in &self.headers.0 {
            output.extend_from_slice(k.as_bytes());
            output.extend_from_slice(b": ");
            output.extend_from_slice(v.as_bytes());
            output.extend_from_slice(b"\r\n");
        }

        output.extend_from_slice(b"\r\n");
        output.extend_from_slice(&self.body);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_from_str() {
        assert_eq!(Method::from_str("GET"), Method::Get);
        assert_eq!(Method::from_str("POST"), Method::Post);
        assert_eq!(Method::from_str("get"), Method::Get);
    }

    #[test]
    fn test_status_code() {
        assert_eq!(StatusCode::OK.reason_phrase(), "OK");
        assert_eq!(StatusCode::NOT_FOUND.reason_phrase(), "Not Found");
    }

    #[test]
    fn test_response_into_bytes() {
        let resp = Response::text("Hello").with_header("x-custom", "value");
        let bytes = resp.into_bytes();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Hello"));
    }
}
