use async_trait::async_trait;

use crate::error::Result;
use crate::http::{Request, Response, StatusCode};

use super::{Middleware, Next};

#[derive(Clone, Default)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
}

pub struct Cors {
    config: CorsConfig,
}

impl Cors {
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        self.config
            .allowed_origins
            .iter()
            .any(|allowed| allowed == "*" || allowed == origin)
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn handle(&self, request: Request, next: Next<'_>) -> Result<Response> {
        let origin = request.header("origin").map(str::to_string);
        let preflight = request.header("access-control-request-method").is_some();
        let mut response = if preflight
            && origin
                .as_deref()
                .map(|origin| self.origin_allowed(origin))
                .unwrap_or(false)
        {
            Response::empty(StatusCode::NoContent)
        } else {
            next.run(request).await?
        };

        if let Some(origin) = origin.as_deref() {
            if self.origin_allowed(origin) {
                response
                    .headers
                    .insert("access-control-allow-origin", origin);
                response.headers.insert("vary", "origin");
                if preflight {
                    response.headers.insert(
                        "access-control-allow-methods",
                        "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
                    );
                }
            }
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HeaderMap, Method, Version};
    use crate::router::Handler;
    use std::sync::Arc;

    struct OkHandler;

    #[async_trait]
    impl Handler for OkHandler {
        async fn handle(&self, _request: Request) -> Result<Response> {
            Ok(Response::text(StatusCode::Ok, "ok"))
        }
    }

    #[tokio::test]
    async fn adds_origin_header() {
        let cors = Cors::new(CorsConfig {
            allowed_origins: vec!["https://example.com".into()],
        });
        let mut headers = HeaderMap::new();
        headers.insert("origin", "https://example.com");
        let request = Request {
            method: Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: Version::Http11,
            headers,
            body: vec![],
            params: Default::default(),
            remote_addr: None,
            request_id: "req".into(),
            session: None,
        };
        let response = crate::middleware::execute(&[Arc::new(cors)], Arc::new(OkHandler), request)
            .await
            .unwrap();
        assert_eq!(
            response.headers.get("access-control-allow-origin"),
            Some("https://example.com")
        );
    }
}
