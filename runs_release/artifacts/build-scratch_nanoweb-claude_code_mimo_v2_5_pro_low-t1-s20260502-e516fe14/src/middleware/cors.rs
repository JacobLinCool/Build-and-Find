use super::MiddlewareDyn;
use crate::http::{Method, Request, Response, StatusCode};
use std::future::Future;
use std::pin::Pin;

/// CORS middleware with configurable allowed origins.
pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
    allow_credentials: bool,
    max_age: u32,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self {
            allowed_origins,
            allow_credentials: false,
            max_age: 3600,
        }
    }

    pub fn allow_credentials(mut self, allow: bool) -> Self {
        self.allow_credentials = allow;
        self
    }

    pub fn max_age(mut self, seconds: u32) -> Self {
        self.max_age = seconds;
        self
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.contains(&"*".to_string())
            || self.allowed_origins.iter().any(|o| o == origin)
    }
}

impl MiddlewareDyn for CorsMiddleware {
    fn before<'a>(
        &'a self,
        req: Request,
    ) -> Pin<Box<dyn Future<Output = Result<Request, Response>> + Send + 'a>> {
        Box::pin(async move {
            // Handle preflight OPTIONS requests
            if req.method == Method::Options && req.header("origin").is_some() {
                let origin = req.header("origin").unwrap_or("").to_string();
                if !self.is_origin_allowed(&origin) {
                    return Err(Response::text(
                        StatusCode::FORBIDDEN,
                        "CORS origin not allowed",
                    ));
                }
                // Return a preflight response - but we let the router handle it
                // So just pass through and the after middleware will add headers
            }
            Ok(req)
        })
    }

    fn after<'a>(
        &'a self,
        req: &'a Request,
        mut resp: Response,
    ) -> Pin<Box<dyn Future<Output = Response> + Send + 'a>> {
        Box::pin(async move {
            if let Some(origin) = req.header("origin") {
                if self.is_origin_allowed(origin) {
                    resp.headers.insert("access-control-allow-origin", origin);
                    resp.headers.insert("vary", "Origin");
                    if self.allow_credentials {
                        resp.headers
                            .insert("access-control-allow-credentials", "true");
                    }
                }
            }

            // Add preflight headers for OPTIONS
            if req.method == Method::Options {
                resp.headers.insert(
                    "access-control-allow-methods",
                    "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
                );
                resp.headers.insert(
                    "access-control-allow-headers",
                    req.header("access-control-request-headers")
                        .unwrap_or("content-type"),
                );
                resp.headers
                    .insert("access-control-max-age", self.max_age.to_string());
            }

            resp
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HeaderMap;

    fn make_req_with_origin(origin: &str) -> Request {
        let mut req = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderMap::new(),
            body: vec![],
            query: Default::default(),
            params: Default::default(),
            request_id: String::new(),
            session_data: Default::default(),
        };
        req.headers.insert("origin", origin);
        req
    }

    #[tokio::test]
    async fn test_cors_allowed_origin() {
        let mw = CorsMiddleware::new(vec!["https://example.com".into()]);
        let req = make_req_with_origin("https://example.com");
        let resp = mw.after(&req, Response::ok()).await;
        assert_eq!(
            resp.headers.get("access-control-allow-origin"),
            Some("https://example.com")
        );
    }

    #[tokio::test]
    async fn test_cors_disallowed_origin() {
        let mw = CorsMiddleware::new(vec!["https://example.com".into()]);
        let req = make_req_with_origin("https://evil.com");
        let resp = mw.after(&req, Response::ok()).await;
        assert!(resp.headers.get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn test_cors_wildcard() {
        let mw = CorsMiddleware::new(vec!["*".into()]);
        let req = make_req_with_origin("https://any.com");
        let resp = mw.after(&req, Response::ok()).await;
        assert_eq!(
            resp.headers.get("access-control-allow-origin"),
            Some("https://any.com")
        );
    }

    #[tokio::test]
    async fn test_cors_preflight() {
        let mw = CorsMiddleware::new(vec!["https://example.com".into()]);
        let mut req = make_req_with_origin("https://example.com");
        req.method = Method::Options;
        req.headers.insert("access-control-request-method", "POST");
        let resp = mw.after(&req, Response::ok()).await;
        assert!(resp.headers.get("access-control-allow-methods").is_some());
        assert!(resp.headers.get("access-control-max-age").is_some());
    }
}
