use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::collections::HashSet;

pub struct Cors {
    allowed_origins: HashSet<String>,
}

impl Cors {
    pub fn new<I, S>(origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed_origins: origins.into_iter().map(Into::into).collect(),
        }
    }

    fn allowed(&self, origin: &str) -> bool {
        self.allowed_origins.contains("*") || self.allowed_origins.contains(origin)
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn before(&self, request: &mut Request) -> Option<Response> {
        if request.method == crate::http::Method::Options {
            if let Some(origin) = request.header("origin") {
                if self.allowed(origin) {
                    let mut response = Response::text(204, "");
                    response.set_header("Access-Control-Allow-Origin", origin);
                    response.set_header(
                        "Access-Control-Allow-Methods",
                        "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
                    );
                    response.set_header(
                        "Access-Control-Allow-Headers",
                        "Content-Type, Authorization, X-CSRF-Token",
                    );
                    return Some(response);
                }
            }
        }
        None
    }

    async fn after(&self, request: &Request, response: &mut Response) {
        if let Some(origin) = request.header("origin") {
            if self.allowed(origin) {
                response.set_header("Access-Control-Allow-Origin", origin);
                response.set_header("Vary", "Origin");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn adds_allowed_origin() {
        let cors = Cors::new(["https://example.test"]);
        let mut req = Request::new(crate::http::Method::Get, "/");
        req.set_header("Origin", "https://example.test");
        let mut res = Response::text(200, "ok");
        cors.after(&req, &mut res).await;
        assert_eq!(
            res.header("access-control-allow-origin"),
            Some("https://example.test")
        );
    }
}
