use super::Middleware;
use crate::http::{Method, Request, Response, StatusCode};
use async_trait::async_trait;

pub struct Cors {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<Method>,
    pub allowed_headers: Vec<String>,
    pub allow_credentials: bool,
}

impl Cors {
    pub fn new() -> Self {
        Self {
            allowed_origins: vec!["*".into()],
            allowed_methods: vec![
                Method::Get,
                Method::Post,
                Method::Put,
                Method::Delete,
                Method::Patch,
                Method::Options,
                Method::Head,
            ],
            allowed_headers: vec!["Content-Type".into(), "Authorization".into()],
            allow_credentials: false,
        }
    }

    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        self.allowed_origins = vec![origin.into()];
        self
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }

    fn put_headers(&self, origin: &str, res: &mut Response) {
        if self.allowed_origins.contains(&"*".to_string()) && !self.allow_credentials {
            res.headers.insert("Access-Control-Allow-Origin", "*");
        } else {
            res.headers.insert("Access-Control-Allow-Origin", origin);
            res.headers.insert("Vary", "Origin");
        }
        if self.allow_credentials {
            res.headers
                .insert("Access-Control-Allow-Credentials", "true");
        }
    }
}

impl Default for Cors {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let origin = req.header("Origin").map(|s| s.to_string());
        if req.method == Method::Options {
            if let Some(origin) = origin {
                if self.origin_allowed(&origin) {
                    let mut r = Response::new(StatusCode::NO_CONTENT);
                    self.put_headers(&origin, &mut r);
                    let methods = self
                        .allowed_methods
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    r.headers.insert("Access-Control-Allow-Methods", methods);
                    r.headers.insert(
                        "Access-Control-Allow-Headers",
                        self.allowed_headers.join(", "),
                    );
                    return Some(r);
                }
            }
        }
        None
    }

    async fn after(&self, req: &Request, res: &mut Response) {
        if let Some(origin) = req.header("Origin") {
            if self.origin_allowed(origin) {
                self.put_headers(origin, res);
            }
        }
    }
}
