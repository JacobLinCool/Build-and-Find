use crate::http::types::{Headers, StatusCode};

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        let mut headers = Headers::new();
        headers.insert("Content-Type", "text/plain");
        Self {
            status,
            headers,
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(200)
    }

    pub fn not_found() -> Self {
        let mut r = Self::new(404);
        r.body = b"Not Found".to_vec();
        r
    }

    pub fn method_not_allowed(allowed: &[&str]) -> Self {
        let mut r = Self::new(405);
        r.headers.insert("Allow", allowed.join(", "));
        r.body = b"Method Not Allowed".to_vec();
        r
    }

    pub fn bad_request(msg: &str) -> Self {
        let mut r = Self::new(400);
        r.body = msg.as_bytes().to_vec();
        r
    }

    pub fn internal_error(msg: &str) -> Self {
        let mut r = Self::new(500);
        r.body = msg.as_bytes().to_vec();
        r
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self.headers
            .insert("Content-Length", self.body.len().to_string());
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        self.headers
            .insert("Content-Type", "text/plain; charset=utf-8");
        self.body = text.into_bytes();
        self.headers
            .insert("Content-Length", self.body.len().to_string());
        self
    }

    pub fn with_html(mut self, html: impl Into<String>) -> Self {
        let html = html.into();
        self.headers
            .insert("Content-Type", "text/html; charset=utf-8");
        self.body = html.into_bytes();
        self.headers
            .insert("Content-Length", self.body.len().to_string());
        self
    }

    pub fn with_json(mut self, json: impl serde::Serialize) -> Self {
        match serde_json::to_vec(&json) {
            Ok(bytes) => {
                self.headers.insert("Content-Type", "application/json");
                self.headers
                    .insert("Content-Length", bytes.len().to_string());
                self.body = bytes;
            }
            Err(e) => {
                self.status = 500;
                self.body = format!("Serialization error: {}", e).into_bytes();
            }
        }
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn set_cookie(&mut self, name: &str, value: &str, options: &CookieOptions) {
        let mut cookie = format!("{}={}", name, value);
        if let Some(max_age) = options.max_age {
            cookie.push_str(&format!("; Max-Age={}", max_age));
        }
        if let Some(path) = &options.path {
            cookie.push_str(&format!("; Path={}", path));
        }
        if let Some(domain) = &options.domain {
            cookie.push_str(&format!("; Domain={}", domain));
        }
        if options.secure {
            cookie.push_str("; Secure");
        }
        if options.http_only {
            cookie.push_str("; HttpOnly");
        }
        if let Some(same_site) = &options.same_site {
            cookie.push_str(&format!("; SameSite={}", same_site));
        }
        self.headers.append("Set-Cookie", cookie);
    }

    pub fn redirect(location: &str, status: u16) -> Self {
        let mut r = Self::new(status);
        r.headers.insert("Location", location);
        r.body = format!("Redirecting to {}", location).into_bytes();
        r
    }

    pub fn into_bytes(&self, suppress_body: bool) -> Vec<u8> {
        let reason = StatusCode::reason(self.status);
        let mut out = format!("HTTP/1.1 {} {}\r\n", self.status, reason).into_bytes();

        let body = if suppress_body {
            &[][..]
        } else {
            &self.body[..]
        };
        let content_length = body.len();

        let mut has_content_length = false;
        for (name, value) in self.headers.iter() {
            if name.to_lowercase() == "content-length" {
                // Override with actual body length
                out.extend_from_slice(format!("Content-Length: {}\r\n", content_length).as_bytes());
                has_content_length = true;
            } else {
                out.extend_from_slice(format!("{}: {}\r\n", name, value).as_bytes());
            }
        }

        if !has_content_length && !suppress_body {
            out.extend_from_slice(format!("Content-Length: {}\r\n", content_length).as_bytes());
        } else if !has_content_length && suppress_body {
            out.extend_from_slice(b"Content-Length: 0\r\n");
        }

        out.extend_from_slice(b"\r\n");
        if !suppress_body {
            out.extend_from_slice(body);
        }
        out
    }
}

#[derive(Debug, Default, Clone)]
pub struct CookieOptions {
    pub max_age: Option<i64>,
    pub path: Option<String>,
    pub domain: Option<String>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
}

impl CookieOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn http_only(mut self) -> Self {
        self.http_only = true;
        self
    }

    pub fn secure(mut self) -> Self {
        self.secure = true;
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn max_age(mut self, seconds: i64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    pub fn same_site(mut self, value: impl Into<String>) -> Self {
        self.same_site = Some(value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_bytes() {
        let r = Response::ok().with_text("hello");
        let bytes = r.into_bytes(false);
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Content-Length: 5"));
        assert!(s.ends_with("hello"));
    }

    #[test]
    fn test_suppress_body() {
        let r = Response::ok().with_text("hello");
        let bytes = r.into_bytes(true);
        let s = String::from_utf8(bytes).unwrap();
        assert!(!s.contains("hello"));
        assert!(s.contains("Content-Length: 0"));
    }

    #[test]
    fn test_json_response() {
        #[derive(serde::Serialize)]
        struct Data {
            value: i32,
        }
        let r = Response::ok().with_json(Data { value: 42 });
        assert_eq!(r.headers.get("content-type"), Some("application/json"));
    }

    #[test]
    fn test_set_cookie() {
        let mut r = Response::ok();
        r.set_cookie(
            "session",
            "abc123",
            &CookieOptions::new().http_only().secure().path("/"),
        );
        let cookies: Vec<_> = r.headers.get_all("Set-Cookie");
        assert_eq!(cookies.len(), 1);
        assert!(cookies[0].contains("session=abc123"));
        assert!(cookies[0].contains("HttpOnly"));
        assert!(cookies[0].contains("Secure"));
    }
}
