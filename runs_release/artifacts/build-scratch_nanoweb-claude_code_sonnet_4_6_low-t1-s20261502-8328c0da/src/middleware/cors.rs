use crate::error::NanowebError;
use crate::http::types::{Method, Request, Response, StatusCode};
use crate::middleware::{BoxFuture, Middleware, Next};

pub struct CorsMiddleware {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age: u32,
}

impl CorsMiddleware {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        CorsMiddleware {
            allowed_origins,
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "OPTIONS".to_string(),
            ],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-Request-Id".to_string(),
            ],
            allow_credentials: false,
            max_age: 86400,
        }
    }

    pub fn permissive() -> Self {
        CorsMiddleware::new(vec!["*".to_string()])
    }
}

impl Middleware for CorsMiddleware {
    fn handle<'a>(&'a self, req: Request, next: Next) -> BoxFuture<Result<Response, NanowebError>> {
        let origin = req.headers.get("origin").map(|s| s.to_string());
        let is_preflight = req.method == Method::Options;
        let allowed_origins = self.allowed_origins.clone();
        let allowed_methods = self.allowed_methods.join(", ");
        let allowed_headers = self.allowed_headers.join(", ");
        let allow_credentials = self.allow_credentials;
        let max_age = self.max_age;

        Box::pin(async move {
            let origin_header = match &origin {
                Some(o) => {
                    if allowed_origins.contains(&"*".to_string()) {
                        Some("*".to_string())
                    } else if allowed_origins.contains(o) {
                        Some(o.clone())
                    } else {
                        None
                    }
                }
                None => None,
            };

            if is_preflight {
                let mut resp = Response::new(StatusCode::OK);
                if let Some(origin_val) = origin_header {
                    resp.headers.set("access-control-allow-origin", origin_val);
                    resp.headers.set("access-control-allow-methods", &allowed_methods);
                    resp.headers.set("access-control-allow-headers", &allowed_headers);
                    resp.headers.set("access-control-max-age", max_age.to_string());
                    if allow_credentials {
                        resp.headers.set("access-control-allow-credentials", "true");
                    }
                }
                return Ok(resp);
            }

            let mut resp = next.run(req).await?;

            if let Some(origin_val) = origin_header {
                resp.headers.set("access-control-allow-origin", origin_val);
                if allow_credentials {
                    resp.headers.set("access-control-allow-credentials", "true");
                }
            }

            Ok(resp)
        })
    }
}
