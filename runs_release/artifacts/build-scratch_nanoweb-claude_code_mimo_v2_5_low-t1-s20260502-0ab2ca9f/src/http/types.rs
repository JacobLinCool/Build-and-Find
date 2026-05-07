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
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "GET" => Some(Method::Get),
            "POST" => Some(Method::Post),
            "PUT" => Some(Method::Put),
            "DELETE" => Some(Method::Delete),
            "PATCH" => Some(Method::Patch),
            "HEAD" => Some(Method::Head),
            "OPTIONS" => Some(Method::Options),
            _ => None,
        }
    }

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

    pub fn all() -> &'static [Method] {
        &[
            Method::Get,
            Method::Post,
            Method::Put,
            Method::Delete,
            Method::Patch,
            Method::Head,
            Method::Options,
        ]
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusCode(pub u16);

impl StatusCode {
    pub fn new(code: u16) -> Self {
        StatusCode(code)
    }

    pub fn is_success(&self) -> bool {
        self.0 >= 200 && self.0 < 300
    }

    pub fn canonical_reason(&self) -> &'static str {
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
            415 => "Unsupported Media Type",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    pub fn new() -> Self {
        Headers {
            entries: Vec::new(),
        }
    }

    pub fn insert(&mut self, name: String, value: String) {
        self.entries.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
        self.entries.push((name, value));
    }

    pub fn append(&mut self, name: String, value: String) {
        self.entries.push((name, value));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    pub fn content_length(&self) -> Option<usize> {
        self.get("content-length").and_then(|v| v.parse().ok())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.get("content-type")
    }

    pub fn transfer_encoding(&self) -> Option<&str> {
        self.get("transfer-encoding")
    }

    pub fn is_chunked(&self) -> bool {
        self.transfer_encoding()
            .map(|v| v.eq_ignore_ascii_case("chunked"))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub version: String,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub query: HashMap<String, String>,
    pub request_id: String,
    pub extensions: HashMap<String, String>,
}

impl Request {
    pub fn new(method: Method, path: String) -> Self {
        Request {
            method,
            path,
            version: "HTTP/1.1".to_string(),
            headers: Headers::new(),
            body: Vec::new(),
            params: HashMap::new(),
            query: HashMap::new(),
            request_id: uuid::Uuid::new_v4().to_string(),
            extensions: HashMap::new(),
        }
    }

    pub fn path_with_query(&self) -> String {
        if self.query.is_empty() {
            self.path.clone()
        } else {
            let qs: Vec<String> = self
                .query
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            format!("{}?{}", self.path, qs.join("&"))
        }
    }

    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> crate::error::Result<T> {
        crate::body::parse_body(self)
    }

    pub fn client_ip(&self) -> &str {
        self.headers
            .get("x-forwarded-for")
            .and_then(|v| v.split(',').next())
            .map(|s| s.trim())
            .or_else(|| self.extensions.get("client_ip").map(|s| s.as_str()))
            .unwrap_or("127.0.0.1")
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new() -> Self {
        Response {
            status: StatusCode::new(200),
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        let mut r = Self::new();
        r.status = StatusCode::new(200);
        r
    }

    pub fn not_found() -> Self {
        let mut r = Self::new();
        r.status = StatusCode::new(404);
        r.set_text("Not Found");
        r
    }

    pub fn bad_request(msg: &str) -> Self {
        let mut r = Self::new();
        r.status = StatusCode::new(400);
        r.set_text(msg);
        r
    }

    pub fn internal_error(msg: &str) -> Self {
        let mut r = Self::new();
        r.status = StatusCode::new(500);
        r.set_text(msg);
        r
    }

    pub fn redirect(url: &str) -> Self {
        let mut r = Self::new();
        r.status = StatusCode::new(302);
        r.headers.insert("location".into(), url.into());
        r
    }

    pub fn not_modified() -> Self {
        let mut r = Self::new();
        r.status = StatusCode::new(304);
        r
    }

    pub fn method_not_allowed(allowed: &str) -> Self {
        let mut r = Self::new();
        r.status = StatusCode::new(405);
        r.headers.insert("allow".into(), allowed.into());
        r.set_text("Method Not Allowed");
        r
    }

    pub fn set_text(&mut self, body: &str) {
        self.body = body.as_bytes().to_vec();
        self.headers
            .insert("content-type".into(), "text/plain; charset=utf-8".into());
        self.headers
            .insert("content-length".into(), self.body.len().to_string());
    }

    pub fn set_html(&mut self, body: &str) {
        self.body = body.as_bytes().to_vec();
        self.headers
            .insert("content-type".into(), "text/html; charset=utf-8".into());
        self.headers
            .insert("content-length".into(), self.body.len().to_string());
    }

    pub fn set_json(&mut self, body: &str) {
        self.body = body.as_bytes().to_vec();
        self.headers
            .insert("content-type".into(), "application/json".into());
        self.headers
            .insert("content-length".into(), self.body.len().to_string());
    }

    pub fn set_body(&mut self, body: Vec<u8>, content_type: &str) {
        self.headers
            .insert("content-type".into(), content_type.into());
        self.headers
            .insert("content-length".into(), body.len().to_string());
        self.body = body;
    }
}

impl Default for Response {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_query_string(path: &str) -> (String, HashMap<String, String>) {
    match path.split_once('?') {
        Some((base, query)) => {
            let params: HashMap<String, String> = query
                .split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    let key = parts.next()?;
                    let val = parts.next().unwrap_or("");
                    Some((key.to_string(), val.to_string()))
                })
                .collect();
            (base.to_string(), params)
        }
        None => (path.to_string(), HashMap::new()),
    }
}
