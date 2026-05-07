//! CORS middleware with configurable allowed origins.

use crate::error::Result;
use crate::http::{Method, Request, Response, StatusCode};

use super::{BoxFuture, Middleware, Next};

pub struct CorsMiddleware {
    pub allowed_origins: Vec<String>,
    pub allow_credentials: bool,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub max_age_seconds: u64,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self {
            allowed_origins,
            allow_credentials: false,
            allowed_methods: vec![
                "GET".into(),
                "POST".into(),
                "PUT".into(),
                "DELETE".into(),
                "PATCH".into(),
                "OPTIONS".into(),
                "HEAD".into(),
            ],
            allowed_headers: vec!["Content-Type".into(), "Authorization".into()],
            max_age_seconds: 600,
        }
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }

    fn apply_headers(&self, resp: &mut Response, origin: &str) {
        let header_origin =
            if self.allowed_origins.iter().any(|o| o == "*") && !self.allow_credentials {
                "*".to_string()
            } else {
                origin.to_string()
            };
        resp.headers
            .insert("access-control-allow-origin", header_origin);
        if self.allow_credentials {
            resp.headers
                .insert("access-control-allow-credentials", "true");
        }
        resp.headers.insert("vary", "Origin");
    }
}

impl Middleware for CorsMiddleware {
    fn handle<'a>(
        &'a self,
        req: &'a mut Request,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<Response>> {
        Box::pin(async move {
            let origin = req.header("origin").map(|s| s.to_string());
            if let Some(origin) = origin.as_deref() {
                if !self.origin_allowed(origin) {
                    // Origin sent but not allowed: pass through without CORS headers.
                    return next.run(req).await;
                }
                if req.method == Method::Options
                    && req.header("access-control-request-method").is_some()
                {
                    let mut resp = Response::new(StatusCode::NO_CONTENT);
                    self.apply_headers(&mut resp, origin);
                    resp.headers.insert(
                        "access-control-allow-methods",
                        self.allowed_methods.join(", "),
                    );
                    resp.headers.insert(
                        "access-control-allow-headers",
                        self.allowed_headers.join(", "),
                    );
                    resp.headers
                        .insert("access-control-max-age", self.max_age_seconds.to_string());
                    return Ok(resp);
                }
                let mut resp = next.run(req).await?;
                self.apply_headers(&mut resp, origin);
                return Ok(resp);
            }
            next.run(req).await
        })
    }
}
