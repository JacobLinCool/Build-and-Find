use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Method {
    GET,
    POST,
    PUT,
    DELETE,
    HEAD,
    OPTIONS,
    PATCH,
    TRACE,
    CONNECT,
    Custom(String),
}

impl Method {
    pub fn from_str(s: &str) -> Self {
        match s {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            "PATCH" => Method::PATCH,
            "TRACE" => Method::TRACE,
            "CONNECT" => Method::CONNECT,
            other => Method::Custom(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Method::GET => "GET",
            Method::POST => "POST",
            Method::PUT => "PUT",
            Method::DELETE => "DELETE",
            Method::HEAD => "HEAD",
            Method::OPTIONS => "OPTIONS",
            Method::PATCH => "PATCH",
            Method::TRACE => "TRACE",
            Method::CONNECT => "CONNECT",
            Method::Custom(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Created,
    Accepted,
    NoContent,
    MovedPermanently,
    Found,
    NotModified,
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    Conflict,
    Gone,
    UnprocessableEntity,
    TooManyRequests,
    InternalServerError,
    NotImplemented,
    BadGateway,
    ServiceUnavailable,
    Custom(u16),
}

impl Status {
    pub fn code(&self) -> u16 {
        match self {
            Status::Ok => 200,
            Status::Created => 201,
            Status::Accepted => 202,
            Status::NoContent => 204,
            Status::MovedPermanently => 301,
            Status::Found => 302,
            Status::NotModified => 304,
            Status::BadRequest => 400,
            Status::Unauthorized => 401,
            Status::Forbidden => 403,
            Status::NotFound => 404,
            Status::MethodNotAllowed => 405,
            Status::Conflict => 409,
            Status::Gone => 410,
            Status::UnprocessableEntity => 422,
            Status::TooManyRequests => 429,
            Status::InternalServerError => 500,
            Status::NotImplemented => 501,
            Status::BadGateway => 502,
            Status::ServiceUnavailable => 503,
            Status::Custom(c) => *c,
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            Status::Ok => "OK",
            Status::Created => "Created",
            Status::Accepted => "Accepted",
            Status::NoContent => "No Content",
            Status::MovedPermanently => "Moved Permanently",
            Status::Found => "Found",
            Status::NotModified => "Not Modified",
            Status::BadRequest => "Bad Request",
            Status::Unauthorized => "Unauthorized",
            Status::Forbidden => "Forbidden",
            Status::NotFound => "Not Found",
            Status::MethodNotAllowed => "Method Not Allowed",
            Status::Conflict => "Conflict",
            Status::Gone => "Gone",
            Status::UnprocessableEntity => "Unprocessable Entity",
            Status::TooManyRequests => "Too Many Requests",
            Status::InternalServerError => "Internal Server Error",
            Status::NotImplemented => "Not Implemented",
            Status::BadGateway => "Bad Gateway",
            Status::ServiceUnavailable => "Service Unavailable",
            Status::Custom(_) => "Unknown",
        }
    }

    pub fn class(&self) -> &str {
        let c = self.code();
        if c < 200 {
            "1xx"
        } else if c < 300 {
            "2xx"
        } else if c < 400 {
            "3xx"
        } else if c < 500 {
            "4xx"
        } else {
            "5xx"
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub query: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub extensions: HashMap<String, String>,
}

impl Request {
    pub fn new(method: Method, path: String) -> Self {
        Request {
            method,
            path,
            query: String::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            body: Vec::new(),
            params: HashMap::new(),
            extensions: HashMap::new(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header("content-type")
    }

    pub fn body_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.body)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: Status,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: Status) -> Self {
        Response {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Response::new(Status::Ok)
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        let s: String = text.into();
        self.body = s.into_bytes();
        self.headers.insert(
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        );
        self
    }

    pub fn with_json(mut self, json: impl Into<String>) -> Self {
        let s: String = json.into();
        self.body = s.into_bytes();
        self.headers.insert(
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        );
        self
    }

    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        let s: String = html.into();
        self.body = s.into_bytes();
        self.headers.insert(
            "content-type".to_string(),
            "text/html; charset=utf-8".to_string(),
        );
        self
    }

    pub fn set_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_lowercase(), value.into());
        self
    }

    pub fn to_bytes(&self, include_body: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let status_line = format!(
            "HTTP/1.1 {} {}\r\n",
            self.status.code(),
            self.status.reason()
        );
        out.extend_from_slice(status_line.as_bytes());

        let mut headers = self.headers.clone();
        if include_body && !self.body.is_empty() {
            headers
                .entry("content-length".to_string())
                .or_insert_with(|| self.body.len().to_string());
        } else if !include_body {
            headers.insert("content-length".to_string(), "0".to_string());
        }

        for (k, v) in &headers {
            let line = format!("{}: {}\r\n", k, v);
            out.extend_from_slice(line.as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        if include_body {
            out.extend_from_slice(&self.body);
        }
        out
    }
}
