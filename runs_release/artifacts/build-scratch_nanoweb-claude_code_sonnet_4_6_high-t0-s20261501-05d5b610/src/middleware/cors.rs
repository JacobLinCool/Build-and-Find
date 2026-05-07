use crate::error::NanowebError;
use crate::http::{Method, Request, Response};
use crate::middleware::{Middleware, Next};

#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub exposed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age: Option<u32>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "OPTIONS".to_string(),
            ],
            allowed_headers: vec!["Content-Type".to_string(), "Authorization".to_string()],
            exposed_headers: Vec::new(),
            allow_credentials: false,
            max_age: Some(86400),
        }
    }
}

pub struct CorsMiddleware {
    config: CorsConfig,
}

impl CorsMiddleware {
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }

    pub fn permissive() -> Self {
        Self::new(CorsConfig::default())
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.config
            .allowed_origins
            .iter()
            .any(|o| o == "*" || o == origin)
    }

    fn apply_cors_headers(&self, origin: &str, response: &mut Response) {
        if self.config.allowed_origins.contains(&"*".to_string()) && !self.config.allow_credentials
        {
            response.headers.insert("Access-Control-Allow-Origin", "*");
        } else {
            response
                .headers
                .insert("Access-Control-Allow-Origin", origin);
            response.headers.append("Vary", "Origin");
        }

        if self.config.allow_credentials {
            response
                .headers
                .insert("Access-Control-Allow-Credentials", "true");
        }

        if !self.config.exposed_headers.is_empty() {
            response.headers.insert(
                "Access-Control-Expose-Headers",
                self.config.exposed_headers.join(", "),
            );
        }
    }
}

#[async_trait::async_trait]
impl Middleware for CorsMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let origin = req.headers.get("origin").map(|s| s.to_string());

        // Handle preflight
        if req.method == Method::Options {
            if let Some(ref origin) = origin {
                if self.is_origin_allowed(origin) {
                    let mut resp = Response::new(204);
                    resp.headers.insert("Content-Length", "0");
                    self.apply_cors_headers(origin, &mut resp);
                    resp.headers.insert(
                        "Access-Control-Allow-Methods",
                        self.config.allowed_methods.join(", "),
                    );
                    resp.headers.insert(
                        "Access-Control-Allow-Headers",
                        self.config.allowed_headers.join(", "),
                    );
                    if let Some(max_age) = self.config.max_age {
                        resp.headers
                            .insert("Access-Control-Max-Age", max_age.to_string());
                    }
                    return Ok(resp);
                }
            }
        }

        let mut response = next.run(req).await?;

        if let Some(ref origin) = origin {
            if self.is_origin_allowed(origin) {
                self.apply_cors_headers(origin, &mut response);
            }
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Headers, Version};
    use crate::middleware::MiddlewareChain;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_cors_adds_headers() {
        let mw = Arc::new(CorsMiddleware::permissive());
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let mut req = Request::new(
            Method::Get,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        req.headers.insert("Origin", "https://example.com");
        let resp = chain.run(req).await.unwrap();
        assert!(resp.headers.get("access-control-allow-origin").is_some());
    }

    #[tokio::test]
    async fn test_preflight_response() {
        let mw = Arc::new(CorsMiddleware::permissive());
        let handler = Arc::new(|_req: Request| -> crate::middleware::BoxFuture<_> {
            Box::pin(async { Ok(Response::ok()) })
        });
        let chain = MiddlewareChain::new(vec![mw], handler);
        let mut req = Request::new(
            Method::Options,
            "/".to_string(),
            Version::Http11,
            Headers::new(),
            Vec::new(),
            None,
            "id".to_string(),
        );
        req.headers.insert("Origin", "https://example.com");
        let resp = chain.run(req).await.unwrap();
        assert_eq!(resp.status, 204);
        assert!(resp.headers.get("access-control-allow-methods").is_some());
    }
}
