pub mod auth;
pub mod cors;
pub mod csrf;
pub mod gzip;
pub mod logging;
pub mod rate_limit;

use crate::http::{Request, Response};
use crate::router::HandlerFn;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn before(&self, _req: &mut Request) -> Option<Response> {
        None
    }

    async fn after(&self, _req: &Request, res: Response) -> Response {
        res
    }
}

#[derive(Clone)]
pub struct Next {
    middleware: Arc<Vec<Arc<dyn Middleware>>>,
    handler: HandlerFn,
}

impl Next {
    pub fn new(middleware: Arc<Vec<Arc<dyn Middleware>>>, handler: HandlerFn) -> Self {
        Self {
            middleware,
            handler,
        }
    }

    pub async fn run(&self, mut req: Request) -> Response {
        let mut executed = Vec::new();
        for (idx, mw) in self.middleware.iter().enumerate() {
            if let Some(res) = mw.before(&mut req).await {
                return apply_after(&req, res, &self.middleware[..=idx]).await;
            }
            executed.push(idx);
        }
        let res = self.handler.call(req.clone()).await;
        let chain: Vec<Arc<dyn Middleware>> = executed
            .into_iter()
            .map(|idx| self.middleware[idx].clone())
            .collect();
        apply_after_owned(&req, res, &chain).await
    }
}

async fn apply_after(req: &Request, mut res: Response, chain: &[Arc<dyn Middleware>]) -> Response {
    for mw in chain.iter().rev() {
        res = mw.after(req, res).await;
    }
    res
}

async fn apply_after_owned(
    req: &Request,
    mut res: Response,
    chain: &[Arc<dyn Middleware>],
) -> Response {
    for mw in chain.iter().rev() {
        res = mw.after(req, res).await;
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    struct HeaderMw;
    #[async_trait]
    impl Middleware for HeaderMw {
        async fn after(&self, _req: &Request, mut res: Response) -> Response {
            res.header("X-Test", "1");
            res
        }
    }

    #[tokio::test]
    async fn after_runs() {
        let req = crate::http::Request {
            method: crate::http::Method::Get,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: "HTTP/1.1".into(),
            headers: Default::default(),
            body: vec![],
            peer_addr: None,
            request_id: "r".into(),
            params: Default::default(),
        };
        let handler: HandlerFn = Arc::new(|_: Request| async {
            crate::http::Response::new(crate::http::StatusCode::Ok)
        });
        let next = Next::new(Arc::new(vec![Arc::new(HeaderMw)]), handler);
        let res = next.run(req).await;
        assert_eq!(res.headers["X-Test"], "1");
    }
}
