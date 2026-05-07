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

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Method {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "GET" => Ok(Method::Get),
            "POST" => Ok(Method::Post),
            "PUT" => Ok(Method::Put),
            "DELETE" => Ok(Method::Delete),
            "PATCH" => Ok(Method::Patch),
            "HEAD" => Ok(Method::Head),
            "OPTIONS" => Ok(Method::Options),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub params: HashMap<String, String>,
    pub body: Vec<u8>,
    pub request_id: String,
    pub remote_addr: Option<String>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let lower = name.to_lowercase();
        self.headers.get(&lower).map(|s| s.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(|s| s.as_str())
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }

    pub fn content_length(&self) -> Option<usize> {
        self.header("content-length")?.parse().ok()
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(200)
    }

    pub fn text(status: u16, text: &str) -> Self {
        let mut resp = Self::new(status);
        resp.set_header("content-type", "text/plain; charset=utf-8");
        resp.body = text.as_bytes().to_vec();
        resp
    }

    pub fn html(status: u16, html: &str) -> Self {
        let mut resp = Self::new(status);
        resp.set_header("content-type", "text/html; charset=utf-8");
        resp.body = html.as_bytes().to_vec();
        resp
    }

    pub fn json_value(status: u16, value: &serde_json::Value) -> Self {
        let mut resp = Self::new(status);
        resp.set_header("content-type", "application/json");
        if let Ok(bytes) = serde_json::to_vec(value) {
            resp.body = bytes;
        }
        resp
    }

    pub fn set_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.headers
            .insert(name.into().to_lowercase(), value.into());
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    pub fn status_text(&self) -> &'static str {
        match self.status {
            100 => "Continue",
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
            408 => "Request Timeout",
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }

    pub fn status_class(&self) -> &'static str {
        match self.status {
            100..=199 => "1xx",
            200..=299 => "2xx",
            300..=399 => "3xx",
            400..=499 => "4xx",
            500..=599 => "5xx",
            _ => "other",
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let status_line = format!("HTTP/1.1 {} {}\r\n", self.status, self.status_text());
        out.extend_from_slice(status_line.as_bytes());

        let mut headers = self.headers.clone();
        if !headers.contains_key("content-length") {
            headers.insert("content-length".to_string(), self.body.len().to_string());
        }
        if !headers.contains_key("connection") {
            headers.insert("connection".to_string(), "close".to_string());
        }

        for (k, v) in &headers {
            out.extend_from_slice(k.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(v.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&self.body);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_as_str() {
        assert_eq!(Method::Get.as_str(), "GET");
        assert_eq!(Method::Post.as_str(), "POST");
    }

    #[test]
    fn test_method_from_str() {
        assert_eq!("GET".parse::<Method>().unwrap(), Method::Get);
        assert!("INVALID".parse::<Method>().is_err());
    }

    #[test]
    fn test_response_text() {
        let resp = Response::text(200, "hello");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
        assert_eq!(
            resp.header("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[test]
    fn test_response_to_bytes() {
        let resp = Response::text(200, "OK");
        let bytes = resp.to_bytes();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("content-length: 2\r\n"));
    }

    #[test]
    fn test_request_helpers() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        let req = Request {
            method: Method::Get,
            target: "/test?foo=bar".to_string(),
            path: "/test".to_string(),
            query: {
                let mut m = HashMap::new();
                m.insert("foo".to_string(), "bar".to_string());
                m
            },
            version: "HTTP/1.1".to_string(),
            headers,
            params: HashMap::new(),
            body: Vec::new(),
            request_id: "test-id".to_string(),
            remote_addr: None,
        };
        assert_eq!(req.header("Content-Type").unwrap(), "application/json");
        assert_eq!(req.query_param("foo").unwrap(), "bar");
        assert_eq!(req.content_type().unwrap(), "application/json");
    }
}
