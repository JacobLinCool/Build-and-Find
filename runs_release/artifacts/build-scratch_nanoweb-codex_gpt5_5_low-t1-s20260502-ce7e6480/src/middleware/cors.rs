use super::Middleware;
use crate::error::Result;
use crate::http::{Request, Response, StatusCode};
use async_trait::async_trait;
use std::collections::HashSet;

pub struct Cors {
    origins: HashSet<String>,
}

impl Cors {
    pub fn new(origins: Vec<String>) -> Self {
        Self {
            origins: origins.into_iter().collect(),
        }
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn before(&self, req: &mut Request) -> Result<Option<Response>> {
        if matches!(req.method, crate::http::Method::Options)
            && req.header("access-control-request-method").is_some()
        {
            return Ok(Some(Response::new(StatusCode::NO_CONTENT, Vec::new())));
        }
        Ok(None)
    }

    async fn after(&self, req: &Request, mut res: Response) -> Result<Response> {
        if let Some(origin) = req.header("origin") {
            if self.origins.contains("*") || self.origins.contains(origin) {
                res.headers
                    .insert("access-control-allow-origin".into(), origin.into());
                res.headers.insert(
                    "access-control-allow-methods".into(),
                    "GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS".into(),
                );
                res.headers.insert(
                    "access-control-allow-headers".into(),
                    "content-type, authorization, x-csrf-token".into(),
                );
            }
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode};
    use std::collections::HashMap;

    #[tokio::test]
    async fn allows_configured_origin() {
        let mut headers = HashMap::new();
        headers.insert("origin".into(), "https://a.test".into());
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers,
            body: vec![],
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        let res = Cors::new(vec!["https://a.test".into()])
            .after(&req, Response::new(StatusCode::OK, ""))
            .await
            .unwrap();
        assert_eq!(res.headers["access-control-allow-origin"], "https://a.test");
    }
}
