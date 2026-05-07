use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response};
use crate::middleware::{BoxFuture, Middleware, Next};

#[derive(Clone, Debug)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age: u32,
}

impl Default for CorsConfig {
    fn default() -> Self {
        CorsConfig {
            allowed_origins: vec!["*".into()],
            allowed_methods: vec![
                "GET".into(),
                "POST".into(),
                "PUT".into(),
                "DELETE".into(),
                "PATCH".into(),
                "OPTIONS".into(),
            ],
            allowed_headers: vec!["Content-Type".into(), "Authorization".into()],
            allow_credentials: false,
            max_age: 86400,
        }
    }
}

pub struct CorsMiddleware {
    config: CorsConfig,
}

impl CorsMiddleware {
    pub fn new(config: CorsConfig) -> Self {
        CorsMiddleware { config }
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.config.allowed_origins.contains(&"*".to_string())
            || self.config.allowed_origins.iter().any(|o| o == origin)
    }

    fn add_cors_headers(&self, resp: &mut Response, origin: &str) {
        if self.config.allowed_origins.contains(&"*".to_string()) {
            resp.set_header("Access-Control-Allow-Origin", "*");
        } else if self.is_origin_allowed(origin) {
            resp.set_header("Access-Control-Allow-Origin", origin);
            resp.add_header("Vary", "Origin");
        }

        if self.config.allow_credentials {
            resp.set_header("Access-Control-Allow-Credentials", "true");
        }
    }
}

impl Middleware for CorsMiddleware {
    fn call(&self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let origin = req.get_header("origin").unwrap_or("").to_string();
        let is_preflight = req.method == Method::Options
            && req.get_header("access-control-request-method").is_some();

        let allowed_methods = self.config.allowed_methods.join(", ");
        let allowed_headers = self.config.allowed_headers.join(", ");
        let max_age = self.config.max_age.to_string();
        let is_allowed = self.is_origin_allowed(&origin) || origin.is_empty();
        let allow_wildcard = self.config.allowed_origins.contains(&"*".to_string());
        let allow_creds = self.config.allow_credentials;

        Box::pin(async move {
            if is_preflight && !origin.is_empty() {
                let mut resp = Response::new(204);
                if allow_wildcard {
                    resp.set_header("Access-Control-Allow-Origin", "*");
                } else if is_allowed {
                    resp.set_header("Access-Control-Allow-Origin", &origin);
                    resp.add_header("Vary", "Origin");
                }
                resp.set_header("Access-Control-Allow-Methods", &allowed_methods);
                resp.set_header("Access-Control-Allow-Headers", &allowed_headers);
                resp.set_header("Access-Control-Max-Age", &max_age);
                if allow_creds {
                    resp.set_header("Access-Control-Allow-Credentials", "true");
                }
                return Ok(resp);
            }

            let mut resp = next.run(req).await?;

            if !origin.is_empty() {
                if allow_wildcard {
                    resp.set_header("Access-Control-Allow-Origin", "*");
                } else if is_allowed {
                    resp.set_header("Access-Control-Allow-Origin", &origin);
                    resp.add_header("Vary", "Origin");
                }
                if allow_creds {
                    resp.set_header("Access-Control-Allow-Credentials", "true");
                }
            }

            Ok(resp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::types::{HttpVersion, Method};
    use crate::middleware::{FnHandler, Next};
    use std::collections::HashMap;
    use std::sync::Arc;

    fn make_req_with_origin(origin: &str) -> Request {
        Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![("origin".to_string(), origin.to_string())],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        }
    }

    #[tokio::test]
    async fn test_cors_adds_header() {
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let cors = Arc::new(CorsMiddleware::new(CorsConfig::default()));
        let mw: Vec<Arc<dyn Middleware>> = vec![cors];
        let next = Next::new(handler, mw);
        let resp = next
            .run(make_req_with_origin("https://example.com"))
            .await
            .unwrap();
        assert_eq!(resp.get_header("access-control-allow-origin"), Some("*"));
    }

    #[tokio::test]
    async fn test_cors_preflight() {
        use crate::http::types::Method;
        let handler = Arc::new(FnHandler::new(|_req: Request| async {
            Ok(Response::text("ok"))
        }));
        let cors = Arc::new(CorsMiddleware::new(CorsConfig::default()));
        let mw: Vec<Arc<dyn Middleware>> = vec![cors];
        let next = Next::new(handler, mw);
        let req = Request {
            method: Method::Options,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: HttpVersion::Http11,
            headers: vec![
                ("origin".to_string(), "https://example.com".to_string()),
                (
                    "access-control-request-method".to_string(),
                    "POST".to_string(),
                ),
            ],
            body: vec![],
            params: HashMap::new(),
            remote_addr: "127.0.0.1".into(),
            request_id: "test".into(),
        };
        let resp = next.run(req).await.unwrap();
        assert_eq!(resp.status, 204);
    }
}
