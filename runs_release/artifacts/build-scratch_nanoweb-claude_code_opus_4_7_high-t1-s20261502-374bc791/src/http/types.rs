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
    Other,
}

impl Method {
    pub fn parse(s: &str) -> Self {
        match s {
            "GET" => Method::Get,
            "POST" => Method::Post,
            "PUT" => Method::Put,
            "DELETE" => Method::Delete,
            "PATCH" => Method::Patch,
            "HEAD" => Method::Head,
            "OPTIONS" => Method::Options,
            _ => Method::Other,
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
            Method::Other => "OTHER",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "HTTP/1.0" => Some(Version::Http10),
            "HTTP/1.1" => Some(Version::Http11),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Version::Http10 => "HTTP/1.0",
            Version::Http11 => "HTTP/1.1",
        }
    }
}

#[derive(Debug, Clone, Copy)]
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
    pub const PAYLOAD_TOO_LARGE: StatusCode = StatusCode(413);
    pub const TOO_MANY_REQUESTS: StatusCode = StatusCode(429);
    pub const INTERNAL_SERVER_ERROR: StatusCode = StatusCode(500);
    pub const NOT_IMPLEMENTED: StatusCode = StatusCode(501);

    pub fn class(&self) -> &'static str {
        match self.0 / 100 {
            1 => "1xx",
            2 => "2xx",
            3 => "3xx",
            4 => "4xx",
            5 => "5xx",
            _ => "xxx",
        }
    }

    pub fn reason(&self) -> &'static str {
        match self.0 {
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
            409 => "Conflict",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }
}

/// Case-insensitive HTTP headers backed by an ordered list.
#[derive(Debug, Clone, Default)]
pub struct Headers {
    inner: Vec<(String, String)>,
}

impl Headers {
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        let lower = name.to_ascii_lowercase();
        for (k, v) in self.inner.iter_mut() {
            if k.eq_ignore_ascii_case(&lower) {
                *v = value;
                return;
            }
        }
        self.inner.push((name, value));
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.inner.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        for (k, v) in &self.inner {
            if k.eq_ignore_ascii_case(name) {
                return Some(v.as_str());
            }
        }
        None
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        self.inner
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn remove(&mut self, name: &str) {
        self.inner.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.inner.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// An incoming HTTP request, fully parsed.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub path: String,
    pub query: String,
    pub version: Version,
    pub headers: Headers,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub remote_ip: String,
    pub request_id: String,
    pub extensions: HashMap<String, String>,
}

impl Request {
    pub fn new() -> Self {
        Self {
            method: Method::Get,
            target: String::new(),
            path: String::new(),
            query: String::new(),
            version: Version::Http11,
            headers: Headers::new(),
            body: Vec::new(),
            params: HashMap::new(),
            remote_ip: String::new(),
            request_id: String::new(),
            extensions: HashMap::new(),
        }
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    pub fn body_as<T: serde::de::DeserializeOwned>(&self) -> crate::error::Result<T> {
        crate::body::parse_body(self)
    }

    pub fn query_pairs(&self) -> Vec<(String, String)> {
        crate::body::parse_query(&self.query)
    }
}

impl Default for Request {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: StatusCode,
    pub version: Version,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            version: Version::Http11,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(StatusCode::OK)
    }

    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn text(mut self, body: impl Into<String>) -> Self {
        let body = body.into();
        self.headers
            .insert("content-type", "text/plain; charset=utf-8");
        self.body = body.into_bytes();
        self
    }

    pub fn html(mut self, body: impl Into<String>) -> Self {
        let body = body.into();
        self.headers
            .insert("content-type", "text/html; charset=utf-8");
        self.body = body.into_bytes();
        self
    }

    pub fn json<T: serde::Serialize>(mut self, value: &T) -> crate::error::Result<Self> {
        let body = serde_json::to_vec(value)?;
        self.headers.insert("content-type", "application/json");
        self.body = body;
        Ok(self)
    }

    pub fn body_bytes(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn empty() -> Self {
        Self::new(StatusCode::NO_CONTENT)
    }

    /// Serialize the response to bytes per HTTP/1.1, optionally suppressing the body
    /// (used for HEAD responses, where Content-Length is preserved).
    pub fn to_bytes(&self, suppress_body: bool) -> Vec<u8> {
        let mut buf = Vec::with_capacity(256 + self.body.len());
        let line = format!(
            "{} {} {}\r\n",
            self.version.as_str(),
            self.status.0,
            self.status.reason()
        );
        buf.extend_from_slice(line.as_bytes());
        let mut has_cl = false;
        let mut has_date = false;
        for (k, v) in self.headers.iter() {
            if k.eq_ignore_ascii_case("content-length") {
                has_cl = true;
            }
            if k.eq_ignore_ascii_case("date") {
                has_date = true;
            }
            buf.extend_from_slice(k.as_bytes());
            buf.extend_from_slice(b": ");
            buf.extend_from_slice(v.as_bytes());
            buf.extend_from_slice(b"\r\n");
        }
        if !has_cl {
            let cl = format!("content-length: {}\r\n", self.body.len());
            buf.extend_from_slice(cl.as_bytes());
        }
        if !has_date {
            let date = crate::http::types::http_date_now();
            let dh = format!("date: {}\r\n", date);
            buf.extend_from_slice(dh.as_bytes());
        }
        buf.extend_from_slice(b"\r\n");
        if !suppress_body {
            buf.extend_from_slice(&self.body);
        }
        buf
    }
}

pub fn http_date_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_http_date(secs)
}

/// Format a UNIX timestamp as an IMF-fixdate (e.g. "Sun, 06 Nov 1994 08:49:37 GMT").
pub fn format_http_date(secs: u64) -> String {
    let days_since_epoch = (secs / 86400) as i64;
    let secs_of_day = (secs % 86400) as u32;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    // Day of week: 1970-01-01 was a Thursday (index 4 if Sun=0). Use Sun=0.
    let dow = ((days_since_epoch + 4).rem_euclid(7)) as usize;
    let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let (year, month, day) = days_to_ymd(days_since_epoch);
    let month_names = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        day_names[dow],
        day,
        month_names[(month - 1) as usize],
        year,
        h,
        m,
        s
    )
}

fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Algorithm per Howard Hinnant.
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_case_insensitive() {
        let mut h = Headers::new();
        h.insert("Content-Type", "text/plain");
        assert_eq!(h.get("content-type"), Some("text/plain"));
        assert_eq!(h.get("CONTENT-TYPE"), Some("text/plain"));
    }

    #[test]
    fn headers_overwrite_on_insert() {
        let mut h = Headers::new();
        h.insert("X-Foo", "a");
        h.insert("x-foo", "b");
        assert_eq!(h.get("X-Foo"), Some("b"));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn method_parsing() {
        assert_eq!(Method::parse("GET"), Method::Get);
        assert_eq!(Method::parse("POST"), Method::Post);
        assert_eq!(Method::parse("FOO"), Method::Other);
    }

    #[test]
    fn http_date_format() {
        // 1970-01-01 00:00:00 UTC was a Thursday.
        let s = format_http_date(0);
        assert!(s.starts_with("Thu, 01 Jan 1970"));
    }

    #[test]
    fn response_serializes() {
        let r = Response::ok().text("hello");
        let bytes = r.to_bytes(false);
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("HTTP/1.1 200 OK"));
        assert!(s.contains("content-length: 5"));
        assert!(s.ends_with("hello"));
    }
}
