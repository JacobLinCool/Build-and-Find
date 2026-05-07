use super::{Middleware, Next};
use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response, Status};
use async_trait::async_trait;

pub struct CorsMiddleware {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age: u32,
}

impl Default for CorsMiddleware {
    fn default() -> Self {
        CorsMiddleware {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec!["GET", "POST", "PUT", "DELETE", "OPTIONS", "PATCH"]
                .into_iter()
                .map(String::from)
                .collect(),
            allowed_headers: vec!["content-type", "authorization", "x-requested-with"]
                .into_iter()
                .map(String::from)
                .collect(),
            allow_credentials: false,
            max_age: 86400,
        }
    }
}

impl CorsMiddleware {
    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }

    fn apply_cors_headers(&self, resp: &mut Response, origin: Option<&str>) {
        if let Some(orig) = origin {
            if self.is_origin_allowed(orig) {
                resp.headers
                    .insert("access-control-allow-origin".to_string(), orig.to_string());
            } else if self.allowed_origins.contains(&"*".to_string()) {
                resp.headers
                    .insert("access-control-allow-origin".to_string(), "*".to_string());
            }
        } else if self.allowed_origins.contains(&"*".to_string()) {
            resp.headers
                .insert("access-control-allow-origin".to_string(), "*".to_string());
        }

        resp.headers.insert(
            "access-control-allow-methods".to_string(),
            self.allowed_methods.join(", "),
        );
        resp.headers.insert(
            "access-control-allow-headers".to_string(),
            self.allowed_headers.join(", "),
        );
        if self.allow_credentials {
            resp.headers.insert(
                "access-control-allow-credentials".to_string(),
                "true".to_string(),
            );
        }
        resp.headers.insert(
            "access-control-max-age".to_string(),
            self.max_age.to_string(),
        );
    }
}

#[async_trait]
impl Middleware for CorsMiddleware {
    async fn handle(&self, req: Request, next: Next<'_>) -> Result<Response, NanowebError> {
        let origin = req.header("origin").map(String::from);

        // Handle preflight
        if req.method == Method::OPTIONS {
            let mut resp = Response::new(Status::NoContent);
            self.apply_cors_headers(&mut resp, origin.as_deref());
            return Ok(resp);
        }

        let mut resp = next.run(req).await?;
        self.apply_cors_headers(&mut resp, origin.as_deref());
        Ok(resp)
    }
}
