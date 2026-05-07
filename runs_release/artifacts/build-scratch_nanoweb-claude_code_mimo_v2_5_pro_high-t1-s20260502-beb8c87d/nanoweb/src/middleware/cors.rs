use super::Middleware;
use crate::http::types::{Method, Request, Response};

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self { allowed_origins }
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.contains(&"*".to_string())
            || self.allowed_origins.iter().any(|o| o == origin)
    }
}

impl Middleware for CorsMiddleware {
    fn name(&self) -> &str {
        "cors"
    }

    fn before(&self, req: &mut Request) -> Option<Response> {
        if req.method == Method::Options {
            let origin = req.header("origin").unwrap_or("*");
            let allowed = self.is_origin_allowed(origin);

            if allowed {
                let mut resp = Response::new(204);
                resp.set_header("access-control-allow-origin", origin);
                resp.set_header(
                    "access-control-allow-methods",
                    "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
                );
                resp.set_header(
                    "access-control-allow-headers",
                    "Content-Type, Authorization, X-Requested-With",
                );
                resp.set_header("access-control-max-age", "86400");
                return Some(resp);
            }
        }
        None
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        if let Some(origin) = req.header("origin") {
            if self.is_origin_allowed(origin) {
                resp.set_header("access-control-allow-origin", origin);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cors_preflight() {
        let mw = CorsMiddleware::new(vec!["https://example.com".to_string()]);
        let mut headers = std::collections::HashMap::new();
        headers.insert("origin".to_string(), "https://example.com".to_string());
        let mut req = Request {
            method: Method::Options,
            target: "/api".to_string(),
            path: "/api".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let resp = mw.before(&mut req).unwrap();
        assert_eq!(resp.status, 204);
        assert_eq!(
            resp.header("access-control-allow-origin").unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn test_cors_disallowed_origin() {
        let mw = CorsMiddleware::new(vec!["https://example.com".to_string()]);
        let mut headers = std::collections::HashMap::new();
        headers.insert("origin".to_string(), "https://evil.com".to_string());
        let mut req = Request {
            method: Method::Options,
            target: "/api".to_string(),
            path: "/api".to_string(),
            query: Default::default(),
            version: "HTTP/1.1".to_string(),
            headers,
            params: Default::default(),
            body: vec![],
            request_id: "test".to_string(),
            remote_addr: None,
        };
        assert!(mw.before(&mut req).is_none());
    }
}
