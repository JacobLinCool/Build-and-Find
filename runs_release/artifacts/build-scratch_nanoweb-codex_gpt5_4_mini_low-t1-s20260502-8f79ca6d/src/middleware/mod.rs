use crate::http::{Request, Response};
use async_trait::async_trait;

pub mod auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn before(&self, req: &mut Request) -> Option<Response> {
        let _ = req;
        None
    }
    async fn after(&self, req: &Request, resp: &mut Response) {
        let _ = (req, resp);
    }
}

pub struct Chain {
    items: Vec<Box<dyn Middleware>>,
}

impl Chain {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    pub fn push<M: Middleware + 'static>(&mut self, m: M) {
        self.items.push(Box::new(m));
    }
    pub async fn run_before(&self, req: &mut Request) -> Option<Response> {
        for m in &self.items {
            if let Some(resp) = m.before(req).await {
                return Some(resp);
            }
        }
        None
    }
    pub async fn run_after(&self, req: &Request, resp: &mut Response) {
        for m in &self.items {
            m.after(req, resp).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct M;
    #[async_trait]
    impl Middleware for M {}
    #[tokio::test]
    async fn runs() {
        let mut c = Chain::new();
        c.push(M);
        let mut req = Request {
            method: crate::http::Method::Get,
            target: "/".into(),
            version: crate::http::Version::Http11,
            headers: Default::default(),
            body: vec![],
            path_params: Default::default(),
            query_params: Default::default(),
            remote_addr: None,
            request_id: "1".into(),
        };
        assert!(c.run_before(&mut req).await.is_none());
    }
}
