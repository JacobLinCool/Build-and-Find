use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::collections::HashSet;

pub struct Cors {
    allowed: HashSet<String>,
}

impl Cors {
    pub fn new(origins: Vec<String>) -> Self {
        Self {
            allowed: origins.into_iter().collect(),
        }
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn after(&self, req: &Request, mut res: Response) -> Response {
        if let Some(origin) = req.header("origin") {
            if self.allowed.contains("*") || self.allowed.contains(origin) {
                res.header("Access-Control-Allow-Origin", origin);
                res.header(
                    "Access-Control-Allow-Methods",
                    "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS",
                );
                res.header(
                    "Access-Control-Allow-Headers",
                    "Content-Type, Authorization, X-CSRF-Token",
                );
            }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn construct() {
        let _ = super::Cors::new(vec!["*".into()]);
    }
}
