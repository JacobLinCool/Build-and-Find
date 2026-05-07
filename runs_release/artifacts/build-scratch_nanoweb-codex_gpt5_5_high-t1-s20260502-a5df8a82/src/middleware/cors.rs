use async_trait::async_trait;

use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};
use crate::middleware::Middleware;

pub struct Cors {
    allowed_origins: Vec<String>,
}

impl Cors {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self { allowed_origins }
    }

    fn is_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|v| v == "*" || v == origin)
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn before(&self, request: &mut Request) -> Result<Option<Response>> {
        if request.method == Method::Options
            && request.header("access-control-request-method").is_some()
        {
            let mut response = Response::new(StatusCode::NO_CONTENT);
            if let Some(origin) = request
                .header("origin")
                .filter(|origin| self.is_allowed(origin))
            {
                response.set_header("Access-Control-Allow-Origin", origin.to_string());
            }
            response.set_header(
                "Access-Control-Allow-Methods",
                "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
            );
            response.set_header(
                "Access-Control-Allow-Headers",
                "Content-Type, Authorization, X-CSRF-Token",
            );
            return Ok(Some(response));
        }
        Ok(None)
    }

    async fn after(&self, request: &Request, response: &mut Response) -> Result<()> {
        if let Some(origin) = request
            .header("origin")
            .filter(|origin| self.is_allowed(origin))
        {
            response.set_header("Access-Control-Allow-Origin", origin.to_string());
            response.set_header("Vary", "Origin");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn adds_allowed_origin() {
        let cors = Cors::new(vec!["https://example.com".into()]);
        let mut req = Request::new(Method::Get, "/");
        req.set_header("Origin", "https://example.com");
        let mut res = Response::text(StatusCode::OK, "ok");
        cors.after(&req, &mut res).await.unwrap();
        assert_eq!(
            res.header("access-control-allow-origin"),
            Some("https://example.com")
        );
    }
}
