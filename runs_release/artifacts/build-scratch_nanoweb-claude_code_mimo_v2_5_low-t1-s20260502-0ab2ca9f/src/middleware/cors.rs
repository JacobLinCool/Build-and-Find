use crate::http::{Method, Request, Response};
use crate::middleware::Middleware;

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
    allow_credentials: bool,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>, allow_credentials: bool) -> Self {
        CorsMiddleware {
            allowed_origins,
            allow_credentials,
        }
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }
}

impl Middleware for CorsMiddleware {
    fn before(&self, req: &mut Request) -> Option<Response> {
        if req.method == Method::Options {
            let origin = req.headers.get("origin").unwrap_or("");
            let mut res = Response::new();
            res.status = crate::http::StatusCode::new(204);

            if self.is_origin_allowed(origin) || self.allowed_origins.contains(&"*".to_string()) {
                res.headers
                    .insert("access-control-allow-origin".into(), origin.to_string());
                res.headers.insert(
                    "access-control-allow-methods".into(),
                    "GET, POST, PUT, DELETE, PATCH, OPTIONS".into(),
                );
                res.headers.insert(
                    "access-control-allow-headers".into(),
                    "Content-Type, Authorization".into(),
                );
                if self.allow_credentials {
                    res.headers
                        .insert("access-control-allow-credentials".into(), "true".into());
                }
            }
            return Some(res);
        }
        None
    }

    fn after(&self, req: &Request, res: &mut Response) {
        let origin = req.headers.get("origin").unwrap_or("");
        if self.is_origin_allowed(origin) || self.allowed_origins.contains(&"*".to_string()) {
            res.headers
                .insert("access-control-allow-origin".into(), origin.to_string());
            if self.allow_credentials {
                res.headers
                    .insert("access-control-allow-credentials".into(), "true".into());
            }
        }
    }
}
