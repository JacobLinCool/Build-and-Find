use super::Middleware;
use crate::http::{Method, Request, Response};
use std::future::Future;
use std::pin::Pin;

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
    allowed_methods: Vec<String>,
    allowed_headers: Vec<String>,
    max_age: u32,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self {
            allowed_origins,
            allowed_methods: vec![
                "GET".into(),
                "POST".into(),
                "PUT".into(),
                "DELETE".into(),
                "PATCH".into(),
                "HEAD".into(),
                "OPTIONS".into(),
            ],
            allowed_headers: vec!["content-type".into(), "authorization".into()],
            max_age: 3600,
        }
    }

    pub fn with_methods(mut self, methods: Vec<String>) -> Self {
        self.allowed_methods = methods;
        self
    }

    pub fn with_headers(mut self, headers: Vec<String>) -> Self {
        self.allowed_headers = headers;
        self
    }

    pub fn with_max_age(mut self, max_age: u32) -> Self {
        self.max_age = max_age;
        self
    }
}

impl Middleware for CorsMiddleware {
    fn before(
        &self,
        req: &mut Request,
    ) -> Pin<Box<dyn Future<Output = Option<Response>> + Send + 'static>> {
        let origin = req.header("origin").unwrap_or("").to_string();
        let method = req.method;
        let allowed_origins = self.allowed_origins.clone();
        let allowed_methods = self.allowed_methods.join(", ");
        let allowed_headers = self.allowed_headers.join(", ");
        let max_age = self.max_age;
        let is_preflight =
            method == Method::Options && req.header("access-control-request-method").is_some();

        Box::pin(async move {
            if is_preflight {
                let mut resp = Response::new(204);
                if allowed_origins.contains(&"*".to_string()) {
                    resp = resp.with_header("access-control-allow-origin", "*");
                } else if allowed_origins.contains(&origin) {
                    resp = resp.with_header("access-control-allow-origin", &origin);
                }
                resp = resp
                    .with_header("access-control-allow-methods", &allowed_methods)
                    .with_header("access-control-allow-headers", &allowed_headers)
                    .with_header("access-control-max-age", &max_age.to_string());
                return Some(resp);
            }
            None
        })
    }

    fn after(&self, req: &Request, resp: &mut Response) {
        let origin = req.header("origin").unwrap_or("");
        if origin.is_empty() {
            return;
        }

        if self.allowed_origins.contains(&"*".to_string()) {
            resp.set_header("access-control-allow-origin", "*");
        } else if self.allowed_origins.iter().any(|o| o == origin) {
            resp.set_header("access-control-allow-origin", origin);
        }
        resp.set_header("access-control-allow-credentials", "true");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cors_preflight() {
        let mw = CorsMiddleware::new(vec!["https://example.com".into()]);
        let mut req = Request::new(Method::Options, "/api".into(), "HTTP/1.1".into());
        req.headers
            .insert("origin".into(), "https://example.com".into());
        req.headers
            .insert("access-control-request-method".into(), "POST".into());

        let resp = mw.before(&mut req).await;
        assert!(resp.is_some());
        assert_eq!(resp.unwrap().status, 204);
    }

    #[tokio::test]
    async fn test_cors_simple_request() {
        let mw = CorsMiddleware::new(vec!["*".into()]);
        let mut req = Request::new(Method::Get, "/api".into(), "HTTP/1.1".into());
        req.headers
            .insert("origin".into(), "https://example.com".into());

        let mut resp = Response::ok();
        mw.after(&req, &mut resp);
        assert!(resp
            .headers
            .iter()
            .any(|(k, v)| k == "access-control-allow-origin" && v == "*"));
    }
}
