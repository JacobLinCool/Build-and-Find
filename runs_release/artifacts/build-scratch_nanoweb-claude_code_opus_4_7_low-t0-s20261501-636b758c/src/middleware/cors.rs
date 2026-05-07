use super::Middleware;
use crate::http::{Method, Request, Response, StatusCode};
use crate::NanowebError;
use async_trait::async_trait;

pub struct Cors {
    pub allowed_origins: Vec<String>,
    pub allow_credentials: bool,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
}

impl Cors {
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
            ],
            allowed_headers: vec!["Content-Type".into(), "Authorization".into()],
        }
    }

    fn origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }

    fn apply(&self, headers: &mut crate::http::Headers, origin: &str) {
        if self.origin_allowed(origin) {
            let val = if self.allowed_origins.iter().any(|o| o == "*") && !self.allow_credentials {
                "*".to_string()
            } else {
                origin.to_string()
            };
            headers.insert("Access-Control-Allow-Origin", val);
            if self.allow_credentials {
                headers.insert("Access-Control-Allow-Credentials", "true");
            }
            headers.insert("Vary", "Origin");
        }
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>, NanowebError> {
        if req.method == Method::Options {
            let origin = req.header("origin").unwrap_or("").to_string();
            let mut r = Response::new(StatusCode::NO_CONTENT);
            self.apply(&mut r.headers, &origin);
            r.headers.insert(
                "Access-Control-Allow-Methods",
                self.allowed_methods.join(", "),
            );
            r.headers.insert(
                "Access-Control-Allow-Headers",
                self.allowed_headers.join(", "),
            );
            return Ok(Some(r));
        }
        Ok(None)
    }

    async fn after(&self, req: &Request, mut resp: Response) -> Result<Response, NanowebError> {
        let origin = req.header("origin").unwrap_or("").to_string();
        if !origin.is_empty() {
            self.apply(&mut resp.headers, &origin);
        }
        Ok(resp)
    }
}
