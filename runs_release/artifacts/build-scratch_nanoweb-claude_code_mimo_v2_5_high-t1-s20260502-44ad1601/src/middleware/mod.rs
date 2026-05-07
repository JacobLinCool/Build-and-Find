pub mod basic_auth;
pub mod compression;
pub mod cors;
pub mod csrf;
pub mod logging;
pub mod rate_limit;

use crate::http::request::Request;
use crate::http::response::Response;
use std::sync::Arc;

pub trait Middleware: Send + Sync {
    fn handle(&self, req: Request, next: Box<dyn FnOnce(Request) -> Response + Send>) -> Response;
}

pub struct MiddlewareChain {
    middlewares: Arc<Vec<Box<dyn Middleware>>>,
}

impl MiddlewareChain {
    pub fn new() -> Self {
        MiddlewareChain {
            middlewares: Arc::new(Vec::new()),
        }
    }

    pub fn from_vec(mws: Vec<Box<dyn Middleware>>) -> Self {
        MiddlewareChain {
            middlewares: Arc::new(mws),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.middlewares.is_empty()
    }

    pub fn execute(
        &self,
        req: Request,
        handler: Box<dyn FnOnce(Request) -> Response + Send>,
    ) -> Response {
        let mws = self.middlewares.clone();
        execute_at(mws, 0, req, handler)
    }
}

impl Clone for MiddlewareChain {
    fn clone(&self) -> Self {
        MiddlewareChain {
            middlewares: self.middlewares.clone(),
        }
    }
}

fn execute_at(
    mws: Arc<Vec<Box<dyn Middleware>>>,
    idx: usize,
    req: Request,
    handler: Box<dyn FnOnce(Request) -> Response + Send>,
) -> Response {
    if idx >= mws.len() {
        return handler(req);
    }
    let next_mws = mws.clone();
    mws[idx].handle(
        req,
        Box::new(move |req| execute_at(next_mws, idx + 1, req, handler)),
    )
}

impl Default for MiddlewareChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestMw {
        label: String,
    }

    impl Middleware for TestMw {
        fn handle(
            &self,
            mut req: Request,
            next: Box<dyn FnOnce(Request) -> Response + Send>,
        ) -> Response {
            req.headers.insert(format!("x-{}", self.label), "1".into());
            next(req)
        }
    }

    #[test]
    fn test_middleware_chain_order() {
        let chain = MiddlewareChain::from_vec(vec![
            Box::new(TestMw { label: "a".into() }),
            Box::new(TestMw { label: "b".into() }),
        ]);

        let req = Request {
            method: crate::http::parser::Method::Get,
            path: "/".into(),
            version: "HTTP/1.1".into(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
            params: std::collections::HashMap::new(),
            query: std::collections::HashMap::new(),
            remote_addr: None,
            request_id: "test".into(),
        };

        let resp = chain.execute(
            req,
            Box::new(|req| {
                assert_eq!(req.headers.get("x-a").unwrap(), "1");
                assert_eq!(req.headers.get("x-b").unwrap(), "1");
                Response::text("ok")
            }),
        );
        assert_eq!(resp.status, 200);
    }
}
