use super::Middleware;
use crate::http::{Request, Response};
use async_trait::async_trait;

pub struct Cors {
    allowed: Vec<String>,
}

impl Cors {
    pub fn new(allowed: Vec<String>) -> Self {
        Self { allowed }
    }
}

#[async_trait]
impl Middleware for Cors {
    async fn after(&self, req: &Request, resp: &mut Response) {
        if let Some(origin) = req.header("origin") {
            if self.allowed.iter().any(|o| o == "*" || o == origin) {
                resp.headers
                    .insert("access-control-allow-origin".into(), origin.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn adds_header() {
        let req = Request {
            method: crate::http::Method::Get,
            target: "/".into(),
            version: crate::http::Version::Http11,
            headers: [("origin".into(), "https://x".into())]
                .into_iter()
                .collect(),
            body: vec![],
            path_params: Default::default(),
            query_params: Default::default(),
            remote_addr: None,
            request_id: "1".into(),
        };
        let mut resp = Response::new(200, Vec::new());
        Cors::new(vec!["https://x".into()])
            .after(&req, &mut resp)
            .await;
        assert_eq!(
            resp.headers.get("access-control-allow-origin").unwrap(),
            "https://x"
        );
    }
}
