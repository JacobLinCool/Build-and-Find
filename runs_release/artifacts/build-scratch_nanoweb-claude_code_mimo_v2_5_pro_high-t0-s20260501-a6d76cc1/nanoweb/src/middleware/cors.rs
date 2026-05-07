use super::Middleware;
use crate::http::{Method, Request, Response};
use async_trait::async_trait;

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
    allow_any: bool,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        let allow_any = allowed_origins.contains(&"*".to_string());
        Self {
            allowed_origins,
            allow_any,
        }
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allow_any || self.allowed_origins.iter().any(|o| o == origin)
    }
}

#[async_trait]
impl Middleware for CorsMiddleware {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        if req.method == Method::Options {
            let origin = req.header("origin").unwrap_or("").to_string();
            let mut resp = Response::new(204);
            if self.is_origin_allowed(&origin) || self.allow_any {
                let origin_val = if self.allow_any {
                    "*".to_string()
                } else {
                    origin
                };
                resp.set_header("access-control-allow-origin", origin_val);
                resp.set_header(
                    "access-control-allow-methods",
                    "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
                );
                resp.set_header(
                    "access-control-allow-headers",
                    "content-type, authorization, x-request-id",
                );
                resp.set_header("access-control-max-age", "86400");
            }
            return Some(resp);
        }
        None
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        let origin = req.header("origin").unwrap_or("");
        if self.is_origin_allowed(origin) {
            let origin_val = if self.allow_any {
                "*".to_string()
            } else {
                origin.to_string()
            };
            resp.set_header("access-control-allow-origin", origin_val);
        }
    }
}
