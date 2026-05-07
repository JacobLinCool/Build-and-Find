pub mod auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::error::Result;
use crate::http::{Request, Response};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn before(&self, _req: &mut Request) -> Result<Option<Response>> {
        Ok(None)
    }
    async fn after(&self, _req: &Request, res: Response) -> Result<Response> {
        Ok(res)
    }
}

#[derive(Default, Clone)]
pub struct Stack {
    items: Vec<Arc<dyn Middleware>>,
}

impl Stack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push<M: Middleware + 'static>(&mut self, mw: M) {
        self.items.push(Arc::new(mw));
    }

    pub async fn run<F, Fut>(&self, mut req: Request, handler: F) -> Result<Response>
    where
        F: FnOnce(Request) -> Fut,
        Fut: std::future::Future<Output = Result<Response>>,
    {
        for mw in &self.items {
            if let Some(res) = mw.before(&mut req).await? {
                return Ok(res);
            }
        }
        let snapshot = req.clone();
        let mut res = handler(req).await?;
        for mw in self.items.iter().rev() {
            res = mw.after(&snapshot, res).await?;
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, StatusCode};
    use std::collections::HashMap;

    struct Add;
    #[async_trait]
    impl Middleware for Add {
        async fn after(&self, _: &Request, mut res: Response) -> Result<Response> {
            res.headers.insert("x-test".into(), "1".into());
            Ok(res)
        }
    }

    #[tokio::test]
    async fn stack_runs() {
        let mut s = Stack::new();
        s.push(Add);
        let req = Request {
            method: Method::Get,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: vec![],
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        let res = s
            .run(req, |_| async { Ok(Response::new(StatusCode::OK, "")) })
            .await
            .unwrap();
        assert_eq!(res.headers["x-test"], "1");
    }
}
