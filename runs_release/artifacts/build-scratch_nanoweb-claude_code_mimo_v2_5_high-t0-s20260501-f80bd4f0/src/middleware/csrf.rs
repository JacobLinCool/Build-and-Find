use crate::http::types::{Request, Response, StatusCode};
use crate::middleware::Middleware;

pub struct CsrfProtection {
    cookie_name: String,
    header_name: String,
}

impl CsrfProtection {
    pub fn new() -> Self {
        Self {
            cookie_name: "csrf_token".into(),
            header_name: "x-csrf-token".into(),
        }
    }
}

impl Default for CsrfProtection {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for CsrfProtection {
    fn before(&self, req: &mut Request) -> Option<Response> {
        // Only protect state-changing methods
        match req.method {
            crate::http::types::Method::Post
            | crate::http::types::Method::Put
            | crate::http::types::Method::Delete
            | crate::http::types::Method::Patch => {}
            _ => return None,
        }

        // Get token from header
        let header_token = req.header(&self.header_name);

        // Get token from cookie
        let cookie_token = req.header("cookie").and_then(|c| {
            c.split(';')
                .find(|p| p.trim().starts_with(&format!("{}=", self.cookie_name)))
                .map(|p| {
                    p.trim()
                        .strip_prefix(&format!("{}=", self.cookie_name))
                        .unwrap_or("")
                        .to_string()
                })
        });

        match (header_token, cookie_token) {
            (Some(header), Some(cookie)) if header == cookie && !header.is_empty() => None,
            _ => Some(Response::new(StatusCode::FORBIDDEN).with_body(b"CSRF token mismatch")),
        }
    }

    fn name(&self) -> &str {
        "csrf"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_allows_get() {
        let csrf = CsrfProtection::new();
        let mut req = Request {
            method: crate::http::types::Method::Get,
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::types::Headers::new(),
            body: Vec::new(),
            params: Default::default(),
            request_id: "test".into(),
        };
        assert!(csrf.before(&mut req).is_none());
    }

    #[test]
    fn csrf_blocks_post_without_token() {
        let csrf = CsrfProtection::new();
        let mut req = Request {
            method: crate::http::types::Method::Post,
            path: "/".into(),
            query: String::new(),
            version: "HTTP/1.1".into(),
            headers: crate::http::types::Headers::new(),
            body: Vec::new(),
            params: Default::default(),
            request_id: "test".into(),
        };
        assert!(csrf.before(&mut req).is_some());
    }
}
