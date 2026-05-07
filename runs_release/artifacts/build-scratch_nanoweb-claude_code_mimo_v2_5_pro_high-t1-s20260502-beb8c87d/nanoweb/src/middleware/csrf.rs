use super::Middleware;
use crate::http::types::{Method, Request, Response};
use rand::rngs::OsRng;
use rand::Rng;

const CSRF_TOKEN_HEADER: &str = "x-csrf-token";
const CSRF_COOKIE: &str = "nanoweb_csrf";

pub struct CsrfMiddleware;

impl CsrfMiddleware {
    pub fn new() -> Self {
        Self
    }

    fn generate_token() -> String {
        let mut rng = OsRng;
        let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        hex_encode(&bytes)
    }
}

impl Middleware for CsrfMiddleware {
    fn name(&self) -> &str {
        "csrf"
    }

    fn before(&self, req: &mut Request) -> Option<Response> {
        // Safe methods don't need CSRF protection
        if matches!(req.method, Method::Get | Method::Head | Method::Options) {
            return None;
        }

        // Check header token
        let header_token = req.header(CSRF_TOKEN_HEADER);
        let cookie_token = req.header("cookie").and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                if c.starts_with(&format!("{CSRF_COOKIE}=")) {
                    Some(&c[CSRF_COOKIE.len() + 1..])
                } else {
                    None
                }
            })
        });

        match (header_token, cookie_token) {
            (Some(ht), Some(ct)) if ht == ct => None, // valid
            _ => Some(Response::text(403, "CSRF token missing or mismatched")),
        }
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        // Set CSRF cookie on GET requests if not present
        if matches!(req.method, Method::Get | Method::Head) {
            if req
                .header("cookie")
                .and_then(|c| c.find(CSRF_COOKIE))
                .is_none()
            {
                let token = Self::generate_token();
                resp.set_header(
                    "set-cookie",
                    format!("{CSRF_COOKIE}={token}; Path=/; SameSite=Strict"),
                );
            }
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_csrf_blocks_post_without_token() {
        let mw = CsrfMiddleware::new();
        let mut req = Request {
            method: Method::Post,
            target: "/submit".to_string(),
            path: "/submit".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        assert!(mw.before(&mut req).is_some());
    }

    #[test]
    fn test_csrf_allows_get() {
        let mw = CsrfMiddleware::new();
        let mut req = Request {
            method: Method::Get,
            target: "/page".to_string(),
            path: "/page".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        assert!(mw.before(&mut req).is_none());
    }

    #[test]
    fn test_csrf_allows_matching_tokens() {
        let mw = CsrfMiddleware::new();
        let mut headers = HashMap::new();
        headers.insert(CSRF_TOKEN_HEADER.to_string(), "abc123".to_string());
        headers.insert("cookie".to_string(), format!("{CSRF_COOKIE}=abc123"));
        let mut req = Request {
            method: Method::Post,
            target: "/submit".to_string(),
            path: "/submit".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        assert!(mw.before(&mut req).is_none());
    }

    #[test]
    fn test_csrf_sets_cookie_on_get() {
        let mw = CsrfMiddleware::new();
        let req = Request {
            method: Method::Get,
            target: "/page".to_string(),
            path: "/page".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let mut resp = Response::ok();
        mw.after(&req, &mut resp);
        assert!(resp.header("set-cookie").is_some());
    }
}
