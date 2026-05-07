use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        Response {
            status,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    pub fn ok() -> Self {
        Self::new(200)
    }

    pub fn text(body: &str) -> Self {
        let mut resp = Self::ok();
        resp.headers
            .insert("content-type".into(), "text/plain; charset=utf-8".into());
        resp.body = body.as_bytes().to_vec();
        resp
    }

    pub fn html(body: &str) -> Self {
        let mut resp = Self::ok();
        resp.headers
            .insert("content-type".into(), "text/html; charset=utf-8".into());
        resp.body = body.as_bytes().to_vec();
        resp
    }

    pub fn json<T: serde::Serialize>(value: &T) -> Self {
        let body = serde_json::to_vec(value).unwrap_or_default();
        let mut resp = Self::ok();
        resp.headers
            .insert("content-type".into(), "application/json".into());
        resp.body = body;
        resp
    }

    pub fn not_found() -> Self {
        let mut resp = Self::new(404);
        resp.body = b"Not Found".to_vec();
        resp
    }

    pub fn bad_request(msg: &str) -> Self {
        let mut resp = Self::new(400);
        resp.body = msg.as_bytes().to_vec();
        resp
    }

    pub fn redirect(location: &str) -> Self {
        let mut resp = Self::new(302);
        resp.headers.insert("location".into(), location.into());
        resp
    }

    pub fn set_header(&mut self, key: &str, value: &str) {
        self.headers.insert(key.to_lowercase(), value.to_string());
    }

    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers.get(&key.to_lowercase()).map(|s| s.as_str())
    }

    pub fn status_text(&self) -> &str {
        match self.status {
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
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            _ => "Unknown",
        }
    }

    pub fn to_http_bytes(&self) -> Vec<u8> {
        let mut output = Vec::new();
        let status_line = format!("HTTP/1.1 {} {}\r\n", self.status, self.status_text());
        output.extend_from_slice(status_line.as_bytes());

        // Ensure content-length is set
        if !self.headers.contains_key("content-length") {
            let cl = format!("Content-Length: {}\r\n", self.body.len());
            output.extend_from_slice(cl.as_bytes());
        }

        for (key, value) in &self.headers {
            let header_line = format!("{}: {}\r\n", key, value);
            output.extend_from_slice(header_line.as_bytes());
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
    fn test_response_200() {
        let resp = Response::text("hello");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hello");
    }

    #[test]
    fn test_response_to_http_bytes() {
        let resp = Response::text("hi");
        let bytes = resp.to_http_bytes();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(s.contains("Content-Length: 2\r\n"));
        assert!(s.ends_with("hi"));
    }

    #[test]
    fn test_json_response() {
        let resp = Response::json(&serde_json::json!({"key": "value"}));
        assert_eq!(
            resp.headers.get("content-type").unwrap(),
            "application/json"
        );
        assert!(resp.body.windows(3).any(|w| w == b"key"));
    }
}
