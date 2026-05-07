use super::Middleware;
use crate::http::{Method, Request, Response, StatusCode};
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct Cors {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age_secs: u64,
}

impl Cors {
    pub fn new(allowed_origins: Vec<String>) -> Self {
        Self {
            allowed_origins,
            allowed_methods: vec![
                "GET".into(),
                "POST".into(),
                "PUT".into(),
                "DELETE".into(),
                "PATCH".into(),
                "HEAD".into(),
                "OPTIONS".into(),
            ],
            allowed_headers: vec!["Content-Type".into(), "Authorization".into()],
            allow_credentials: false,
            max_age_secs: 600,
        }
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }

    fn echo_origin(&self, origin: &str) -> String {
        if self.allowed_origins.iter().any(|o| o == "*") && !self.allow_credentials {
            "*".into()
        } else {
            origin.to_string()
        }
    }
}

#[async_trait]
impl Middleware for Cors {
    fn name(&self) -> &'static str {
        "cors"
    }

    async fn before(&self, req: &mut Request) -> Option<Response> {
        if req.method != Method::Options {
            return None;
        }
        let origin = req.header("Origin").map(|s| s.to_string())?;
        if !self.is_origin_allowed(&origin) {
            return None;
        }
        let mut r = Response::new(StatusCode::NO_CONTENT);
        r.headers
            .insert("Access-Control-Allow-Origin", self.echo_origin(&origin));
        r.headers.insert(
            "Access-Control-Allow-Methods",
            self.allowed_methods.join(", "),
        );
        r.headers.insert(
            "Access-Control-Allow-Headers",
            self.allowed_headers.join(", "),
        );
        r.headers
            .insert("Access-Control-Max-Age", self.max_age_secs.to_string());
        if self.allow_credentials {
            r.headers.insert("Access-Control-Allow-Credentials", "true");
        }
        Some(r)
    }

    async fn after(&self, req: &Request, resp: &mut Response) {
        let origin = match req.header("Origin") {
            Some(o) => o,
            None => return,
        };
        if !self.is_origin_allowed(origin) {
            return;
        }
        resp.headers
            .insert("Access-Control-Allow-Origin", self.echo_origin(origin));
        if self.allow_credentials {
            resp.headers
                .insert("Access-Control-Allow-Credentials", "true");
        }
    }
}
