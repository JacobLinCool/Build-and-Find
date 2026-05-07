use crate::http::request::Request;
use crate::http::response::Response;
use crate::middleware::Middleware;
use std::sync::atomic::{AtomicU64, Ordering};

static CSRF_COUNTER: AtomicU64 = AtomicU64::new(1);

pub struct CsrfMiddleware {
    cookie_name: String,
    header_name: String,
}

impl CsrfMiddleware {
    pub fn new() -> Self {
        CsrfMiddleware {
            cookie_name: "csrf_token".into(),
            header_name: "x-csrf-token".into(),
        }
    }
}

impl Default for CsrfMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl Middleware for CsrfMiddleware {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response {
        let method = req.method.as_str();
        let is_state_changing =
            method == "POST" || method == "PUT" || method == "DELETE" || method == "PATCH";

        if !is_state_changing {
            return next(req);
        }

        let cookie_token = req.cookie(&self.cookie_name);
        let header_token = req.header(&self.header_name).map(|s| s.to_string());

        match (cookie_token, header_token) {
            (Some(ct), Some(ht)) if ct == ht && !ct.is_empty() => {
                // Valid CSRF token
                next(req)
            }
            _ => {
                // Missing or mismatched CSRF token
                Response::new(403)
            }
        }
    }
}

pub fn generate_csrf_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let val = CSRF_COUNTER.fetch_add(1, Ordering::SeqCst);
    let bytes: [u8; 16] = rng.gen();
    format!(
        "{}_{}",
        val,
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD_NO_PAD, &bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csrf_passes_for_get() {
        let mw = CsrfMiddleware::new();
        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 200);
    }

    #[test]
    fn test_csrf_blocks_post_without_token() {
        let mw = CsrfMiddleware::new();
        let req = Request {
            method: crate::http::parser::Method::Post,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 403);
    }

    #[test]
    fn test_csrf_allows_post_with_valid_token() {
        let mw = CsrfMiddleware::new();
        let mut headers = std::collections::HashMap::new();
        headers.insert("x-csrf-token".into(), "abc123".into());
        headers.insert("cookie".into(), "csrf_token=abc123".into());
        let req = Request {
            method: crate::http::parser::Method::Post,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers,
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };
        let resp = mw.handle(req, Box::new(|_| Response::text("ok")));
        assert_eq!(resp.status, 200);
    }
}
